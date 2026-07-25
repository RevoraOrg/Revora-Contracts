#![cfg(test)]

use crate::{DataKey2, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events},
    Address, BytesN, Env,
};

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset_admin = Address::generate(&env);
    let payout_asset = crate::test_utils::create_token(&env, &payout_asset_admin);
    crate::test_utils::mint_tokens(&env, &payout_asset, &issuer, 1_000_000);

    client.register_offering(&issuer, &symbol_short!("def"), &token, &5_000, &payout_asset, &0);

    (env, client, issuer, token, payout_asset)
}

#[test]
fn holder_jurisdiction_and_allowlist_are_stored_with_audit_event() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);
    let events_before = env.events().all().len();

    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("us"),
    );
    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us"), symbol_short!("ca")],
    );

    assert_eq!(
        client.get_holder_jurisdiction(&issuer, &symbol_short!("def"), &token, &holder),
        Some(symbol_short!("us"))
    );
    assert_eq!(
        client.get_allowed_jurisdictions(&issuer, &symbol_short!("def"), &token),
        soroban_sdk::vec![&env, symbol_short!("us"), symbol_short!("ca")]
    );
    assert!(env.events().all().len() >= events_before + 2);
}

#[test]
fn set_holder_share_rejects_disallowed_jurisdiction_and_emits_audit_event() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("uk"),
    );
    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us")],
    );
    let events_before = env.events().all().len();

    let result =
        client.try_set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &2_500u32);
    assert_eq!(result, Err(Ok(RevoraError::JurisdictionDisallowed)));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder), 0);
    assert!(env.events().all().len() > events_before);
}

#[test]
fn removing_a_jurisdiction_does_not_break_existing_holder_claims() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("us"),
    );
    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us")],
    );
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &4_000);

    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("ca")],
    );

    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);
    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);

    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder), 4_000);
    assert_eq!(payout, 40_000);
    assert_eq!(crate::test_utils::get_balance(&env, &payout_asset, &holder), 40_000);
}

#[test]
fn apply_snapshot_shares_rejects_disallowed_jurisdiction_without_partial_state() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_snapshot_config(&issuer, &symbol_short!("def"), &token, &true);
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("uk"),
    );
    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us")],
    );

    let hash = BytesN::from_array(&env, &[7; 32]);
    client.commit_snapshot(&issuer, &symbol_short!("def"), &token, &1, &hash);
    let events_before = env.events().all().len();

    let holders = soroban_sdk::vec![&env, (holder.clone(), 2_500u32)];
    let result =
        client.try_apply_snapshot_shares(&issuer, &symbol_short!("def"), &token, &1, &0, &holders);

    assert_eq!(result, Err(Ok(RevoraError::JurisdictionDisallowed)));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder), 0);
    assert_eq!(client.get_snapshot_holder_count(&issuer, &symbol_short!("def"), &token, &1), 0);
    assert!(env.events().all().len() > events_before);
}

#[test]
fn issuer_transfer_migrates_allowed_jurisdictions() {
    let (env, client, old_issuer, token, _payout_asset) = setup_offering();
    let new_issuer = Address::generate(&env);
    let contract_id = client.address.clone();

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey2::IssuerCount, &1_u32);
        env.storage().persistent().set(&DataKey2::IssuerItem(0), &old_issuer);
        env.storage()
            .persistent()
            .set(&DataKey2::IssuerRegistered(old_issuer.clone()), &true);
        env.storage()
            .persistent()
            .set(&DataKey2::NamespaceCount(old_issuer.clone()), &1_u32);
        env.storage()
            .persistent()
            .set(&DataKey2::NamespaceItem(old_issuer.clone(), 0), &symbol_short!("def"));
        env.storage()
            .persistent()
            .set(&DataKey2::NamespaceRegistered(old_issuer.clone(), symbol_short!("def")), &true);
    });

    client.set_allowed_jurisdictions(
        &old_issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us"), symbol_short!("sg")],
    );
    client.propose_issuer_transfer(&old_issuer, &symbol_short!("def"), &token, &new_issuer);
    client.accept_issuer_transfer(&new_issuer, &symbol_short!("def"), &token);

    assert_eq!(
        client.get_allowed_jurisdictions(&new_issuer, &symbol_short!("def"), &token),
        soroban_sdk::vec![&env, symbol_short!("us"), symbol_short!("sg")]
    );
    assert_eq!(
        client.get_allowed_jurisdictions(&old_issuer, &symbol_short!("def"), &token).len(),
        0
    );
}
