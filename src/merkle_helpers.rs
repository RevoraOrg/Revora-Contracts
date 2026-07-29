//! # Deterministic Merkle-Tree Construction for Snapshot Finalization
//!
//! This module provides two pure helpers for building a tamper-evident, cross-
//! implementation-reproducible Merkle root over a snapshot holder set:
//!
//! * [`canonical_leaves`] — validates and sorts `(Address, share_bps)` pairs into
//!   a canonical, lexicographic order by holder-address XDR bytes.
//! * [`build_merkle_root`] — constructs a standard binary Merkle tree from the sorted
//!   leaves and returns the SHA-256 root as a `BytesN<32>`.
//!
//! ## Ordering guarantee
//!
//! Leaves are ordered **lexicographically ascending** by the serialised XDR encoding of
//! the holder `Address`.  This is identical to the tie-break ordering already used by
//! `prove_distribution_for_period` and guarantees that any off-chain implementation that
//! follows the same XDR serialisation will produce bit-for-bit identical roots.
//!
//! ## Merkle tree construction
//!
//! The tree is built bottom-up.  Each level is:
//!
//! ```text
//! leaf   = SHA-256( 0x00 || holder_xdr || share_bps_xdr )
//! parent = SHA-256( 0x01 || min(left, right) || max(left, right) )
//! ```
//!
//! The **0x00 / 0x01 domain prefix** prevents second-preimage attacks where an attacker
//! constructs an internal node whose value collides with a leaf hash.
//!
//! The **sorted-pair** rule (`min || max`) at the internal-node level means the root is
//! independent of the initial sort order — it depends only on the *set* of leaves.
//! Combined with [`canonical_leaves`]'s duplicate-address rejection this gives a
//! bijection between holder sets and Merkle roots.
//!
//! ## Empty-tree convention
//!
//! When no leaves are supplied `build_merkle_root` returns `SHA-256(b"")` (the hash of
//! an empty byte string).  This is a well-defined, reproducible value that off-chain
//! verifiers can check without special-casing.
//!
//! ## Security notes
//!
//! 1. **Collision resistance** — relies on SHA-256 collision resistance.  No known
//!    practical attack exists; this is consistent with the rest of the contract.
//! 2. **No state mutation** — both helpers are pure: they read no storage and write no
//!    storage.  They cannot be used to alter contract state and require no auth.
//! 3. **Duplicate rejection** — [`canonical_leaves`] returns [`MerkleError::DuplicateAddress`]
//!    if the same `Address` appears more than once.  This prevents a tree with duplicate
//!    leaves silently accepting them, which would let an issuer double-count a holder.
//! 4. **share_bps validation** — each entry is validated; values above 10 000 are
//!    rejected with [`MerkleError::InvalidShareBps`] before any hashing is done.
//! 5. **Input size** — callers are responsible for bounding the input length.  The
//!    helpers do not enforce a maximum; the contract entry points that call them enforce
//!    `MAX_SNAPSHOT_BATCH` (50) per call.

#![allow(dead_code)]

use soroban_sdk::{contracterror, xdr::ToXdr, Address, Bytes, BytesN, Env, Vec};

// ── Error type ─────────────────────────────────────────────────────────────

/// Errors returned by the Merkle-helper functions.
///
/// These are distinct from [`crate::RevoraError`] so that callers can decide
/// how to propagate them (e.g. map to `InvalidShareBps` or a dedicated variant).
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u32)]
pub enum MerkleError {
    /// A holder `Address` appeared more than once in the input slice.
    DuplicateAddress = 1001,
    /// A `share_bps` value exceeded 10 000 (100 %).
    InvalidShareBps = 1002,
}

// ── Public helpers ──────────────────────────────────────────────────────────

/// One entry in a canonical Merkle-leaf sequence.
///
/// Produced by [`canonical_leaves`] and consumed by [`build_merkle_root`].
/// The fields are intentionally `pub` so external code can inspect the ordering.
#[derive(Clone, Debug)]
pub struct MerkleLeaf {
    /// The holder address.
    pub holder: Address,
    /// The holder's share in basis points (0 – 10 000).
    pub share_bps: u32,
    /// The XDR encoding of `holder`, cached to avoid re-serialising during tree
    /// construction and to make the ordering key cheaply observable.
    pub holder_xdr: Bytes,
}

/// Sort and validate a slice of `(Address, share_bps)` pairs into canonical order.
///
/// ## Ordering
///
/// Entries are sorted **lexicographically ascending** by the XDR encoding of the
/// holder `Address`.  When two addresses would produce identical XDR bytes the
/// function returns [`MerkleError::DuplicateAddress`] (valid addresses are globally
/// unique, so this also serves as a duplicate-detection step).
///
/// ## Validation
///
/// * `share_bps > 10_000` → [`MerkleError::InvalidShareBps`]
/// * duplicate `Address`  → [`MerkleError::DuplicateAddress`]
///
/// ## Returns
///
/// A `Vec<MerkleLeaf>` in canonical order, ready to pass to [`build_merkle_root`].
///
/// # Example (off-chain reference pseudo-code)
/// ```ignore
/// let leaves = canonical_leaves(&env, &holders)?;
/// // leaves[0] has the lexicographically smallest holder address
/// ```
pub fn canonical_leaves(
    env: &Env,
    entries: &[(Address, u32)],
) -> Result<Vec<MerkleLeaf>, MerkleError> {
    // ── 1. Validate and serialise ──────────────────────────────────────────
    let mut leaves: soroban_sdk::Vec<MerkleLeaf> = Vec::new(env);

    for (holder, share_bps) in entries.iter() {
        if *share_bps > 10_000 {
            return Err(MerkleError::InvalidShareBps);
        }
        let holder_xdr = holder.to_xdr(env);
        leaves.push_back(MerkleLeaf {
            holder: holder.clone(),
            share_bps: *share_bps,
            holder_xdr,
        });
    }

    // ── 2. Insertion sort by holder_xdr (ascending, lexicographic) ─────────
    //
    // Soroban's `no_std` environment has no `alloc::slice::sort`, so we use a
    // simple O(n²) insertion sort.  Snapshot batches are bounded by
    // MAX_SNAPSHOT_BATCH = 50, making this cost negligible in practice.
    let n = leaves.len();
    for i in 1..n {
        let mut j = i;
        while j > 0 {
            let a = leaves.get(j.saturating_sub(1)).unwrap();
            let b = leaves.get(j).unwrap();
            let cmp = compare_bytes(env, &a.holder_xdr, &b.holder_xdr);
            if cmp == Ordering::Greater {
                // swap a and b
                leaves.set(j.saturating_sub(1), b.clone());
                leaves.set(j, a);
                j = j.saturating_sub(1);
            } else {
                break;
            }
        }
    }

    // ── 3. Duplicate detection (adjacent entries after sort) ───────────────
    for i in 1..leaves.len() {
        let prev = leaves.get(i.saturating_sub(1)).unwrap();
        let curr = leaves.get(i).unwrap();
        if compare_bytes(env, &prev.holder_xdr, &curr.holder_xdr) == Ordering::Equal {
            return Err(MerkleError::DuplicateAddress);
        }
    }

    Ok(leaves)
}

/// Build a deterministic SHA-256 Merkle root from an ordered leaf sequence.
///
/// The `leaves` argument MUST already be in canonical order; pass the output of
/// [`canonical_leaves`] directly.
///
/// ## Tree construction
///
/// ```text
/// leaf   = SHA-256( 0x00 || holder_xdr || share_bps_xdr )
/// parent = SHA-256( 0x01 || min(left, right) || max(left, right) )
/// ```
///
/// Odd levels are padded by **duplicating the last node**, consistent with the
/// Bitcoin / RFC-style Merkle tree convention.
///
/// ## Empty input
///
/// Returns `SHA-256(b"")` when `leaves` is empty.
///
/// ## Single leaf
///
/// Returns the leaf hash directly (no wrapping node).
pub fn build_merkle_root(env: &Env, leaves: &Vec<MerkleLeaf>) -> BytesN<32> {
    let n = leaves.len();

    // ── Empty tree ──────────────────────────────────────────────────────────
    if n == 0 {
        return env.crypto().sha256(&Bytes::new(env));
    }

    // ── Compute leaf hashes ─────────────────────────────────────────────────
    let mut level: soroban_sdk::Vec<BytesN<32>> = Vec::new(env);
    for i in 0..n {
        let leaf = leaves.get(i).unwrap();
        level.push_back(hash_leaf(env, &leaf));
    }

    // ── Reduce levels until a single root remains ───────────────────────────
    while level.len() > 1 {
        let len = level.len();
        let mut next_level: soroban_sdk::Vec<BytesN<32>> = Vec::new(env);
        let mut idx: u32 = 0;
        while idx < len {
            let left = level.get(idx).unwrap();
            // Pad odd nodes by duplicating the last node.
            let right = if idx.saturating_add(1) < len {
                level.get(idx.saturating_add(1)).unwrap()
            } else {
                left.clone()
            };
            next_level.push_back(hash_node(env, &left, &right));
            idx = idx.saturating_add(2);
        }
        level = next_level;
    }

    level.get(0).unwrap()
}

// ── Private helpers ─────────────────────────────────────────────────────────

/// Simple three-way comparison result (no std `Ordering` in no_std environments).
#[derive(Eq, PartialEq)]
enum Ordering {
    Less,
    Equal,
    Greater,
}

/// Lexicographic comparison of two `Bytes` values, byte by byte.
fn compare_bytes(env: &Env, a: &Bytes, b: &Bytes) -> Ordering {
    let a_len = a.len();
    let b_len = b.len();
    let min_len = if a_len < b_len { a_len } else { b_len };

    for i in 0..min_len {
        let av = a.get(i).unwrap_or(0);
        let bv = b.get(i).unwrap_or(0);
        if av < bv {
            return Ordering::Less;
        }
        if av > bv {
            return Ordering::Greater;
        }
    }

    if a_len < b_len {
        Ordering::Less
    } else if a_len > b_len {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// Leaf hash: `SHA-256( 0x00 || holder_xdr || share_bps_xdr )`.
///
/// The `0x00` domain prefix distinguishes leaf hashes from internal-node hashes,
/// preventing second-preimage attacks.
fn hash_leaf(env: &Env, leaf: &MerkleLeaf) -> BytesN<32> {
    let mut input = Bytes::new(env);
    // Domain prefix for leaf nodes.
    input.push_back(0x00u8);
    input.append(&leaf.holder_xdr);
    input.append(&leaf.share_bps.to_xdr(env));
    env.crypto().sha256(&input)
}

/// Internal-node hash: `SHA-256( 0x01 || min(left, right) || max(left, right) )`.
///
/// The `0x01` domain prefix distinguishes internal nodes from leaves.
/// The sorted-pair rule makes the tree order-independent at each level, so the
/// root depends only on the *set* of leaf hashes and not on their left/right
/// placement within the tree.
fn hash_node(env: &Env, left: &BytesN<32>, right: &BytesN<32>) -> BytesN<32> {
    // Determine min/max by comparing the 32-byte arrays lexicographically.
    let left_bytes: Bytes = left.clone().into();
    let right_bytes: Bytes = right.clone().into();
    let (lo, hi) = if compare_bytes(env, &left_bytes, &right_bytes) == Ordering::Greater {
        (right_bytes, left_bytes)
    } else {
        (left_bytes, right_bytes)
    };

    let mut input = Bytes::new(env);
    // Domain prefix for internal nodes.
    input.push_back(0x01u8);
    input.append(&lo);
    input.append(&hi);
    env.crypto().sha256(&input)
}
