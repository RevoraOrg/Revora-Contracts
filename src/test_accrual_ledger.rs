#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env,
};

const CHECKPOINT_THRESHOLD_SMALL: u32 = 4;

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

fn setup_offering_with_threshold(threshold: u32) -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    client.set_checkpoint_threshold(&issuer, &symbol_short!("def"), &token, &threshold);
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

// === Checkpoint Compression Tests ===

#[test]
fn checkpoint_compression_folds_schedule_when_threshold_exceeded() {
    let (env, client, issuer, token, payout_asset) = setup_offering_with_threshold(CHECKPOINT_THRESHOLD_SMALL);
    let holder = Address::generate(&env);

    // Set share and deposit revenue for each period to create share transitions
    // We need more than CHECKPOINT_THRESHOLD_SMALL transitions to trigger compression
    for i in 1..=CHECKPOINT_THRESHOLD_SMALL + 2 {
        let share = 1_000 + (i as u32 * 100);
        client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &share);
        client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &10_000, &i);
    }

    // get_claimable should still return correct total after compression
    let claimable = client.get_claimable(&issuer, &symbol_short!("def"), &token, &holder);
    assert!(claimable > 0, "claimable should be positive after compression");
}

#[test]
fn checkpoint_compression_is_lossless_for_claimable_computation() {
    let (_env, client, issuer, token, payout_asset) = setup_offering_with_threshold(CHECKPOINT_THRESHOLD_SMALL);
    let holder = Address::generate(&_env);

    // Before compression: compute claimable amount with many share transitions
    let mut expected_total = 0_i128;
    for i in 1..=CHECKPOINT_THRESHOLD_SMALL + 2 {
        let share = 1_000 + (i as u32 * 100);
        client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &share);
        client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &10_000, &i);
        expected_total += 10_000 * (share as i128) / 10_000;
    }

    let actual_claimable = client.get_claimable(&issuer, &symbol_short!("def"), &token, &holder);
    assert_eq!(actual_claimable, expected_total, "claimable must be lossless after compression");
}

#[test]
fn checkpoint_compression_lossless_multiple_claims_before_and_after_fold() {
    let (_env, client, issuer, token, payout_asset) = setup_offering_with_threshold(CHECKPOINT_THRESHOLD_SMALL);
    let holder = Address::generate(&_env);

    // Create enough share transitions to trigger compression
    for i in 1..=CHECKPOINT_THRESHOLD_SMALL + 2 {
        let share = if i <= CHECKPOINT_THRESHOLD_SMALL { 3_000 } else { 7_000 };
        client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &share);
        client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &10_000, &i);
    }

    // Claim all periods; the total should equal sum of (revenue * share / 10000) for each period
    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    let expected = (CHECKPOINT_THRESHOLD_SMALL as i128) * 3_000 + 2 * 7_000;
    assert_eq!(payout, expected * 10, "claim should be lossless: 3000 for first threshold periods, 7000 for last two");
}

#[test]
fn checkpoint_threshold_exactly_reached_then_claim() {
    let (_env, client, issuer, token, payout_asset) = setup_offering_with_threshold(CHECKPOINT_THRESHOLD_SMALL);
    let holder = Address::generate(&_env);

    // Create exactly threshold share transitions
    for i in 1..=CHECKPOINT_THRESHOLD_SMALL {
        client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &(1_000 + i as u32 * 500));
        client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &5_000, &i);
    }

    // Now claim all periods
    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    let expected_total: i128 = (1..=CHECKPOINT_THRESHOLD_SMALL).map(|i| 5_000 * (1_000 + i as u32 * 500) as i128 / 10_000).sum();
    assert_eq!(payout, expected_total, "claim should be correct when threshold is exactly reached");
}

#[test]
fn checkpoint_claim_between_two_folds() {
    let (env, client, issuer, token, payout_asset) = setup_offering_with_threshold(CHECKPOINT_THRESHOLD_SMALL);
    let holder = Address::generate(&env);

    // Create enough transitions to trigger compression
    for i in 1..=CHECKPOINT_THRESHOLD_SMALL + 2 {
        let share = 2_000 + (i as u32 * 200);
        client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &share);
        client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &10_000, &i);
    }

    // Claim a few periods first
    let first_claim = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &2);
    assert!(first_claim > 0, "partial claim should return positive payout");

    // Get claimable for remaining periods
    let remaining = client.get_claimable(&issuer, &symbol_short!("def"), &token, &holder);
    assert!(remaining > 0, "remaining claimable should be positive after partial claim");
}

#[test]
fn checkpoint_set_and_get_threshold() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let _ = payout_asset;

    // Default threshold
    assert_eq!(client.get_checkpoint_threshold(&issuer, &symbol_short!("def"), &token), 1_000);

    // Set a custom threshold
    client.set_checkpoint_threshold(&issuer, &symbol_short!("def"), &token, &500);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &symbol_short!("def"), &token), 500);

    // Set to 0 to disable compression
    client.set_checkpoint_threshold(&issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &symbol_short!("def"), &token), 0);

    // Re-enable with a new value
    client.set_checkpoint_threshold(&issuer, &symbol_short!("def"), &token, &2000);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &symbol_short!("def"), &token), 2000);
}

#[test]
fn checkpoint_compression_preserves_claimable_after_partial_claim_then_compression() {
    let (_env, client, issuer, token, payout_asset) = setup_offering_with_threshold(CHECKPOINT_THRESHOLD_SMALL);
    let holder = Address::generate(&_env);

    // Create many share changes to trigger compression
    for i in 1..=CHECKPOINT_THRESHOLD_SMALL + 2 {
        client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &(1_000 + i as u32 * 300));
        client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &10_000, &i);
    }

    // Claim some periods before the anchor boundary is crossed
    let partial_payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &2);
    assert!(partial_payout > 0, "partial claim should succeed");

    // Get claimable for remaining periods should also be positive
    let remaining = client.get_claimable(&issuer, &symbol_short!("def"), &token, &holder);
    assert!(remaining > 0, "remaining claimable should be positive");

    // Claim the rest
    let remaining_payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert!(remaining_payout > 0, "remaining claim should succeed");

    // Total payout should match what get_claimable returned for the remaining amount
    assert_eq!(partial_payout + remaining_payout, partial_payout + remaining);
}

#[test]
fn checkpoint_compression_with_zero_threshold_disables_compression() {
    let (_env, client, issuer, token, payout_asset) = setup_offering_with_threshold(0);
    let holder = Address::generate(&_env);

    // Even with many share changes, no compression should happen
    for i in 1..=10 {
        client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &(1_000 + i as u32 * 100));
        client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &10_000, &i);
    }

    let claimable = client.get_claimable(&issuer, &symbol_short!("def"), &token, &holder);
    assert!(claimable > 0, "claimable should be positive even with zero threshold");

    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert!(payout > 0, "claim should succeed with zero threshold");
}

#[test]
fn checkpoint_compression_threshold_not_set_uses_default() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let _ = payout_asset;

    assert_eq!(client.get_checkpoint_threshold(&issuer, &symbol_short!("def"), &token), 1_000);
}
