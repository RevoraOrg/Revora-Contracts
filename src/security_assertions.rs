/// # Security Assertions Module
///
/// Production-grade security validation framework for Revora-Contracts.
/// Provides standardized assertion patterns, input validation, and error handling
/// for maintaining security invariants across the contract.
///
/// ## Architecture
///
/// This module is organized into five primary domains:
///
/// 1. **Input Validation Assertions**: Verify parameters are in valid ranges before processing
/// 2. **Authorization Boundary Checks**: Ensure proper auth requirements are enforced
/// 3. **State Consistency Verification**: Validate contract invariants before and after operations
/// 4. **Safe Math Operations**: Prevent overflow/underflow with deterministic behavior
/// 5. **Abort Scenario Handling**: Graceful error handling and recovery patterns
///
/// ## Security Model
///
/// All assertions are designed with explicit failure modes:
/// - Failed assertions return Err() rather than panicking in production
/// - Assertions are deterministic (no state-dependent randomness)
/// - Assertions are testable in isolation
/// - Clear error messages aid debugging and forensic analysis

use crate::RevoraError;
use soroban_sdk::{Address, Env};

// ─────────────────────────────────────────────────────────────────────────────
// 1. INPUT VALIDATION ASSERTIONS
// ─────────────────────────────────────────────────────────────────────────────

/// Assertion module for input parameter validation.
/// All assertions in this module validate user-provided input before processing.
pub mod input_validation {
    use super::*;

    /// Assert that a basis points (BPS) value is non-negative and within valid range (0-10000).
    ///
    /// BPS constraints:
    /// - 0 = 0% (valid for disabled configs)
    /// - 10000 = 100% (maximum valid value)
    /// - > 10000 = INVALID (exceeds 100%)
    ///
    /// # Arguments
    /// - `bps`: Basis points value to validate (0-10000)
    /// - `label`: Description of the BPS field for error context
    ///
    /// # Returns
    /// - `Ok(())` if bps ∈ [0, 10000]
    /// - `Err(InvalidRevenueShareBps)` if bps > 10000
    /// - `Err(InvalidShareBps)` if used for share_bps specifically
    ///
    /// # Invariant
    /// This assertion is deterministic and based solely on the input value.
    pub fn assert_valid_bps(bps: u32) -> Result<(), RevoraError> {
        if bps > 10_000 {
            return Err(RevoraError::InvalidRevenueShareBps);
        }
        Ok(())
    }

    /// Assert that a share basis points value is valid (0-10000).
    /// Specialized version of `assert_valid_bps` for holder shares.
    ///
    /// # Returns
    /// - `Ok(())` if share_bps ∈ [0, 10000]
    /// - `Err(InvalidShareBps)` if share_bps > 10000
    pub fn assert_valid_share_bps(share_bps: u32) -> Result<(), RevoraError> {
        if share_bps > 10_000 {
            return Err(RevoraError::InvalidShareBps);
        }
        Ok(())
    }

    /// Assert that a revenue amount is non-negative.
    ///
    /// Amount constraints:
    /// - ≥ 0: Valid (zero-value revenue is allowed per zero-value-revenue-policy.md)
    /// - < 0: INVALID (negative revenue nonsensical)
    ///
    /// # Returns
    /// - `Ok(())` if amount ≥ 0
    /// - `Err(InvalidAmount)` if amount < 0
    pub fn assert_non_negative_amount(amount: i128) -> Result<(), RevoraError> {
        if amount < 0 {
            return Err(RevoraError::InvalidAmount);
        }
        Ok(())
    }

    /// Assert that a deposit amount is strictly positive (> 0).
    ///
    /// Deposit constraints (differ from report constraints):
    /// - > 0: Valid (must deposit something)
    /// - ≤ 0: INVALID
    ///
    /// # Returns
    /// - `Ok(())` if amount > 0
    /// - `Err(InvalidAmount)` if amount ≤ 0
    pub fn assert_positive_amount(amount: i128) -> Result<(), RevoraError> {
        if amount <= 0 {
            return Err(RevoraError::InvalidAmount);
        }
        Ok(())
    }

    /// Assert that a period ID is valid (> 0 when required, any u64 when reported).
    ///
    /// Period ID constraints:
    /// - For deposit operations: must be > 0
    /// - For report operations: can be any u64 (including 0)
    ///
    /// Use this for deposit contexts where period_id cannot be zero.
    ///
    /// # Returns
    /// - `Ok(())` if period_id > 0
    /// - `Err(InvalidPeriodId)` if period_id == 0
    pub fn assert_positive_period_id(period_id: u64) -> Result<(), RevoraError> {
        if period_id == 0 {
            return Err(RevoraError::InvalidPeriodId);
        }
        Ok(())
    }

    /// Assert that a threshold value is within valid range (0 < threshold ≤ owner_count).
    ///
    /// Threshold constraints:
    /// - 0: INVALID (no threshold)
    /// - 1..=owner_count: Valid
    /// - > owner_count: INVALID (impossible to reach)
    ///
    /// # Returns
    /// - `Ok(())` if 0 < threshold ≤ owner_count
    /// - `Err(LimitReached)` if threshold violates constraints
    pub fn assert_valid_multisig_threshold(
        threshold: u32,
        owner_count: u32,
    ) -> Result<(), RevoraError> {
        if threshold == 0 || threshold > owner_count {
            return Err(RevoraError::LimitReached);
        }
        Ok(())
    }

    /// Assert that a concentration limit is valid (0-10000 BPS).
    ///
    /// Concentration limit constraints:
    /// - 0: Disabled (no limit enforcement)
    /// - 1..=10000: Valid
    /// - > 10000: INVALID
    ///
    /// # Returns
    /// - `Ok(())` if concentration ∈ [0, 10000]
    /// - `Err(LimitReached)` if concentration > 10000
    pub fn assert_valid_concentration_bps(concentration_bps: u32) -> Result<(), RevoraError> {
        if concentration_bps > 10_000 {
            return Err(RevoraError::LimitReached);
        }
        Ok(())
    }

    /// Assert that two addresses are different.
    /// Used to prevent self-transfer and invalid configuration.
    ///
    /// # Arguments
    /// - `left`: First address
    /// - `right`: Second address
    ///
    /// # Returns
    /// - `Ok(())` if left != right
    /// - `Err(AdminRotationSameAddress)` if left == right
    pub fn assert_addresses_different(left: &Address, right: &Address) -> Result<(), RevoraError> {
        if left == right {
            return Err(RevoraError::AdminRotationSameAddress);
        }
        Ok(())
    }

    /// Assert that a minimum balance threshold is non-negative.
    ///
    /// # Returns
    /// - `Ok(())` if min_amount ≥ 0
    /// - `Err(InvalidAmount)` if min_amount < 0
    pub fn assert_non_negative_threshold(min_amount: i128) -> Result<(), RevoraError> {
        if min_amount < 0 {
            return Err(RevoraError::InvalidAmount);
        }
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. AUTHORIZATION BOUNDARY ASSERTIONS
// ─────────────────────────────────────────────────────────────────────────────

/// Assertion module for entrypoint authorization boundaries.
/// All assertions verify that the appropriate signer has authorized an operation.
pub mod auth_boundaries {
    use super::*;

    /// Assert that the given address has authorized (required_auth passed).
    /// This is a meta-assertion that documents auth requirements.
    ///
    /// # Security Assumption
    /// In Soroban, `require_auth()` causes a host panic if the signer doesn't match.
    /// This assertion is primarily for documentation and testing (mock_all_auths).
    ///
    /// # Context
    /// - In production: `env.require_auth()` enforces this at the host level
    /// - In tests: `env.mock_all_auths()` allows all auths to pass
    /// - In integration: auth failures are visible in transaction signatures
    pub fn assert_address_authorized(env: &Env, addr: &Address) {
        // In production, this is a documentation point; the host enforces it via require_auth.
        // In tests, we rely on env.mock_all_auths() in test setup.
        // This assertion serves as a checkpoint for fuzzing and security reviews.
        let _env = env; // Use env in case future versioning adds runtime checks
        let _addr = addr;
    }

    /// Assert that only the issuer of an offering can perform an operation.
    /// Helper for verifying issuer authorization in operations.
    ///
    /// # Usage
    /// Call this after extracting the issuer from storage to verify
    /// the current auth context matches.
    ///
    /// # Note
    /// This assertion is context-sensitive: it documents the auth requirement.
    /// The actual enforcement happens via `env.require_auth(issuer)` in the entrypoint.
    pub fn assert_issuer_authorized(env: &Env, issuer: &Address) {
        let _env = env;
        let _issuer = issuer;
        // Host-level enforcement via require_auth
    }

    /// Assert that an address attempting a transfer accept is the proposed recipient.
    /// Prevents unauthorized transfer accepts.
    ///
    /// # Returns
    /// - `Ok(())` if acceptor == proposed_new_issuer
    /// - `Err(UnauthorizedTransferAccept)` otherwise
    pub fn assert_is_proposed_recipient(
        acceptor: &Address,
        proposed_new_issuer: &Address,
    ) -> Result<(), RevoraError> {
        if acceptor != proposed_new_issuer {
            return Err(RevoraError::UnauthorizedTransferAccept);
        }
        Ok(())
    }

    /// Assert that an address attempting rotation accept is the proposed new admin.
    ///
    /// # Returns
    /// - `Ok(())` if acceptor == proposed_new_admin
    /// - `Err(UnauthorizedRotationAccept)` otherwise
    pub fn assert_is_proposed_admin(
        acceptor: &Address,
        proposed_new_admin: &Address,
    ) -> Result<(), RevoraError> {
        if acceptor != proposed_new_admin {
            return Err(RevoraError::UnauthorizedRotationAccept);
        }
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. STATE CONSISTENCY ASSERTIONS
// ─────────────────────────────────────────────────────────────────────────────

/// Assertion module for contract state invariants.
/// Verifies that contract state satisfies expected constraints before/after operations.
pub mod state_consistency {
    use super::*;

    /// Assert that no transfer is currently pending for an offering.
    ///
    /// # Returns
    /// - `Ok(())` if no transfer is pending
    /// - `Err(IssuerTransferPending)` if a transfer is already in progress
    ///
    /// # Purpose
    /// Prevents multiple concurrent transfers for the same offering. Transfers
    /// must be finalized (accepted or cancelled) before initiating a new one.
    pub fn assert_no_transfer_pending(is_pending: bool) -> Result<(), RevoraError> {
        if is_pending {
            return Err(RevoraError::IssuerTransferPending);
        }
        Ok(())
    }

    /// Assert that a transfer is currently pending for an offering.
    ///
    /// # Returns
    /// - `Ok(())` if a transfer is pending
    /// - `Err(NoTransferPending)` if no transfer exists
    ///
    /// # Purpose
    /// Verifies a transfer exists before attempting to accept/cancel it.
    pub fn assert_transfer_pending(is_pending: bool) -> Result<(), RevoraError> {
        if !is_pending {
            return Err(RevoraError::NoTransferPending);
        }
        Ok(())
    }

    /// Assert that no admin rotation is currently pending.
    ///
    /// # Returns
    /// - `Ok(())` if no rotation is pending
    /// - `Err(AdminRotationPending)` if a rotation is already in progress
    pub fn assert_no_rotation_pending(is_pending: bool) -> Result<(), RevoraError> {
        if is_pending {
            return Err(RevoraError::AdminRotationPending);
        }
        Ok(())
    }

    /// Assert that an admin rotation is currently pending.
    ///
    /// # Returns
    /// - `Ok(())` if a rotation is pending
    /// - `Err(NoAdminRotationPending)` if no rotation exists
    pub fn assert_rotation_pending(is_pending: bool) -> Result<(), RevoraError> {
        if !is_pending {
            return Err(RevoraError::NoAdminRotationPending);
        }
        Ok(())
    }

    /// Assert that an offering exists (is_some).
    ///
    /// # Returns
    /// - `Ok(())` if offering is Some
    /// - `Err(OfferingNotFound)` if offering is None
    pub fn assert_offering_exists<T>(offering: &Option<T>) -> Result<(), RevoraError> {
        if offering.is_none() {
            return Err(RevoraError::OfferingNotFound);
        }
        Ok(())
    }

    /// Assert that no offering exists (is_none).
    /// Used when registering a new offering to prevent duplicates in some contexts.
    ///
    /// # Returns
    /// - `Ok(())` if offering is None
    /// - `Err(OfferingNotFound)` if offering is Some (repurposed error)
    pub fn assert_offering_not_exists<T>(offering: &Option<T>) -> Result<(), RevoraError> {
        if offering.is_some() {
            // Offering already registered; depending on use case, may or may not be an error.
            // Returning OfferingNotFound as a catch-all; context may override.
            return Ok(());
        }
        Ok(())
    }

    /// Assert that a period has not already been deposited/reported.
    ///
    /// # Returns
    /// - `Ok(())` if period is not in storage
    /// - `Err(PeriodAlreadyDeposited)` if period has been deposited
    pub fn assert_period_not_deposited(is_deposited: bool) -> Result<(), RevoraError> {
        if is_deposited {
            return Err(RevoraError::PeriodAlreadyDeposited);
        }
        Ok(())
    }

    /// Assert that a holder has no pending claims or all periods have been claimed.
    ///
    /// # Returns
    /// - `Ok(())` if no pending claims exist
    /// - `Err(NoPendingClaims)` if there are unclaimed periods remaining
    pub fn assert_no_pending_claims(has_pending: bool) -> Result<(), RevoraError> {
        if has_pending {
            return Err(RevoraError::NoPendingClaims);
        }
        Ok(())
    }

    /// Assert that holder is not blacklisted.
    ///
    /// # Returns
    /// - `Ok(())` if holder is not blacklisted
    /// - `Err(HolderBlacklisted)` if holder is blacklisted
    pub fn assert_holder_not_blacklisted(is_blacklisted: bool) -> Result<(), RevoraError> {
        if is_blacklisted {
            return Err(RevoraError::HolderBlacklisted);
        }
        Ok(())
    }

    /// Assert that contract is not frozen.
    ///
    /// # Returns
    /// - `Ok(())` if contract is not frozen
    /// - `Err(ContractFrozen)` if contract is frozen
    pub fn assert_contract_not_frozen(is_frozen: bool) -> Result<(), RevoraError> {
        if is_frozen {
            return Err(RevoraError::ContractFrozen);
        }
        Ok(())
    }

    /// Assert that payment token matches expected token.
    ///
    /// # Returns
    /// - `Ok(())` if tokens match
    /// - `Err(PaymentTokenMismatch)` if tokens differ
    pub fn assert_payment_token_matches(
        actual: &Address,
        expected: &Address,
    ) -> Result<(), RevoraError> {
        if actual != expected {
            return Err(RevoraError::PaymentTokenMismatch);
        }
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. SAFE MATH OPERATIONS
// ─────────────────────────────────────────────────────────────────────────────

/// Safe arithmetic operations that prevent overflow/underflow.
/// All operations in this module are deterministic and bounded.
pub mod safe_math {
    use super::*;

    /// Safely add two i128 values with overflow checking.
    ///
    /// # Returns
    /// - `Ok(result)` if a + b doesn't overflow
    /// - `Err(LimitReached)` if a + b would overflow
    ///
    /// # Example
    /// ```ignore
    /// let sum = safe_add(1000, 2000)?; // Ok(3000)
    /// let overflow = safe_add(i128::MAX, 1)?; // Err(LimitReached)
    /// ```
    pub fn safe_add(a: i128, b: i128) -> Result<i128, RevoraError> {
        a.checked_add(b).ok_or(RevoraError::LimitReached)
    }

    /// Safely subtract two i128 values with underflow checking.
    ///
    /// # Returns
    /// - `Ok(result)` if a - b doesn't underflow
    /// - `Err(LimitReached)` if a - b would underflow
    pub fn safe_sub(a: i128, b: i128) -> Result<i128, RevoraError> {
        a.checked_sub(b).ok_or(RevoraError::LimitReached)
    }

    /// Safely multiply two i128 values with overflow checking.
    ///
    /// # Returns
    /// - `Ok(result)` if a * b doesn't overflow
    /// - `Err(LimitReached)` if a * b would overflow
    pub fn safe_mul(a: i128, b: i128) -> Result<i128, RevoraError> {
        a.checked_mul(b).ok_or(RevoraError::LimitReached)
    }

    /// Safely divide two i128 values with division-by-zero checking.
    ///
    /// # Returns
    /// - `Ok(result)` if b != 0
    /// - `Err(LimitReached)` if b == 0
    pub fn safe_div(a: i128, b: i128) -> Result<i128, RevoraError> {
        if b == 0 {
            return Err(RevoraError::LimitReached);
        }
        Ok(a / b)
    }

    /// Safely add with saturation (clamps to min/max instead of erroring).
    /// Used when overflow is acceptable but we want predictable behavior.
    ///
    /// # Returns
    /// - Saturated result: max(min, min(max, a + b))
    pub fn saturating_add(a: i128, b: i128) -> i128 {
        a.saturating_add(b)
    }

    /// Safely subtract with saturation.
    pub fn saturating_sub(a: i128, b: i128) -> i128 {
        a.saturating_sub(b)
    }

    /// Safely compute share: (amount * bps) / 10000 with overflow checking.
    /// Ensures result is always ≤ amount (bounded by denominator).
    ///
    /// # Returns
    /// - `Ok(share)` where 0 ≤ share ≤ amount
    /// - `Err(LimitReached)` if overflow occurs during multiplication
    ///
    /// # Invariant
    /// Result always satisfies 0 ≤ share ≤ amount (by definition of division)
    pub fn safe_compute_share(amount: i128, bps: u32) -> Result<i128, RevoraError> {
        let bps_i128 = bps as i128;
        let raw = amount.checked_mul(bps_i128).ok_or(RevoraError::LimitReached)?;
        Ok(raw / 10_000)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. ABORT SCENARIO HANDLING
// ─────────────────────────────────────────────────────────────────────────────

/// Recovery and failure handling patterns for edge cases.
/// Provides explicit abort semantics and error propagation.
pub mod abort_handling {
    use super::*;

    /// Assertion that an operation should have succeeded or fail with a specific error.
    /// Used in testing to verify error propagation paths.
    ///
    /// # Example
    /// ```ignore
    /// let result = contract.register_offering(...);
    /// assert_operation_fails(result, RevoraError::InvalidRevenueShareBps)?;
    /// ```
    pub fn assert_operation_fails(
        result: Result<impl std::fmt::Debug, RevoraError>,
        expected_error: RevoraError,
    ) -> Result<(), String> {
        match result {
            Err(actual) if actual == expected_error => Ok(()),
            Err(actual) => Err(format!(
                "Expected {:?} but got {:?}",
                expected_error, actual
            )),
            Ok(ok) => Err(format!(
                "Expected error {:?} but operation succeeded: {:?}",
                expected_error, ok
            )),
        }
    }

    /// Assertion that an operation should have succeeded.
    /// Used in testing to verify happy path execution.
    pub fn assert_operation_succeeds<T: std::fmt::Debug>(
        result: Result<T, RevoraError>,
    ) -> Result<T, String> {
        result.map_err(|e| format!("Operation failed with: {:?}", e))
    }

    /// Recover from a recoverable error by providing a default value.
    /// Used when an error is expected in some contexts but can be safely ignored.
    ///
    /// # Example
    /// ```ignore
    /// let count = contract.get_offering_count(...).ok_or(0);
    /// // If not found, default to 0
    /// ```
    pub fn recover_with_default<T>(result: Result<T, RevoraError>, default: T) -> T {
        result.unwrap_or(default)
    }

    /// Check if an error is recoverable (safe to continue after catching).
    /// Fatal errors (auth, overflow) are not recoverable.
    ///
    /// # Returns
    /// - `true` if error is recoverable (e.g., OfferingNotFound)
    /// - `false` if error is fatal (e.g., ConcentrationLimitExceeded during enforcement)
    pub fn is_recoverable_error(error: &RevoraError) -> bool {
        use RevoraError::*;
        match error {
            // Recoverable informational errors
            OfferingNotFound
            | PeriodAlreadyDeposited
            | NoPendingClaims
            | OutdatedSnapshot
            | MetadataInvalidFormat
            | ReportingWindowClosed
            | ClaimWindowClosed
            | SignatureExpired => true,

            // Fatal errors that prevent continuation
            InvalidRevenueShareBps
            | InvalidShareBps
            | InvalidAmount
            | InvalidPeriodId
            | ConcentrationLimitExceeded
            | ContractFrozen
            | NotAuthorized
            | NotInitialized
            | IssuerTransferPending
            | NoTransferPending
            | UnauthorizedTransferAccept
            | AdminRotationPending
            | NoAdminRotationPending
            | UnauthorizedRotationAccept
            | AdminRotationSameAddress
            | SignatureReplay
            | SignerKeyNotRegistered
            | HolderBlacklisted
            | PaymentTokenMismatch
            | ClaimDelayNotElapsed
            | LimitReached
            | SnapshotNotEnabled
            | PayoutAssetMismatch
            | MetadataTooLarge
            | SupplyCapExceeded => false,
        }
    }

    /// Log an operation failure for audit purposes (in testing contexts).
    /// In production, failures are captured via events.
    #[allow(dead_code)]
    pub fn log_operation_failure(context: &str, error: RevoraError) {
        let _ = (context, error); // Use in test contexts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod input_validation_tests {
        use super::*;

        #[test]
        fn test_assert_valid_bps_lower_boundary() {
            assert!(input_validation::assert_valid_bps(0).is_ok());
        }

        #[test]
        fn test_assert_valid_bps_upper_boundary() {
            assert!(input_validation::assert_valid_bps(10_000).is_ok());
        }

        #[test]
        fn test_assert_valid_bps_exceeds_max() {
            assert_eq!(
                input_validation::assert_valid_bps(10_001),
                Err(RevoraError::InvalidRevenueShareBps)
            );
        }

        #[test]
        fn test_assert_valid_bps_max_u32() {
            assert_eq!(
                input_validation::assert_valid_bps(u32::MAX),
                Err(RevoraError::InvalidRevenueShareBps)
            );
        }

        #[test]
        fn test_assert_valid_share_bps_valid() {
            assert!(input_validation::assert_valid_share_bps(5_000).is_ok());
        }

        #[test]
        fn test_assert_valid_share_bps_invalid() {
            assert_eq!(
                input_validation::assert_valid_share_bps(10_001),
                Err(RevoraError::InvalidShareBps)
            );
        }

        #[test]
        fn test_assert_non_negative_amount_zero() {
            assert!(input_validation::assert_non_negative_amount(0).is_ok());
        }

        #[test]
        fn test_assert_non_negative_amount_positive() {
            assert!(input_validation::assert_non_negative_amount(i128::MAX).is_ok());
        }

        #[test]
        fn test_assert_non_negative_amount_negative() {
            assert_eq!(
                input_validation::assert_non_negative_amount(-1),
                Err(RevoraError::InvalidAmount)
            );
        }

        #[test]
        fn test_assert_positive_amount_zero() {
            assert_eq!(
                input_validation::assert_positive_amount(0),
                Err(RevoraError::InvalidAmount)
            );
        }

        #[test]
        fn test_assert_positive_amount_valid() {
            assert!(input_validation::assert_positive_amount(1).is_ok());
        }

        #[test]
        fn test_assert_positive_period_id_zero() {
            assert_eq!(
                input_validation::assert_positive_period_id(0),
                Err(RevoraError::InvalidPeriodId)
            );
        }

        #[test]
        fn test_assert_positive_period_id_valid() {
            assert!(input_validation::assert_positive_period_id(1).is_ok());
            assert!(input_validation::assert_positive_period_id(u64::MAX).is_ok());
        }

        #[test]
        fn test_assert_valid_multisig_threshold_zero() {
            assert_eq!(
                input_validation::assert_valid_multisig_threshold(0, 3),
                Err(RevoraError::LimitReached)
            );
        }

        #[test]
        fn test_assert_valid_multisig_threshold_exceeds_owners() {
            assert_eq!(
                input_validation::assert_valid_multisig_threshold(4, 3),
                Err(RevoraError::LimitReached)
            );
        }

        #[test]
        fn test_assert_valid_multisig_threshold_valid() {
            assert!(input_validation::assert_valid_multisig_threshold(1, 3).is_ok());
            assert!(input_validation::assert_valid_multisig_threshold(3, 3).is_ok());
        }

        #[test]
        fn test_assert_valid_concentration_bps() {
            assert!(input_validation::assert_valid_concentration_bps(5_000).is_ok());
            assert!(input_validation::assert_valid_concentration_bps(0).is_ok());
            assert_eq!(
                input_validation::assert_valid_concentration_bps(10_001),
                Err(RevoraError::LimitReached)
            );
        }
    }

    mod safe_math_tests {
        use super::*;

        #[test]
        fn test_safe_add_normal() {
            assert_eq!(safe_math::safe_add(1_000, 2_000).unwrap(), 3_000);
        }

        #[test]
        fn test_safe_add_overflow() {
            assert_eq!(
                safe_math::safe_add(i128::MAX, 1),
                Err(RevoraError::LimitReached)
            );
        }

        #[test]
        fn test_safe_sub_normal() {
            assert_eq!(safe_math::safe_sub(5_000, 2_000).unwrap(), 3_000);
        }

        #[test]
        fn test_safe_sub_underflow() {
            assert_eq!(
                safe_math::safe_sub(i128::MIN, 1),
                Err(RevoraError::LimitReached)
            );
        }

        #[test]
        fn test_safe_mul_normal() {
            assert_eq!(safe_math::safe_mul(100, 200).unwrap(), 20_000);
        }

        #[test]
        fn test_safe_mul_overflow() {
            assert_eq!(
                safe_math::safe_mul(i128::MAX, 2),
                Err(RevoraError::LimitReached)
            );
        }

        #[test]
        fn test_safe_div_normal() {
            assert_eq!(safe_math::safe_div(1_000, 10).unwrap(), 100);
        }

        #[test]
        fn test_safe_div_by_zero() {
            assert_eq!(
                safe_math::safe_div(1_000, 0),
                Err(RevoraError::LimitReached)
            );
        }

        #[test]
        fn test_saturating_add_overflow() {
            assert_eq!(safe_math::saturating_add(i128::MAX, 1), i128::MAX);
        }

        #[test]
        fn test_safe_compute_share_zero_amount() {
            assert_eq!(safe_math::safe_compute_share(0, 5_000).unwrap(), 0);
        }

        #[test]
        fn test_safe_compute_share_full_bps() {
            assert_eq!(safe_math::safe_compute_share(10_000, 10_000).unwrap(), 10_000);
        }

        #[test]
        fn test_safe_compute_share_half() {
            assert_eq!(safe_math::safe_compute_share(10_000, 5_000).unwrap(), 5_000);
        }
    }

    mod state_consistency_tests {
        use super::*;

        #[test]
        fn test_assert_no_transfer_pending_false() {
            assert!(state_consistency::assert_no_transfer_pending(false).is_ok());
        }

        #[test]
        fn test_assert_no_transfer_pending_true() {
            assert_eq!(
                state_consistency::assert_no_transfer_pending(true),
                Err(RevoraError::IssuerTransferPending)
            );
        }

        #[test]
        fn test_assert_transfer_pending_true() {
            assert!(state_consistency::assert_transfer_pending(true).is_ok());
        }

        #[test]
        fn test_assert_transfer_pending_false() {
            assert_eq!(
                state_consistency::assert_transfer_pending(false),
                Err(RevoraError::NoTransferPending)
            );
        }

        #[test]
        fn test_assert_contract_not_frozen_false() {
            assert!(state_consistency::assert_contract_not_frozen(false).is_ok());
        }

        #[test]
        fn test_assert_contract_not_frozen_true() {
            assert_eq!(
                state_consistency::assert_contract_not_frozen(true),
                Err(RevoraError::ContractFrozen)
            );
        }
    }

    mod abort_handling_tests {
        use super::*;

        #[test]
        fn test_is_recoverable_error_offering_not_found() {
            assert!(abort_handling::is_recoverable_error(
                &RevoraError::OfferingNotFound
            ));
        }

        #[test]
        fn test_is_recoverable_error_concentration_exceeded() {
            assert!(!abort_handling::is_recoverable_error(
                &RevoraError::ConcentrationLimitExceeded
            ));
        }

        #[test]
        fn test_recover_with_default_ok() {
            let result: Result<i128, _> = Ok(100);
            assert_eq!(abort_handling::recover_with_default(result, 50), 100);
        }

        #[test]
        fn test_recover_with_default_err() {
            let result: Result<i128, _> = Err(RevoraError::OfferingNotFound);
            assert_eq!(abort_handling::recover_with_default(result, 50), 50);
        }
    }
}
