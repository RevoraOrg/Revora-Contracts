#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env,
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
fn claim_uses_historical_share_for_unclaimed_periods() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &2_500);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &2);

    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(payout, 75_000);
}

#[test]
fn zeroing_share_does_not_burn_already_accrued_claims() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &0);

    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(payout, 50_000);
}

#[test]
fn get_claimable_uses_historical_share_schedule() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &4_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &1_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &2);

    assert_eq!(client.get_claimable(&issuer, &symbol_short!("def"), &token, &holder), 50_000);
}

#[test]
fn delay_barrier_preserves_pre_change_accrual() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.set_claim_delay(&issuer, &symbol_short!("def"), &token, &100);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    env.ledger().with_mut(|li| li.timestamp = 1_050);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &2_500);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &2);

    env.ledger().with_mut(|li| li.timestamp = 1_100);
    assert_eq!(client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0), 50_000);

    env.ledger().with_mut(|li| li.timestamp = 1_150);
    assert_eq!(client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0), 25_000);
}
