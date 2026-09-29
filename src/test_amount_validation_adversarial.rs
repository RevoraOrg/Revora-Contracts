#![cfg(test)]
extern crate std;

use crate::{
    AmountValidationCategory, AmountValidationMatrix, RevoraError,
};
use soroban_sdk::{symbol_short, Env};

#[test]
fn test_validate_revenue_deposit_adversarial() {
    let _env = Env::default();

    // Amount <= 0 fails
    assert_eq!(
        AmountValidationMatrix::validate(0, AmountValidationCategory::RevenueDeposit),
        Err((RevoraError::InvalidAmount, symbol_short!("must_pos")))
    );
    assert_eq!(
        AmountValidationMatrix::validate(-1, AmountValidationCategory::RevenueDeposit),
        Err((RevoraError::InvalidAmount, symbol_short!("must_pos")))
    );
    assert_eq!(
        AmountValidationMatrix::validate(i128::MIN, AmountValidationCategory::RevenueDeposit),
        Err((RevoraError::InvalidAmount, symbol_short!("must_pos")))
    );

    // Valid path
    assert_eq!(
        AmountValidationMatrix::validate(1, AmountValidationCategory::RevenueDeposit),
        Ok(())
    );
    assert_eq!(
        AmountValidationMatrix::validate(i128::MAX, AmountValidationCategory::RevenueDeposit),
        Ok(())
    );
}

#[test]
fn test_validate_no_neg_invalid_amount_categories() {
    let _env = Env::default();
    
    let categories = [
        AmountValidationCategory::RevenueReport,
        AmountValidationCategory::HolderShare,
        AmountValidationCategory::MinRevenueThreshold,
        AmountValidationCategory::SupplyCap,
        AmountValidationCategory::InvestmentMinStake,
        AmountValidationCategory::InvestmentMaxStake,
        AmountValidationCategory::MaxTotalSupplyShares,
    ];

    for category in categories {
        // Invalid path
        assert_eq!(
            AmountValidationMatrix::validate(-1, category),
            Err((RevoraError::InvalidAmount, symbol_short!("no_neg")))
        );
        assert_eq!(
            AmountValidationMatrix::validate(i128::MIN, category),
            Err((RevoraError::InvalidAmount, symbol_short!("no_neg")))
        );

        // Valid path
        assert_eq!(AmountValidationMatrix::validate(0, category), Ok(()));
        assert_eq!(AmountValidationMatrix::validate(1, category), Ok(()));
        assert_eq!(AmountValidationMatrix::validate(i128::MAX, category), Ok(()));
    }
}

#[test]
fn test_validate_period_id() {
    let _env = Env::default();
    
    // Invalid path
    assert_eq!(
        AmountValidationMatrix::validate(-1, AmountValidationCategory::PeriodId),
        Err((RevoraError::InvalidPeriodId, symbol_short!("no_neg")))
    );
    assert_eq!(
        AmountValidationMatrix::validate(i128::MIN, AmountValidationCategory::PeriodId),
        Err((RevoraError::InvalidPeriodId, symbol_short!("no_neg")))
    );

    // Valid path
    assert_eq!(AmountValidationMatrix::validate(0, AmountValidationCategory::PeriodId), Ok(()));
    assert_eq!(AmountValidationMatrix::validate(1, AmountValidationCategory::PeriodId), Ok(()));
    assert_eq!(AmountValidationMatrix::validate(i128::MAX, AmountValidationCategory::PeriodId), Ok(()));
}

#[test]
fn test_validate_snapshot_reference() {
    let _env = Env::default();
    
    // Amount <= 0 fails
    assert_eq!(
        AmountValidationMatrix::validate(0, AmountValidationCategory::SnapshotReference),
        Err((RevoraError::InvalidAmount, symbol_short!("snap_pos")))
    );
    assert_eq!(
        AmountValidationMatrix::validate(-1, AmountValidationCategory::SnapshotReference),
        Err((RevoraError::InvalidAmount, symbol_short!("snap_pos")))
    );
    assert_eq!(
        AmountValidationMatrix::validate(i128::MIN, AmountValidationCategory::SnapshotReference),
        Err((RevoraError::InvalidAmount, symbol_short!("snap_pos")))
    );

    // Valid path
    assert_eq!(AmountValidationMatrix::validate(1, AmountValidationCategory::SnapshotReference), Ok(()));
    assert_eq!(AmountValidationMatrix::validate(i128::MAX, AmountValidationCategory::SnapshotReference), Ok(()));
}

#[test]
fn test_validate_simulation() {
    let _env = Env::default();
    
    // Any amount is valid
    assert_eq!(AmountValidationMatrix::validate(i128::MIN, AmountValidationCategory::Simulation), Ok(()));
    assert_eq!(AmountValidationMatrix::validate(-1, AmountValidationCategory::Simulation), Ok(()));
    assert_eq!(AmountValidationMatrix::validate(0, AmountValidationCategory::Simulation), Ok(()));
    assert_eq!(AmountValidationMatrix::validate(1, AmountValidationCategory::Simulation), Ok(()));
    assert_eq!(AmountValidationMatrix::validate(i128::MAX, AmountValidationCategory::Simulation), Ok(()));
}

#[test]
fn test_validate_stake_range() {
    // min > max fails (when max > 0)
    assert_eq!(AmountValidationMatrix::validate_stake_range(200, 100), Err(RevoraError::InvalidAmount));
    assert_eq!(AmountValidationMatrix::validate_stake_range(i128::MAX, 1), Err(RevoraError::InvalidAmount));

    // Valid paths
    assert_eq!(AmountValidationMatrix::validate_stake_range(100, 200), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_stake_range(100, 100), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_stake_range(100, 0), Ok(())); // max=0 means unlimited
    assert_eq!(AmountValidationMatrix::validate_stake_range(0, 0), Ok(()));
}

#[test]
fn test_validate_snapshot_monotonic() {
    // new <= last fails
    assert_eq!(AmountValidationMatrix::validate_snapshot_monotonic(100, 100), Err(RevoraError::OutdatedSnapshot));
    assert_eq!(AmountValidationMatrix::validate_snapshot_monotonic(99, 100), Err(RevoraError::OutdatedSnapshot));
    assert_eq!(AmountValidationMatrix::validate_snapshot_monotonic(i128::MIN, 100), Err(RevoraError::OutdatedSnapshot));

    // Valid path
    assert_eq!(AmountValidationMatrix::validate_snapshot_monotonic(101, 100), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_snapshot_monotonic(i128::MAX, 100), Ok(()));
}

#[test]
fn test_validate_detailed() {
    let _env = Env::default();
    
    // Error case
    let res_err = AmountValidationMatrix::validate_detailed(-1, AmountValidationCategory::RevenueDeposit);
    assert_eq!(res_err.amount, -1);
    assert_eq!(res_err.category, AmountValidationCategory::RevenueDeposit);
    assert_eq!(res_err.is_valid, false);
    assert_eq!(res_err.error_code, Some(RevoraError::InvalidAmount as u32));
    assert_eq!(res_err.reason, symbol_short!("must_pos"));

    // Success case
    let res_ok = AmountValidationMatrix::validate_detailed(1, AmountValidationCategory::RevenueDeposit);
    assert_eq!(res_ok.amount, 1);
    assert_eq!(res_ok.category, AmountValidationCategory::RevenueDeposit);
    assert_eq!(res_ok.is_valid, true);
    assert_eq!(res_ok.error_code, None);
    assert_eq!(res_ok.reason, symbol_short!("valid"));
}

#[test]
fn test_validate_batch() {
    let _env = Env::default();
    
    let amounts_ok = [1, 2, 3, 4, 5];
    assert_eq!(AmountValidationMatrix::validate_batch(&amounts_ok, AmountValidationCategory::RevenueDeposit), None);

    let amounts_err = [1, 2, -3, 4, 5];
    assert_eq!(AmountValidationMatrix::validate_batch(&amounts_err, AmountValidationCategory::RevenueDeposit), Some(2));
    
    let amounts_err_zero = [1, 2, 0, 4, 5];
    assert_eq!(AmountValidationMatrix::validate_batch(&amounts_err_zero, AmountValidationCategory::RevenueDeposit), Some(2));
}
