//! Adversarial coverage for [`AmountValidationMatrix::validate_snapshot_monotonic`].
//!
//! The helper enforces strictly increasing snapshot references. Its contract:
//! accept only `new_ref > last_ref`, and reject every non-increasing pair
//! (including an exact replay) with `OutdatedSnapshot`. These tests pin the
//! boundary matrix across the full `i128` range.

#![cfg(test)]

use crate::{AmountValidationMatrix, RevoraError};

#[test]
fn accepts_strictly_increasing_references() {
    assert_eq!(AmountValidationMatrix::validate_snapshot_monotonic(1, 0), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_snapshot_monotonic(2, 1), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_snapshot_monotonic(0, -1), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_snapshot_monotonic(-1, -2), Ok(()));
}

#[test]
fn rejects_an_exact_replay() {
    assert_eq!(
        AmountValidationMatrix::validate_snapshot_monotonic(0, 0),
        Err(RevoraError::OutdatedSnapshot)
    );
    assert_eq!(
        AmountValidationMatrix::validate_snapshot_monotonic(42, 42),
        Err(RevoraError::OutdatedSnapshot)
    );
    assert_eq!(
        AmountValidationMatrix::validate_snapshot_monotonic(-7, -7),
        Err(RevoraError::OutdatedSnapshot)
    );
}

#[test]
fn rejects_a_regressing_reference() {
    assert_eq!(
        AmountValidationMatrix::validate_snapshot_monotonic(0, 1),
        Err(RevoraError::OutdatedSnapshot)
    );
    assert_eq!(
        AmountValidationMatrix::validate_snapshot_monotonic(-2, -1),
        Err(RevoraError::OutdatedSnapshot)
    );
    assert_eq!(
        AmountValidationMatrix::validate_snapshot_monotonic(-1, 0),
        Err(RevoraError::OutdatedSnapshot)
    );
}

#[test]
fn handles_the_i128_boundaries() {
    assert_eq!(
        AmountValidationMatrix::validate_snapshot_monotonic(i128::MIN + 1, i128::MIN),
        Ok(())
    );
    assert_eq!(
        AmountValidationMatrix::validate_snapshot_monotonic(i128::MAX, i128::MAX - 1),
        Ok(())
    );
    assert_eq!(AmountValidationMatrix::validate_snapshot_monotonic(i128::MAX, i128::MIN), Ok(()));
    assert_eq!(
        AmountValidationMatrix::validate_snapshot_monotonic(i128::MIN, i128::MAX),
        Err(RevoraError::OutdatedSnapshot)
    );
    assert_eq!(
        AmountValidationMatrix::validate_snapshot_monotonic(i128::MAX, i128::MAX),
        Err(RevoraError::OutdatedSnapshot)
    );
    assert_eq!(
        AmountValidationMatrix::validate_snapshot_monotonic(i128::MIN, i128::MIN),
        Err(RevoraError::OutdatedSnapshot)
    );
}

#[test]
fn replay_rejection_is_stable_across_repeated_calls() {
    for _ in 0..3 {
        assert_eq!(
            AmountValidationMatrix::validate_snapshot_monotonic(5, 5),
            Err(RevoraError::OutdatedSnapshot)
        );
        assert_eq!(AmountValidationMatrix::validate_snapshot_monotonic(6, 5), Ok(()));
    }
}
