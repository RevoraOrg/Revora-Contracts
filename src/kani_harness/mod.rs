//! Bounded Kani verification harnesses for `compute_share` rounding modes (Issue #465).
//!
//! Enabled only with `--features kani` so default `cargo test` / CI are unaffected.
//! Proofs exhaustively cover `amount ∈ [-2^32, 2^32]` and `bps ∈ [0, 10_000]`.

#[cfg(any(feature = "kani", test))]
pub mod compute_share;

/// Kani bounded verification harness for `cancel_issuer_transfer` (Issue #577).
///
/// The harness models the issuer-transfer state machine as a pure-Rust function
/// and proves:
/// - No orphan `PendingIssuerTransfer` storage key after a successful cancel.
/// - The offering's issuer field is unchanged by cancel.
/// - Cancel with no proposal pending returns `NoTransferPending`.
/// - Propose → Cancel leaves storage identical to the pre-propose baseline.
/// - Stored `expiry_secs` is always `0` or within `[MIN_EXPIRY, MAX_EXPIRY]`.
/// - Double-cancel is rejected with `NoTransferPending`.
/// - Unauthorised cancel is rejected.
///
/// The `#[cfg(test)]` shims run as ordinary cargo tests in CI without the Kani toolchain.
pub mod issuer_transfer_cancel;

/// Kani bounded verification harness for blacklist add/remove idempotency (Issue #575).
///
/// Models the per-offering blacklist as a set and proves that arbitrary bounded
/// sequences of add/remove operations converge to the reference set semantics:
/// adds and removes are idempotent, operations on distinct addresses commute,
/// and the final state is always a set. Includes the add-remove-add-on-the-same-
/// address edge case from the issue.
pub mod blacklist_idempotency;
