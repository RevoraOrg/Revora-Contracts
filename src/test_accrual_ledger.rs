#![cfg(test)]

use crate::{AutoReinvestConfig, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env,
};

// ── Setup helper ─────────────────────────────────────────────────────────────

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

// ── Existing accrual-ledger tests (preserved) ────────────────────────────────

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

// ── Auto-reinvestment tests ───────────────────────────────────────────────────

/// Happy path: holder enables auto-reinvest, claim converts dividend to shares.
///
/// Setup:
///   - holder share: 5 000 bps (50 %)
///   - deposit: 100 000 units  →  holder owed 50 000
///   - NAV per share: 5 000 units per bps
///   - expected share_delta: floor(50_000 / 5_000) = 10 bps
///   - expected new holder share: 5 000 + 10 = 5 010 bps
///   - claim return value: 0 (no tokens transferred; all reinvested)
#[test]
fn auto_reinvest_happy_path_converts_dividend_to_shares() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    // Enable auto-reinvest: NAV = 5 000 units per bps-share.
    client.set_auto_reinvest(&holder, &issuer, &symbol_short!("def"), &token, &true, &5_000_i128);

    // Claim should reinvest the 50 000-unit dividend as 10 new bps shares.
    let returned = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(returned, 0, "all payout should be reinvested, not transferred");

    let new_share = client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder);
    assert_eq!(new_share, 5_010, "holder should have 10 new bps");
}

/// get_auto_reinvest returns the stored config after set_auto_reinvest.
#[test]
fn get_auto_reinvest_returns_stored_config() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    // Nothing stored yet.
    let before = client.get_auto_reinvest(&issuer, &symbol_short!("def"), &token, &holder);
    assert!(before.is_none());

    client.set_auto_reinvest(&holder, &issuer, &symbol_short!("def"), &token, &true, &10_000_i128);

    let after = client.get_auto_reinvest(&issuer, &symbol_short!("def"), &token, &holder);
    let cfg = after.expect("config should be stored");
    assert!(cfg.enabled);
    assert_eq!(cfg.nav_per_share_e7, 10_000);
}

/// Disabled path: auto-reinvest off → claim transfers tokens normally.
#[test]
fn auto_reinvest_disabled_transfers_tokens_normally() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    // Explicitly disabled.
    client.set_auto_reinvest(&holder, &issuer, &symbol_short!("def"), &token, &false, &5_000_i128);

    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(payout, 50_000, "tokens should be transferred when reinvest is off");

    // Share unchanged.
    let share = client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder);
    assert_eq!(share, 5_000);
}

/// NAV = 0 guard: set_auto_reinvest with enabled=true and nav=0 must fail.
#[test]
fn set_auto_reinvest_rejects_zero_nav_when_enabled() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    let result = client.try_set_auto_reinvest(
        &holder,
        &issuer,
        &symbol_short!("def"),
        &token,
        &true,
        &0_i128,
    );
    assert!(result.is_err(), "nav=0 with enabled=true should be rejected");
}

/// NAV = 0 allowed when disabled (NAV field is ignored).
#[test]
fn set_auto_reinvest_allows_zero_nav_when_disabled() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    let result = client.try_set_auto_reinvest(
        &holder,
        &issuer,
        &symbol_short!("def"),
        &token,
        &false,
        &0_i128,
    );
    assert!(result.is_ok(), "nav=0 is fine when disabled");
}

/// Cap-exhausted rejection: when the supply cap would be exceeded, reinvestment
/// falls back to a normal cash transfer rather than reverting.
///
/// Setup:
///   - max_total_supply_shares = 5 005 bps
///   - holder already has 5 000 bps
///   - floor(50 000 / 5 000) = 10 bps delta would exceed the cap by 5 bps
///   - Expected: cap guard kicks in, token transfer happens instead
#[test]
fn auto_reinvest_falls_back_to_cash_when_cap_exhausted() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    // Set a tight supply cap: 5 005 bps total (only 5 extra bps available).
    client.set_max_total_supply_shares(&issuer, &symbol_short!("def"), &token, &5_005_i128);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    // NAV 5 000 → delta 10 bps, but only 5 bps headroom under the cap.
    client.set_auto_reinvest(&holder, &issuer, &symbol_short!("def"), &token, &true, &5_000_i128);

    // Claim should fall back to cash transfer, not revert.
    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(payout, 50_000, "should receive cash when cap is exhausted");

    // Share must be unchanged.
    let share = client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder);
    assert_eq!(share, 5_000, "share should be unchanged after cap rejection");
}

/// No config at all → claim behaves as normal cash transfer.
#[test]
fn auto_reinvest_no_config_is_normal_claim() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    // No set_auto_reinvest call at all.
    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(payout, 50_000);
}

/// share_delta rounds down (floor division); tiny dividend below 1 bps NAV
/// still lets the claim succeed (period index advances) with no transfer.
#[test]
fn auto_reinvest_tiny_dividend_below_one_bps_advances_index() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &1);
    // 100 units revenue × 1 bps = 0.01 units owed; floor(0 / huge_nav) = 0 delta
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100, &1);

    // NAV so large that share_delta = floor(tiny/huge) = 0.
    client.set_auto_reinvest(
        &holder,
        &issuer,
        &symbol_short!("def"),
        &token,
        &true,
        &1_000_000_i128,
    );

    // Claim should succeed with 0 payout (delta is 0, so no reinvest and payout=0).
    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    // Payout is 0 because 100 * 1 / 10_000 = 0 (truncated).
    assert_eq!(payout, 0);
}

/// Auto-reinvest can be toggled off and subsequent claim returns cash.
#[test]
fn auto_reinvest_toggle_off_restores_cash_claim() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    // Enable and then immediately disable before claiming.
    client.set_auto_reinvest(&holder, &issuer, &symbol_short!("def"), &token, &true, &5_000_i128);
    client.set_auto_reinvest(&holder, &issuer, &symbol_short!("def"), &token, &false, &5_000_i128);

    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(payout, 50_000, "should receive cash after toggling reinvest off");
}
