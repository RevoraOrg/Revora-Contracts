/// # Contract Self-Test Module
///
/// Provides a `self_test()` entrypoint that runs contract-internal invariant
/// checks against a fixed canary dataset embedded in the WASM binary.
/// Returns `0` (pass) on success or a non-zero reason code on first failure.
///
/// ## Architecture
///
/// 1. A binary canary dataset (`canary_data.bin`) is embedded at compile time
///    via `include_bytes!` and linked into the WASM binary.
/// 2. Each invariant check in this module parses a slice of the canary blob,
///    runs the corresponding assertion from `crate::security_assertions` or
///    `crate::assert_semver_forward`, and returns a non-zero error code if
///    the expected outcome does not match.
/// 3. The public `self_test()` entrypoint iterates through all checks and
///    returns the first failing code, or `0` if all pass.
///
/// ## Canary Dataset Format
///
/// The binary canary (`canary_data.bin`) is a packed little-endian blob:
///
/// | Offset | Size | Field                          | Expected Value  |
/// |--------|------|--------------------------------|-----------------|
/// | 0      | 4    | magic                          | b"CANR"         |
/// | 4      | 4    | version (u32)                  | 1               |
/// | 8      | 4    | valid_bps (u32)                | 5000            |
/// | 12     | 4    | invalid_bps (u32)              | 10001           |
/// | 16     | 4    | valid_share_bps (u32)          | 2500            |
/// | 20     | 4    | invalid_share_bps (u32)        | 10001           |
/// | 24     | 16   | valid_nonneg_amount (i128)     | 1000000         |
/// | 40     | 16   | invalid_neg_amount (i128)      | -1              |
/// | 56     | 16   | valid_pos_amount (i128)        | 1               |
/// | 72     | 16   | invalid_zero_amount (i128)     | 0               |
/// | 88     | 8    | valid_period_id (u64)          | 1               |
/// | 96     | 8    | invalid_period_id (u64)        | 0               |
/// | 104    | 48   | safe_add: a, b, expected       | 1000, 2000, 3000|
/// | 152    | 48   | safe_sub: a, b, expected       | 5000, 2000, 3000|
/// | 200    | 48   | safe_mul: a, b, expected       | 100, 200, 20000 |
/// | 248    | 48   | safe_div: a, b, expected       | 1000, 10, 100   |
/// | 296    | 36   | compute_share: amount(16),     | 10000, 5000,    |
/// |        |      |   bps(4), expected(16)         | 5000            |
/// | 332    | 12   | semver_from: major(4),         | (1,0,0)         |
/// |        |      |   minor(4), patch(4)           |                 |
/// | 344    | 12   | semver_to_valid: major(4),     | (1,0,1)         |
/// |        |      |   minor(4), patch(4)           |                 |
/// | 356    | 12   | semver_to_downgrade:           | (0,9,0)         |
/// |        |      |   major(4), minor(4), patch(4) |                 |
/// | 368    | 4    | valid_concentration_bps (u32)  | 5000            |
/// | 372    | 4    | invalid_concentration_bps(u32) | 10001           |
/// | 376    | 4    | multisig_threshold (u32)       | 2               |
/// | 380    | 4    | multisig_owner_count (u32)     | 3               |
/// | 384    | 4    | footer_magic                   | b"ANRY"         |
/// | 388    |      | end of blob                    |                 |
///
/// Total size: 388 bytes.
///
/// ## Security
///
/// - The self-test is a pure read-only entrypoint; it does not read or write
///   contract storage.
/// - The canary data is fixed at compile time and cannot be altered post-deploy.
/// - A corrupted canary (altered footer magic) causes a `CanaryCorrupted`
///   failure, preventing false passes from accidentally modified data.

/// Status code returned when all self-test checks pass.
const SELF_TEST_PASS: u32 = 0;

/// Canonical size of the canary blob in bytes.
const CANARY_BLOB_SIZE: usize = 388;

// ── Canary field offsets ────────────────────────────────────────────────────
const OFF_MAGIC: usize = 0; // 4 bytes
const OFF_VERSION: usize = 4; // u32
const OFF_VALID_BPS: usize = 8; // u32
const OFF_INVALID_BPS: usize = 12; // u32
const OFF_VALID_SHARE_BPS: usize = 16; // u32
const OFF_INVALID_SHARE_BPS: usize = 20; // u32
const OFF_VALID_NONNEG_AMOUNT: usize = 24; // i128 (16 bytes)
const OFF_INVALID_NEG_AMOUNT: usize = 40; // i128
const OFF_VALID_POS_AMOUNT: usize = 56; // i128
const OFF_INVALID_ZERO_AMOUNT: usize = 72; // i128
const OFF_VALID_PERIOD_ID: usize = 88; // u64
const OFF_INVALID_PERIOD_ID: usize = 96; // u64
const OFF_SAFE_ADD_A: usize = 104; // i128
const OFF_SAFE_ADD_B: usize = 120; // i128
const OFF_SAFE_ADD_EXPECTED: usize = 136; // i128
const OFF_SAFE_SUB_A: usize = 152; // i128
const OFF_SAFE_SUB_B: usize = 168; // i128
const OFF_SAFE_SUB_EXPECTED: usize = 184; // i128
const OFF_SAFE_MUL_A: usize = 200; // i128
const OFF_SAFE_MUL_B: usize = 216; // i128
const OFF_SAFE_MUL_EXPECTED: usize = 232; // i128
const OFF_SAFE_DIV_A: usize = 248; // i128
const OFF_SAFE_DIV_B: usize = 264; // i128
const OFF_SAFE_DIV_EXPECTED: usize = 280; // i128
const OFF_COMPUTE_SHARE_AMOUNT: usize = 296; // i128
const OFF_COMPUTE_SHARE_BPS: usize = 312; // u32
const OFF_COMPUTE_SHARE_EXPECTED: usize = 316; // i128
const OFF_SEMVER_FROM_MAJOR: usize = 332; // u32
const OFF_SEMVER_FROM_MINOR: usize = 336; // u32
const OFF_SEMVER_FROM_PATCH: usize = 340; // u32
const OFF_SEMVER_TO_VALID_MAJOR: usize = 344; // u32
const OFF_SEMVER_TO_VALID_MINOR: usize = 348; // u32
const OFF_SEMVER_TO_VALID_PATCH: usize = 352; // u32
const OFF_SEMVER_TO_DOWNGRADE_MAJOR: usize = 356; // u32
const OFF_SEMVER_TO_DOWNGRADE_MINOR: usize = 360; // u32
const OFF_SEMVER_TO_DOWNGRADE_PATCH: usize = 364; // u32
const OFF_VALID_CONCENTRATION_BPS: usize = 368; // u32
const OFF_INVALID_CONCENTRATION_BPS: usize = 372; // u32
const OFF_MULTISIG_THRESHOLD: usize = 376; // u32
const OFF_MULTISIG_OWNER_COUNT: usize = 380; // u32
const OFF_FOOTER_MAGIC: usize = 384; // 4 bytes

/// Self-test failure reason codes.
///
/// Each variant maps to a specific invariant check that can fail.
/// A non-zero return from `self_test()` identifies the first failure.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SelfTestErrorCode {
    /// All checks passed.
    Pass = 0,
    /// Canary blob has wrong size or corrupted magic/footer.
    CanaryCorrupted = 1,
    /// BPS upper-bound validation rejected a valid value.
    BpsValidationRejectValid = 2,
    /// BPS lower-bound validation accepted an invalid value.
    BpsValidationAcceptInvalid = 3,
    /// Share BPS upper-bound validation rejected a valid value.
    ShareBpsValidationRejectValid = 4,
    /// Share BPS lower-bound validation accepted an invalid value.
    ShareBpsValidationAcceptInvalid = 5,
    /// Non-negative amount validation rejected zero.
    NonNegAmountRejectZero = 6,
    /// Non-negative amount validation accepted negative.
    NonNegAmountAcceptNeg = 7,
    /// Positive amount validation accepted zero.
    PosAmountAcceptZero = 8,
    /// Positive amount validation rejected positive.
    PosAmountRejectPositive = 9,
    /// Period ID validation accepted zero.
    PeriodIdAcceptZero = 10,
    /// Period ID validation rejected valid.
    PeriodIdRejectValid = 11,
    /// Safe add returned wrong result.
    SafeAddWrongResult = 12,
    /// Safe sub returned wrong result.
    SafeSubWrongResult = 13,
    /// Safe mul returned wrong result.
    SafeMulWrongResult = 14,
    /// Safe div returned wrong result.
    SafeDivWrongResult = 15,
    /// Compute share returned wrong result.
    ComputeShareWrongResult = 16,
    /// Semver forward check returned wrong result for valid upgrade.
    SemverRejectUpgrade = 17,
    /// Semver forward check returned wrong result for downgrade.
    SemverAcceptDowngrade = 18,
    /// Concentration BPS validation rejected a valid value.
    ConcentrationBpsRejectValid = 19,
    /// Concentration BPS validation accepted an invalid value.
    ConcentrationBpsAcceptInvalid = 20,
    /// Multisig threshold validation returned wrong result.
    MultisigThresholdWrong = 21,
}

impl SelfTestErrorCode {
    /// Returns `true` if this code represents a pass.
    pub fn is_pass(self) -> bool {
        self as u32 == 0
    }

    /// Returns the raw `u32` value of this error code.
    pub fn to_u32(self) -> u32 {
        self as u32
    }
}

// ── Low-level parsing helpers ────────────────────────────────────────────────

/// Read a `u32` from a little-endian byte slice at the given offset.
#[inline(always)]
fn read_u32_le(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}

/// Read a `u64` from a little-endian byte slice at the given offset.
#[inline(always)]
fn read_u64_le(buf: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
        buf[offset + 4],
        buf[offset + 5],
        buf[offset + 6],
        buf[offset + 7],
    ])
}

/// Read an `i128` from a little-endian byte slice at the given offset.
#[inline(always)]
fn read_i128_le(buf: &[u8], offset: usize) -> i128 {
    i128::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
        buf[offset + 4],
        buf[offset + 5],
        buf[offset + 6],
        buf[offset + 7],
        buf[offset + 8],
        buf[offset + 9],
        buf[offset + 10],
        buf[offset + 11],
        buf[offset + 12],
        buf[offset + 13],
        buf[offset + 14],
        buf[offset + 15],
    ])
}

/// Check that the canary blob has the expected magic, size, and footer.
fn validate_canary_integrity(buf: &[u8]) -> Result<(), SelfTestErrorCode> {
    if buf.len() != CANARY_BLOB_SIZE {
        return Err(SelfTestErrorCode::CanaryCorrupted);
    }
    if &buf[OFF_MAGIC..OFF_MAGIC + 4] != b"CANR" {
        return Err(SelfTestErrorCode::CanaryCorrupted);
    }
    if &buf[OFF_FOOTER_MAGIC..OFF_FOOTER_MAGIC + 4] != b"ANRY" {
        return Err(SelfTestErrorCode::CanaryCorrupted);
    }
    Ok(())
}

// ── Individual invariant checks ──────────────────────────────────────────────

/// Check BPS validation assertions.
fn check_bps_validation(buf: &[u8]) -> Result<(), SelfTestErrorCode> {
    let valid_bps = read_u32_le(buf, OFF_VALID_BPS);
    let invalid_bps = read_u32_le(buf, OFF_INVALID_BPS);

    // Valid BPS (5000) must pass
    if crate::security_assertions::input_validation::assert_valid_bps(valid_bps).is_err() {
        return Err(SelfTestErrorCode::BpsValidationRejectValid);
    }
    // Invalid BPS (10001) must fail
    if crate::security_assertions::input_validation::assert_valid_bps(invalid_bps).is_ok() {
        return Err(SelfTestErrorCode::BpsValidationAcceptInvalid);
    }
    Ok(())
}

/// Check share BPS validation assertions.
fn check_share_bps_validation(buf: &[u8]) -> Result<(), SelfTestErrorCode> {
    let valid_share_bps = read_u32_le(buf, OFF_VALID_SHARE_BPS);
    let invalid_share_bps = read_u32_le(buf, OFF_INVALID_SHARE_BPS);

    if crate::security_assertions::input_validation::assert_valid_share_bps(valid_share_bps).is_err() {
        return Err(SelfTestErrorCode::ShareBpsValidationRejectValid);
    }
    if crate::security_assertions::input_validation::assert_valid_share_bps(invalid_share_bps).is_ok() {
        return Err(SelfTestErrorCode::ShareBpsValidationAcceptInvalid);
    }
    Ok(())
}

/// Check amount validation assertions.
fn check_amount_validation(buf: &[u8]) -> Result<(), SelfTestErrorCode> {
    let valid_nonneg = read_i128_le(buf, OFF_VALID_NONNEG_AMOUNT);
    let invalid_neg = read_i128_le(buf, OFF_INVALID_NEG_AMOUNT);
    let valid_pos = read_i128_le(buf, OFF_VALID_POS_AMOUNT);
    let invalid_zero = read_i128_le(buf, OFF_INVALID_ZERO_AMOUNT);

    // Non-negative: allow zero and positive, reject negative
    if crate::security_assertions::input_validation::assert_non_negative_amount(valid_nonneg).is_err() {
        return Err(SelfTestErrorCode::NonNegAmountRejectZero);
    }
    if crate::security_assertions::input_validation::assert_non_negative_amount(invalid_neg).is_ok() {
        return Err(SelfTestErrorCode::NonNegAmountAcceptNeg);
    }

    // Positive: reject zero, accept positive
    if crate::security_assertions::input_validation::assert_positive_amount(valid_pos).is_err() {
        return Err(SelfTestErrorCode::PosAmountRejectPositive);
    }
    if crate::security_assertions::input_validation::assert_positive_amount(invalid_zero).is_ok() {
        return Err(SelfTestErrorCode::PosAmountAcceptZero);
    }
    Ok(())
}

/// Check period ID validation assertions.
fn check_period_id_validation(buf: &[u8]) -> Result<(), SelfTestErrorCode> {
    let valid_period_id = read_u64_le(buf, OFF_VALID_PERIOD_ID);
    let invalid_period_id = read_u64_le(buf, OFF_INVALID_PERIOD_ID);

    if crate::security_assertions::input_validation::assert_positive_period_id(valid_period_id).is_err() {
        return Err(SelfTestErrorCode::PeriodIdRejectValid);
    }
    if crate::security_assertions::input_validation::assert_positive_period_id(invalid_period_id).is_ok() {
        return Err(SelfTestErrorCode::PeriodIdAcceptZero);
    }
    Ok(())
}

/// Check safe math addition.
fn check_safe_add(buf: &[u8]) -> Result<(), SelfTestErrorCode> {
    let a = read_i128_le(buf, OFF_SAFE_ADD_A);
    let b = read_i128_le(buf, OFF_SAFE_ADD_B);
    let expected = read_i128_le(buf, OFF_SAFE_ADD_EXPECTED);

    match crate::security_assertions::safe_math::safe_add(a, b) {
        Ok(actual) if actual == expected => Ok(()),
        _ => Err(SelfTestErrorCode::SafeAddWrongResult),
    }
}

/// Check safe math subtraction.
fn check_safe_sub(buf: &[u8]) -> Result<(), SelfTestErrorCode> {
    let a = read_i128_le(buf, OFF_SAFE_SUB_A);
    let b = read_i128_le(buf, OFF_SAFE_SUB_B);
    let expected = read_i128_le(buf, OFF_SAFE_SUB_EXPECTED);

    match crate::security_assertions::safe_math::safe_sub(a, b) {
        Ok(actual) if actual == expected => Ok(()),
        _ => Err(SelfTestErrorCode::SafeSubWrongResult),
    }
}

/// Check safe math multiplication.
fn check_safe_mul(buf: &[u8]) -> Result<(), SelfTestErrorCode> {
    let a = read_i128_le(buf, OFF_SAFE_MUL_A);
    let b = read_i128_le(buf, OFF_SAFE_MUL_B);
    let expected = read_i128_le(buf, OFF_SAFE_MUL_EXPECTED);

    match crate::security_assertions::safe_math::safe_mul(a, b) {
        Ok(actual) if actual == expected => Ok(()),
        _ => Err(SelfTestErrorCode::SafeMulWrongResult),
    }
}

/// Check safe math division.
fn check_safe_div(buf: &[u8]) -> Result<(), SelfTestErrorCode> {
    let a = read_i128_le(buf, OFF_SAFE_DIV_A);
    let b = read_i128_le(buf, OFF_SAFE_DIV_B);
    let expected = read_i128_le(buf, OFF_SAFE_DIV_EXPECTED);

    match crate::security_assertions::safe_math::safe_div(a, b) {
        Ok(actual) if actual == expected => Ok(()),
        _ => Err(SelfTestErrorCode::SafeDivWrongResult),
    }
}

/// Check compute_share.
fn check_compute_share(buf: &[u8]) -> Result<(), SelfTestErrorCode> {
    let amount = read_i128_le(buf, OFF_COMPUTE_SHARE_AMOUNT);
    let bps = read_u32_le(buf, OFF_COMPUTE_SHARE_BPS);
    let expected = read_i128_le(buf, OFF_COMPUTE_SHARE_EXPECTED);

    match crate::security_assertions::safe_math::safe_compute_share(amount, bps) {
        Ok(actual) if actual == expected => Ok(()),
        _ => Err(SelfTestErrorCode::ComputeShareWrongResult),
    }
}

/// Check semver forward assertions.
fn check_semver(buf: &[u8]) -> Result<(), SelfTestErrorCode> {
    let from = (
        read_u32_le(buf, OFF_SEMVER_FROM_MAJOR),
        read_u32_le(buf, OFF_SEMVER_FROM_MINOR),
        read_u32_le(buf, OFF_SEMVER_FROM_PATCH),
    );
    let to_valid = (
        read_u32_le(buf, OFF_SEMVER_TO_VALID_MAJOR),
        read_u32_le(buf, OFF_SEMVER_TO_VALID_MINOR),
        read_u32_le(buf, OFF_SEMVER_TO_VALID_PATCH),
    );
    let to_downgrade = (
        read_u32_le(buf, OFF_SEMVER_TO_DOWNGRADE_MAJOR),
        read_u32_le(buf, OFF_SEMVER_TO_DOWNGRADE_MINOR),
        read_u32_le(buf, OFF_SEMVER_TO_DOWNGRADE_PATCH),
    );

    // Valid upgrade must succeed
    if crate::assert_semver_forward(from, to_valid).is_err() {
        return Err(SelfTestErrorCode::SemverRejectUpgrade);
    }
    // Downgrade must fail
    if crate::assert_semver_forward(from, to_downgrade).is_ok() {
        return Err(SelfTestErrorCode::SemverAcceptDowngrade);
    }
    Ok(())
}

/// Check concentration BPS validation.
fn check_concentration_bps(buf: &[u8]) -> Result<(), SelfTestErrorCode> {
    let valid = read_u32_le(buf, OFF_VALID_CONCENTRATION_BPS);
    let invalid = read_u32_le(buf, OFF_INVALID_CONCENTRATION_BPS);

    if crate::security_assertions::input_validation::assert_valid_concentration_bps(valid).is_err() {
        return Err(SelfTestErrorCode::ConcentrationBpsRejectValid);
    }
    if crate::security_assertions::input_validation::assert_valid_concentration_bps(invalid).is_ok() {
        return Err(SelfTestErrorCode::ConcentrationBpsAcceptInvalid);
    }
    Ok(())
}

/// Check multisig threshold validation.
fn check_multisig_threshold(buf: &[u8]) -> Result<(), SelfTestErrorCode> {
    let threshold = read_u32_le(buf, OFF_MULTISIG_THRESHOLD);
    let owner_count = read_u32_le(buf, OFF_MULTISIG_OWNER_COUNT);

    let result =
        crate::security_assertions::input_validation::assert_valid_multisig_threshold(
            threshold,
            owner_count,
        );
    if result.is_err() {
        return Err(SelfTestErrorCode::MultisigThresholdWrong);
    }
    Ok(())
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Canary data embedded in the WASM binary at compile time.
///
/// The binary file is generated by `build_canary.py` (or equivalent) and
/// checked into the repository. It is loaded via `include_bytes!` to guarantee
/// the data is part of the compiled WASM and cannot be altered post-deploy.
pub(crate) static CANARY_DATA: &[u8] = &include_bytes!("canary_data.bin")[..];

/// Run all self-test invariant checks against the embedded canary dataset.
///
/// Returns `Ok(())` if all checks pass, or `Err(SelfTestErrorCode)` with the
/// first failure reason code.
///
/// This function is deterministic, stateless, and safe to call at any time.
/// It does not read or write contract storage.
pub fn run_self_test() -> Result<(), SelfTestErrorCode> {
    let buf = CANARY_DATA;

    // 1. Validate canary integrity first
    validate_canary_integrity(buf)?;

    // 2. Run invariant checks sequentially (fail-fast)
    check_bps_validation(buf)?;
    check_share_bps_validation(buf)?;
    check_amount_validation(buf)?;
    check_period_id_validation(buf)?;
    check_safe_add(buf)?;
    check_safe_sub(buf)?;
    check_safe_mul(buf)?;
    check_safe_div(buf)?;
    check_compute_share(buf)?;
    check_semver(buf)?;
    check_concentration_bps(buf)?;
    check_multisig_threshold(buf)?;

    Ok(())
}

/// Convenience wrapper: runs the self-test and returns a `u32` status code.
///
/// - `0` = all checks passed
/// - Non-zero = the reason code of the first failure
///
/// This is the entrypoint called by the contract's `self_test()` method.
pub fn self_test_status() -> u32 {
    match run_self_test() {
        Ok(()) => SELF_TEST_PASS,
        Err(code) => code.to_u32(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that the embedded canary data has the correct size.
    #[test]
    fn test_canary_blob_size() {
        assert_eq!(CANARY_DATA.len(), CANARY_BLOB_SIZE);
    }

    /// Verify that the canary integrity check passes.
    #[test]
    fn test_canary_integrity() {
        assert!(validate_canary_integrity(CANARY_DATA).is_ok());
    }

    /// Verify that a corrupted canary (wrong footer) is detected.
    #[test]
    fn test_canary_integrity_corrupted_footer() {
        let mut corrupted = CANARY_DATA.to_vec();
        let len = corrupted.len();
        // Corrupt the footer magic
        corrupted[len - 1] ^= 0xFF;
        assert_eq!(
            validate_canary_integrity(&corrupted),
            Err(SelfTestErrorCode::CanaryCorrupted)
        );
    }

    /// Verify that a corrupted canary (wrong size) is detected.
    #[test]
    fn test_canary_integrity_wrong_size() {
        let truncated = &CANARY_DATA[..100];
        assert_eq!(
            validate_canary_integrity(truncated),
            Err(SelfTestErrorCode::CanaryCorrupted)
        );
    }

    /// Verify that a corrupted canary (wrong magic) is detected.
    #[test]
    fn test_canary_integrity_corrupted_magic() {
        let mut corrupted = CANARY_DATA.to_vec();
        corrupted[0] ^= 0xFF;
        assert_eq!(
            validate_canary_integrity(&corrupted),
            Err(SelfTestErrorCode::CanaryCorrupted)
        );
    }

    /// Verify the full self-test passes with the real canary data.
    #[test]
    fn test_self_test_passes() {
        assert_eq!(run_self_test(), Ok(()));
    }

    /// Verify the convenience wrapper returns 0 on pass.
    #[test]
    fn test_self_test_status_pass() {
        assert_eq!(self_test_status(), 0);
    }

    /// Verify individual check functions pass with real canary data.
    #[test]
    fn test_individual_bps_check() {
        assert!(check_bps_validation(CANARY_DATA).is_ok());
    }

    #[test]
    fn test_individual_share_bps_check() {
        assert!(check_share_bps_validation(CANARY_DATA).is_ok());
    }

    #[test]
    fn test_individual_amount_check() {
        assert!(check_amount_validation(CANARY_DATA).is_ok());
    }

    #[test]
    fn test_individual_period_id_check() {
        assert!(check_period_id_validation(CANARY_DATA).is_ok());
    }

    #[test]
    fn test_individual_safe_add_check() {
        assert!(check_safe_add(CANARY_DATA).is_ok());
    }

    #[test]
    fn test_individual_safe_sub_check() {
        assert!(check_safe_sub(CANARY_DATA).is_ok());
    }

    #[test]
    fn test_individual_safe_mul_check() {
        assert!(check_safe_mul(CANARY_DATA).is_ok());
    }

    #[test]
    fn test_individual_safe_div_check() {
        assert!(check_safe_div(CANARY_DATA).is_ok());
    }

    #[test]
    fn test_individual_compute_share_check() {
        assert!(check_compute_share(CANARY_DATA).is_ok());
    }

    #[test]
    fn test_individual_semver_check() {
        assert!(check_semver(CANARY_DATA).is_ok());
    }

    #[test]
    fn test_individual_concentration_bps_check() {
        assert!(check_concentration_bps(CANARY_DATA).is_ok());
    }

    #[test]
    fn test_individual_multisig_threshold_check() {
        assert!(check_multisig_threshold(CANARY_DATA).is_ok());
    }

    /// Verify that individual checks return the correct error when canary data is corrupted.
    #[test]
    fn test_bps_check_on_corrupted_returns_error() {
        let mut corrupted = CANARY_DATA.to_vec();
        // Flip bits in the valid_bps field to make it invalid
        let off = OFF_VALID_BPS;
        corrupted[off] = 0xFF;
        corrupted[off + 1] = 0xFF;
        corrupted[off + 2] = 0xFF;
        corrupted[off + 3] = 0x7F; // > 10000
        let result = check_bps_validation(&corrupted);
        assert!(result.is_err());
    }

    /// Verify that corrupted safe_add data causes failure.
    #[test]
    fn test_safe_add_on_corrupted_returns_error() {
        let mut corrupted = CANARY_DATA.to_vec();
        // Change expected value to wrong number
        let off = OFF_SAFE_ADD_EXPECTED;
        corrupted[off] = 0x01; // expected becomes 1
        let result = check_safe_add(&corrupted);
        assert!(result.is_err());
    }

    /// Verify SelfTestErrorCode conversions.
    #[test]
    fn test_error_code_is_pass() {
        assert!(SelfTestErrorCode::Pass.is_pass());
        assert!(!SelfTestErrorCode::CanaryCorrupted.is_pass());
    }

    #[test]
    fn test_error_code_to_u32() {
        assert_eq!(SelfTestErrorCode::Pass.to_u32(), 0);
        assert_eq!(SelfTestErrorCode::CanaryCorrupted.to_u32(), 1);
        assert_eq!(SelfTestErrorCode::BpsValidationRejectValid.to_u32(), 2);
    }
}
