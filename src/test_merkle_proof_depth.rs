//! Tests for the Merkle proof depth bound (`MAX_PROOF_DEPTH = 32`).
//!
//! Coverage:
//!
//! | Area                                        | Tests |
//! |---------------------------------------------|-------|
//! | `MAX_PROOF_DEPTH` constant value            | 1     |
//! | helper — valid proofs                       | 5     |
//! | helper — depth-bound rejection              | 4     |
//! | contract entrypoint — happy path            | 2     |
//! | contract entrypoint — ProofTooDeep error    | 2     |
//! | event emission — proof_reject_depth         | 2     |
//! | integration — build + verify round-trip     | 2     |

#![cfg(test)]

use crate::merkle_helpers::{
    build_merkle_root, canonical_leaves, verify_merkle_proof as helper_verify, MerkleError,
    MAX_PROOF_DEPTH,
};
use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short, testutils::Address as _, testutils::BytesN as _, testutils::Events as _, Address, BytesN, Env, Vec,
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
    use soroban_sdk::{xdr::ToXdr, Bytes};
    let mut input = Bytes::new(env);
    input.push_back(0x00u8);
    input.append(&holder.to_xdr(env));
    input.append(&share_bps.to_xdr(env));
    env.crypto().sha256(&input)
}

/// Internal-node hash matching the on-chain construction:
///   `SHA-256( 0x01 || min(left, right) || max(left, right) )`
fn make_node_hash(env: &Env, left: &BytesN<32>, right: &BytesN<32>) -> BytesN<32> {
    use soroban_sdk::Bytes;
    let lb: Bytes = left.clone().into();
    let rb: Bytes = right.clone().into();
    let (lo, hi) = if lb > rb { (rb, lb) } else { (lb, rb) };
    let mut input = Bytes::new(env);
    input.push_back(0x01u8);
    input.append(&lo);
    input.append(&hi);
    env.crypto().sha256(&input)
}

/// Build a synthetic proof chain of `depth` sibling hashes.
///
/// Returns `(leaf_hash, root, proof_vec)` where walking `proof_vec` from
/// `leaf_hash` via sorted-pair node hashing reaches exactly `root`.
fn build_proof_chain(env: &Env, depth: u32) -> (BytesN<32>, BytesN<32>, Vec<BytesN<32>>) {
    let holder = Address::generate(env);
    let leaf_hash = make_leaf_hash(env, &holder, 5_000);

    let mut proof: Vec<BytesN<32>> = Vec::new(env);
    let mut current = leaf_hash.clone();

    for _ in 0..depth {
        let sibling = BytesN::random(env);
        proof.push_back(sibling.clone());
        current = make_node_hash(env, &current, &sibling);
    }

    (leaf_hash, current, proof)
}

// ══════════════════════════════════════════════════════════════════════════════
// Constant
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn max_proof_depth_is_32() {
    assert_eq!(MAX_PROOF_DEPTH, 32);
}

// ══════════════════════════════════════════════════════════════════════════════
// helper verify_merkle_proof — valid proofs
// ══════════════════════════════════════════════════════════════════════════════

/// Empty proof, leaf == root → Ok(true).
#[test]
fn helper_empty_proof_leaf_equals_root_ok_true() {
    let env = make_env();
    let holder = Address::generate(&env);
    let leaf = make_leaf_hash(&env, &holder, 5_000);
    let proof: Vec<BytesN<32>> = Vec::new(&env);
    assert_eq!(helper_verify(&env, leaf.clone(), leaf, &proof), Ok(true));
}

/// Empty proof, leaf != root → Ok(false).
#[test]
fn helper_empty_proof_leaf_not_root_ok_false() {
    let env = make_env();
    let holder = Address::generate(&env);
    let leaf = make_leaf_hash(&env, &holder, 5_000);
    let wrong_root = BytesN::random(&env);
    let proof: Vec<BytesN<32>> = Vec::new(&env);
    assert_eq!(helper_verify(&env, leaf, wrong_root, &proof), Ok(false));
}

/// Depth-1 proof with correct sibling → Ok(true).
#[test]
fn helper_depth_1_valid_proof_ok_true() {
    let env = make_env();
    let holder = Address::generate(&env);
    let leaf = make_leaf_hash(&env, &holder, 5_000);
    let sibling = BytesN::random(&env);
    let root = make_node_hash(&env, &leaf, &sibling);
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    proof.push_back(sibling);
    assert_eq!(helper_verify(&env, leaf, root, &proof), Ok(true));
}

/// Depth-2 proof with correct siblings → Ok(true).
#[test]
fn helper_depth_2_valid_proof_ok_true() {
    let env = make_env();
    let (leaf, root, proof) = build_proof_chain(&env, 2);
    assert_eq!(helper_verify(&env, leaf, root, &proof), Ok(true));
}

/// Depth-1 proof with wrong sibling → Ok(false).
#[test]
fn helper_depth_1_tampered_sibling_ok_false() {
    let env = make_env();
    let holder = Address::generate(&env);
    let leaf = make_leaf_hash(&env, &holder, 5_000);
    let real_sibling = BytesN::random(&env);
    let root = make_node_hash(&env, &leaf, &real_sibling);

    // Different sibling — path will not reach root.
    let wrong_sibling = BytesN::random(&env);
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    proof.push_back(wrong_sibling);
    assert_eq!(helper_verify(&env, leaf, root, &proof), Ok(false));
}

// ══════════════════════════════════════════════════════════════════════════════
// helper verify_merkle_proof — depth bound
// ══════════════════════════════════════════════════════════════════════════════

/// Proof of exactly MAX_PROOF_DEPTH (32) → accepted, returns Ok(true).
#[test]
fn helper_proof_at_max_depth_ok_true() {
    let env = make_env();
    let (leaf, root, proof) = build_proof_chain(&env, MAX_PROOF_DEPTH);
    assert_eq!(proof.len(), MAX_PROOF_DEPTH);
    assert_eq!(helper_verify(&env, leaf, root, &proof), Ok(true));
}

/// Proof of MAX_PROOF_DEPTH + 1 (33) → Err(ProofTooDeep).
#[test]
fn helper_proof_one_over_max_depth_err_proof_too_deep() {
    let env = make_env();
    let leaf = BytesN::random(&env);
    let root = BytesN::random(&env);
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    for _ in 0..=MAX_PROOF_DEPTH {
        // produces MAX_PROOF_DEPTH + 1 entries
        proof.push_back(BytesN::random(&env));
    }
    assert_eq!(proof.len(), MAX_PROOF_DEPTH + 1);
    assert_eq!(helper_verify(&env, leaf, root, &proof), Err(MerkleError::ProofTooDeep));
}

/// Proof of depth 100 (well above MAX_PROOF_DEPTH) → Err(ProofTooDeep).
#[test]
fn helper_proof_depth_100_err_proof_too_deep() {
    let env = make_env();
    let leaf = BytesN::random(&env);
    let root = BytesN::random(&env);
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    for _ in 0..100 {
        proof.push_back(BytesN::random(&env));
    }
    assert_eq!(helper_verify(&env, leaf, root, &proof), Err(MerkleError::ProofTooDeep));
}

/// Proof of depth 0 (empty) → accepted (lower boundary).
#[test]
fn helper_proof_depth_zero_accepted() {
    let env = make_env();
    let holder = Address::generate(&env);
    let leaf = make_leaf_hash(&env, &holder, 5_000);
    let proof: Vec<BytesN<32>> = Vec::new(&env);
    // leaf == root is Ok(true); leaf != root is Ok(false) — neither is an error.
    assert!(helper_verify(&env, leaf.clone(), leaf, &proof).is_ok());
}

// ══════════════════════════════════════════════════════════════════════════════
// Contract entrypoint — verify_merkle_proof
// ══════════════════════════════════════════════════════════════════════════════

/// Contract entrypoint: valid proof at MAX_PROOF_DEPTH → Ok(true).
#[test]
fn contract_proof_at_max_depth_ok_true() {
    let env = make_env();
    let client = make_client(&env);
    let caller = Address::generate(&env);
    let (leaf, root, proof) = build_proof_chain(&env, MAX_PROOF_DEPTH);

    let result = client.try_verify_merkle_proof(&caller, &leaf, &root, &proof);
    assert_eq!(result, Ok(Ok(true)));
}

/// Contract entrypoint: valid proof with wrong root → Ok(false).
#[test]
fn contract_valid_depth_wrong_root_ok_false() {
    let env = make_env();
    let client = make_client(&env);
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);
    let leaf = make_leaf_hash(&env, &holder, 5_000);
    let wrong_root = BytesN::random(&env);
    let proof: Vec<BytesN<32>> = Vec::new(&env);

    let result = client.try_verify_merkle_proof(&caller, &leaf, &wrong_root, &proof);
    assert_eq!(result, Ok(Ok(false)));
}

/// Contract entrypoint: proof of MAX_PROOF_DEPTH + 1 → Err(ProofTooDeep).
#[test]
fn contract_proof_one_over_max_depth_err_proof_too_deep() {
    let env = make_env();
    let client = make_client(&env);
    let caller = Address::generate(&env);
    let leaf = BytesN::random(&env);
    let root = BytesN::random(&env);
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    for _ in 0..=MAX_PROOF_DEPTH {
        proof.push_back(BytesN::random(&env));
    }
    assert_eq!(proof.len(), MAX_PROOF_DEPTH + 1);

    let result = client.try_verify_merkle_proof(&caller, &leaf, &root, &proof);
    assert_eq!(result.err(), Some(Ok(RevoraError::ProofTooDeep)));
}

/// Contract entrypoint: proof of depth 100 → Err(ProofTooDeep).
#[test]
fn contract_proof_depth_100_err_proof_too_deep() {
    let env = make_env();
    let client = make_client(&env);
    let caller = Address::generate(&env);
    let leaf = BytesN::random(&env);
    let root = BytesN::random(&env);
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    for _ in 0..100 {
        proof.push_back(BytesN::random(&env));
    }

    let result = client.try_verify_merkle_proof(&caller, &leaf, &root, &proof);
    assert_eq!(result.err(), Some(Ok(RevoraError::ProofTooDeep)));
}

// ══════════════════════════════════════════════════════════════════════════════
// Event emission — proof_reject_depth
// ══════════════════════════════════════════════════════════════════════════════

/// Oversized proof emits a `prf_rej_d` event before returning ProofTooDeep.
#[test]
fn contract_oversized_proof_emits_proof_reject_depth_event() {
    let env = make_env();
    let client = make_client(&env);
    let caller = Address::generate(&env);
    let leaf = BytesN::random(&env);
    let root = BytesN::random(&env);
    let mut proof: Vec<BytesN<32>> = Vec::new(&env);
    for _ in 0..=MAX_PROOF_DEPTH {
        proof.push_back(BytesN::random(&env));
    }

    let _ = client.try_verify_merkle_proof(&caller, &leaf, &root, &proof);

    let all_events = env.events().all();
    assert!(!all_events.is_empty(), "at least one event must be emitted");

    let reject_sym = symbol_short!("prf_rej_d");
    let found = all_events
        .iter()
        .any(|(_cid, topics, _data)| topics.get(0) == Some(soroban_sdk::Val::from(reject_sym)));
    assert!(found, "prf_rej_d event must appear in the event log");
}

/// Valid-depth proof does NOT emit the `prf_rej_d` event.
#[test]
fn contract_valid_depth_proof_no_reject_event() {
    let env = make_env();
    let client = make_client(&env);
    let caller = Address::generate(&env);
    let (leaf, root, proof) = build_proof_chain(&env, MAX_PROOF_DEPTH);

    let _ = client.try_verify_merkle_proof(&caller, &leaf, &root, &proof);

    let all_events = env.events().all();
    let reject_sym = symbol_short!("prf_rej_d");
    let found = all_events
        .iter()
        .any(|(_cid, topics, _data)| topics.get(0) == Some(soroban_sdk::Val::from(reject_sym)));
    assert!(!found, "prf_rej_d must NOT be emitted for valid-depth proofs");
}

// ══════════════════════════════════════════════════════════════════════════════
// Integration — build_merkle_root + verify_merkle_proof round-trip
// ══════════════════════════════════════════════════════════════════════════════

/// Build a real 2-leaf tree, verify each leaf's membership with a correct proof.
#[test]
fn round_trip_two_leaf_tree_both_leaves_verify() {
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

    // Proof for leaf[0]: sibling is leaf[1].
    let mut proof_l0: Vec<BytesN<32>> = Vec::new(&env);
    proof_l0.push_back(h_l1.clone());
    assert!(
        helper_verify(&env, h_l0.clone(), root.clone(), &proof_l0).unwrap(),
        "leaf[0] must verify against root"
    );

    // Proof for leaf[1]: sibling is leaf[0].
    let mut proof_l1: Vec<BytesN<32>> = Vec::new(&env);
    proof_l1.push_back(h_l0);
    assert!(
        helper_verify(&env, h_l1, root, &proof_l1).unwrap(),
        "leaf[1] must verify against root"
    );
}

/// A tampered sibling in an otherwise-valid proof causes verification to fail.
#[test]
fn round_trip_tampered_sibling_fails_verification() {
    let env = make_env();
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);

    let entries = [(h1.clone(), 4_000u32), (h2.clone(), 6_000u32)];
    let leaves = canonical_leaves(&env, &entries).unwrap();
    let root = build_merkle_root(&env, &leaves);

    let l0 = leaves.get(0).unwrap();
    let h_l0 = make_leaf_hash(&env, &l0.holder, l0.share_bps);

    // Wrong sibling — verification must return false, not an error.
    let mut bad_proof: Vec<BytesN<32>> = Vec::new(&env);
    bad_proof.push_back(BytesN::random(&env));

    assert_eq!(
        helper_verify(&env, h_l0, root, &bad_proof),
        Ok(false),
        "tampered sibling must produce Ok(false)"
    );
}
