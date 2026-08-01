#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Unix timestamp for 2024-01-15T00:00:00Z.
const TS_JAN_2024: u64 = 1_705_276_800;
/// Unix timestamp for 2024-03-15T00:00:00Z (still in fiscal year 2024 with Apr start).
const TS_MAR_2024: u64 = 1_710_460_800;
/// Unix timestamp for 2024-04-01T00:00:00Z (start of fiscal year 2025 with Apr start).
const TS_APR_2024: u64 = 1_711_929_600;
/// Unix timestamp for 2024-12-15T00:00:00Z (fiscal year 2025 with Apr start).
const TS_DEC_2024: u64 = 1_734_249_600;
/// Unix timestamp for 2025-03-15T00:00:00Z (still fiscal year 2025 with Apr start).
const TS_MAR_2025: u64 = 1_742_511_600;

fn setup_env(ts: u64) -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = ts);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_admin = Address::generate(&env);
    let payout_asset = crate::test_utils::create_token(&env, &payout_admin);
    crate::test_utils::mint_tokens(&env, &payout_asset, &issuer, 1_000_000);

    client.register_offering(&issuer, &symbol_short!("def"), &token, &10_000, &payout_asset, &0);

    (env, client, issuer, token, payout_asset)
}

/// Jump the ledger to a specific timestamp.
fn set_time(env: &Env, ts: u64) {
    env.ledger().with_mut(|li| li.timestamp = ts);
}

// ── config roundtrip ─────────────────────────────────────────────────────────

#[test]
fn fiscal_year_config_default_and_roundtrip() {
    let (env, client, issuer, token, _payout) = setup_env(TS_JAN_2024);
    let ns = symbol_short!("def");

    // Default is January (1).
    assert_eq!(
        client.get_fiscal_year_start(&issuer, &ns, &token),
        1,
        "default should be January",
    );

    // Set to April (4).
    client.set_fiscal_year_start(&issuer, &ns, &token, &4);
    assert_eq!(client.get_fiscal_year_start(&issuer, &ns, &token), 4);

    // Set to December (12).
    client.set_fiscal_year_start(&issuer, &ns, &token, &12);
    assert_eq!(client.get_fiscal_year_start(&issuer, &ns, &token), 12);

    // Invalid: 0 should be rejected.
    let result = client.try_set_fiscal_year_start(&issuer, &ns, &token, &0);
    assert!(result.is_err(), "month 0 should be rejected");

    // Invalid: 13 should be rejected.
    let result = client.try_set_fiscal_year_start(&issuer, &ns, &token, &13);
    assert!(result.is_err(), "month 13 should be rejected");
}

// ── Year boundary tests ──────────────────────────────────────────────────────

#[test]
fn year_boundary_april_start() {
    let (env, client, issuer, token, payout_asset) = setup_env(TS_JAN_2024);
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);

    // Holder gets 100% share.
    client.set_holder_share(&issuer, &ns, &token, &holder, &10_000);

    // Configure fiscal year starting in April (4).
    client.set_fiscal_year_start(&issuer, &ns, &token, &4);

    // Deposit revenue and claim in March 2024 → fiscal year 2024 (Apr 2023 – Mar 2024).
    set_time(&env, TS_MAR_2024);
    client.deposit_revenue(&issuer, &ns, &token, &payout_asset, &100_000, &1);
    client.claim(&holder, &issuer, &ns, &token, &0);

    // Verify tax year 2024 summary.
    let fy2024 = client.get_holder_tax_year(&issuer, &ns, &token, &holder, &2024);
    assert_eq!(fy2024.return_of_capital, 100_000, "FY2024 should have 100k RoC");
    assert_eq!(fy2024.capital_gains, 0, "FY2024 should have 0 CG");
    assert_eq!(fy2024.ordinary_income, 0, "FY2024 should have 0 ordinary");

    // Deposit revenue and claim in April 2024 → fiscal year 2025 (Apr 2024 – Mar 2025).
    set_time(&env, TS_APR_2024);
    client.deposit_revenue(&issuer, &ns, &token, &payout_asset, &50_000, &2);
    client.claim(&holder, &issuer, &ns, &token, &0);

    // Verify tax year 2025 summary.
    let fy2025 = client.get_holder_tax_year(&issuer, &ns, &token, &holder, &2025);
    assert_eq!(fy2025.return_of_capital, 50_000, "FY2025 should have 50k RoC");
    assert_eq!(fy2025.capital_gains, 0, "FY2025 should have 0 CG");
    assert_eq!(fy2025.ordinary_income, 0, "FY2025 should have 0 ordinary");

    // Verify FY2024 unchanged.
    let fy2024_check = client.get_holder_tax_year(&issuer, &ns, &token, &holder, &2024);
    assert_eq!(fy2024_check.return_of_capital, 100_000, "FY2024 should be unchanged");
}

// ── Multi-year holder ────────────────────────────────────────────────────────

#[test]
fn multi_year_holder_accumulates_correctly() {
    let (env, client, issuer, token, payout_asset) = setup_env(TS_JAN_2024);
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder, &10_000);

    // Jan 2024: deposit & claim → FY2024 (Jan start).
    set_time(&env, TS_JAN_2024);
    client.deposit_revenue(&issuer, &ns, &token, &payout_asset, &100_000, &1);
    client.claim(&holder, &issuer, &ns, &token, &0);

    // Dec 2024: deposit & claim → still FY2024 (still Dec).
    set_time(&env, TS_DEC_2024);
    client.deposit_revenue(&issuer, &ns, &token, &payout_asset, &75_000, &2);
    client.claim(&holder, &issuer, &ns, &token, &0);

    // Mar 2025: deposit & claim → FY2025 (Mar 2025, fiscal year Jan start).
    set_time(&env, TS_MAR_2025);
    client.deposit_revenue(&issuer, &ns, &token, &payout_asset, &50_000, &3);
    client.claim(&holder, &issuer, &ns, &token, &0);

    // Verify accumulations.
    let fy2024 = client.get_holder_tax_year(&issuer, &ns, &token, &holder, &2024);
    assert_eq!(fy2024.return_of_capital, 175_000, "FY2024: 100k + 75k");
    assert_eq!(fy2024.capital_gains, 0);
    assert_eq!(fy2024.ordinary_income, 0);

    let fy2025 = client.get_holder_tax_year(&issuer, &ns, &token, &holder, &2025);
    assert_eq!(fy2025.return_of_capital, 50_000, "FY2025: 50k");
    assert_eq!(fy2025.capital_gains, 0);
    assert_eq!(fy2025.ordinary_income, 0);
}

// ── Default fiscal year (January start) ──────────────────────────────────────

#[test]
fn fiscal_year_january_default() {
    let (env, client, issuer, token, payout_asset) = setup_env(TS_JAN_2024);
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder, &10_000);

    // Jan 2024 → FY2024.
    set_time(&env, TS_JAN_2024);
    client.deposit_revenue(&issuer, &ns, &token, &payout_asset, &100_000, &1);
    client.claim(&holder, &issuer, &ns, &token, &0);

    let fy2024 = client.get_holder_tax_year(&issuer, &ns, &token, &holder, &2024);
    assert_eq!(fy2024.return_of_capital, 100_000);
    assert_eq!(fy2024.ordinary_income, 0);

    // Dec 2024 → still FY2024 (default Jan start).
    set_time(&env, TS_DEC_2024);
    client.deposit_revenue(&issuer, &ns, &token, &payout_asset, &50_000, &2);
    client.claim(&holder, &issuer, &ns, &token, &0);

    let fy2024b = client.get_holder_tax_year(&issuer, &ns, &token, &holder, &2024);
    assert_eq!(fy2024b.return_of_capital, 150_000, "accumulated: 100k + 50k");
    assert_eq!(fy2024b.ordinary_income, 0);
}

// ── No-activity holder ───────────────────────────────────────────────────────

#[test]
fn no_activity_holder_returns_zeros() {
    let (env, client, issuer, token, _payout_asset) = setup_env(TS_JAN_2024);
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);

    // Holder has no shares and never claimed.
    let summary = client.get_holder_tax_year(&issuer, &ns, &token, &holder, &2024);
    assert_eq!(summary.return_of_capital, 0, "no activity → zero RoC");
    assert_eq!(summary.capital_gains, 0, "no activity → zero CG");
    assert_eq!(summary.ordinary_income, 0, "no activity → zero ordinary");

    // Different year also zero.
    let summary_2025 = client.get_holder_tax_year(&issuer, &ns, &token, &holder, &2025);
    assert_eq!(summary_2025.return_of_capital, 0);
    assert_eq!(summary_2025.capital_gains, 0);
    assert_eq!(summary_2025.ordinary_income, 0);
}

// ── Timestamp helper correctness ─────────────────────────────────────────────

#[test]
fn test_timestamp_to_year_month() {
    use crate::tax_bucket::{timestamp_to_month, timestamp_to_year};

    // Epoch → year 1970, month 1.
    assert_eq!(timestamp_to_year(0), 1970);
    assert_eq!(timestamp_to_month(0), 1);

    // 2024-01-15T00:00:00Z → year 2024, month 1.
    assert_eq!(timestamp_to_year(TS_JAN_2024), 2024);
    assert_eq!(timestamp_to_month(TS_JAN_2024), 1);

    // 2024-04-01T00:00:00Z → year 2024, month 4.
    assert_eq!(timestamp_to_year(TS_APR_2024), 2024);
    assert_eq!(timestamp_to_month(TS_APR_2024), 4);

    // 2024-12-15T00:00:00Z → year 2024, month 12.
    assert_eq!(timestamp_to_year(TS_DEC_2024), 2024);
    assert_eq!(timestamp_to_month(TS_DEC_2024), 12);

    // 2025-03-15T00:00:00Z → year 2025, month 3.
    assert_eq!(timestamp_to_year(TS_MAR_2025), 2025);
    assert_eq!(timestamp_to_month(TS_MAR_2025), 3);
}

#[test]
fn test_fiscal_year_from_ts() {
    use crate::tax_bucket::fiscal_year_from_ts;

    // Apr start: Mar 2024 → FY2024, Apr 2024 → FY2025.
    assert_eq!(fiscal_year_from_ts(TS_MAR_2024, 4), 2024);
    assert_eq!(fiscal_year_from_ts(TS_APR_2024, 4), 2025);

    // Jan start: everything maps to its calendar year.
    assert_eq!(fiscal_year_from_ts(TS_JAN_2024, 1), 2024);
    assert_eq!(fiscal_year_from_ts(TS_DEC_2024, 1), 2024);
    assert_eq!(fiscal_year_from_ts(TS_MAR_2025, 1), 2025);
}
