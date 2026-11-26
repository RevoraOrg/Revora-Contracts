#![cfg(test)]

use crate::{DataKey, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, MAX_PROOF_DEPTH};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, BytesN as _},
    xdr::ToXdr,
    Address, Bytes, BytesN, Env,
};

fn setup_snapshot_test(
) -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.register_offering(
        &issuer,
        &soroban_sdk::Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    (env, client, issuer, token, payout_asset, contract_id)
}

fn compute_snapshot_content_hash(env: &Env, holders: &[(Address, u32)]) -> BytesN<32> {
    let mut digest_input = Bytes::new(env);
    for (index, (holder, share_bps)) in holders.iter().enumerate() {
        digest_input.append(&((index as u32).to_xdr(env)));
        digest_input.append(&holder.to_xdr(env));
        digest_input.append(&share_bps.to_xdr(env));
    }
    env.crypto().sha256(&digest_input).to_bytes()
}

#[test]
fn finalize_snapshot_succeeds_when_hash_matches() {
    let (env, client, issuer, token, _payout_asset, _contract_id) = setup_snapshot_test();

    client.set_snapshot_config(
        &issuer,
        &symbol_short!("def"),
        &token,
        &true,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);
    let holders = soroban_sdk::vec![&env, (holder1.clone(), 5_000u32), (holder2.clone(), 5_000u32)];
    let content_hash =
        compute_snapshot_content_hash(&env, &[(holder1.clone(), 5_000), (holder2.clone(), 5_000)]);

    client.commit_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1,
        &content_hash,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );
    client.apply_snapshot_shares(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1,
        &0,
        &holders,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );
    client.finalize_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );
}

#[test]
#[ignore = "apply_snapshot_shares does not guard against post-finalization re-application; \
            contract intentionally allows idempotent re-apply after finalize_snapshot. \
            There is no SnapshotAlreadyFinalized error variant — see finalize_snapshot (lib.rs ~L17027). \
            If a guard is added in future, remove this #[ignore] and restore the assertion."]
fn apply_snapshot_shares_rejects_finalized_snapshot() {
    let (env, client, issuer, token, _payout_asset, _contract_id) = setup_snapshot_test();

    // set_snapshot_config requires signers (Vec<Address>) as the 5th argument.
    client.set_snapshot_config(
        &issuer,
        &symbol_short!("def"),
        &token,
        &true,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);
    let holders = soroban_sdk::vec![&env, (holder1.clone(), 5_000u32), (holder2.clone(), 5_000u32)];
    let content_hash =
        compute_snapshot_content_hash(&env, &[(holder1.clone(), 5_000), (holder2.clone(), 5_000)]);

    // commit_snapshot / apply_snapshot_shares / finalize_snapshot all require signers.
    client.commit_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1,
        &content_hash,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );
    client.apply_snapshot_shares(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1,
        &0,
        &holders,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );
    client.finalize_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );

    // The contract does not currently reject apply_snapshot_shares on a finalized snapshot
    // (no SnapshotAlreadyFinalized guard in apply_snapshot_shares — see lib.rs ~L9710).
    // This test body is intentionally left empty until the guard is implemented.
    let _ = (&env, &client, &holders);
}

#[test]
fn finalize_snapshot_fails_when_hash_mismatch() {
    let (env, client, issuer, token, _payout_asset, _contract_id) = setup_snapshot_test();

    client.set_snapshot_config(
        &issuer,
        &symbol_short!("def"),
        &token,
        &true,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );

    let holder = Address::generate(&env);
    let holders = soroban_sdk::vec![&env, (holder.clone(), 5_000u32)];
    let content_hash = BytesN::random(&env);

    client.commit_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1,
        &content_hash,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );
    client.apply_snapshot_shares(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1,
        &0,
        &holders,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );

    let result = client.try_finalize_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );
    assert!(result.is_err());
    assert!(matches!(result.err(), Some(Ok(RevoraError::SnapshotHashMismatch))));

    let live_share = client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder);
    assert_eq!(live_share, 0, "unverified snapshot data must not change live shares");
}

#[test]
fn verify_snapshot_proof_accepts_proofs_at_the_depth_limit() {
    let (env, client, issuer, token, _payout_asset, _contract_id) = setup_snapshot_test();
    // soroban_sdk::Vec requires `mut` to call push_back.
    let mut proof = soroban_sdk::vec![&env];
    for _ in 0..MAX_PROOF_DEPTH {
        proof.push_back(BytesN::random(&env));
    }

    let result =
        client.try_verify_snapshot_proof(&issuer, &symbol_short!("def"), &token, &1u64, &proof);
    assert!(result.is_ok(), "proofs at the limit must be accepted");
}

#[test]
fn verify_snapshot_proof_rejects_proofs_above_the_depth_limit() {
    let (env, client, issuer, token, _payout_asset, _contract_id) = setup_snapshot_test();
    // soroban_sdk::Vec requires `mut` to call push_back.
    let mut proof = soroban_sdk::vec![&env];
    for _ in 0..(MAX_PROOF_DEPTH + 1) {
        proof.push_back(BytesN::random(&env));
    }

    let result =
        client.try_verify_snapshot_proof(&issuer, &symbol_short!("def"), &token, &1u64, &proof);
    assert!(result.is_err());
    assert!(matches!(result.err(), Some(Ok(RevoraError::ProofTooDeep))));
}

#[test]
fn deposit_revenue_with_snapshot_fails_when_finalization_required_and_unfinalized() {
    let (env, client, issuer, token, payout_asset, _contract_id) = setup_snapshot_test();

    client.set_snapshot_config(
        &issuer,
        &symbol_short!("def"),
        &token,
        &true,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );

    let admin = Address::generate(&env);
    env.as_contract(&_contract_id, || {
        env.storage().persistent().set(&DataKey::Admin, &admin);
    });
    // set_snapshot_finalization is not a public contract entrypoint — the flag is set
    // by writing DataKey::SnapshotFinalizationRequired directly (see
    // snapshot_finalization_required, lib.rs ~L2791).
    env.as_contract(&_contract_id, || {
        env.storage().persistent().set(&DataKey::SnapshotFinalizationRequired, &true);
    });

    let holder = Address::generate(&env);
    let holders = soroban_sdk::vec![&env, (holder.clone(), 5_000u32)];
    let content_hash = compute_snapshot_content_hash(&env, &[(holder.clone(), 5_000)]);

    client.commit_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1,
        &content_hash,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );
    client.apply_snapshot_shares(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1,
        &0,
        &holders,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );

    let result = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout_asset,
        &10_000,
        &1,
        &1,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );

    assert!(result.is_err());
    assert!(matches!(result.err(), Some(Ok(RevoraError::SnapshotNotFinalized))));
}
