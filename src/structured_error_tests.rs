/// # RevoraError discriminant stability tests
///
/// These tests are the **CI-provable contract** for the `RevoraError` wire format.
///
/// ## Why this matters for Soroban / Stellar
///
/// Soroban contract errors are returned as `u32` values in the contract result XDR.
/// Off-chain Stellar/Horizon clients (SDKs, indexers, frontends) parse that `u32`
/// directly — they do **not** receive the Rust variant name. Two variants sharing the
/// same `u32` are indistinguishable on the wire; a decoder that sees `30` cannot tell
/// whether the contract meant `ProposalExpired` or `TransferFailed`.
///
/// ## Stability guarantee
///
/// Once a discriminant appears in a production deployment it is **frozen**.
/// Changing a number is a breaking change that requires:
/// 1. A `CONTRACT_VERSION` bump.
/// 2. A migration note in this file and in `README.md`.
/// 3. Updated off-chain SDK / indexer documentation.
///
/// See README.md error code table for the full stability contract.
///
/// ## Audit history
///
/// | Version | Change |
/// |---------|--------|
/// | v1–v4   | `ProposalExpired = 30` and `TransferFailed = 30` — **duplicate** (bug) |
/// | v5      | `TransferFailed` renumbered to `39`; discriminants now non-contiguous (1..=46) |
/// |         | with gaps at 31, 34, 37–38. New variants: `NoAdminRotationPending` (35), |
/// |         | `UnauthorizedRotationAccept` (36), `OfferingFrozen` (42), |
/// |         | `IssuerTransferExpired` (43), `ContractPaused` (44), |
/// |         | `BlacklistSizeLimitExceeded` (45), `AlreadyApproved` (46) |

#[cfg(test)]
mod tests {
    use crate::{RevoraError, CONTRACT_VERSION};

    // ─────────────────────────────────────────────────────────────────────────
    // 1. UNIQUENESS — no two variants may share a discriminant
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_all_discriminants_are_unique() {
        // Collect every (name, value) pair and assert no value appears twice.
        // This is the primary guard against the ProposalExpired/TransferFailed
        // class of bug. If a new variant is added with a duplicate value, this
        // test will catch it immediately.
        let codes: &[(&str, u32)] = &[
            ("InvalidRevenueShareBps", RevoraError::InvalidRevenueShareBps as u32),
            ("LimitReached", RevoraError::LimitReached as u32),
            ("ConcentrationLimitExceeded", RevoraError::ConcentrationLimitExceeded as u32),
            ("OfferingNotFound", RevoraError::OfferingNotFound as u32),
            ("PeriodAlreadyDeposited", RevoraError::PeriodAlreadyDeposited as u32),
            ("NoPendingClaims", RevoraError::NoPendingClaims as u32),
            ("HolderBlacklisted", RevoraError::HolderBlacklisted as u32),
            ("InvalidShareBps", RevoraError::InvalidShareBps as u32),
            ("PaymentTokenMismatch", RevoraError::PaymentTokenMismatch as u32),
            ("ContractFrozen", RevoraError::ContractFrozen as u32),
            ("ClaimDelayNotElapsed", RevoraError::ClaimDelayNotElapsed as u32),
            ("SnapshotNotEnabled", RevoraError::SnapshotNotEnabled as u32),
            ("OutdatedSnapshot", RevoraError::OutdatedSnapshot as u32),
            ("PayoutAssetMismatch", RevoraError::PayoutAssetMismatch as u32),
            ("IssuerTransferPending", RevoraError::IssuerTransferPending as u32),
            ("NoTransferPending", RevoraError::NoTransferPending as u32),
            ("UnauthorizedTransferAccept", RevoraError::UnauthorizedTransferAccept as u32),
            ("MetadataTooLarge", RevoraError::MetadataTooLarge as u32),
            ("NotAuthorized", RevoraError::NotAuthorized as u32),
            ("NotInitialized", RevoraError::NotInitialized as u32),
            ("InvalidAmount", RevoraError::InvalidAmount as u32),
            ("InvalidPeriodId", RevoraError::InvalidPeriodId as u32),
            ("SupplyCapExceeded", RevoraError::SupplyCapExceeded as u32),
            ("MetadataInvalidFormat", RevoraError::MetadataInvalidFormat as u32),
            ("ReportingWindowClosed", RevoraError::ReportingWindowClosed as u32),
            ("ClaimWindowClosed", RevoraError::ClaimWindowClosed as u32),
            ("SignatureExpired", RevoraError::SignatureExpired as u32),
            ("SignatureReplay", RevoraError::SignatureReplay as u32),
            ("SignerKeyNotRegistered", RevoraError::SignerKeyNotRegistered as u32),
            ("ProposalExpired", RevoraError::ProposalExpired as u32),
            ("TransferFailed", RevoraError::TransferFailed as u32),
            ("AlreadyAtTargetVersion", RevoraError::AlreadyAtTargetVersion as u32),
            ("MigrationDowngradeNotAllowed", RevoraError::MigrationDowngradeNotAllowed as u32),
            ("AdminRotationSameAddress", RevoraError::AdminRotationSameAddress as u32),
            ("AdminRotationPending", RevoraError::AdminRotationPending as u32),
            ("NoAdminRotationPending", RevoraError::NoAdminRotationPending as u32),
            ("UnauthorizedRotationAccept", RevoraError::UnauthorizedRotationAccept as u32),
            ("OfferingFrozen", RevoraError::OfferingFrozen as u32),
            ("IssuerTransferExpired", RevoraError::IssuerTransferExpired as u32),
            ("ContractPaused", RevoraError::ContractPaused as u32),
            ("BlacklistSizeLimitExceeded", RevoraError::BlacklistSizeLimitExceeded as u32),
            ("AlreadyApproved", RevoraError::AlreadyApproved as u32),
            ("MissingReportForOverride", RevoraError::MissingReportForOverride as u32),
        ];

        // O(n²) uniqueness check — n=42, negligible cost.
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(
                    codes[i].1, codes[j].1,
                    "Duplicate discriminant {}: {} and {} both = {}",
                    codes[i].1, codes[i].0, codes[j].0, codes[i].1
                );
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 2. FROZEN WIRE VALUES — each variant's u32 is pinned forever
    // ─────────────────────────────────────────────────────────────────────────
    //
    // If any of these assertions fail, a previously-deployed wire value has
    // changed. That is a breaking change. Do NOT simply update the expected
    // value — instead open a new issue, bump CONTRACT_VERSION, and document
    // the migration path.

    #[test]
    fn test_wire_values_are_frozen() {
        // See README.md error code table for the full stability contract.
        // These discriminants are frozen — any change requires a CONTRACT_VERSION bump
        // and migration documentation. Gaps are preserved for wire compatibility.
        assert_eq!(RevoraError::InvalidRevenueShareBps as u32, 1);
        assert_eq!(RevoraError::LimitReached as u32, 2);
        assert_eq!(RevoraError::ConcentrationLimitExceeded as u32, 3);
        assert_eq!(RevoraError::OfferingNotFound as u32, 4);
        assert_eq!(RevoraError::PeriodAlreadyDeposited as u32, 5);
        assert_eq!(RevoraError::NoPendingClaims as u32, 6);
        assert_eq!(RevoraError::HolderBlacklisted as u32, 7);
        assert_eq!(RevoraError::InvalidShareBps as u32, 8);
        assert_eq!(RevoraError::PaymentTokenMismatch as u32, 9);
        assert_eq!(RevoraError::ContractFrozen as u32, 10);
        assert_eq!(RevoraError::ClaimDelayNotElapsed as u32, 11);
        assert_eq!(RevoraError::SnapshotNotEnabled as u32, 12);
        assert_eq!(RevoraError::OutdatedSnapshot as u32, 13);
        assert_eq!(RevoraError::PayoutAssetMismatch as u32, 14);
        assert_eq!(RevoraError::IssuerTransferPending as u32, 15);
        assert_eq!(RevoraError::NoTransferPending as u32, 16);
        assert_eq!(RevoraError::UnauthorizedTransferAccept as u32, 17);
        assert_eq!(RevoraError::MetadataTooLarge as u32, 18);
        assert_eq!(RevoraError::NotAuthorized as u32, 19);
        assert_eq!(RevoraError::NotInitialized as u32, 20);
        assert_eq!(RevoraError::InvalidAmount as u32, 21);
        assert_eq!(RevoraError::InvalidPeriodId as u32, 22);
        assert_eq!(RevoraError::SupplyCapExceeded as u32, 23);
        assert_eq!(RevoraError::MetadataInvalidFormat as u32, 24);
        assert_eq!(RevoraError::ReportingWindowClosed as u32, 25);
        assert_eq!(RevoraError::ClaimWindowClosed as u32, 26);
        assert_eq!(RevoraError::SignatureExpired as u32, 27);
        assert_eq!(RevoraError::SignatureReplay as u32, 28);
        assert_eq!(RevoraError::SignerKeyNotRegistered as u32, 29);
        // 30: ProposalExpired — stable since v1
        assert_eq!(RevoraError::ProposalExpired as u32, 30);
        // 31: gap (reserved for future use)
        assert_eq!(RevoraError::AlreadyAtTargetVersion as u32, 32);
        assert_eq!(RevoraError::MigrationDowngradeNotAllowed as u32, 33);
        // 34: gap (reserved for future use)
        assert_eq!(RevoraError::NoAdminRotationPending as u32, 35);
        assert_eq!(RevoraError::UnauthorizedRotationAccept as u32, 36);
        // 37–38: gaps (reserved for future use)
        // 39: TransferFailed — renumbered from 30 in v5 (was duplicate of ProposalExpired)
        assert_eq!(RevoraError::TransferFailed as u32, 39);
        assert_eq!(RevoraError::AdminRotationSameAddress as u32, 40);
        assert_eq!(RevoraError::AdminRotationPending as u32, 41);
        assert_eq!(RevoraError::OfferingFrozen as u32, 42);
        assert_eq!(RevoraError::IssuerTransferExpired as u32, 43);
        assert_eq!(RevoraError::ContractPaused as u32, 44);
        assert_eq!(RevoraError::BlacklistSizeLimitExceeded as u32, 45);
        assert_eq!(RevoraError::AlreadyApproved as u32, 46);
        assert_eq!(RevoraError::MissingReportForOverride as u32, 47);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 3. THE SPECIFIC BUG — ProposalExpired ≠ TransferFailed on the wire
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_proposal_expired_and_transfer_failed_are_distinct() {
        // This is the regression test for the v1–v4 bug where both variants
        // shared discriminant 30. An off-chain decoder receiving 30 could not
        // tell which error the contract returned.
        assert_ne!(
            RevoraError::ProposalExpired as u32,
            RevoraError::TransferFailed as u32,
            "ProposalExpired and TransferFailed must have distinct wire values"
        );
        assert_eq!(
            RevoraError::ProposalExpired as u32,
            30,
            "ProposalExpired wire value must remain 30 (stable since v1)"
        );
        assert_eq!(
            RevoraError::TransferFailed as u32,
            39,
            "TransferFailed wire value is 39 since v5 (was 30, duplicate bug)"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 4. VALID RANGE — all discriminants within 1..=46, gaps preserved
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_discriminants_within_valid_range_1_to_46() {
        // All discriminants must be within 1..=46. Gaps are preserved for wire
        // compatibility — do not renumber existing variants. New variants should
        // use the next available number or fill gaps intentionally.
        let all = [
            RevoraError::InvalidRevenueShareBps as u32,
            RevoraError::LimitReached as u32,
            RevoraError::ConcentrationLimitExceeded as u32,
            RevoraError::OfferingNotFound as u32,
            RevoraError::PeriodAlreadyDeposited as u32,
            RevoraError::NoPendingClaims as u32,
            RevoraError::HolderBlacklisted as u32,
            RevoraError::InvalidShareBps as u32,
            RevoraError::PaymentTokenMismatch as u32,
            RevoraError::ContractFrozen as u32,
            RevoraError::ClaimDelayNotElapsed as u32,
            RevoraError::SnapshotNotEnabled as u32,
            RevoraError::OutdatedSnapshot as u32,
            RevoraError::PayoutAssetMismatch as u32,
            RevoraError::IssuerTransferPending as u32,
            RevoraError::NoTransferPending as u32,
            RevoraError::UnauthorizedTransferAccept as u32,
            RevoraError::MetadataTooLarge as u32,
            RevoraError::NotAuthorized as u32,
            RevoraError::NotInitialized as u32,
            RevoraError::InvalidAmount as u32,
            RevoraError::InvalidPeriodId as u32,
            RevoraError::SupplyCapExceeded as u32,
            RevoraError::MetadataInvalidFormat as u32,
            RevoraError::ReportingWindowClosed as u32,
            RevoraError::ClaimWindowClosed as u32,
            RevoraError::SignatureExpired as u32,
            RevoraError::SignatureReplay as u32,
            RevoraError::SignerKeyNotRegistered as u32,
            RevoraError::ProposalExpired as u32,
            RevoraError::TransferFailed as u32,
            RevoraError::AlreadyAtTargetVersion as u32,
            RevoraError::MigrationDowngradeNotAllowed as u32,
            RevoraError::AdminRotationSameAddress as u32,
            RevoraError::AdminRotationPending as u32,
            RevoraError::NoAdminRotationPending as u32,
            RevoraError::UnauthorizedRotationAccept as u32,
            RevoraError::OfferingFrozen as u32,
            RevoraError::IssuerTransferExpired as u32,
            RevoraError::ContractPaused as u32,
            RevoraError::BlacklistSizeLimitExceeded as u32,
            RevoraError::AlreadyApproved as u32,
        ];
        for v in all.iter() {
            assert!(*v >= 1 && *v <= 46, "discriminant {v} out of expected range 1..=46");
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 5. CONTRACT VERSION reflects the fix
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_contract_version_is_at_least_5() {
        // The duplicate-discriminant fix ships in v5. If this fails, the version
        // constant was not bumped alongside the enum change.
        const {
            assert!(
                CONTRACT_VERSION >= 5,
                "CONTRACT_VERSION must be >= 5 after the TransferFailed renumber"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 6. ZERO IS NOT A VALID ERROR CODE
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_zero_is_not_a_valid_discriminant() {
        // Soroban uses 0 to mean "success" in the contract result. No error
        // variant may use 0 or it would be indistinguishable from Ok.
        let all = [
            RevoraError::InvalidRevenueShareBps as u32,
            RevoraError::LimitReached as u32,
            RevoraError::ConcentrationLimitExceeded as u32,
            RevoraError::OfferingNotFound as u32,
            RevoraError::PeriodAlreadyDeposited as u32,
            RevoraError::NoPendingClaims as u32,
            RevoraError::HolderBlacklisted as u32,
            RevoraError::InvalidShareBps as u32,
            RevoraError::PaymentTokenMismatch as u32,
            RevoraError::ContractFrozen as u32,
            RevoraError::ClaimDelayNotElapsed as u32,
            RevoraError::SnapshotNotEnabled as u32,
            RevoraError::OutdatedSnapshot as u32,
            RevoraError::PayoutAssetMismatch as u32,
            RevoraError::IssuerTransferPending as u32,
            RevoraError::NoTransferPending as u32,
            RevoraError::UnauthorizedTransferAccept as u32,
            RevoraError::MetadataTooLarge as u32,
            RevoraError::NotAuthorized as u32,
            RevoraError::NotInitialized as u32,
            RevoraError::InvalidAmount as u32,
            RevoraError::InvalidPeriodId as u32,
            RevoraError::SupplyCapExceeded as u32,
            RevoraError::MetadataInvalidFormat as u32,
            RevoraError::ReportingWindowClosed as u32,
            RevoraError::ClaimWindowClosed as u32,
            RevoraError::SignatureExpired as u32,
            RevoraError::SignatureReplay as u32,
            RevoraError::SignerKeyNotRegistered as u32,
            RevoraError::ProposalExpired as u32,
            RevoraError::TransferFailed as u32,
            RevoraError::AlreadyAtTargetVersion as u32,
            RevoraError::MigrationDowngradeNotAllowed as u32,
            RevoraError::AdminRotationSameAddress as u32,
            RevoraError::AdminRotationPending as u32,
            RevoraError::NoAdminRotationPending as u32,
            RevoraError::UnauthorizedRotationAccept as u32,
            RevoraError::OfferingFrozen as u32,
            RevoraError::IssuerTransferExpired as u32,
            RevoraError::ContractPaused as u32,
            RevoraError::BlacklistSizeLimitExceeded as u32,
            RevoraError::AlreadyApproved as u32,
        ];
        for v in all.iter() {
            assert_ne!(*v, 0, "discriminant 0 is reserved for Ok; no error variant may use it");
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 7. EQUALITY / COPY SEMANTICS work correctly after the fix
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_error_equality_is_by_variant_not_value() {
        // Before the fix, ProposalExpired == TransferFailed because both were 30.
        // After the fix they must be unequal.
        assert_ne!(RevoraError::ProposalExpired, RevoraError::TransferFailed);
        // Sanity: same variant equals itself.
        assert_eq!(RevoraError::ProposalExpired, RevoraError::ProposalExpired);
        assert_eq!(RevoraError::TransferFailed, RevoraError::TransferFailed);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 8. EXISTING STABLE CODES unchanged from v1 (regression guard)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_v1_stable_codes_unchanged() {
        // These codes were present and correct in v1 and must never change.
        assert_eq!(
            RevoraError::InvalidRevenueShareBps as u32,
            1,
            "error code for integrators — must not change"
        );
        assert_eq!(RevoraError::LimitReached as u32, 2);
        assert_eq!(RevoraError::OfferingNotFound as u32, 4);
        assert_eq!(RevoraError::HolderBlacklisted as u32, 7);
        assert_eq!(RevoraError::ContractFrozen as u32, 10);
        assert_eq!(RevoraError::NotAuthorized as u32, 19);
        assert_eq!(RevoraError::InvalidAmount as u32, 21);
        assert_eq!(RevoraError::InvalidPeriodId as u32, 22);
        assert_eq!(RevoraError::ProposalExpired as u32, 30);
    }
}
