//! Tests for the batched multi-proof Merkle verifier (`verify_multi_proof`).
//!
//! Coverage:
//!
//! | Area                                          | Tests |
//! |-----------------------------------------------|-------|
//! | helper — empty leaves / no flags              | 2     |
//! | helper — single leaf via multi-proof          | 2     |
//! | helper — two-leaf multi-proof                 | 2     |
//! | helper — three-leaf multi-proof               | 1     |
//! | helper — invalid flag / ordering rejection    | 3     |
//! | helper — depth-bound rejection                | 2     |
//! | helper — flags=0 (proof sibling)              | 1     |
//! | contract entrypoint — happy path              | 2     |
//! | contract entrypoint — error mapping           | 2     |
//! | gas comparison — single vs multi-proof        | 1     |

#![cfg(test)]

use crate::merkle_helpers::{
    build_merkle_root, canonical_leaves, verify_merkle_proof as helper_verify,
    verify_multi_proof as helper_multi_verify, MerkleError, MAX_PROOF_DEPTH,
};
use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short, testutils::Address as _, testutils::BytesN as _, xdr::ToXdr, Address, Bytes,
    BytesN, Env, Vec,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn make_client(env: &Env) -> RevoraRevenueShareClient<'static> {
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &contract_id)
}

/// Leaf hash matching the on-chain construction:
///   `SHA-256( 0x00 || holder_xdr || share_bps_xdr )`
fn make_leaf_hash(env: &Env, holder: &Address, share_bps: u32) -> BytesN<32> {
    let mut input = Bytes::new(env);
    input.push_back(0x00u8);
    input.append(&holder.to_xdr(env));
    input.append(&share_bps.to_xdr(env));
    env.crypto().sha256(&input)
}

/// Build a multi-proof for two leaves generated from random holders.
///
/// Returns `(root, leaves_vec, proof_vec, flags_vec)` suitable for
/// `verify_multi_proof`.  All leaves must verify against `root`.
fn build_two_leaf_multi_proof(env: &Env) -> (BytesN<32>, Vec<BytesN<32>>, Vec<BytesN<32>>, Vec<u32>) {
    let h1 = Address::generate(env);
    let h2 = Address::generate(env);
    let entries = [(h1.clone(), 4_000u32), (h2.clone(), 6_000u32)];
    let leaves = canonical_leaves(env, &entries).unwrap();
    let root = build_merkle_root(env, &leaves);

    let l0 = leaves.get(0).unwrap();
    let l1 = leaves.get(1).unwrap();
    let h_l0 = make_leaf_hash(env, &l0.holder, l0.share_bps);
    let h_l1 = make_leaf_hash(env, &l1.holder, l1.share_bps);

    let mut leaves_vec: Vec<BytesN<32>> = Vec::new(env);
    leaves_vec.push_back(h_l0.clone());
    leaves_vec.push_back(h_l1.clone());

    // Multi-proof for two adjacent leaves: one flag (both children from leaves).
    let mut flags: Vec<u32> = Vec::new(env);
    flags.push_back(1);

    let proof: Vec<BytesN<32>> = Vec::new(env);

    (root, leaves_vec, proof, flags)
}

// ══════════════════════════════════════════════════════════════════════════════
// helper verify_multi_proof — empty leaves / no flags
// ══════════════════════════════════════════════════════════════════════════════

/// Empty leaves, empty proof, no flags → Ok(true) when root == SHA-256(b"").
#[test]
fn helper_empty_all_ok_true() {
    let env = make_env();
    let empty_root = env.crypto().sha256(&Bytes::new(&env));
    let leaves: Vec<BytesN<32>> = Vec::new(&env);
    let proof: Vec<BytesN<32>> = Vec::new(&env);
    let flags: Vec<u32> = Vec::new(&env);
    assert_eq!(
        helper_multi_verify(&env, empty_root, &leaves, &proof, &flags),
        Ok(true)
    );
}

/// Empty leaves, empty proof, no flags → Ok(false) when root != SHA-256(b"").
#[test]
fn helper_empty_all_wrong_root_ok_false() {
    let env = make_env();
    let wrong_root = BytesN::random(&env);
    let leaves: Vec<BytesN<32>> = Vec::new(&env);
    let proof: Vec<BytesN<32>> = Vec::new(&env);
    let flags: Vec<u32> = Vec::new(&env);
    assert_eq!(
        helper_multi_verify(&env, wrong_root, &leaves, &proof, &flags),
        Ok(false)
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// helper verify_multi_proof — single leaf via multi-proof
// ══════════════════════════════════════════════════════════════════════════════

/// Single leaf, empty proof, no flags → Ok(true) when leaf == root.
#[test]
fn helper_single_leaf_no_flags_matches_root() {
    let env = make_env();
    let holder = Address::generate(&env);
    let leaf = make_leaf_hash(&env, &holder, 5_000);
    let mut leaves: Vec<BytesN<32>> = Vec::new(&env);
    leaves.push_back(leaf.clone());
    let proof: Vec<BytesN<32>> = Vec::new(&env);
    let flags: Vec<u32> = Vec::new(&env);
    assert_eq!(
        helper_multi_verify(&env, leaf, &leaves, &proof, &flags),
        Ok(true)
    );
}

/// Single leaf with wrong root → Ok(false).
#[test]
fn helper_single_leaf_wrong_root_ok_false() {
    let env = make_env();
    let holder = Address::generate(&env);
    let leaf = make_leaf_hash(&env, &holder, 5_000);
    let mut leaves: Vec<BytesN<32>> = Vec::new(&env);
    leaves.push_back(leaf);
    let wrong_root = BytesN::random(&env);
    let proof: Vec<BytesN<32>> = Vec::new(&env);
    let flags: Vec<u32> = Vec::new(&env);
    assert_eq!(
        helper_multi_verify(&env, wrong_root, &leaves, &proof, &flags),
        Ok(false)
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// helper verify_multi_proof — two-leaf multi-proof
// ══════════════════════════════════════════════════════════════════════════════

/// Two-leaf multi-proof verifies both leaves against the real Merkle root.
#[test]
fn helper_two_leaf_multi_proof_ok_true() {
    let env = make_env();
    let (root, leaves, proof, flags) = build_two_leaf_multi_proof(&env);
    assert_eq!(
        helper_multi_verify(&env, root, &leaves, &proof, &flags),
        Ok(true)
    );
}

/// Two-leaf multi-proof with tampered root → Ok(false).
#[test]
fn helper_two_leaf_tampered_root_ok_false() {
    let env = make_env();
    let (_root, leaves, proof, flags) = build_two_leaf_multi_proof(&env);
    let wrong_root = BytesN::random(&env);
    assert_eq!(
        helper_multi_verify(&env, wrong_root, &leaves, &proof, &flags),
        Ok(false)
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// helper verify_multi_proof — three-leaf multi-proof
// ══════════════════════════════════════════════════════════════════════════════

/// Three-leaf multi-proof: build a 3-leaf tree and verify all three leaves.
///
/// Tree structure for 3 leaves A, B, C:
///   level 0: hash(A,B), C
///   level 1: hash(hash(A,B), C) → root
///
/// Multi-proof flags: [1, 1]
///   Step 0: pair A and B (flag=1: both from leaves) → hash(A,B)
///   Step 1: pair hash(A,B) with C
///     - first child: hash(A,B) comes from computed hashes
///     - second child: C is a leaf (flag=1)
#[test]
fn helper_three_leaf_multi_proof_ok_true() {
    let env = make_env();
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);
    let h3 = Address::generate(&env);

    let entries = [(h1.clone(), 3_000u32), (h2.clone(), 3_000u32), (h3.clone(), 4_000u32)];
    let leaves = canonical_leaves(&env, &entries).unwrap();
    let root = build_merkle_root(&env, &leaves);

    let l0 = leaves.get(0).unwrap();
    let l1 = leaves.get(1).unwrap();
    let l2 = leaves.get(2).unwrap();
    let h_l0 = make_leaf_hash(&env, &l0.holder, l0.share_bps);
    let h_l1 = make_leaf_hash(&env, &l1.holder, l1.share_bps);
    let h_l2 = make_leaf_hash(&env, &l2.holder, l2.share_bps);

    let mut leaves_vec: Vec<BytesN<32>> = Vec::new(&env);
    leaves_vec.push_back(h_l0.clone());
    leaves_vec.push_back(h_l1.clone());
    leaves_vec.push_back(h_l2.clone());

    // Step 0: pair A and B → both children are leaves (flags[0]=1)
    // Step 1: pair hash(A,B) with C
    //   - first child: hash(A,B) comes from computed hashes
    //   - second child: C is a leaf (flags[1]=1)
    let mut flags: Vec<u32> = Vec::new(&env);
    flags.push_back(1);
    flags.push_back(1);

    let proof: Vec<BytesN<32>> = Vec::new(&env);

    assert_eq!(
        helper_multi_verify(&env, root, &leaves_vec, &proof, &flags),
        Ok(true)
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// helper verify_multi_proof — ordering / flag rejection
// ══════════════════════════════════════════════════════════════════════════════

/// Structural invariant violation: leaves without flags → InconsistentLeafOrdering.
#[test]
fn helper_unconsumed_leaves_err_inconsistent_ordering() {
    let env = make_env();
    let holder = Address::generate(&env);
    let leaf = make_leaf_hash(&env, &holder, 5_000);
    let mut leaves: Vec<BytesN<32>> = Vec::new(&env);
    leaves.push_back(leaf);
    let proof: Vec<BytesN<32>> = Vec::new(&env);
    let flags: Vec<u32> = Vec::new(&env);
    let root = BytesN::random(&env);
    assert_eq!(
        helper_multi_verify(&env, root, &leaves, &proof, &flags),
        Err(MerkleError::InconsistentLeafOrdering)
    );
}

/// Structural invariant violation: too many flags for the leaf+proof count.
#[test]
fn helper_unconsumed_proof_err_inconsistent_ordering() {
    let env = make_env();
    let holder = Address::generate(&env);
    let leaf = make_leaf_hash(&env, &holder, 5_000);
    let mut leaves: Vec<BytesN<32>> = Vec::new(&env);
    leaves.push_back(leaf);
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    proof.push_back(BytesN::random(&env));
    // 1 leaf + 1 proof = 2 inputs, needs 1 flag (2+1=3 ≠ 1+1=2 for 0 flags and wrong for 2 flags).
    // flags.len()=1, leaves.len()+proof.len()=2 → 1+1=2==2, passes invariant.
    // But the flag=1 means second child from leaves/hashes, not proof → proof unconsumed.
    let mut flags: Vec<u32> = Vec::new(&env);
    flags.push_back(1);
    let root = BytesN::random(&env);
    assert_eq!(
        helper_multi_verify(&env, root, &leaves, &proof, &flags),
        Err(MerkleError::InconsistentLeafOrdering)
    );
}

/// Inconsistent leaf ordering: leaves provided in swapped order still fail.
#[test]
fn helper_swapped_leaf_order_ok_false_or_err() {
    let env = make_env();
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);
    let entries = [(h1.clone(), 4_000u32), (h2.clone(), 6_000u32)];
    let leaves = canonical_leaves(&env, &entries).unwrap();
    let root = build_merkle_root(&env, &leaves);

    let l0 = leaves.get(0).unwrap();
    let l1 = leaves.get(1).unwrap();
    let h_l0 = make_leaf_hash(&env, &l0.holder, l0.share_bps);
    let h_l1 = make_leaf_hash(&env, &l1.holder, l1.share_bps);

    // Swap the leaf order: put l1 first instead of l0.
    let mut leaves_vec: Vec<BytesN<32>> = Vec::new(&env);
    leaves_vec.push_back(h_l1);
    leaves_vec.push_back(h_l0);

    let mut flags: Vec<u32> = Vec::new(&env);
    flags.push_back(1);

    let proof: Vec<BytesN<32>> = Vec::new(&env);

    let result = helper_multi_verify(&env, root, &leaves_vec, &proof, &flags);
    // With sorted-pair hashing the root mismatch is possible but not guaranteed
    // (depends on hash values); the key property is that swapped leaves do NOT
    // produce Ok(true).
    assert_ne!(result, Ok(true), "swapped leaf order must not pass verification");
}

// ══════════════════════════════════════════════════════════════════════════════
// helper verify_multi_proof — depth bound
// ══════════════════════════════════════════════════════════════════════════════

/// Multi-proof with proof.len() > MAX_PROOF_DEPTH → ProofTooDeep.
#[test]
fn helper_multi_proof_exceeds_depth_err_proof_too_deep() {
    let env = make_env();
    let root = BytesN::random(&env);
    let leaves: Vec<BytesN<32>> = Vec::new(&env);
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    for _ in 0..=MAX_PROOF_DEPTH {
        proof.push_back(BytesN::random(&env));
    }
    let flags: Vec<u32> = Vec::new(&env);
    assert_eq!(
        helper_multi_verify(&env, root, &leaves, &proof, &flags),
        Err(MerkleError::ProofTooDeep)
    );
}

/// Multi-proof at exactly MAX_PROOF_DEPTH is accepted (with valid flags).
#[test]
fn helper_multi_proof_at_max_depth_ok() {
    let env = make_env();
    let holder = Address::generate(&env);
    let leaf = make_leaf_hash(&env, &holder, 5_000);
    let mut leaves: Vec<BytesN<32>> = Vec::new(&env);
    leaves.push_back(leaf);

    // Build a proof chain of exactly MAX_PROOF_DEPTH siblings.
    // Use 1 leaf + MAX_PROOF_DEPTH proof elements → MAX_PROOF_DEPTH+1 inputs
    // → MAX_PROOF_DEPTH flags. All flags=0 (second child from proof).
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    let mut flags: Vec<u32> = Vec::new(&env);
    for _ in 0..MAX_PROOF_DEPTH {
        proof.push_back(BytesN::random(&env));
        flags.push_back(0);
    }
    // Result will be Ok(true) or Ok(false) depending on the random root,
    // but crucially NOT Err(ProofTooDeep).
    let root = BytesN::random(&env);
    let result = helper_multi_verify(&env, root, &leaves, &proof, &flags);
    assert!(result.is_ok(), "proof at MAX_PROOF_DEPTH must not be rejected");
}

/// Multi-proof with a single leaf and one external proof sibling (flag=0)
/// correctly reconstructs a known root.
#[test]
fn helper_single_leaf_with_proof_sibling_ok_true() {
    let env = make_env();
    let holder = Address::generate(&env);
    let leaf = make_leaf_hash(&env, &holder, 5_000);
    let sibling = BytesN::random(&env);

    // Compute the expected root: hash_node(leaf, sibling)
    // Replicate the sorted-pair logic used by hash_node.
    let lb: Bytes = leaf.clone().into();
    let sb: Bytes = sibling.clone().into();
    let (lo, hi) = if lb > sb { (sb, lb) } else { (lb, sb) };
    let mut input = Bytes::new(&env);
    input.push_back(0x01u8);
    input.append(&lo);
    input.append(&hi);
    let expected_root = env.crypto().sha256(&input);

    let mut leaves: Vec<BytesN<32>> = Vec::new(&env);
    leaves.push_back(leaf);
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    proof.push_back(sibling);
    let mut flags: Vec<u32> = Vec::new(&env);
    flags.push_back(0);

    assert_eq!(
        helper_multi_verify(&env, expected_root, &leaves, &proof, &flags),
        Ok(true)
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Contract entrypoint — verify_multi_proof
// ══════════════════════════════════════════════════════════════════════════════

/// Contract entrypoint: two-leaf multi-proof → Ok(Ok(true)).
#[test]
fn contract_two_leaf_multi_proof_ok_true() {
    let env = make_env();
    let client = make_client(&env);
    let caller = Address::generate(&env);
    let (root, leaves, proof, flags) = build_two_leaf_multi_proof(&env);
    let result = client.try_verify_multi_proof(&caller, &root, &leaves, &proof, &flags);
    assert_eq!(result, Ok(Ok(true)));
}

/// Contract entrypoint: wrong root → Ok(Ok(false)).
#[test]
fn contract_wrong_root_ok_false() {
    let env = make_env();
    let client = make_client(&env);
    let caller = Address::generate(&env);
    let (_root, leaves, proof, flags) = build_two_leaf_multi_proof(&env);
    let wrong_root = BytesN::random(&env);
    let result = client.try_verify_multi_proof(&caller, &wrong_root, &leaves, &proof, &flags);
    assert_eq!(result, Ok(Ok(false)));
}

/// Contract entrypoint: proof > MAX_PROOF_DEPTH → Err(ProofTooDeep).
#[test]
fn contract_multi_proof_exceeds_depth_err_proof_too_deep() {
    let env = make_env();
    let client = make_client(&env);
    let caller = Address::generate(&env);
    let root = BytesN::random(&env);
    let leaves: Vec<BytesN<32>> = Vec::new(&env);
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    for _ in 0..=MAX_PROOF_DEPTH {
        proof.push_back(BytesN::random(&env));
    }
    let flags: Vec<u32> = Vec::new(&env);
    let result = client.try_verify_multi_proof(&caller, &root, &leaves, &proof, &flags);
    assert_eq!(result.err(), Some(Ok(RevoraError::ProofTooDeep)));
}

/// Contract entrypoint: oversized proof emits proof_reject_depth event.
#[test]
fn contract_multi_proof_reject_emits_event() {
    let env = make_env();
    let client = make_client(&env);
    let caller = Address::generate(&env);
    let root = BytesN::random(&env);
    let leaves: Vec<BytesN<32>> = Vec::new(&env);
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    for _ in 0..=MAX_PROOF_DEPTH {
        proof.push_back(BytesN::random(&env));
    }
    let flags: Vec<u32> = Vec::new(&env);

    let _ = client.try_verify_multi_proof(&caller, &root, &leaves, &proof, &flags);

    let all_events = env.events().all();
    let reject_sym = symbol_short!("prf_rej_d");
    let found = all_events.iter().any(|(_cid, topics, _data)| {
        topics.get(0) == Some(soroban_sdk::Val::from(reject_sym))
    });
    assert!(found, "prf_rej_d event must be emitted for oversized multi-proof");
}

// ══════════════════════════════════════════════════════════════════════════════
// Gas comparison — single-proof loop vs multi-proof
// ══════════════════════════════════════════════════════════════════════════════

/// Verify that a multi-proof call verifies N=2 leaves in a single invocation
/// whereas the single-proof approach would require 2 separate calls.
/// This test asserts correctness equivalence (both approaches agree).
#[test]
fn gas_comparison_two_leaves_equivalence() {
    let env = make_env();
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);
    let entries = [(h1.clone(), 4_000u32), (h2.clone(), 6_000u32)];
    let leaves = canonical_leaves(&env, &entries).unwrap();
    let root = build_merkle_root(&env, &leaves);

    let l0 = leaves.get(0).unwrap();
    let l1 = leaves.get(1).unwrap();
    let h_l0 = make_leaf_hash(&env, &l0.holder, l0.share_bps);
    let h_l1 = make_leaf_hash(&env, &l1.holder, l1.share_bps);

    // ── Single-proof loop: 2 independent verify_merkle_proof calls ──────
    let mut proof_l0: Vec<BytesN<32>> = Vec::new(&env);
    proof_l0.push_back(h_l1.clone());
    let single_ok0 = helper_verify(&env, h_l0.clone(), root.clone(), &proof_l0).unwrap();

    let mut proof_l1: Vec<BytesN<32>> = Vec::new(&env);
    proof_l1.push_back(h_l0.clone());
    let single_ok1 = helper_verify(&env, h_l1.clone(), root.clone(), &proof_l1).unwrap();

    assert!(single_ok0, "single-proof for leaf[0] must pass");
    assert!(single_ok1, "single-proof for leaf[1] must pass");

    // ── Multi-proof: single call verifies both leaves ───────────────────
    let mut multi_leaves: Vec<BytesN<32>> = Vec::new(&env);
    multi_leaves.push_back(h_l0);
    multi_leaves.push_back(h_l1);

    let mut flags: Vec<u32> = Vec::new(&env);
    flags.push_back(1);

    let multi_proof: Vec<BytesN<32>> = Vec::new(&env);
    let multi_ok = helper_multi_verify(&env, root, &multi_leaves, &multi_proof, &flags).unwrap();

    assert!(multi_ok, "multi-proof for both leaves must pass");
    // Both approaches agree: both return true with correct inputs.
}
