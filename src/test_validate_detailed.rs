//! Focused adversarial coverage for `AmountValidationMatrix::validate_detailed`
//! (issue #1029).
//!
//! `validate_detailed` is the context-preserving wrapper around the amount
//! validation matrix: unlike `validate`, it never returns a bare `Result` — it
//! always echoes the original `amount` and `category` and classifies the
//! outcome with an `is_valid` flag, an optional `error_code`, and a machine
//! readable `reason` symbol. These tests pin the success, boundary, and
//! rejection paths for every category.

#![cfg(test)]

use super::*;
use soroban_sdk::{symbol_short, Symbol};

fn validate(amount: i128, category: AmountValidationCategory) -> AmountValidationResult {
    AmountValidationMatrix::validate_detailed(amount, category)
}

fn assert_valid(amount: i128, category: AmountValidationCategory) {
    let r = validate(amount, category);
    assert!(r.is_valid, "expected {:?} to accept {}", category, amount);
    assert_eq!(r.amount, amount, "amount must be echoed back unchanged");
    assert_eq!(r.category, category, "category must be echoed back unchanged");
    assert_eq!(r.error_code, None, "a valid result must not carry an error code");
    assert_eq!(r.reason, symbol_short!("valid"));
}

fn assert_invalid(
    amount: i128,
    category: AmountValidationCategory,
    code: RevoraError,
    reason: Symbol,
) {
    let r = validate(amount, category);
    assert!(!r.is_valid, "expected {:?} to reject {}", category, amount);
    assert_eq!(r.amount, amount);
    assert_eq!(r.category, category);
    assert_eq!(r.error_code, Some(code as u32));
    assert_eq!(r.reason, reason);
}

#[test]
fn revenue_deposit_requires_strictly_positive() {
    assert_valid(1, AmountValidationCategory::RevenueDeposit);
    assert_valid(i128::MAX, AmountValidationCategory::RevenueDeposit);
    assert_invalid(
        0,
        AmountValidationCategory::RevenueDeposit,
        RevoraError::InvalidAmount,
        symbol_short!("must_pos"),
    );
    assert_invalid(
        -1,
        AmountValidationCategory::RevenueDeposit,
        RevoraError::InvalidAmount,
        symbol_short!("must_pos"),
    );
    assert_invalid(
        i128::MIN,
        AmountValidationCategory::RevenueDeposit,
        RevoraError::InvalidAmount,
        symbol_short!("must_pos"),
    );
}

#[test]
fn zero_tolerant_categories_accept_zero_and_reject_negative() {
    let zero_ok = [
        AmountValidationCategory::RevenueReport,
        AmountValidationCategory::HolderShare,
        AmountValidationCategory::MinRevenueThreshold,
        AmountValidationCategory::SupplyCap,
        AmountValidationCategory::InvestmentMinStake,
        AmountValidationCategory::InvestmentMaxStake,
        AmountValidationCategory::MaxTotalSupplyShares,
    ];
    for category in zero_ok {
        assert_valid(0, category);
        assert_invalid(-1, category, RevoraError::InvalidAmount, symbol_short!("no_neg"));
    }
}

#[test]
fn snapshot_reference_requires_strictly_positive() {
    assert_valid(1, AmountValidationCategory::SnapshotReference);
    assert_invalid(
        0,
        AmountValidationCategory::SnapshotReference,
        RevoraError::InvalidAmount,
        symbol_short!("snap_pos"),
    );
    assert_invalid(
        -1,
        AmountValidationCategory::SnapshotReference,
        RevoraError::InvalidAmount,
        symbol_short!("snap_pos"),
    );
}

#[test]
fn period_id_rejects_negative_with_period_error() {
    assert_valid(0, AmountValidationCategory::PeriodId);
    assert_valid(42, AmountValidationCategory::PeriodId);
    assert_invalid(
        -1,
        AmountValidationCategory::PeriodId,
        RevoraError::InvalidPeriodId,
        symbol_short!("no_neg"),
    );
}

#[test]
fn simulation_accepts_every_i128() {
    for amount in [i128::MIN, -1, 0, 1, i128::MAX] {
        assert_valid(amount, AmountValidationCategory::Simulation);
    }
}

#[test]
fn detailed_result_never_disagrees_with_the_matrix() {
    let categories = [
        AmountValidationCategory::RevenueDeposit,
        AmountValidationCategory::RevenueReport,
        AmountValidationCategory::HolderShare,
        AmountValidationCategory::MinRevenueThreshold,
        AmountValidationCategory::SupplyCap,
        AmountValidationCategory::InvestmentMinStake,
        AmountValidationCategory::InvestmentMaxStake,
        AmountValidationCategory::SnapshotReference,
        AmountValidationCategory::PeriodId,
        AmountValidationCategory::Simulation,
        AmountValidationCategory::MaxTotalSupplyShares,
    ];
    for category in categories {
        for amount in [i128::MIN, -1, 0, 1, i128::MAX] {
            let detailed = validate(amount, category);
            let plain = AmountValidationMatrix::validate(amount, category);
            assert_eq!(
                detailed.is_valid,
                plain.is_ok(),
                "detailed/plain mismatch for {:?} at {}",
                category,
                amount,
            );
        }
    }
}
