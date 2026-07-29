//! Tests for [`crate::merkle_helpers`].
//!
//! Coverage targets:
//!
//! | Area                              | Tests |
//! |-----------------------------------|-------|
//! | canonical_leaves – ordering       | 4     |
//! | canonical_leaves – validation     | 3     |
//! | canonical_leaves – edge cases     | 2     |
//! | build_merkle_root – correctness   | 5     |
//! | build_merkle_root – determinism   | 3     |
//! | Security / property checks        | 3     |
//! | Integration with snapshot helpers | 2     |
//!
//! All tests use `Env::default()` + `mock_all_auths()` for a deterministic,
//! CI-safe environment.

#![cfg(test)]

use crate::merkle_helpers::{build_merkle_root, canonical_leaves, MerkleError};
use soroban_sdk::{
    testutils::Address as _,
    xdr::ToXdr,
    Address, Bytes, BytesN, Env,
};

// ── Helpers ─────────────────────────────────────────────────────────────────

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Compute the expected leaf hash independently, matching the spec:
///   SHA-256( 0x00 || holder_xdr || share_bps_xdr )
fn expected_leaf_hash(env: &Env, holder: &Address, share_bps: u32) -> BytesN<32> {
    let mut input = Bytes::new(env);
    input.push_back(0x00u8);
    input.append(&holder.to_xdr(env));
    input.append(&share_bps.to_xdr(env));
    env.crypto().sha256(&input)
}

/// Compute the expected internal-node hash independently:
///   SHA-256( 0x01 || min(left, right) || max(left, right) )
fn expected_node_hash(env: &Env, left: &BytesN<32>, right: &BytesN<32>) -> BytesN<32> {
    let lb: Bytes = left.clone().into();
    let rb: Bytes = right.clone().into();
    // Determine lexicographic min/max
    let (lo, hi) = if lb.get(0).unwrap_or(0) > rb.get(0).unwrap_or(0)
        || (lb.get(0).unwrap_or(0) == rb.get(0).unwrap_or(0)
            && lb.get(1).unwrap_or(0) > rb.get(1).unwrap_or(0))
    {
        (rb, lb)
    } else {
        (lb, rb)
    };
    let mut input = Bytes::new(env);
    input.push_back(0x01u8);
    input.append(&lo);
    input.append(&hi);
    env.crypto().sha256(&input)
}

// ══════════════════════════════════════════════════════════════════════════════
// canonical_leaves – ordering
// ══════════════════════════════════════════════════════════════════════════════

/// canonical_leaves with a single entry returns a one-element Vec.
#[test]
fn canonical_leaves_single_entry() {
    let env = make_env();
    let holder = Address::generate(&env);
    let entries = [(holder.clone(), 5_000u32)];
    let leaves = canonical_leaves(&env, &entries).expect("should succeed");
    assert_eq!(leaves.len(), 1);
    assert_eq!(leaves.get(0).unwrap().holder, holder);
    assert_eq!(leaves.get(0).unwrap().share_bps, 5_000);
}

/// canonical_leaves sorts two entries so the lexicographically smaller address
/// comes first, regardless of the input order.
#[test]
fn canonical_leaves_two_entries_sorted_ascending() {
    let env = make_env();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let a_xdr = a.to_xdr(&env);
    let b_xdr = b.to_xdr(&env);

    // Determine expected order by comparing XDR bytes.
    let (expected_first, expected_second) = {
        let mut a_lt_b = false;
        for i in 0..a_xdr.len().min(b_xdr.len()) {
            let av = a_xdr.get(i).unwrap_or(0);
            let bv = b_xdr.get(i).unwrap_or(0);
            if av < bv {
                a_lt_b = true;
                break;
            } else if av > bv {
                break;
            }
        }
        if a_lt_b { (a.clone(), b.clone()) } else { (b.clone(), a.clone()) }
    };

    // Supply them in *reverse* expected order so the sort is exercised.
    let entries = [(expected_second.clone(), 3_000u32), (expected_first.clone(), 7_000u32)];
    let leaves = canonical_leaves(&env, &entries).expect("should succeed");

    assert_eq!(leaves.len(), 2);
    assert_eq!(leaves.get(0).unwrap().holder, expected_first);
    assert_eq!(leaves.get(1).unwrap().holder, expected_second);
}

/// canonical_leaves output is identical regardless of the input order of three entries.
#[test]
fn canonical_leaves_order_is_input_independent() {
    let env = make_env();
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);
    let h3 = Address::generate(&env);

    let entries_abc = [
        (h1.clone(), 1_000u32),
        (h2.clone(), 2_000u32),
        (h3.clone(), 3_000u32),
    ];
    let entries_cba = [
        (h3.clone(), 3_000u32),
        (h2.clone(), 2_000u32),
        (h1.clone(), 1_000u32),
    ];
    let entries_bac = [
        (h2.clone(), 2_000u32),
        (h1.clone(), 1_000u32),
        (h3.clone(), 3_000u32),
    ];

    let leaves_abc = canonical_leaves(&env, &entries_abc).unwrap();
    let leaves_cba = canonical_leaves(&env, &entries_cba).unwrap();
    let leaves_bac = canonical_leaves(&env, &entries_bac).unwrap();

    assert_eq!(leaves_abc.len(), 3);
    assert_eq!(leaves_cba.len(), 3);
    assert_eq!(leaves_bac.len(), 3);

    // All permutations should produce the same address sequence.
    for i in 0..3u32 {
        assert_eq!(
            leaves_abc.get(i).unwrap().holder,
            leaves_cba.get(i).unwrap().holder,
            "abc vs cba mismatch at index {i}"
        );
        assert_eq!(
            leaves_abc.get(i).unwrap().holder,
            leaves_bac.get(i).unwrap().holder,
            "abc vs bac mismatch at index {i}"
        );
    }
}

/// canonical_leaves correctly caches the holder_xdr in each MerkleLeaf.
#[test]
fn canonical_leaves_holder_xdr_matches_address_to_xdr() {
    let env = make_env();
    let holder = Address::generate(&env);
    let entries = [(holder.clone(), 5_000u32)];
    let leaves = canonical_leaves(&env, &entries).unwrap();
    let leaf = leaves.get(0).unwrap();
    assert_eq!(leaf.holder_xdr, holder.to_xdr(&env));
}

// ══════════════════════════════════════════════════════════════════════════════
// canonical_leaves – validation
// ══════════════════════════════════════════════════════════════════════════════

/// canonical_leaves rejects share_bps > 10_000 with InvalidShareBps.
#[test]
fn canonical_leaves_rejects_share_bps_over_ten_thousand() {
    let env = make_env();
    let holder = Address::generate(&env);
    let entries = [(holder.clone(), 10_001u32)];
    let result = canonical_leaves(&env, &entries);
    assert_eq!(result, Err(MerkleError::InvalidShareBps));
}

/// canonical_leaves allows share_bps == 10_000 (exactly 100 %).
#[test]
fn canonical_leaves_allows_max_share_bps() {
    let env = make_env();
    let holder = Address::generate(&env);
    let entries = [(holder.clone(), 10_000u32)];
    assert!(canonical_leaves(&env, &entries).is_ok());
}

/// canonical_leaves rejects duplicate addresses with DuplicateAddress.
#[test]
fn canonical_leaves_rejects_duplicate_address() {
    let env = make_env();
    let holder = Address::generate(&env);
    // Same holder appears twice with different share_bps.
    let entries = [(holder.clone(), 5_000u32), (holder.clone(), 3_000u32)];
    let result = canonical_leaves(&env, &entries);
    assert_eq!(result, Err(MerkleError::DuplicateAddress));
}

// ══════════════════════════════════════════════════════════════════════════════
// canonical_leaves – edge cases
// ══════════════════════════════════════════════════════════════════════════════

/// canonical_leaves with an empty slice returns an empty Vec.
#[test]
fn canonical_leaves_empty_input() {
    let env = make_env();
    let leaves = canonical_leaves(&env, &[]).unwrap();
    assert_eq!(leaves.len(), 0);
}

/// canonical_leaves allows share_bps == 0 (zero-share holder).
#[test]
fn canonical_leaves_zero_share_bps_is_valid() {
    let env = make_env();
    let holder = Address::generate(&env);
    let entries = [(holder.clone(), 0u32)];
    let result = canonical_leaves(&env, &entries);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().get(0).unwrap().share_bps, 0);
}

// ══════════════════════════════════════════════════════════════════════════════
// build_merkle_root – correctness
// ══════════════════════════════════════════════════════════════════════════════

/// build_merkle_root of an empty leaf set returns SHA-256(b"").
#[test]
fn build_merkle_root_empty_returns_sha256_of_empty() {
    let env = make_env();
    let leaves = soroban_sdk::Vec::new(&env);
    let root = build_merkle_root(&env, &leaves);
    let expected = env.crypto().sha256(&Bytes::new(&env));
    assert_eq!(root, expected);
}

/// build_merkle_root of a single leaf returns the leaf hash directly.
#[test]
fn build_merkle_root_single_leaf_is_leaf_hash() {
    let env = make_env();
    let holder = Address::generate(&env);
    let entries = [(holder.clone(), 5_000u32)];
    let leaves = canonical_leaves(&env, &entries).unwrap();
    let root = build_merkle_root(&env, &leaves);
    let expected = expected_leaf_hash(&env, &holder, 5_000);
    assert_eq!(root, expected);
}

/// build_merkle_root of two leaves equals hash_node(leaf_a, leaf_b) with
/// the sorted-pair rule applied.
#[test]
fn build_merkle_root_two_leaves_is_node_of_two_leaf_hashes() {
    let env = make_env();
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);

    // canonical_leaves sorts them; we use the canonical output.
    let entries = [(h1.clone(), 3_000u32), (h2.clone(), 7_000u32)];
    let leaves = canonical_leaves(&env, &entries).unwrap();
    let root = build_merkle_root(&env, &leaves);

    let l0 = leaves.get(0).unwrap();
    let l1 = leaves.get(1).unwrap();
    let h_l0 = expected_leaf_hash(&env, &l0.holder, l0.share_bps);
    let h_l1 = expected_leaf_hash(&env, &l1.holder, l1.share_bps);

    // Manually compute sorted-pair node.
    let lb0: Bytes = h_l0.clone().into();
    let lb1: Bytes = h_l1.clone().into();
    let (lo, hi) = if lb0 > lb1 { (lb1, lb0) } else { (lb0, lb1) };
    let mut node_input = Bytes::new(&env);
    node_input.push_back(0x01u8);
    node_input.append(&lo);
    node_input.append(&hi);
    let expected_root = env.crypto().sha256(&node_input);

    assert_eq!(root, expected_root);
}

/// build_merkle_root output is independent of how entries were originally ordered
/// (relies on canonical_leaves having normalised the order first).
#[test]
fn build_merkle_root_same_for_any_input_permutation() {
    let env = make_env();
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);
    let h3 = Address::generate(&env);

    let entries_fwd = [(h1.clone(), 100u32), (h2.clone(), 200u32), (h3.clone(), 300u32)];
    let entries_rev = [(h3.clone(), 300u32), (h2.clone(), 200u32), (h1.clone(), 100u32)];

    let leaves_fwd = canonical_leaves(&env, &entries_fwd).unwrap();
    let leaves_rev = canonical_leaves(&env, &entries_rev).unwrap();

    let root_fwd = build_merkle_root(&env, &leaves_fwd);
    let root_rev = build_merkle_root(&env, &leaves_rev);

    assert_eq!(root_fwd, root_rev, "roots must match regardless of input order");
}

/// build_merkle_root is sensitive to share_bps: changing a value changes the root.
#[test]
fn build_merkle_root_changes_when_share_bps_changes() {
    let env = make_env();
    let holder = Address::generate(&env);

    let leaves_a = canonical_leaves(&env, &[(holder.clone(), 5_000u32)]).unwrap();
    let leaves_b = canonical_leaves(&env, &[(holder.clone(), 5_001u32)]).unwrap();

    let root_a = build_merkle_root(&env, &leaves_a);
    let root_b = build_merkle_root(&env, &leaves_b);

    assert_ne!(root_a, root_b, "different share_bps must produce different roots");
}

// ══════════════════════════════════════════════════════════════════════════════
// build_merkle_root – determinism
// ══════════════════════════════════════════════════════════════════════════════

/// Two calls with identical inputs return identical roots.
#[test]
fn build_merkle_root_is_deterministic() {
    let env = make_env();
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);
    let entries = [(h1.clone(), 4_000u32), (h2.clone(), 6_000u32)];

    let leaves1 = canonical_leaves(&env, &entries).unwrap();
    let leaves2 = canonical_leaves(&env, &entries).unwrap();

    let root1 = build_merkle_root(&env, &leaves1);
    let root2 = build_merkle_root(&env, &leaves2);

    assert_eq!(root1, root2);
}

/// Adding a new holder to the set changes the root.
#[test]
fn build_merkle_root_changes_when_holder_added() {
    let env = make_env();
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);

    let entries_one = [(h1.clone(), 5_000u32)];
    let entries_two = [(h1.clone(), 5_000u32), (h2.clone(), 5_000u32)];

    let leaves_one = canonical_leaves(&env, &entries_one).unwrap();
    let leaves_two = canonical_leaves(&env, &entries_two).unwrap();

    let root_one = build_merkle_root(&env, &leaves_one);
    let root_two = build_merkle_root(&env, &leaves_two);

    assert_ne!(root_one, root_two, "different holder sets must produce different roots");
}

/// Replacing one holder address (keeping share_bps the same) changes the root.
#[test]
fn build_merkle_root_changes_when_holder_replaced() {
    let env = make_env();
    let h_old = Address::generate(&env);
    let h_new = Address::generate(&env);

    let leaves_old = canonical_leaves(&env, &[(h_old.clone(), 5_000u32)]).unwrap();
    let leaves_new = canonical_leaves(&env, &[(h_new.clone(), 5_000u32)]).unwrap();

    let root_old = build_merkle_root(&env, &leaves_old);
    let root_new = build_merkle_root(&env, &leaves_new);

    assert_ne!(root_old, root_new, "different holders must produce different roots");
}

// ══════════════════════════════════════════════════════════════════════════════
// Security / property checks
// ══════════════════════════════════════════════════════════════════════════════

/// Leaf hashes must not equal internal-node hashes even when the same holder
/// data is hashed twice (domain separation via 0x00/0x01 prefix).
///
/// Specifically: hash_leaf(h, s) must differ from a node hash whose children
/// both equal that leaf hash.  Without domain separation these would collide.
#[test]
fn leaf_hash_differs_from_node_hash_of_same_inputs() {
    let env = make_env();
    let holder = Address::generate(&env);

    // Build a two-leaf tree where both leaves are identical in *value* (same
    // holder, same bps).  This is normally disallowed by canonical_leaves, so
    // we construct the scenario at the hash level directly.
    let leaf_hash = expected_leaf_hash(&env, &holder, 5_000);

    // A node whose both children are leaf_hash.
    let lb: Bytes = leaf_hash.clone().into();
    let mut node_input = Bytes::new(&env);
    node_input.push_back(0x01u8);
    node_input.append(&lb);
    node_input.append(&lb);
    let node_hash = env.crypto().sha256(&node_input);

    // They must be different — domain prefixes ensure this.
    assert_ne!(
        leaf_hash, node_hash,
        "leaf and node hashes must be domain-separated"
    );
}

/// canonical_leaves prevents double-counting: a holder cannot appear twice even
/// with different share_bps.
#[test]
fn canonical_leaves_no_double_counting() {
    let env = make_env();
    let holder = Address::generate(&env);

    // Attempt to add the same holder with two different allocations.
    let result = canonical_leaves(
        &env,
        &[(holder.clone(), 3_000u32), (holder.clone(), 7_000u32)],
    );
    assert_eq!(
        result,
        Err(MerkleError::DuplicateAddress),
        "duplicate holder must be rejected to prevent double-counting"
    );
}

/// The Merkle root of N holders changes when any single holder's share_bps is
/// mutated, even by 1 bps.  This ensures the root is a commitment to exact bps.
#[test]
fn merkle_root_commits_to_exact_share_bps() {
    let env = make_env();
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);
    let h3 = Address::generate(&env);

    let entries_orig = [
        (h1.clone(), 3_333u32),
        (h2.clone(), 3_333u32),
        (h3.clone(), 3_334u32),
    ];
    let mut entries_mutated = entries_orig.clone();
    // Mutate h2's share by 1 bps.
    entries_mutated[1].1 = 3_334;

    let leaves_orig = canonical_leaves(&env, &entries_orig).unwrap();
    let leaves_mutated = canonical_leaves(&env, &entries_mutated).unwrap();

    let root_orig = build_merkle_root(&env, &leaves_orig);
    let root_mutated = build_merkle_root(&env, &leaves_mutated);

    assert_ne!(
        root_orig, root_mutated,
        "1-bps change must produce a different root"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Integration: canonical ordering matches prove_distribution tie-break rule
// ══════════════════════════════════════════════════════════════════════════════

/// The canonical ordering (ascending XDR bytes) is consistent with the tie-break
/// ordering documented for `prove_distribution_for_period`: when two holders have
/// the same share_bps the one with lexicographically smaller XDR bytes sorts first.
///
/// This test verifies that `canonical_leaves` produces that same ordering.
#[test]
fn canonical_leaves_ordering_consistent_with_prove_distribution_tie_break() {
    let env = make_env();

    // Generate two holders and determine which has the smaller XDR encoding.
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let a_xdr = a.to_xdr(&env);
    let b_xdr = b.to_xdr(&env);

    // Same share_bps for both — pure tie-break scenario.
    let entries = [(a.clone(), 5_000u32), (b.clone(), 5_000u32)];
    let leaves = canonical_leaves(&env, &entries).unwrap();

    // The leaf with the smaller XDR must come first.
    let first_xdr = leaves.get(0).unwrap().holder_xdr;
    let second_xdr = leaves.get(1).unwrap().holder_xdr;

    // Verify first_xdr <= second_xdr lexicographically.
    let mut first_is_lte = true;
    for i in 0..first_xdr.len().min(second_xdr.len()) {
        let fv = first_xdr.get(i).unwrap_or(0);
        let sv = second_xdr.get(i).unwrap_or(0);
        if fv < sv {
            break; // first < second: definitely <=
        }
        if fv > sv {
            first_is_lte = false;
            break;
        }
    }
    assert!(
        first_is_lte,
        "canonical_leaves must place the smaller-XDR address first"
    );
}

/// Calling `canonical_leaves` then `build_merkle_root` on a two-holder snapshot
/// produces the same root whether entries are presented in sorted or unsorted order.
///
/// This is the end-to-end integration check that mirrors the off-chain indexer
/// workflow described in the module documentation.
#[test]
fn end_to_end_root_stable_across_orderings() {
    let env = make_env();
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);

    // Forward order
    let fwd = canonical_leaves(&env, &[(h1.clone(), 2_000u32), (h2.clone(), 8_000u32)]).unwrap();
    // Reverse order
    let rev = canonical_leaves(&env, &[(h2.clone(), 8_000u32), (h1.clone(), 2_000u32)]).unwrap();

    let root_fwd = build_merkle_root(&env, &fwd);
    let root_rev = build_merkle_root(&env, &rev);

    assert_eq!(
        root_fwd, root_rev,
        "root must be identical regardless of presentation order"
    );
}
