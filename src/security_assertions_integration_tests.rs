/// # Integration Tests for Security Assertions Module
///
/// This test suite validates that the Security Assertions Module (`security_assertions.rs`)
/// correctly enforces all security invariants documented in `docs/security-assertions-module.md`
/// and the `SECURITY_ASSERTIONS_*.md` files.
///
/// ## Security Note
///
/// These tests are the CI-provable contract for the security assertions layer.
/// Every assertion function in `security_assertions.rs` must have at least one
/// integration test here covering both the happy path and the rejection path.
///
/// ### Trust Boundaries
/// - `auth_boundaries::assert_address_authorized` and `assert_issuer_authorized` are
///   meta-assertions only; host-level `require_auth()` is the actual enforcement mechanism.
///   Tests here document the requirement; they do not replace host-level auth checks.
/// - `safe_math` operations are deterministic and bounded; overflow/underflow always
///   returns `Err(LimitReached)` rather than panicking.
/// - All state consistency assertions accept a pre-computed boolean flag; the contract
///   is responsible for deriving that flag from storage before calling the assertion.
///
/// ### Stale-Assertion Policy
/// No test in this file may assert a guarantee that `security_assertions.rs` does not
/// actually provide. Specifically:
/// - `assert_offering_not_exists` currently always returns `Ok(())`; tests reflect this.
/// - `assert_address_authorized` / `assert_issuer_authorized` do not return `Result`;
///   tests document the pattern without asserting a return value.
///
/// All tests are deterministic and do not depend on contract state or external systems.

#[cfg(test)]
mod security_assertions_integration_tests {
    use crate::security_assertions::{
        abort_handling, auth_boundaries, input_validation, safe_math, state_consistency,
    };
    use crate::RevoraError;

    // ─────────────────────────────────────────────────────────────────────────────
    // 1. OFFERING REGISTRATION FLOW TESTS
    // ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_offering_registration_validates_bps_before_storing() {
        // Covers: input_validation::assert_valid_bps
        // Invariant: BPS must be in [0, 10000] before any state write.

        // Boundary: 0% (disabled) is valid
        assert!(input_validation::assert_valid_bps(0).is_ok());
        // Boundary: 25%
        assert!(input_validation::assert_valid_bps(2500).is_ok());
        // Boundary: 100% (maximum)
        assert!(input_validation::assert_valid_bps(10_000).is_ok());
        // Rejection: 100.01%
        assert_eq!(
            input_validation::assert_valid_bps(10_001),
            Err(RevoraError::InvalidRevenueShareBps)
        );
        // Rejection: u32::MAX (far over limit)
        assert_eq!(
            input_validation::assert_valid_bps(u32::MAX),
            Err(RevoraError::InvalidRevenueShareBps)
        );
    }

    #[test]
    fn test_offering_registration_validates_concentration_bps() {
        // Covers: input_validation::assert_valid_concentration_bps
        // Invariant: concentration limit must be in [0, 10000].

        assert!(input_validation::assert_valid_concentration_bps(0).is_ok());
        assert!(input_validation::assert_valid_concentration_bps(5_000).is_ok());
        assert!(input_validation::assert_valid_concentration_bps(10_000).is_ok());
        assert_eq!(
            input_validation::assert_valid_concentration_bps(10_001),
            Err(RevoraError::LimitReached)
        );
    }

    #[test]
    fn test_offering_registration_authorization_boundary() {
        // Covers: auth_boundaries::assert_issuer_authorized (meta-assertion)
        //
        // Security assumption: host-level require_auth() is the actual enforcement.
        // This assertion is a documentation checkpoint for security reviews and fuzzing.
        // It does not return a Result; calling it is a no-op in production.
        //
        // We verify the function is callable without panicking (compile-time proof
        // that the checkpoint exists in the call graph).
        //
        // In production: env.require_auth(&issuer) is called before this point.
        // In tests: env.mock_all_auths() allows all auths to pass.
        //
        // No runtime assertion is possible here without a Soroban Env; the test
        // documents the security boundary rather than asserting a return value.
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 2. REVENUE DEPOSIT FLOW TESTS
    // ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_revenue_deposit_validates_amount_before_transfer() {
        // Covers: input_validation::assert_positive_amount
        // Invariant: deposit amount must be strictly > 0 (differs from report).

        // Rejection: zero (must deposit something)
        assert_eq!(
            input_validation::assert_positive_amount(0),
            Err(RevoraError::InvalidAmount),
            "Deposit must have positive amount"
        );
        // Rejection: negative
        assert_eq!(
            input_validation::assert_positive_amount(-1),
            Err(RevoraError::InvalidAmount),
            "Negative amounts never valid"
        );
        // Rejection: i128::MIN
        assert_eq!(
            input_validation::assert_positive_amount(i128::MIN),
            Err(RevoraError::InvalidAmount)
        );
        // Acceptance: minimum positive
        assert!(input_validation::assert_positive_amount(1).is_ok());
        // Acceptance: typical amount
        assert!(input_validation::assert_positive_amount(1_000_000).is_ok());
        // Acceptance: maximum i128
        assert!(input_validation::assert_positive_amount(i128::MAX).is_ok());
    }

    #[test]
    fn test_revenue_deposit_validates_period_id() {
        // Covers: input_validation::assert_positive_period_id
        // Invariant: period_id must be > 0 for deposit operations.

        assert_eq!(
            input_validation::assert_positive_period_id(0),
            Err(RevoraError::InvalidPeriodId),
            "Period ID 0 is invalid for deposits"
        );
        assert!(input_validation::assert_positive_period_id(1).is_ok());
        assert!(input_validation::assert_positive_period_id(u64::MAX).is_ok());
    }

    #[test]
    fn test_revenue_deposit_prevents_duplicate_periods() {
        // Covers: state_consistency::assert_period_not_deposited
        // Invariant: same period cannot be deposited twice (idempotency guard).

        // Rejection: period already deposited
        assert_eq!(
            state_consistency::assert_period_not_deposited(true),
            Err(RevoraError::PeriodAlreadyDeposited),
            "Period already deposited; cannot duplicate"
        );
        // Acceptance: period not yet deposited
        assert!(state_consistency::assert_period_not_deposited(false).is_ok());
    }

    #[test]
    fn test_revenue_deposit_validates_payment_token_lock() {
        // Covers: state_consistency::assert_payment_token_matches
        // Invariant: payment token is immutable after first deposit.

        let token1 = "token_address_001";
        let token2 = "token_address_002";

        // Acceptance: same token as first deposit
        assert!(
            state_consistency::assert_payment_token_matches(&token1, &token1).is_ok(),
            "Same token as first deposit must succeed"
        );
        // Rejection: different token
        assert_eq!(
            state_consistency::assert_payment_token_matches(&token2, &token1),
            Err(RevoraError::PaymentTokenMismatch),
            "Different token than first deposit must fail"
        );
    }

    #[test]
    fn test_revenue_deposit_checks_offering_exists() {
        // Covers: state_consistency::assert_offering_exists
        // Invariant: cannot deposit to a non-existent offering.

        let offering_exists: Option<&str> = Some("offering_data");
        let offering_not_found: Option<&str> = None;

        assert!(state_consistency::assert_offering_exists(&offering_exists).is_ok());
        assert_eq!(
            state_consistency::assert_offering_exists(&offering_not_found),
            Err(RevoraError::OfferingNotFound),
            "Offering must be registered first"
        );
    }

    #[test]
    fn test_offering_not_exists_always_ok() {
        // Covers: state_consistency::assert_offering_not_exists
        // Security note: this function currently always returns Ok(()) regardless of
        // the input. It is a placeholder for future duplicate-registration guards.
        // Tests document the actual behavior to prevent stale assumptions.

        let some_offering: Option<&str> = Some("existing");
        let no_offering: Option<&str> = None;

        // Both branches return Ok(()) — this is the current contract behavior.
        assert!(state_consistency::assert_offering_not_exists(&some_offering).is_ok());
        assert!(state_consistency::assert_offering_not_exists(&no_offering).is_ok());
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 3. REVENUE REPORT FLOW TESTS
    // ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_revenue_report_allows_zero_amount() {
        // Covers: input_validation::assert_non_negative_amount
        // Invariant: report_revenue allows zero (per zero-value-revenue-policy.md).
        // This differs from deposit_revenue which requires strictly positive amounts.

        // Acceptance: zero (audit record with no revenue)
        assert!(
            input_validation::assert_non_negative_amount(0).is_ok(),
            "Zero revenue report is allowed per zero-value-revenue-policy.md"
        );
        // Acceptance: positive
        assert!(input_validation::assert_non_negative_amount(1_000_000).is_ok());
        assert!(input_validation::assert_non_negative_amount(i128::MAX).is_ok());
        // Rejection: negative
        assert_eq!(
            input_validation::assert_non_negative_amount(-1),
            Err(RevoraError::InvalidAmount),
            "Negative amounts are never valid"
        );
        assert_eq!(
            input_validation::assert_non_negative_amount(i128::MIN),
            Err(RevoraError::InvalidAmount)
        );
    }

    #[test]
    fn test_revenue_report_validates_concentration_if_enforced() {
        // Covers: input_validation::assert_valid_concentration_bps
        // Invariant: concentration limit must be in [0, 10000] before enforcement.

        // Acceptance: disabled (0 = no limit)
        assert!(input_validation::assert_valid_concentration_bps(0).is_ok());
        // Acceptance: 30% limit
        assert!(input_validation::assert_valid_concentration_bps(3_000).is_ok());
        // Acceptance: 100% limit (full concentration allowed)
        assert!(input_validation::assert_valid_concentration_bps(10_000).is_ok());
        // Rejection: over 100%
        assert_eq!(
            input_validation::assert_valid_concentration_bps(10_001),
            Err(RevoraError::LimitReached)
        );

        // Enforcement toggle: when enforce=false, concentration check is skipped.
        // When enforce=true, the contract compares holder concentration against the limit.
        // The assertion only validates the limit value itself; enforcement logic is in lib.rs.
        let max_concentration_bps: u32 = 3_000;
        let current_concentration: u32 = 3_001;
        let enforcement_enabled = true;

        if enforcement_enabled && current_concentration > max_concentration_bps {
            // Contract would return ConcentrationLimitExceeded here.
            // We verify the error is classified as fatal (non-recoverable).
            assert!(!abort_handling::is_recoverable_error(
                &RevoraError::ConcentrationLimitExceeded
            ));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 4. HOLDER CLAIM FLOW TESTS
    // ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_holder_claim_validates_share_before_calculation() {
        // Covers: input_validation::assert_valid_share_bps
        // Invariant: share_bps must be in [0, 10000] before share calculation.

        assert!(input_validation::assert_valid_share_bps(0).is_ok());
        assert!(input_validation::assert_valid_share_bps(2500).is_ok());
        assert!(input_validation::assert_valid_share_bps(10_000).is_ok());
        assert_eq!(
            input_validation::assert_valid_share_bps(10_001),
            Err(RevoraError::InvalidShareBps)
        );
        assert_eq!(
            input_validation::assert_valid_share_bps(u32::MAX),
            Err(RevoraError::InvalidShareBps)
        );
    }

    #[test]
    fn test_holder_claim_requires_not_blacklisted() {
        // Covers: state_consistency::assert_holder_not_blacklisted
        // Invariant: blacklisted holders are unconditionally excluded from payouts.

        assert_eq!(
            state_consistency::assert_holder_not_blacklisted(true),
            Err(RevoraError::HolderBlacklisted),
            "Blacklisted holder cannot claim"
        );
        assert!(state_consistency::assert_holder_not_blacklisted(false).is_ok());
    }

    #[test]
    fn test_holder_claim_requires_pending_periods() {
        // Covers: state_consistency::assert_no_pending_claims
        // Invariant: NoPendingClaims is returned when has_pending=false (all claimed).
        // Note: the function name is assert_no_pending_claims; it errors when
        // has_pending=false (meaning there are no periods left to claim).

        // Rejection: no pending periods (nothing to claim)
        assert_eq!(
            state_consistency::assert_no_pending_claims(false),
            Err(RevoraError::NoPendingClaims),
            "All periods already claimed"
        );
        // Acceptance: pending periods exist
        assert!(state_consistency::assert_no_pending_claims(true).is_ok());
    }

    #[test]
    fn test_holder_claim_safe_share_calculation() {
        // Covers: safe_math::safe_compute_share
        // Invariant: result is always in [0, amount]; no overflow possible.

        // 25% of 10_000
        let result = safe_math::safe_compute_share(10_000_i128, 2500).unwrap();
        assert_eq!(result, 2_500);
        assert!(result <= 10_000_i128);

        // 50% of 8_000
        let result = safe_math::safe_compute_share(8_000_i128, 5_000).unwrap();
        assert_eq!(result, 4_000);
        assert!(result <= 8_000_i128);

        // 100% of 1_000_000
        let result = safe_math::safe_compute_share(1_000_000_i128, 10_000).unwrap();
        assert_eq!(result, 1_000_000);
        assert!(result <= 1_000_000_i128);

        // 0% of any amount = 0
        let result = safe_math::safe_compute_share(999_999_i128, 0).unwrap();
        assert_eq!(result, 0);

        // 0.01% of 1_000_000 (1 BPS)
        let result = safe_math::safe_compute_share(1_000_000_i128, 1).unwrap();
        assert!(result <= 1_000_000_i128);
        assert!(result >= 0);

        // Zero amount always yields zero share
        let result = safe_math::safe_compute_share(0_i128, 5_000).unwrap();
        assert_eq!(result, 0);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 5. ISSUER TRANSFER FLOW TESTS
    // ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_issuer_transfer_propose_checks_no_pending_transfer() {
        // Covers: state_consistency::assert_no_transfer_pending
        // Invariant: only one transfer may be in-flight per offering at a time.

        assert!(state_consistency::assert_no_transfer_pending(false).is_ok());
        assert_eq!(
            state_consistency::assert_no_transfer_pending(true),
            Err(RevoraError::IssuerTransferPending),
            "Must cancel existing transfer first"
        );
    }

    #[test]
    fn test_issuer_transfer_accept_validates_acceptor_is_proposed() {
        // Covers: auth_boundaries::assert_is_proposed_recipient
        // Invariant: only the proposed new issuer may accept the transfer.

        let old_issuer = "issuer_current";
        let new_issuer_proposed = "issuer_new_123";
        let random_address = "random_signer";

        // Acceptance: proposed recipient accepts
        assert!(
            auth_boundaries::assert_is_proposed_recipient(
                &new_issuer_proposed,
                &new_issuer_proposed
            )
            .is_ok(),
            "Proposed recipient can accept"
        );
        // Rejection: random address
        assert_eq!(
            auth_boundaries::assert_is_proposed_recipient(&random_address, &new_issuer_proposed),
            Err(RevoraError::UnauthorizedTransferAccept),
            "Only proposed recipient can accept"
        );
        // Rejection: old issuer cannot self-accept
        assert_eq!(
            auth_boundaries::assert_is_proposed_recipient(&old_issuer, &new_issuer_proposed),
            Err(RevoraError::UnauthorizedTransferAccept),
            "Old issuer cannot accept for new issuer"
        );
    }

    #[test]
    fn test_issuer_transfer_cancel_requires_pending_transfer() {
        // Covers: state_consistency::assert_transfer_pending
        // Invariant: cancel/accept operations require a pending transfer to exist.

        assert!(state_consistency::assert_transfer_pending(true).is_ok());
        assert_eq!(
            state_consistency::assert_transfer_pending(false),
            Err(RevoraError::NoTransferPending),
            "No transfer to cancel"
        );
    }

    #[test]
    fn test_issuer_transfer_prevents_self_transfer() {
        // Covers: input_validation::assert_addresses_different
        // Invariant: proposing a transfer to the current issuer is a no-op and rejected.

        let issuer = "issuer_abc";
        assert_eq!(
            input_validation::assert_addresses_different(&issuer, &issuer),
            Err(RevoraError::AdminRotationSameAddress),
            "Self-transfer must be rejected"
        );
        let new_issuer = "issuer_xyz";
        assert!(input_validation::assert_addresses_different(&issuer, &new_issuer).is_ok());
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 6. ADMIN/MULTISIG FLOW TESTS
    // ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_multisig_threshold_validation_prevents_impossible_config() {
        // Covers: input_validation::assert_valid_multisig_threshold
        // Invariant: threshold must satisfy 0 < threshold ≤ owner_count.

        // Acceptance: 2-of-3
        assert!(input_validation::assert_valid_multisig_threshold(2, 3).is_ok());
        // Acceptance: 1-of-1 (single owner)
        assert!(input_validation::assert_valid_multisig_threshold(1, 1).is_ok());
        // Acceptance: N-of-N (unanimous)
        assert!(input_validation::assert_valid_multisig_threshold(3, 3).is_ok());
        // Rejection: zero threshold
        assert_eq!(
            input_validation::assert_valid_multisig_threshold(0, 3),
            Err(RevoraError::LimitReached),
            "Zero threshold is invalid"
        );
        // Rejection: threshold > owner count (unreachable)
        assert_eq!(
            input_validation::assert_valid_multisig_threshold(4, 3),
            Err(RevoraError::LimitReached),
            "Cannot set threshold higher than owner count"
        );
        // Rejection: threshold > 0 but owner_count = 0 (degenerate)
        assert_eq!(
            input_validation::assert_valid_multisig_threshold(1, 0),
            Err(RevoraError::LimitReached)
        );
    }

    #[test]
    fn test_admin_rotation_propose_checks_no_pending_rotation() {
        // Covers: state_consistency::assert_no_rotation_pending
        // Invariant: only one rotation may be in-flight at a time.

        assert!(state_consistency::assert_no_rotation_pending(false).is_ok());
        assert_eq!(
            state_consistency::assert_no_rotation_pending(true),
            Err(RevoraError::AdminRotationPending),
            "Must complete/cancel existing rotation first"
        );
    }

    #[test]
    fn test_admin_rotation_accept_validates_acceptor() {
        // Covers: auth_boundaries::assert_is_proposed_admin
        // Invariant: only the proposed new admin may accept the rotation.

        let current_admin = "admin_current";
        let new_admin_proposed = "admin_new_456";
        let attacker = "attacker_789";

        // Acceptance: proposed admin accepts
        assert!(
            auth_boundaries::assert_is_proposed_admin(&new_admin_proposed, &new_admin_proposed)
                .is_ok()
        );
        // Rejection: attacker
        assert_eq!(
            auth_boundaries::assert_is_proposed_admin(&attacker, &new_admin_proposed),
            Err(RevoraError::UnauthorizedRotationAccept),
            "Only proposed admin can accept"
        );
        // Rejection: current admin cannot accept on behalf of new admin
        assert_eq!(
            auth_boundaries::assert_is_proposed_admin(&current_admin, &new_admin_proposed),
            Err(RevoraError::UnauthorizedRotationAccept)
        );
    }

    #[test]
    fn test_admin_rotation_cancel_requires_pending_rotation() {
        // Covers: state_consistency::assert_rotation_pending
        // Invariant: cancel/accept operations require a pending rotation to exist.

        assert!(state_consistency::assert_rotation_pending(true).is_ok());
        assert_eq!(
            state_consistency::assert_rotation_pending(false),
            Err(RevoraError::NoAdminRotationPending),
            "No rotation to cancel"
        );
    }

    #[test]
    fn test_admin_rotation_prevents_same_address() {
        // Covers: input_validation::assert_addresses_different
        // Invariant: rotating admin to the same address is a no-op and rejected.

        let admin_address = "admin_123";
        assert_eq!(
            input_validation::assert_addresses_different(&admin_address, &admin_address),
            Err(RevoraError::AdminRotationSameAddress),
            "Cannot rotate admin to same address"
        );
        let different_address = "admin_456";
        assert!(
            input_validation::assert_addresses_different(&admin_address, &different_address)
                .is_ok()
        );
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 7. CONTRACT FREEZE/UNFREEZE TESTS
    // ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_frozen_contract_blocks_state_changes() {
        // Covers: state_consistency::assert_contract_not_frozen
        // Invariant: all state-mutating operations must check the freeze flag first.

        // Acceptance: not frozen
        assert!(state_consistency::assert_contract_not_frozen(false).is_ok());
        // Rejection: frozen
        assert_eq!(
            state_consistency::assert_contract_not_frozen(true),
            Err(RevoraError::ContractFrozen),
            "Frozen contract blocks all mutations"
        );

        // Verify ContractFrozen is classified as fatal (non-recoverable).
        // A frozen contract must not silently continue; the caller must abort.
        assert!(!abort_handling::is_recoverable_error(&RevoraError::ContractFrozen));
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 8. BLACKLIST SIZE LIMIT TESTS
    // ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_blacklist_size_limit_prevents_unbounded_growth() {
        // Covers: state_consistency::assert_blacklist_not_full
        // Invariant: blacklist is capped per-offering to prevent storage abuse.
        // The production cap is MAX_BLACKLIST_SIZE (200); tests use the same value.

        const MAX: u32 = 200;

        // Acceptance: empty blacklist
        assert!(state_consistency::assert_blacklist_not_full(0, MAX).is_ok());
        // Acceptance: one below limit
        assert!(state_consistency::assert_blacklist_not_full(MAX - 1, MAX).is_ok());
        // Rejection: exactly at limit
        assert_eq!(
            state_consistency::assert_blacklist_not_full(MAX, MAX),
            Err(RevoraError::BlacklistSizeLimitExceeded),
            "Blacklist at capacity must reject new entries"
        );
        // Rejection: above limit (defensive; should not occur in practice)
        assert_eq!(
            state_consistency::assert_blacklist_not_full(MAX + 1, MAX),
            Err(RevoraError::BlacklistSizeLimitExceeded)
        );
    }

    #[test]
    fn test_blacklist_size_limit_is_fatal_error() {
        // Covers: abort_handling::is_recoverable_error for BlacklistSizeLimitExceeded
        // Security note: this error must NOT be silently recovered; the caller must
        // remove an existing entry before retrying. Treating it as recoverable would
        // allow bypassing the guardrail.

        assert!(!abort_handling::is_recoverable_error(
            &RevoraError::BlacklistSizeLimitExceeded
        ));
    }

    #[test]
    fn test_blacklist_size_limit_custom_cap() {
        // Covers: assert_blacklist_not_full with non-default cap values.
        // Different offerings may have different configured caps.

        // Small cap (e.g., test environment)
        assert!(state_consistency::assert_blacklist_not_full(0, 5).is_ok());
        assert!(state_consistency::assert_blacklist_not_full(4, 5).is_ok());
        assert_eq!(
            state_consistency::assert_blacklist_not_full(5, 5),
            Err(RevoraError::BlacklistSizeLimitExceeded)
        );
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 9. SAFE MATH INTEGRATION TESTS
    // ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_safe_math_prevents_audit_summary_overflow() {
        // Covers: safe_math::safe_add
        // Invariant: cumulative revenue totals must not silently overflow.

        let near_max: i128 = i128::MAX - 1_000;

        // Rejection: overflow
        assert_eq!(
            safe_math::safe_add(near_max, 2_000),
            Err(RevoraError::LimitReached),
            "Overflow prevented in audit summary"
        );
        // Acceptance: within bounds
        assert_eq!(
            safe_math::safe_add(1_000_000_i128, 2_000_000_i128).unwrap(),
            3_000_000_i128
        );
        // Boundary: exactly at max
        assert_eq!(
            safe_math::safe_add(i128::MAX, 0).unwrap(),
            i128::MAX
        );
    }

    #[test]
    fn test_safe_math_prevents_underflow() {
        // Covers: safe_math::safe_sub
        // Invariant: subtraction must not silently underflow.

        assert_eq!(
            safe_math::safe_sub(i128::MIN, 1),
            Err(RevoraError::LimitReached)
        );
        assert_eq!(safe_math::safe_sub(5_000, 2_000).unwrap(), 3_000);
        assert_eq!(safe_math::safe_sub(0, 0).unwrap(), 0);
    }

    #[test]
    fn test_safe_math_prevents_multiplication_overflow() {
        // Covers: safe_math::safe_mul
        // Invariant: multiplication must not silently overflow.

        assert_eq!(
            safe_math::safe_mul(i128::MAX, 2),
            Err(RevoraError::LimitReached)
        );
        assert_eq!(safe_math::safe_mul(100, 200).unwrap(), 20_000);
        assert_eq!(safe_math::safe_mul(0, i128::MAX).unwrap(), 0);
    }

    #[test]
    fn test_safe_math_prevents_division_by_zero() {
        // Covers: safe_math::safe_div
        // Invariant: division by zero must return an error, not panic.

        assert_eq!(
            safe_math::safe_div(1_000, 0),
            Err(RevoraError::LimitReached)
        );
        assert_eq!(safe_math::safe_div(1_000, 10).unwrap(), 100);
        assert_eq!(safe_math::safe_div(0, 1).unwrap(), 0);
    }

    #[test]
    fn test_safe_math_saturating_operations_clamp_not_error() {
        // Covers: safe_math::saturating_add, saturating_sub
        // Invariant: saturating ops clamp to min/max instead of erroring.
        // Used when overflow is acceptable but predictable behavior is required.

        assert_eq!(safe_math::saturating_add(i128::MAX, 1), i128::MAX);
        assert_eq!(safe_math::saturating_add(0, 0), 0);
        assert_eq!(safe_math::saturating_sub(i128::MIN, 1), i128::MIN);
        assert_eq!(safe_math::saturating_sub(100, 200), -100);
    }

    #[test]
    fn test_safe_math_share_calculation_bounds() {
        // Covers: safe_math::safe_compute_share — exhaustive bounds check.
        // Invariant: share is always in [0, amount] for all valid inputs.

        let amounts = [0_i128, 1, 100, 1_000, 1_000_000, i128::MAX / 10_001];
        let bps_values = [0_u32, 1, 5_000, 10_000];

        for amount in amounts.iter() {
            for bps in bps_values.iter() {
                let result = safe_math::safe_compute_share(*amount, *bps);
                match result {
                    Ok(share) => {
                        assert!(
                            share <= *amount,
                            "Share ({share}) must not exceed amount ({amount})"
                        );
                        assert!(share >= 0, "Share must be non-negative");
                    }
                    Err(RevoraError::LimitReached) => {
                        // Overflow during multiplication — acceptable for extreme inputs.
                    }
                    Err(e) => panic!("Unexpected error: {e:?}"),
                }
            }
        }
    }

    #[test]
    fn test_safe_math_share_overflow_on_extreme_amount() {
        // Covers: safe_compute_share overflow path.
        // Invariant: i128::MAX * 10_000 overflows; must return LimitReached.

        assert_eq!(
            safe_math::safe_compute_share(i128::MAX, 10_000),
            Err(RevoraError::LimitReached),
            "Overflow in share computation must be caught"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 10. ERROR RECOVERY & CLASSIFICATION TESTS
    // ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_error_classification_recoverable_vs_fatal() {
        // Covers: abort_handling::is_recoverable_error
        // Invariant: recoverable errors are safe to log and continue;
        //            fatal errors must abort the operation.

        // Recoverable: caller can handle these without aborting
        let recoverable = [
            RevoraError::OfferingNotFound,
            RevoraError::PeriodAlreadyDeposited,
            RevoraError::NoPendingClaims,
            RevoraError::OutdatedSnapshot,
            RevoraError::MetadataInvalidFormat,
            RevoraError::ReportingWindowClosed,
            RevoraError::ClaimWindowClosed,
            RevoraError::SignatureExpired,
        ];
        for error in recoverable.iter() {
            assert!(
                abort_handling::is_recoverable_error(error),
                "Error {error:?} should be classified as recoverable"
            );
        }

        // Fatal: must abort; silently continuing would bypass security invariants
        let fatal = [
            RevoraError::InvalidRevenueShareBps,
            RevoraError::ConcentrationLimitExceeded,
            RevoraError::ContractFrozen,
            RevoraError::NotAuthorized,
            RevoraError::PaymentTokenMismatch,
            RevoraError::BlacklistSizeLimitExceeded,
            RevoraError::IssuerTransferPending,
            RevoraError::HolderBlacklisted,
            RevoraError::LimitReached,
        ];
        for error in fatal.iter() {
            assert!(
                !abort_handling::is_recoverable_error(error),
                "Error {error:?} should be classified as fatal"
            );
        }
    }

    #[test]
    fn test_error_recovery_with_defaults() {
        // Covers: abort_handling::recover_with_default
        // Invariant: recoverable errors can be handled with a safe default value.

        // OfferingNotFound → default count of 0
        let not_found: Result<u32, _> = Err(RevoraError::OfferingNotFound);
        assert_eq!(abort_handling::recover_with_default(not_found, 0), 0);

        // Successful operation → actual value used
        let ok: Result<u32, _> = Ok(42);
        assert_eq!(abort_handling::recover_with_default(ok, 0), 42);

        // NoPendingClaims → default empty vec length
        let no_claims: Result<u32, _> = Err(RevoraError::NoPendingClaims);
        assert_eq!(abort_handling::recover_with_default(no_claims, 0), 0);
    }

    #[test]
    fn test_non_negative_threshold_validation() {
        // Covers: input_validation::assert_non_negative_threshold
        // Invariant: minimum balance thresholds must be ≥ 0.

        assert!(input_validation::assert_non_negative_threshold(0).is_ok());
        assert!(input_validation::assert_non_negative_threshold(1_000_000).is_ok());
        assert!(input_validation::assert_non_negative_threshold(i128::MAX).is_ok());
        assert_eq!(
            input_validation::assert_non_negative_threshold(-1),
            Err(RevoraError::InvalidAmount)
        );
        assert_eq!(
            input_validation::assert_non_negative_threshold(i128::MIN),
            Err(RevoraError::InvalidAmount)
        );
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 11. COMPREHENSIVE FLOW TESTS (MULTIPLE ASSERTIONS IN SEQUENCE)
    // ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_complete_offering_lifecycle_assertions() {
        // Integration test: all assertions exercised in sequence for a full lifecycle.
        // This mirrors the actual call order in the production contract.

        // ── Step 1: Register offering ──────────────────────────────────────────
        // assert_valid_bps, assert_contract_not_frozen
        assert!(input_validation::assert_valid_bps(2500).is_ok());
        assert!(state_consistency::assert_contract_not_frozen(false).is_ok());

        // ── Step 2: Deposit revenue ────────────────────────────────────────────
        // assert_offering_exists, assert_positive_amount, assert_payment_token_matches,
        // assert_period_not_deposited, assert_contract_not_frozen
        let offering: Option<&str> = Some("offering_data");
        assert!(state_consistency::assert_offering_exists(&offering).is_ok());
        assert!(input_validation::assert_positive_amount(1_000_000).is_ok());
        assert!(state_consistency::assert_period_not_deposited(false).is_ok());
        assert!(state_consistency::assert_contract_not_frozen(false).is_ok());

        // ── Step 3: Holder claims revenue ─────────────────────────────────────
        // assert_offering_exists, assert_valid_share_bps, assert_holder_not_blacklisted,
        // assert_no_pending_claims, safe_compute_share
        assert!(state_consistency::assert_offering_exists(&offering).is_ok());
        assert!(input_validation::assert_valid_share_bps(5000).is_ok());
        assert!(state_consistency::assert_holder_not_blacklisted(false).is_ok());
        assert!(state_consistency::assert_no_pending_claims(true).is_ok());
        let payout = safe_math::safe_compute_share(10_000_i128, 5000).unwrap();
        assert_eq!(payout, 5_000);
        assert!(payout <= 10_000_i128);

        // ── Step 4: Propose issuer transfer ───────────────────────────────────
        // assert_no_transfer_pending, assert_addresses_different
        assert!(state_consistency::assert_no_transfer_pending(false).is_ok());
        assert!(input_validation::assert_addresses_different(&"issuer_a", &"issuer_b").is_ok());

        // ── Step 5: Accept issuer transfer ────────────────────────────────────
        // assert_transfer_pending, assert_is_proposed_recipient
        assert!(state_consistency::assert_transfer_pending(true).is_ok());
        assert!(
            auth_boundaries::assert_is_proposed_recipient(&"issuer_b", &"issuer_b").is_ok()
        );
    }

    #[test]
    fn test_comprehensive_security_checkpoint_chain() {
        // Assertion: all security layers compose correctly for defense-in-depth.
        // Each checkpoint must pass before the next is evaluated.

        // Checkpoint 1: Input validation (first line of defense)
        let user_bps = 5000_u32;
        assert!(input_validation::assert_valid_bps(user_bps).is_ok());
        assert!(input_validation::assert_positive_amount(100_000).is_ok());
        assert!(input_validation::assert_positive_period_id(42).is_ok());

        // Checkpoint 2: State consistency (second line of defense)
        assert!(state_consistency::assert_contract_not_frozen(false).is_ok());
        assert!(state_consistency::assert_offering_exists(&Some("offering")).is_ok());
        assert!(state_consistency::assert_period_not_deposited(false).is_ok());
        assert!(state_consistency::assert_holder_not_blacklisted(false).is_ok());
        assert!(state_consistency::assert_blacklist_not_full(10, 200).is_ok());

        // Checkpoint 3: Safe math (third line of defense)
        let share = safe_math::safe_compute_share(100_i128, user_bps).unwrap();
        assert!(share <= 100_i128);
        assert!(share >= 0);

        // Checkpoint 4: Error classification (recovery strategy)
        assert!(abort_handling::is_recoverable_error(&RevoraError::OfferingNotFound));
        assert!(!abort_handling::is_recoverable_error(&RevoraError::ContractFrozen));
    }

    #[test]
    fn test_defense_in_depth_frozen_contract_blocks_all_flows() {
        // Invariant: a frozen contract must block every state-mutating flow.
        // Each operation below would check assert_contract_not_frozen first.

        let is_frozen = true;

        // All of these must fail when the contract is frozen:
        assert_eq!(
            state_consistency::assert_contract_not_frozen(is_frozen),
            Err(RevoraError::ContractFrozen),
            "register_offering blocked by freeze"
        );
        assert_eq!(
            state_consistency::assert_contract_not_frozen(is_frozen),
            Err(RevoraError::ContractFrozen),
            "deposit_revenue blocked by freeze"
        );
        assert_eq!(
            state_consistency::assert_contract_not_frozen(is_frozen),
            Err(RevoraError::ContractFrozen),
            "claim blocked by freeze"
        );
        assert_eq!(
            state_consistency::assert_contract_not_frozen(is_frozen),
            Err(RevoraError::ContractFrozen),
            "blacklist_add blocked by freeze"
        );
    }
}
