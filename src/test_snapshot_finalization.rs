#![cfg(test)]

use crate::{DataKey, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
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

    client.register_offering(&issuer, &symbol_short!("def"), &token, &5_000, &payout_asset, &0);
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

    client.set_snapshot_config(&issuer, &symbol_short!("def"), &token, &true);

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);
    let holders = soroban_sdk::vec![&env, (holder1.clone(), 5_000u32), (holder2.clone(), 5_000u32)];
    let content_hash =
        compute_snapshot_content_hash(&env, &[(holder1.clone(), 5_000), (holder2.clone(), 5_000)]);

    client.commit_snapshot(&issuer, &symbol_short!("def"), &token, &1, &content_hash);
    client.apply_snapshot_shares(&issuer, &symbol_short!("def"), &token, &1, &0, &holders);
    client.finalize_snapshot(&issuer, &symbol_short!("def"), &token, &1);
}

#[test]
fn finalize_snapshot_fails_when_hash_mismatch() {
    let (env, client, issuer, token, _payout_asset, _contract_id) = setup_snapshot_test();

    client.set_snapshot_config(&issuer, &symbol_short!("def"), &token, &true);

    let holder = Address::generate(&env);
    let holders = soroban_sdk::vec![&env, (holder.clone(), 5_000u32)];
    let content_hash = BytesN::random(&env);

    client.commit_snapshot(&issuer, &symbol_short!("def"), &token, &1, &content_hash);
    client.apply_snapshot_shares(&issuer, &symbol_short!("def"), &token, &1, &0, &holders);

    let result = client.try_finalize_snapshot(&issuer, &symbol_short!("def"), &token, &1);
    assert!(result.is_err());
    assert!(matches!(result.err(), Some(Ok(RevoraError::SnapshotHashMismatch))));
}

#[test]
fn deposit_revenue_with_snapshot_fails_when_finalization_required_and_unfinalized() {
    let (env, client, issuer, token, payout_asset, _contract_id) = setup_snapshot_test();

    client.set_snapshot_config(&issuer, &symbol_short!("def"), &token, &true);

    let admin = Address::generate(&env);
    env.as_contract(&_contract_id, || {
        env.storage().persistent().set(&DataKey::Admin, &admin);
    });
    client.set_snapshot_finalization(&admin, &true);

    let holder = Address::generate(&env);
    let holders = soroban_sdk::vec![&env, (holder.clone(), 5_000u32)];
    let content_hash = compute_snapshot_content_hash(&env, &[(holder.clone(), 5_000)]);

    client.commit_snapshot(&issuer, &symbol_short!("def"), &token, &1, &content_hash);
    client.apply_snapshot_shares(&issuer, &symbol_short!("def"), &token, &1, &0, &holders);

    let result = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout_asset,
        &10_000,
        &1,
        &1,
    );

    assert!(result.is_err());
    assert!(matches!(result.err(), Some(Ok(RevoraError::SnapshotNotFinalized))));
}
