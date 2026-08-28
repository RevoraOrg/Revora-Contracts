#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    token, Address, Env,
};

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn setup_offering(
    env: &Env,
) -> (RevoraRevenueShareClient<'_>, Address, Address, Address, Address, Address) {
    env.mock_all_auths();
    let client = make_client(env);
    let admin = Address::generate(env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(env);
    let offering_token = Address::generate(env);
    let payment_admin = Address::generate(env);
    let payment_token = env.register_stellar_asset_contract_v2(payment_admin.clone());
    let holder = Address::generate(env);

    client.register_offering(&issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &offering_token,
        &1_000,
        &payment_token.address(),
        &0,
        &symbol_short!(""),
        &0);
    client.set_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder, &5_000, &1);

    // Mint to issuer, then deposit revenue so contract has a balance
    token::StellarAssetClient::new(env, &payment_token.address()).mint(&issuer, &1_000_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token.address(),
        &500_000,
        &1,
    );

    (client, issuer, offering_token, payment_token.address(), holder, admin)
}

fn set_timestamp(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

// ── Window management ─────────────────────────────────────────────────────────

#[test]
fn set_redemption_window_ok() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);

    let window =
        client.get_redemption_window(&issuer, &symbol_short!("def"), &offering_token).unwrap();
    assert_eq!(window.start_timestamp, 500);
    assert_eq!(window.end_timestamp, 2000);
}

#[test]
fn set_redemption_window_rejects_non_issuer() {
    let env = Env::default();
    let (client, _issuer, offering_token, ..) = setup_offering(&env);
    let stranger = Address::generate(&env);

    let result = client.try_set_redemption_window(
        &stranger,
        &symbol_short!("def"),
        &offering_token,
        &500,
        &2000,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn set_redemption_window_rejects_bad_range() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);

    let result = client.try_set_redemption_window(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &2000,
        &500,
    );
    assert_eq!(result, Err(Ok(RevoraError::LimitReached)));
}

#[test]
fn get_redemption_window_returns_none_when_unset() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);

    let window = client.get_redemption_window(&issuer, &symbol_short!("def"), &offering_token);
    assert_eq!(window, None);
}

// ── Window overlap rejection ──────────────────────────────────────────────────

#[test]
fn set_redemption_window_rejects_overlap() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);

    // Set first window [500, 2000)
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);

    // Overlap from the left: [100, 1000) collides with [500, 2000)
    let result = client.try_set_redemption_window(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &100,
        &1000,
    );
    assert_eq!(result, Err(Ok(RevoraError::RedemptionWindowOverlap)));

    // Overlap from the right: [1500, 3000) collides with [500, 2000)
    let result = client.try_set_redemption_window(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &1500,
        &3000,
    );
    assert_eq!(result, Err(Ok(RevoraError::RedemptionWindowOverlap)));

    // Full containment: [600, 1800) inside [500, 2000)
    let result = client.try_set_redemption_window(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &600,
        &1800,
    );
    assert_eq!(result, Err(Ok(RevoraError::RedemptionWindowOverlap)));

    // Surrounding: [100, 3000) fully contains [500, 2000)
    let result = client.try_set_redemption_window(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &100,
        &3000,
    );
    assert_eq!(result, Err(Ok(RevoraError::RedemptionWindowOverlap)));
}

#[test]
fn set_redemption_window_allows_contiguous_non_overlapping() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);

    // Set first window [500, 2000)
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);

    // Non-overlapping: [2000, 3000) -- exactly adjacent (end == start)
    let result = client.try_set_redemption_window(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &2000,
        &3000,
    );
    assert_eq!(result, Ok(Ok(())));

    // Existing window unchanged
    let window = client.get_redemption_window(&issuer, &symbol_short!("def"), &offering_token).unwrap();
    assert_eq!(window.start_timestamp, 2000);
    assert_eq!(window.end_timestamp, 3000);
}

// ── request_redemption ────────────────────────────────────────────────────────

#[test]
fn request_redemption_ok() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);

    let result = client.try_request_redemption(
        &holder,
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &2_000,
    );
    assert_eq!(result, Ok(Ok(())));
}

#[test]
fn request_redemption_outside_window() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 3000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);

    let result = client.try_request_redemption(
        &holder,
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &2_000,
    );
    assert_eq!(result, Err(Ok(RevoraError::RedemptionWindowClosed)));
}

#[test]
fn request_redemption_always_open_when_unset() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);

    // No window set — should be always open
    let result = client.try_request_redemption(
        &holder,
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &2_000,
    );
    assert_eq!(result, Ok(Ok(())));
}

#[test]
fn request_redemption_blacklisted() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);
    client.blacklist_add(&issuer, &issuer, &symbol_short!("def"), &offering_token, &holder);

    let result = client.try_request_redemption(
        &holder,
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &2_000,
    );
    assert_eq!(result, Err(Ok(RevoraError::HolderBlacklisted)));
}

#[test]
fn request_redemption_zero_shares() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);

    let result =
        client.try_request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &0);
    assert_eq!(result, Err(Ok(RevoraError::InvalidShareBps)));
}

#[test]
fn request_redemption_exceeds_share() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);

    let result = client.try_request_redemption(
        &holder,
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &10_001,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidShareBps)));
}

#[test]
fn request_redemption_no_share() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);
    let no_share_holder = Address::generate(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);

    let result = client.try_request_redemption(
        &no_share_holder,
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &2_000,
    );
    assert_eq!(result, Err(Ok(RevoraError::NoPendingClaims)));
}

#[test]
fn request_redemption_duplicate() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);

    // First request succeeds
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &2_000);

    // Duplicate fails
    let result = client.try_request_redemption(
        &holder,
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &1_000,
    );
    assert_eq!(result, Err(Ok(RevoraError::LimitReached)));
}

#[test]
fn request_redemption_offering_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let holder = Address::generate(&env);
    let unknown_token = Address::generate(&env);

    let result = client.try_request_redemption(
        &holder,
        &admin,
        &symbol_short!("def"),
        &unknown_token,
        &1_000,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

// ── fulfill_redemption ────────────────────────────────────────────────────────

#[test]
fn fulfill_redemption_ok() {
    let env = Env::default();
    let (client, issuer, offering_token, payment_token, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &2_000);

    let balance_before = token::Client::new(&env, &payment_token).balance(&holder);
    let result = client.try_fulfill_redemption(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &holder,
        &100_000,
    );
    assert_eq!(result, Ok(Ok(100_000)));
    let balance_after = token::Client::new(&env, &payment_token).balance(&holder);
    assert_eq!(balance_after - balance_before, 100_000);

    // Holder share reduced from 5_000 to 3_000
    let share = client.get_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder);
    assert_eq!(share, 3_000);
}

#[test]
fn fulfill_redemption_blacklisted() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &2_000);

    // Blacklist after request, before fulfill
    client.blacklist_add(&issuer, &issuer, &symbol_short!("def"), &offering_token, &holder);

    let result = client.try_fulfill_redemption(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &holder,
        &100_000,
    );
    assert_eq!(result, Err(Ok(RevoraError::HolderBlacklisted)));
}

#[test]
fn fulfill_redemption_no_request() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);

    let result = client.try_fulfill_redemption(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &holder,
        &100_000,
    );
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));
}

#[test]
fn fulfill_redemption_outside_window() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &2_000);

    // Advance past window close
    set_timestamp(&env, 3000);

    let result = client.try_fulfill_redemption(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &holder,
        &100_000,
    );
    assert_eq!(result, Err(Ok(RevoraError::RedemptionWindowClosed)));
}

#[test]
fn fulfill_redemption_zero_share() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &2_000);

    // Clear holder share before fulfill
    client.set_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder, &0, &1);

    let result = client.try_fulfill_redemption(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &holder,
        &100_000,
    );
    assert_eq!(result, Err(Ok(RevoraError::NoPendingClaims)));
}

#[test]
fn fulfill_redemption_capped_to_current_share() {
    let env = Env::default();
    let (client, issuer, offering_token, payment_token, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);
    // Request current share (5_000) in full
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &5_000);

    let balance_before = token::Client::new(&env, &payment_token).balance(&holder);
    let result = client.try_fulfill_redemption(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &holder,
        &200_000,
    );
    assert_eq!(result, Ok(Ok(200_000)));
    let balance_after = token::Client::new(&env, &payment_token).balance(&holder);
    assert_eq!(balance_after - balance_before, 200_000);

    // Share reduced to 0 (full share was redeemed)
    let share = client.get_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder);
    assert_eq!(share, 0);
}

#[test]
fn fulfill_redemption_rejects_zero_amount() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &2_000);

    let result =
        client.try_fulfill_redemption(&issuer, &symbol_short!("def"), &offering_token, &holder, &0);
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
}

#[test]
fn fulfill_redemption_full_flow_then_re_request() {
    let env = Env::default();
    let (client, issuer, offering_token, payment_token, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &5000);

    // Request 2_000 of 5_000 shares
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &2_000);
    client.fulfill_redemption(&issuer, &symbol_short!("def"), &offering_token, &holder, &100_000);
    let share = client.get_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder);
    assert_eq!(share, 3_000);

    // Request remaining 3_000 shares
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &3_000);
    client.fulfill_redemption(&issuer, &symbol_short!("def"), &offering_token, &holder, &150_000);
    let share = client.get_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder);
    assert_eq!(share, 0);
}

#[test]
fn fulfill_redemption_non_issuer_rejected() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);
    let stranger = Address::generate(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &2_000);

    let result = client.try_fulfill_redemption(
        &stranger,
        &symbol_short!("def"),
        &offering_token,
        &holder,
        &100_000,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn redemption_events_emitted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payment_admin = Address::generate(&env);
    let payment_token = env.register_stellar_asset_contract_v2(payment_admin.clone());
    let holder = Address::generate(&env);

    client.register_offering(&issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &offering_token,
        &1_000,
        &payment_token.address(),
        &0,
        &symbol_short!(""),
        &0);
    client.set_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder, &5_000, &1);
    token::StellarAssetClient::new(&env, &payment_token.address()).mint(&issuer, &1_000_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token.address(),
        &500_000,
        &1,
    );

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);

    let before = env.events().all().len();
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &2_000);
    assert!(
        env.events().all().len() > before,
        "expected at least one event from request_redemption"
    );
}

#[test]
fn redemption_inside_window_after_blacklisting_rejected() {
    let env = Env::default();
    let (client, issuer, offering_token, _, holder, ..) = setup_offering(&env);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &2_000);

    // Blacklist while request is pending
    client.blacklist_add(&issuer, &issuer, &symbol_short!("def"), &offering_token, &holder);

    // Fulfill must reject blacklisted holder
    let result = client.try_fulfill_redemption(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &holder,
        &100_000,
    );
    assert_eq!(result, Err(Ok(RevoraError::HolderBlacklisted)));

    // Verify pending request still exists (not consumed)
    let result = client.try_fulfill_redemption(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &holder,
        &100_000,
    );
    assert_eq!(result, Err(Ok(RevoraError::HolderBlacklisted)));
}

#[test]
fn test_set_redemption_fee_bps_success_and_getters() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);
    let treasury = Address::generate(&env);

    assert_eq!(client.get_redemption_fee_bps(&issuer, &symbol_short!("def"), &offering_token), 0);
    assert_eq!(client.get_redemption_fee_config(&issuer, &symbol_short!("def"), &offering_token), None);

    client.set_redemption_fee_bps(&issuer, &symbol_short!("def"), &offering_token, &500, &treasury);

    assert_eq!(client.get_redemption_fee_bps(&issuer, &symbol_short!("def"), &offering_token), 500);
    let cfg = client.get_redemption_fee_config(&issuer, &symbol_short!("def"), &offering_token).unwrap();
    assert_eq!(cfg.fee_bps, 500);
    assert_eq!(cfg.treasury, treasury);
}

#[test]
fn test_set_redemption_fee_bps_exceeds_max_cap() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);
    let treasury = Address::generate(&env);

    let res = client.try_set_redemption_fee_bps(&issuer, &symbol_short!("def"), &offering_token, &5_001, &treasury);
    assert_eq!(res, Err(Ok(RevoraError::InvalidRevenueShareBps)));
}

#[test]
fn test_set_redemption_fee_bps_unauthorized_issuer() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);
    let attacker = Address::generate(&env);
    let treasury = Address::generate(&env);

    let res = client.try_set_redemption_fee_bps(&attacker, &symbol_short!("def"), &offering_token, &500, &treasury);
    assert_eq!(res, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn test_fulfill_redemption_routes_fee_to_treasury() {
    let env = Env::default();
    let (client, issuer, offering_token, payout_token_id, holder, ..) = setup_offering(&env);
    let treasury = Address::generate(&env);

    // Set 10% (1,000 BPS) redemption fee
    client.set_redemption_fee_bps(&issuer, &symbol_short!("def"), &offering_token, &1_000, &treasury);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &2_000);

    let payout_token = soroban_sdk::token::Client::new(&env, &payout_token_id);

    // Fulfill 1,000,000 token payout
    let fulfilled_amount = client.fulfill_redemption(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &holder,
        &1_000_000,
    );
    assert_eq!(fulfilled_amount, 1_000_000);

    // 10% fee (100,000) to treasury, net (900,000) to holder
    assert_eq!(payout_token.balance(&treasury), 100_000);
    assert_eq!(payout_token.balance(&holder), 900_000);
}

#[test]
fn test_fulfill_redemption_max_fee_5000_bps() {
    let env = Env::default();
    let (client, issuer, offering_token, payout_token_id, holder, ..) = setup_offering(&env);
    let treasury = Address::generate(&env);

    // Set max 50% (5,000 BPS) fee
    client.set_redemption_fee_bps(&issuer, &symbol_short!("def"), &offering_token, &5_000, &treasury);

    set_timestamp(&env, 1000);
    client.set_redemption_window(&issuer, &symbol_short!("def"), &offering_token, &500, &2000);
    client.request_redemption(&holder, &issuer, &symbol_short!("def"), &offering_token, &2_000);

    let payout_token = soroban_sdk::token::Client::new(&env, &payout_token_id);

    client.fulfill_redemption(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &holder,
        &1_000_000,
    );

    // 50% fee (500,000) to treasury, net (500,000) to holder
    assert_eq!(payout_token.balance(&treasury), 500_000);
    assert_eq!(payout_token.balance(&holder), 500_000);
}

#[test]
fn test_cliff_taper_lockup_schedule_success_and_getters() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);

    // Default when unset is 10,000 BPS (100% unlocked)
    assert_eq!(client.get_unlocked_bps(&issuer, &symbol_short!("def"), &offering_token), 10_000);
    assert_eq!(client.get_lockup_schedule(&issuer, &symbol_short!("def"), &offering_token), None);

    // Set CliffTaper: 20% (2000 BPS) bulk unlock at ts=1000, linear taper until ts=2000
    let sched = crate::LockupSchedule::CliffTaper {
        cliff_ts: 1000,
        cliff_bps: 2000,
        taper_end_ts: 2000,
    };
    client.set_lockup_schedule(&issuer, &symbol_short!("def"), &offering_token, &sched);

    assert_eq!(
        client.get_lockup_schedule(&issuer, &symbol_short!("def"), &offering_token),
        Some(sched)
    );

    // Before cliff (ts = 500): 0% unlocked
    set_timestamp(&env, 500);
    assert_eq!(client.get_unlocked_bps(&issuer, &symbol_short!("def"), &offering_token), 0);

    // At cliff (ts = 1000): 20% (2000 BPS) unlocked
    set_timestamp(&env, 1000);
    assert_eq!(client.get_unlocked_bps(&issuer, &symbol_short!("def"), &offering_token), 2_000);

    // Midpoint between cliff and taper_end (ts = 1500): 20% + (50% of 80%) = 60% (6000 BPS)
    set_timestamp(&env, 1500);
    assert_eq!(client.get_unlocked_bps(&issuer, &symbol_short!("def"), &offering_token), 6_000);

    // At taper_end (ts = 2000): 100% (10000 BPS) unlocked
    set_timestamp(&env, 2000);
    assert_eq!(client.get_unlocked_bps(&issuer, &symbol_short!("def"), &offering_token), 10_000);

    // After taper_end (ts = 3000): 100% (10000 BPS) unlocked
    set_timestamp(&env, 3000);
    assert_eq!(client.get_unlocked_bps(&issuer, &symbol_short!("def"), &offering_token), 10_000);
}

#[test]
fn test_cliff_taper_edge_case_10000_bps_and_taper_equal_cliff() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);

    // Edge case: cliff_bps = 10,000 and taper_end_ts == cliff_ts
    let sched = crate::LockupSchedule::CliffTaper {
        cliff_ts: 1000,
        cliff_bps: 10_000,
        taper_end_ts: 1000,
    };
    client.set_lockup_schedule(&issuer, &symbol_short!("def"), &offering_token, &sched);

    set_timestamp(&env, 999);
    assert_eq!(client.get_unlocked_bps(&issuer, &symbol_short!("def"), &offering_token), 0);

    set_timestamp(&env, 1000);
    assert_eq!(client.get_unlocked_bps(&issuer, &symbol_short!("def"), &offering_token), 10_000);
}

#[test]
fn test_cliff_taper_invalid_params_rejected() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);

    // Invalid cliff_bps > 10,000
    let invalid_bps = crate::LockupSchedule::CliffTaper {
        cliff_ts: 1000,
        cliff_bps: 10_001,
        taper_end_ts: 2000,
    };
    let res1 = client.try_set_lockup_schedule(&issuer, &symbol_short!("def"), &offering_token, &invalid_bps);
    assert_eq!(res1, Err(Ok(RevoraError::InvalidRevenueShareBps)));

    // Invalid taper_end_ts < cliff_ts
    let invalid_end = crate::LockupSchedule::CliffTaper {
        cliff_ts: 2000,
        cliff_bps: 2000,
        taper_end_ts: 1000,
    };
    let res2 = client.try_set_lockup_schedule(&issuer, &symbol_short!("def"), &offering_token, &invalid_end);
    assert_eq!(res2, Err(Ok(RevoraError::InvalidAmount)));
}

// ── extend_lockup ────────────────────────────────────────────────────────────

#[test]
fn test_extend_lockup_extends_taper_end_ts() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);

    // Set initial lockup: cliff at 1000, taper end at 2000.
    let sched = crate::LockupSchedule::CliffTaper {
        cliff_ts: 1000,
        cliff_bps: 2000,
        taper_end_ts: 2000,
    };
    client.set_lockup_schedule(&issuer, &symbol_short!("def"), &offering_token, &sched);

    // Extend taper_end_ts from 2000 to 3000.
    let attestation = crate::SignedAttestation {
        network_id: env.ledger().network_id(),
        digest: env.crypto().sha256(&soroban_sdk::Bytes::new(&env)),
    };
    let result = client.try_extend_lockup(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &issuer,
        &3000u64,
        &attestation,
    );
    assert_eq!(result, Ok(Ok(())));

    // Verify the schedule was updated.
    let updated =
        client.get_lockup_schedule(&issuer, &symbol_short!("def"), &offering_token).unwrap();
    if let crate::LockupSchedule::CliffTaper { cliff_ts, cliff_bps, taper_end_ts } = updated {
        assert_eq!(cliff_ts, 1000);
        assert_eq!(cliff_bps, 2000);
        assert_eq!(taper_end_ts, 3000);
    } else {
        panic!("expected CliffTaper variant");
    }
}

#[test]
fn test_extend_lockup_rejects_shortening() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);

    // Set initial lockup: taper_end_ts = 2000.
    let sched = crate::LockupSchedule::CliffTaper {
        cliff_ts: 1000,
        cliff_bps: 2000,
        taper_end_ts: 2000,
    };
    client.set_lockup_schedule(&issuer, &symbol_short!("def"), &offering_token, &sched);

    // Attempt to shorten to 1500 is rejected.
    let attestation = crate::SignedAttestation {
        network_id: env.ledger().network_id(),
        digest: env.crypto().sha256(&soroban_sdk::Bytes::new(&env)),
    };
    let result = client.try_extend_lockup(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &issuer,
        &1500u64,
        &attestation,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));

    // Attempt with same timestamp is also rejected.
    let result = client.try_extend_lockup(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &issuer,
        &2000u64,
        &attestation,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
}

#[test]
fn test_extend_lockup_no_schedule_rejected() {
    let env = Env::default();
    let (client, issuer, offering_token, ..) = setup_offering(&env);

    // No lockup schedule set — extend must fail.
    let attestation = crate::SignedAttestation {
        network_id: env.ledger().network_id(),
        digest: env.crypto().sha256(&soroban_sdk::Bytes::new(&env)),
    };
    let result = client.try_extend_lockup(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &issuer,
        &3000u64,
        &attestation,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
}
