#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
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

// ── holder_statement_diff tests ──

#[test]
fn holder_statement_diff_basic_delta_across_periods() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    // Period 1: holder has 5_000 bps, revenue 100_000
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    // Period 2: holder share reduced to 2_500 bps, revenue 100_000
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &2_500);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &2);

    let delta = client.holder_statement_diff(
        &holder, &issuer, &symbol_short!("def"), &token, &1, &2,
    );

    // share_delta: 2_500 - 5_000 = -2_500
    assert_eq!(delta.share_delta, -2_500);
    // claimed_delta: period 2 accrued (5_000*100/10000 + 2_500*100/10000) - period 1 accrued (5_000*100/10000)
    // period 1 claimable: 5_000 * 100_000 / 10_000 = 50_000
    // period 2 claimable: 5_000*100/10000=50_000 + 2_500*100/10000=25_000 = 75_000
    // delta = 75_000 - 50_000 = 25_000
    assert_eq!(delta.claimed_delta, 25_000);
}

#[test]
fn holder_statement_diff_same_period_yields_zero_delta() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &2);

    let delta = client.holder_statement_diff(
        &holder, &issuer, &symbol_short!("def"), &token, &2, &2,
    );

    assert_eq!(delta.share_delta, 0);
    assert_eq!(delta.claimed_delta, 0);
}

#[test]
fn holder_statement_diff_period_a_greater_than_b_rejected() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &2);

    let result = client.try_holder_statement_diff(
        &holder, &issuer, &symbol_short!("def"), &token, &2, &1,
    );

    assert_eq!(result, Err(Ok(RevoraError::InvalidPeriodId)));
}

#[test]
fn holder_statement_diff_zero_period_rejected() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    // period_a = 0 should be rejected
    let result = client.try_holder_statement_diff(
        &holder, &issuer, &symbol_short!("def"), &token, &0, &1,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidPeriodId)));

    // period_b = 0 should also be rejected (period_a > period_b wins)
    let result2 = client.try_holder_statement_diff(
        &holder, &issuer, &symbol_short!("def"), &token, &1, &0,
    );
    assert_eq!(result2, Err(Ok(RevoraError::InvalidPeriodId)));

    // Both zero should be rejected
    let result3 = client.try_holder_statement_diff(
        &holder, &issuer, &symbol_short!("def"), &token, &0, &0,
    );
    assert_eq!(result3, Err(Ok(RevoraError::InvalidPeriodId)));
}

#[test]
fn holder_statement_diff_period_out_of_range_rejected() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    // Only 1 period exists; period 2 is out of range
    let result = client.try_holder_statement_diff(
        &holder, &issuer, &symbol_short!("def"), &token, &1, &2,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidPeriodId)));
}

#[test]
fn holder_statement_diff_holder_with_no_shares_returns_zero() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    // No shares set for this holder
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &200_000, &2);

    let delta = client.holder_statement_diff(
        &holder, &issuer, &symbol_short!("def"), &token, &1, &2,
    );

    assert_eq!(delta.share_delta, 0);
    assert_eq!(delta.claimed_delta, 0);
}

#[test]
fn holder_statement_diff_share_increase_delta_positive() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &1_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &2);

    let delta = client.holder_statement_diff(
        &holder, &issuer, &symbol_short!("def"), &token, &1, &2,
    );

    // share_delta: 5_000 - 1_000 = 4_000 (positive)
    assert_eq!(delta.share_delta, 4_000);
    // period 1 claimable: 1_000 * 100_000 / 10_000 = 10_000
    // period 2 claimable (cumulative): 10_000 + 5_000 * 100_000 / 10_000 = 60_000
    // delta = 60_000 - 10_000 = 50_000
    assert_eq!(delta.claimed_delta, 50_000);
}

#[test]
fn holder_statement_diff_multiple_periods_full_range() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &2);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &3);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &4);

    let delta = client.holder_statement_diff(
        &holder, &issuer, &symbol_short!("def"), &token, &1, &4,
    );

    // share never changed
    assert_eq!(delta.share_delta, 0);
    // 4 periods * 50_000 per period = 200_000 cumulative through period 4
    // 1 period * 50_000 = 50_000 cumulative through period 1
    // delta = 200_000 - 50_000 = 150_000
    assert_eq!(delta.claimed_delta, 150_000);
}

#[test]
fn holder_statement_diff_with_claim_partially_settled() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &2);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &3);

    // Claim period 1 only
    client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);

    // Diff between periods 1-3 should still compute on full accrued amounts
    // (held_statement_diff uses the accrual ledger, not the claim state)
    let delta = client.holder_statement_diff(
        &holder, &issuer, &symbol_short!("def"), &token, &1, &3,
    );

    assert_eq!(delta.share_delta, 0);
    // 3 periods * 50_000 = 150_000 cumulative through period 3
    // 1 period * 50_000 = 50_000 cumulative through period 1
    // delta = 100_000
    assert_eq!(delta.claimed_delta, 100_000);
}
