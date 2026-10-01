//! Adversarial coverage for [`AmountValidationMatrix::validate_stake_range`].
//!
//! The helper guards the `[min_stake, max_stake]` investment window. Its
//! contract: reject with `InvalidAmount` only when a positive maximum is
//! strictly smaller than the minimum; treat an unset/non-positive maximum as an
//! open upper bound. These tests pin the full boundary matrix, including the
//! `i128` extremes.

#![cfg(test)]

use crate::{AmountValidationMatrix, RevoraError};

#[test]
fn accepts_ordered_ranges() {
    assert_eq!(AmountValidationMatrix::validate_stake_range(0, 0), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_stake_range(1, 1), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_stake_range(1, 2), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_stake_range(-5, 10), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_stake_range(0, 1), Ok(()));
}

#[test]
fn rejects_inverted_ranges_with_a_positive_maximum() {
    assert_eq!(AmountValidationMatrix::validate_stake_range(2, 1), Err(RevoraError::InvalidAmount));
    assert_eq!(
        AmountValidationMatrix::validate_stake_range(10, 9),
        Err(RevoraError::InvalidAmount)
    );
    assert_eq!(AmountValidationMatrix::validate_stake_range(1, 0), Ok(())); // max == 0 is treated as "no upper bound", not inverted
}

#[test]
fn treats_non_positive_maximum_as_an_open_upper_bound() {
    // max_stake <= 0 means "unset"; ordering is not enforced.
    assert_eq!(AmountValidationMatrix::validate_stake_range(5, 0), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_stake_range(i128::MAX, 0), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_stake_range(5, -1), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_stake_range(0, -1), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_stake_range(i128::MAX, -1), Ok(()));
}

#[test]
fn rejection_is_not_symmetric() {
    // The ordering guard is directional: swapping the arguments flips the result.
    assert_eq!(AmountValidationMatrix::validate_stake_range(5, 1), Err(RevoraError::InvalidAmount));
    assert_eq!(AmountValidationMatrix::validate_stake_range(1, 5), Ok(()));
}

#[test]
fn handles_the_i128_boundaries() {
    assert_eq!(AmountValidationMatrix::validate_stake_range(i128::MIN, i128::MAX), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_stake_range(i128::MIN, i128::MIN), Ok(()));
    assert_eq!(AmountValidationMatrix::validate_stake_range(i128::MAX, i128::MAX), Ok(()));
    // A non-positive maximum leaves the pair valid regardless of the minimum.
    assert_eq!(AmountValidationMatrix::validate_stake_range(i128::MAX, i128::MIN), Ok(()));
    // The one inverted case that is still rejected when the max is positive.
    assert_eq!(
        AmountValidationMatrix::validate_stake_range(i128::MAX, 1),
        Err(RevoraError::InvalidAmount)
    );
}

#[test]
fn rejection_never_mutates_or_panics_on_repeated_calls() {
    // Pure helper: repeated adversarial calls stay deterministic.
    for _ in 0..3 {
        assert_eq!(
            AmountValidationMatrix::validate_stake_range(7, 3),
            Err(RevoraError::InvalidAmount)
        );
        assert_eq!(AmountValidationMatrix::validate_stake_range(3, 7), Ok(()));
    }
}
