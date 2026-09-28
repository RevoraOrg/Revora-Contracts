//! Adversarial test suite for `report_revenue_with_attestation`.
//!
//! `report_revenue_with_attestation` is a thin wrapper around
//! `report_revenue_internal` that passes the caller-supplied `quote_bytes` and
//! `signature` down to `convert_report_amount_if_needed`. All guards
//! (`require_not_frozen`, `require_not_paused`, `issuer.require_auth`,
//! period-id validation, amount validation, offering lookup, duplicate-report
//! logic) are therefore shared with `report_revenue` and must behave identically.
//!
//! The attestation-specific path is exercised only when `payout_asset ≠
//! offering.payout_asset`.  When they are equal `convert_report_amount_if_needed`
//! returns immediately with the original amount — making the `quote_bytes` /
//! `signature` arguments irrelevant, which lets us test every guard without
//! constructing a valid off-chain signature.
//!
//! ## Test axes
//!
//! | # | Axis | Error / Assertion |
//! |---|------|-------------------|
//! | 1 | Happy path (same payout_asset → no FX) | `Ok(())`, storage persisted |
//! | 2 | Zero period_id rejected | `InvalidPeriodId`, storage unchanged |
//! | 3 | Negative amount rejected | `NegativeAmount` / `InvalidAmount`, storage unchanged |
//! | 4 | Offering not found | `OfferingNotFound`, storage unchanged |
//! | 5 | Contract frozen | `ContractFrozen`, storage unchanged |
//! | 6 | Contract paused | `ContractPaused`, storage unchanged |
//! | 7 | Duplicate period, override=false → rejected silently (Ok + no write) | storage unchanged |
//! | 8 | Duplicate period, override=true → overwrites | new amount persisted |
//! | 9 | Override on missing period → `MissingReportForOverride` | storage unchanged |
//! | 10 | Sequential periods (1, 2, 3) | each stored correctly |
//! | 11 | Amount = 0 (valid boundary) | `Ok(())`, stored as 0 |
//! | 12 | Amount = i128::MAX (boundary) | `Ok(())`, stored correctly |
//! | 13 | Close-period gate: override on sealed period → `PeriodAlreadyClosed` | storage unchanged |
//! | 14 | Mismatched payout_asset without oracle → `PayoutAssetMismatch` | storage unchanged |
//! | 15 | AuditSummary.total_revenue increments on new report | total = amount |
//! | 16 | AuditSummary.report_count unchanged on override | count stays at 1 |
//! | 17 | quote_bytes + signature ignored when payout_asset matches | Ok with any bytes |
//! | 18 | Period cursor is monotonic — skipping period_id rejected | `InvalidPeriodId` variant |

#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _, LedgerInfo},
    Address, Bytes, BytesN, Env, Vec,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const NS: &str = "def";

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Initialize contract + register one offering.
/// `payout_asset` equals the registered offering's payout asset
/// (same-asset path — no FX conversion).
fn setup(env: &Env) -> (RevoraRevenueShareClient<'_>, Address, Address, Address) {
    env.mock_all_auths();
    let client = make_client(env);
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let payout_asset = Address::generate(env);
    let ns = symbol_short!("def");
    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(env),
        &1u32,
        &ns,
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    (client, issuer, token, payout_asset)
}

/// Dummy 64-byte signature (all zeros) — accepted when payout_asset matches
/// the offering's payout_asset because signature verification is bypassed.
fn dummy_sig(env: &Env) -> BytesN<64> {
    BytesN::from_array(env, &[0u8; 64])
}

/// Empty quote bytes — also fine when payout_asset matches.
fn empty_quote(env: &Env) -> Bytes {
    Bytes::new(env)
}

/// Read back the stored revenue for `period_id` (returns 0 if not found).
fn get_revenue(
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    token: &Address,
    period_id: u64,
) -> i128 {
    let ns = symbol_short!("def");
    client.get_revenue_by_period(issuer, &ns, token, &period_id)
}

// ---------------------------------------------------------------------------
// 1. Happy path — same payout_asset, no FX conversion
// ---------------------------------------------------------------------------

#[test]
fn rwat_happy_path_same_payout_asset() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &1_000_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );
    assert!(result.is_ok(), "Expected Ok, got {:?}", result);
    assert_eq!(get_revenue(&client, &issuer, &token, 1), 1_000);
}

// ---------------------------------------------------------------------------
// 2. Zero period_id rejected
// ---------------------------------------------------------------------------

#[test]
fn rwat_zero_period_id_rejected() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &500_i128, &0u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );

    use crate::RevoraError;
    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::InvalidPeriodId,
        "period_id=0 must return InvalidPeriodId"
    );
    // Storage unchanged — no report for period 0 or 1
    assert_eq!(get_revenue(&client, &issuer, &token, 1), 0);
}

// ---------------------------------------------------------------------------
// 3. Negative amount rejected
// ---------------------------------------------------------------------------

#[test]
fn rwat_negative_amount_rejected() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &-1_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );

    assert!(
        result.is_err(),
        "Negative amount must return an error"
    );
    // Storage unchanged
    assert_eq!(get_revenue(&client, &issuer, &token, 1), 0);
}

// ---------------------------------------------------------------------------
// 4. Offering not found
// ---------------------------------------------------------------------------

#[test]
fn rwat_offering_not_found() {
    let env = Env::default();
    let (client, issuer, _token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);

    use crate::RevoraError;
    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &unknown_token, &payout_asset,
        &100_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );

    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::OfferingNotFound,
        "Unknown token must return OfferingNotFound"
    );
}

// ---------------------------------------------------------------------------
// 5. Contract frozen
// ---------------------------------------------------------------------------

#[test]
fn rwat_frozen_contract_rejected() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    client.freeze();

    use crate::RevoraError;
    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &100_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );

    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::ContractFrozen,
        "Frozen contract must return ContractFrozen"
    );
    // Storage unchanged
    assert_eq!(get_revenue(&client, &issuer, &token, 1), 0);
}

// ---------------------------------------------------------------------------
// 6. Contract paused
// ---------------------------------------------------------------------------

#[test]
fn rwat_paused_contract_rejected() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    client.pause_admin(&issuer);

    use crate::RevoraError;
    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &100_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );

    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::ContractPaused,
        "Paused contract must return ContractPaused"
    );
    assert_eq!(get_revenue(&client, &issuer, &token, 1), 0);
}

// ---------------------------------------------------------------------------
// 7. Duplicate period, override_existing=false → silently rejected (Ok + unchanged)
// ---------------------------------------------------------------------------

#[test]
fn rwat_duplicate_period_override_false_is_noop() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    // First report
    client.report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &100_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );
    assert_eq!(get_revenue(&client, &issuer, &token, 1), 100);

    // Second report for same period, override=false → must return Ok but not change value
    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &999_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );
    assert!(result.is_ok(), "Duplicate with override=false must return Ok");
    assert_eq!(
        get_revenue(&client, &issuer, &token, 1),
        100,
        "Original amount must be unchanged when override=false"
    );
}

// ---------------------------------------------------------------------------
// 8. Duplicate period, override_existing=true → overwrites
// ---------------------------------------------------------------------------

#[test]
fn rwat_duplicate_period_override_true_overwrites() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    client.report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &100_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );

    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &250_i128, &1u64, &true,
        &empty_quote(&env), &dummy_sig(&env),
    );
    assert!(result.is_ok(), "Override=true must succeed, got {:?}", result);
    assert_eq!(
        get_revenue(&client, &issuer, &token, 1),
        250,
        "Amount must be updated to 250 after override"
    );
}

// ---------------------------------------------------------------------------
// 9. Override on missing period → MissingReportForOverride
// ---------------------------------------------------------------------------

#[test]
fn rwat_override_missing_period_rejected() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    use crate::RevoraError;
    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &100_i128, &1u64, &true,  // override=true but no period 1 exists
        &empty_quote(&env), &dummy_sig(&env),
    );

    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::MissingReportForOverride,
        "Override on missing period must return MissingReportForOverride"
    );
    assert_eq!(get_revenue(&client, &issuer, &token, 1), 0);
}

// ---------------------------------------------------------------------------
// 10. Sequential periods 1, 2, 3 all stored independently
// ---------------------------------------------------------------------------

#[test]
fn rwat_sequential_periods_stored_independently() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    for (pid, amt) in [(1u64, 100_i128), (2, 200), (3, 300)] {
        client.report_revenue_with_attestation(
            &issuer, &ns, &token, &payout_asset,
            &amt, &pid, &false,
            &empty_quote(&env), &dummy_sig(&env),
        );
    }

    assert_eq!(get_revenue(&client, &issuer, &token, 1), 100);
    assert_eq!(get_revenue(&client, &issuer, &token, 2), 200);
    assert_eq!(get_revenue(&client, &issuer, &token, 3), 300);
}

// ---------------------------------------------------------------------------
// 11. Amount = 0 (valid boundary)
// ---------------------------------------------------------------------------

#[test]
fn rwat_zero_amount_accepted() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &0_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );
    assert!(result.is_ok(), "amount=0 must be accepted, got {:?}", result);
    assert_eq!(get_revenue(&client, &issuer, &token, 1), 0);
}

// ---------------------------------------------------------------------------
// 12. Amount = i128::MAX (boundary)
// ---------------------------------------------------------------------------

#[test]
fn rwat_max_amount_accepted() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &i128::MAX, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );
    assert!(result.is_ok(), "amount=i128::MAX must be accepted, got {:?}", result);
    assert_eq!(get_revenue(&client, &issuer, &token, 1), i128::MAX);
}

// ---------------------------------------------------------------------------
// 13. Close-period gate: override on sealed period → PeriodAlreadyClosed
// ---------------------------------------------------------------------------

#[test]
fn rwat_override_sealed_period_rejected() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    // Write the initial report
    client.report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &100_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );

    // Seal period 1
    client.close_period(&issuer, &ns, &token, &1u64);

    // Attempt to override a sealed period
    use crate::RevoraError;
    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &999_i128, &1u64, &true,
        &empty_quote(&env), &dummy_sig(&env),
    );

    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::PeriodAlreadyClosed,
        "Override on sealed period must return PeriodAlreadyClosed"
    );
    // Original amount intact
    assert_eq!(get_revenue(&client, &issuer, &token, 1), 100);
}

// ---------------------------------------------------------------------------
// 14. Mismatched payout_asset without any oracle config → PayoutAssetMismatch
// ---------------------------------------------------------------------------

#[test]
fn rwat_mismatched_payout_asset_without_oracle_returns_error() {
    let env = Env::default();
    let (client, issuer, token, _payout_asset) = setup(&env);
    let ns = symbol_short!("def");
    // A *different* asset — not the one registered as the offering's payout_asset
    let foreign_asset = Address::generate(&env);

    use crate::RevoraError;
    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &foreign_asset,
        &100_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );

    // Without an oracle or oracle chain configured, a different payout_asset
    // should return PayoutAssetMismatch (or AllOraclesStale, depending on
    // whether any oracle chain is configured — but storage must be unchanged).
    let err = result.unwrap_err().unwrap();
    assert!(
        err == RevoraError::PayoutAssetMismatch || err == RevoraError::AllOraclesStale,
        "Expected PayoutAssetMismatch or AllOraclesStale for unknown payout asset, got {:?}", err
    );
    // Storage must be unchanged
    assert_eq!(get_revenue(&client, &issuer, &token, 1), 0);
}

// ---------------------------------------------------------------------------
// 15. AuditSummary.total_revenue increments on new report
// ---------------------------------------------------------------------------

#[test]
fn rwat_audit_summary_total_revenue_increments() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    client.report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &500_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );

    let summary = client.get_audit_summary(&issuer, &ns, &token)
        .expect("AuditSummary should exist after first report");
    assert_eq!(summary.total_revenue, 500, "total_revenue must equal reported amount");
    assert_eq!(summary.report_count, 1, "report_count must be 1 after first report");
}

// ---------------------------------------------------------------------------
// 16. AuditSummary.report_count unchanged on override
// ---------------------------------------------------------------------------

#[test]
fn rwat_audit_summary_report_count_unchanged_on_override() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    client.report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &100_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );

    client.report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &999_i128, &1u64, &true,
        &empty_quote(&env), &dummy_sig(&env),
    );

    let summary = client.get_audit_summary(&issuer, &ns, &token)
        .expect("AuditSummary should exist");
    assert_eq!(
        summary.report_count, 1,
        "report_count must not increase on override"
    );
    assert_eq!(
        summary.total_revenue, 999,
        "total_revenue must reflect the overridden amount"
    );
}

// ---------------------------------------------------------------------------
// 17. quote_bytes + signature are irrelevant when payout_asset matches
// ---------------------------------------------------------------------------

#[test]
fn rwat_arbitrary_quote_and_sig_accepted_when_same_payout_asset() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    // Non-empty garbage bytes
    let garbage_quote = Bytes::from_slice(&env, &[0xDE, 0xAD, 0xBE, 0xEF]);
    let garbage_sig = BytesN::from_array(&env, &[0xFF; 64]);

    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &42_i128, &1u64, &false,
        &garbage_quote, &garbage_sig,
    );
    assert!(
        result.is_ok(),
        "Garbage quote/sig must be ignored when payout_asset matches offering, got {:?}", result
    );
    assert_eq!(get_revenue(&client, &issuer, &token, 1), 42);
}

// ---------------------------------------------------------------------------
// 18. Period cursor is monotonic — period 3 rejected if period 1 was last
// ---------------------------------------------------------------------------

#[test]
fn rwat_non_sequential_period_rejected() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup(&env);
    let ns = symbol_short!("def");

    // Report period 1
    client.report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &100_i128, &1u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );

    // Skip period 2 — attempt period 3 directly; should be rejected
    use crate::RevoraError;
    let result = client.try_report_revenue_with_attestation(
        &issuer, &ns, &token, &payout_asset,
        &300_i128, &3u64, &false,
        &empty_quote(&env), &dummy_sig(&env),
    );

    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::InvalidPeriodId,
        "Skipping a period must return InvalidPeriodId"
    );
    // Verify period 3 was not written
    assert_eq!(get_revenue(&client, &issuer, &token, 3), 0);
}
