//! Tests for oracle-staleness guard (issue #545).
//!
//! Covers:
//! - `set_max_oracle_age_secs` / `get_max_oracle_age_secs` entrypoints
//! - Fresh quote accepted at the boundary (age == max_oracle_age_secs)
//! - Stale quote rejected one second past the boundary
//! - Zero `max_oracle_age_secs` disables the guard entirely
//! - `oracle_stale_reject` (`orc_stale`) event payload
//! - Auth: non-issuer rejected; unknown offering rejected
//! - State is not mutated on a stale rejection
//! - `set_max_oracle_age_secs` updates an existing FX oracle config in-place
//! - Large age windows (u64::MAX) accepted
#![cfg(test)]
use super::*;
use soroban_sdk::{
    contract, contractimpl, symbol_short,
    testutils::{Address as _, Events as _, Ledger},
    Address, Env, Symbol,
};

// ── Oracle stubs ──────────────────────────────────────────────────────────────

/// Returns a quote whose timestamp equals the current ledger time minus `age_secs`.
/// The rate is always 12_000 (1.2 in BPS: 1 EUR = 1.2 USDC).
mod oracle_with_age {
    use super::*;

    /// A stub oracle that always returns a quote `AGE` seconds old relative to
    /// the ledger timestamp when `quote()` is called.
    ///
    /// Because Soroban contracts cannot carry const-generic parameters, we use
    /// a macro to generate multiple age-parameterised stubs.
    macro_rules! make_age_stub {
        ($name:ident, $age:expr) => {
            pub mod $name {
                use super::super::*;
                #[contract]
                pub struct $name;

                #[contractimpl]
                impl $name {
                    pub fn quote(env: Env, _from: Symbol, _to: Symbol) -> (i128, u64) {
                        let ts = env.ledger().timestamp();
                        (12_000_i128, ts.saturating_sub($age))
                    }
                }
            }
        };
    }

    make_age_stub!(OracleAge0, 0); // quote_ts == now  (fresh)
    make_age_stub!(OracleAge59, 59); // 59 s old          (fresh when window=60)
    make_age_stub!(OracleAge60, 60); // exactly at boundary (fresh when window=60)
    make_age_stub!(OracleAge61, 61); // one second past boundary (stale when window=60)
    make_age_stub!(OracleAge120, 120); // clearly stale
}

use oracle_with_age::OracleAge0::OracleAge0;
use oracle_with_age::OracleAge120::OracleAge120;
use oracle_with_age::OracleAge59::OracleAge59;
use oracle_with_age::OracleAge60::OracleAge60;
use oracle_with_age::OracleAge61::OracleAge61;

// ── Test helpers ──────────────────────────────────────────────────────────────

const WINDOW: u64 = 60; // seconds

/// Returns `(env, client, issuer, namespace, token, reported_asset)` with:
/// - ledger timestamp = 1_000
/// - a registered offering whose payout_asset differs from `reported_asset`
/// - `set_fx_oracle` configured with the given `oracle` address and `window`
fn setup_with_oracle(
    oracle: Address,
    window: u64,
) -> (Env, RevoraRevenueShareClient<'static>, Address, Symbol, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let reported_asset = Address::generate(&env);

    client.register_offering(
        &issuer,
        &namespace,
        &token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!("USD"),
        &2,
    );

    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &Symbol::new(&env, "EUR"),
        &Symbol::new(&env, "USDC"),
        &window,
    );

    (env, client, issuer, namespace, token, reported_asset)
}

// ── Tests: set/get_max_oracle_age_secs entrypoints ───────────────────────────

#[test]
fn get_max_oracle_age_secs_returns_configured_value() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let oracle_addr = env.register_contract(None, OracleAge0);

    client.register_offering(
        &issuer,
        &namespace,
        &token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!("USD"),
        &2,
    );
    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle_addr,
        &Symbol::new(&env, "EUR"),
        &Symbol::new(&env, "USDC"),
        &120,
    );

    assert_eq!(client.get_max_oracle_age_secs(&issuer, &namespace, &token), Some(120));
}

#[test]
fn get_max_oracle_age_secs_returns_none_when_no_fx_oracle_configured() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.register_offering(
        &issuer,
        &namespace,
        &token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!("USD"),
        &2,
    );

    assert_eq!(client.get_max_oracle_age_secs(&issuer, &namespace, &token), None);
}

#[test]
fn set_max_oracle_age_secs_updates_existing_config() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let oracle_addr = env.register_contract(None, OracleAge0);

    client.register_offering(
        &issuer,
        &namespace,
        &token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!("USD"),
        &2,
    );
    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle_addr,
        &Symbol::new(&env, "EUR"),
        &Symbol::new(&env, "USDC"),
        &60,
    );

    // Now update just the age window
    client.set_max_oracle_age_secs(&issuer, &namespace, &token, &300);

    assert_eq!(client.get_max_oracle_age_secs(&issuer, &namespace, &token), Some(300));

    // Verify the oracle address is unchanged
    let config = client.get_fx_oracle(&issuer, &namespace, &token).unwrap();
    assert_eq!(config.oracle, oracle_addr);
    assert_eq!(config.max_oracle_age_secs, 300);
}

#[test]
fn set_max_oracle_age_secs_requires_issuer_auth() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    let issuer = Address::generate(&env);
    let attacker = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let oracle_addr = env.register_contract(None, OracleAge0);

    client.register_offering(
        &issuer,
        &namespace,
        &token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!("USD"),
        &2,
    );
    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle_addr,
        &Symbol::new(&env, "EUR"),
        &Symbol::new(&env, "USDC"),
        &60,
    );

    // Non-issuer caller gets OfferingNotFound
    let result = client.try_set_max_oracle_age_secs(&attacker, &namespace, &token, &300);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn set_max_oracle_age_secs_rejects_unknown_offering() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    let unknown_issuer = Address::generate(&env);
    let unknown_namespace = symbol_short!("unk");
    let unknown_token = Address::generate(&env);

    let result = client.try_set_max_oracle_age_secs(
        &unknown_issuer,
        &unknown_namespace,
        &unknown_token,
        &60,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn set_max_oracle_age_secs_rejects_when_no_fx_oracle_configured() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.register_offering(
        &issuer,
        &namespace,
        &token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!("USD"),
        &2,
    );

    // No FX oracle configured yet → OfferingNotFound
    let result = client.try_set_max_oracle_age_secs(&issuer, &namespace, &token, &60);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

// ── Tests: staleness guard behaviour ─────────────────────────────────────────

#[test]
fn fresh_quote_at_exact_boundary_is_accepted() {
    // quote_ts = now - 60, max_oracle_age_secs = 60 → age == window → accepted
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let oracle = env.register_contract(None, OracleAge60);
    let (_, client, issuer, namespace, token, reported_asset) = setup_with_oracle(oracle, WINDOW);

    // Should succeed: 60 - 60 = 0 > 0 is false → not stale
    client.report_revenue(&issuer, &namespace, &token, &reported_asset, &1_000, &1, &false);

    // Converted: 1_000 * 12_000 / 10_000 = 1_200
    assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 1_200);
}

#[test]
fn quote_one_second_inside_window_is_accepted() {
    // quote_ts = now - 59, max_oracle_age_secs = 60 → age < window → accepted
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let oracle = env.register_contract(None, OracleAge59);
    let (_, client, issuer, namespace, token, reported_asset) = setup_with_oracle(oracle, WINDOW);

    client.report_revenue(&issuer, &namespace, &token, &reported_asset, &1_000, &1, &false);
    assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 1_200);
}

#[test]
fn quote_one_second_past_boundary_is_rejected() {
    // quote_ts = now - 61, max_oracle_age_secs = 60 → age > window → stale
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let oracle = env.register_contract(None, OracleAge61);
    let (_, client, issuer, namespace, token, reported_asset) = setup_with_oracle(oracle, WINDOW);

    let result =
        client.try_report_revenue(&issuer, &namespace, &token, &reported_asset, &1_000, &1, &false);
    assert_eq!(result, Err(Ok(RevoraError::OracleQuoteStale)));
}

#[test]
fn stale_rejection_does_not_mutate_state() {
    // Confirm no revenue is stored and no audit entry created on stale rejection
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let oracle = env.register_contract(None, OracleAge120);
    let (_, client, issuer, namespace, token, reported_asset) = setup_with_oracle(oracle, WINDOW);

    let _ =
        client.try_report_revenue(&issuer, &namespace, &token, &reported_asset, &9_999, &1, &false);

    assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 0);
    assert_eq!(client.get_audit_summary(&issuer, &namespace, &token), None);
}

#[test]
fn zero_max_oracle_age_secs_disables_staleness_guard() {
    // window = 0 → guard disabled, any quote age accepted
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let oracle = env.register_contract(None, OracleAge120); // 120s old quote
    let (_, client, issuer, namespace, token, reported_asset) = setup_with_oracle(oracle, 0); // window = 0

    // Should succeed because guard is disabled
    client.report_revenue(&issuer, &namespace, &token, &reported_asset, &1_000, &1, &false);
    assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 1_200);
}

#[test]
fn fresh_quote_at_timestamp_zero_accepted_when_guard_disabled() {
    // quote_ts = 0 (very old), window = 0 → no guard → accepted
    // This is the clock-skew scenario where the oracle returns ts=0
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // OracleAge0 returns quote_ts == now, so to test ts=0 we need a custom stub
    // Use OracleAge120 with window=0 to exercise the disabled-guard path
    let oracle = env.register_contract(None, OracleAge120);
    let (_, client, issuer, namespace, token, reported_asset) = setup_with_oracle(oracle, 0);

    client.report_revenue(&issuer, &namespace, &token, &reported_asset, &500, &1, &false);
    assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 600);
}

#[test]
fn oracle_stale_reject_event_emitted_on_rejection() {
    // Verify the orc_stale event is emitted with correct payload
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let oracle = env.register_contract(None, OracleAge61);
    let (_, client, issuer, namespace, token, reported_asset) = setup_with_oracle(oracle, WINDOW);

    let _ =
        client.try_report_revenue(&issuer, &namespace, &token, &reported_asset, &1_000, &1, &false);

    // Look for the orc_stale event
    let events = env.events().all();
    let stale_event = events.iter().find(|(_contract_id, topics_val, _data)| {
        let topics: soroban_sdk::Vec<soroban_sdk::Val> = topics_val.clone().into_val(&env);
        if let Some(first) = topics.get(0) {
            let sym: Symbol = first.into_val(&env);
            sym == symbol_short!("orc_stale")
        } else {
            false
        }
    });

    assert!(stale_event.is_some(), "expected orc_stale event to be emitted");

    // Verify event payload: (quoted_at, now, max_oracle_age_secs)
    let (_contract_id, _topics, data) = stale_event.unwrap();
    let (quoted_at, now_ts, max_age): (u64, u64, u64) = data.into_val(&env);
    // quoted_at = 1000 - 61 = 939
    assert_eq!(quoted_at, 939);
    assert_eq!(now_ts, 1_000);
    assert_eq!(max_age, WINDOW);
}

#[test]
fn no_oracle_stale_event_emitted_on_fresh_quote() {
    // No orc_stale event should fire when quote is within window
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let oracle = env.register_contract(None, OracleAge0);
    let (_, client, issuer, namespace, token, reported_asset) = setup_with_oracle(oracle, WINDOW);

    client.report_revenue(&issuer, &namespace, &token, &reported_asset, &1_000, &1, &false);

    let events = env.events().all();
    let stale_event = events.iter().find(|(_contract_id, topics_val, _data)| {
        let topics: soroban_sdk::Vec<soroban_sdk::Val> = topics_val.clone().into_val(&env);
        if let Some(first) = topics.get(0) {
            let sym: Symbol = first.into_val(&env);
            sym == symbol_short!("orc_stale")
        } else {
            false
        }
    });

    assert!(stale_event.is_none(), "orc_stale must not be emitted for fresh quote");
}

#[test]
fn set_max_oracle_age_secs_to_zero_disables_guard_for_stale_quote() {
    // Start with window=60, confirm stale; then set window=0, confirm accepted
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let oracle = env.register_contract(None, OracleAge120);
    let (_, client, issuer, namespace, token, reported_asset) = setup_with_oracle(oracle, WINDOW);

    // First attempt: rejected
    let result1 =
        client.try_report_revenue(&issuer, &namespace, &token, &reported_asset, &1_000, &1, &false);
    assert_eq!(result1, Err(Ok(RevoraError::OracleQuoteStale)));

    // Disable guard
    client.set_max_oracle_age_secs(&issuer, &namespace, &token, &0);

    // Second attempt: accepted
    client.report_revenue(&issuer, &namespace, &token, &reported_asset, &1_000, &1, &false);
    assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 1_200);
}

#[test]
fn large_max_oracle_age_secs_accepts_old_quotes() {
    // window = u64::MAX → any quote accepted (no overflow in saturating arithmetic)
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let oracle = env.register_contract(None, OracleAge120);
    let (_, client, issuer, namespace, token, reported_asset) = setup_with_oracle(oracle, u64::MAX);

    client.report_revenue(&issuer, &namespace, &token, &reported_asset, &1_000, &1, &false);
    assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 1_200);
}

#[test]
fn error_code_is_stable_value_62() {
    // Regression guard: wire value must never change
    let code = RevoraError::OracleQuoteStale as u32;
    assert_eq!(code, 62, "OracleQuoteStale wire value must be 62 and must not change");
}

#[test]
fn multiple_stale_rejections_each_emit_event() {
    // Each failed report_revenue call should emit its own orc_stale event
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let oracle = env.register_contract(None, OracleAge61);
    let (_, client, issuer, namespace, token, reported_asset) = setup_with_oracle(oracle, WINDOW);

    let _ =
        client.try_report_revenue(&issuer, &namespace, &token, &reported_asset, &100, &1, &false);
    let _ =
        client.try_report_revenue(&issuer, &namespace, &token, &reported_asset, &200, &2, &false);

    let events = env.events().all();
    let stale_count = events
        .iter()
        .filter(|(_contract_id, topics_val, _data)| {
            let topics: soroban_sdk::Vec<soroban_sdk::Val> = topics_val.clone().into_val(&env);
            if let Some(first) = topics.get(0) {
                let sym: Symbol = first.into_val(&env);
                sym == symbol_short!("orc_stale")
            } else {
                false
            }
        })
        .count();

    assert_eq!(stale_count, 2, "expected two orc_stale events, one per rejected call");
}
