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

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &1_000,
        &payment_token.address(),
        &0,
    );
    client.set_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder, &5_000);

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
    client.set_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder, &0);

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

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &1_000,
        &payment_token.address(),
        &0,
    );
    client.set_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder, &5_000);
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
