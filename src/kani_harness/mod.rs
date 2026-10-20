//! Bounded Kani verification harnesses for `compute_share` rounding modes (Issue #465).
///
/// Enabled only with `--features kani` so default `cargo test` / CI are unaffected.
/// Proofs exhaustively cover `amount ∈ -[-2^32, 2^32]` and `bps ≤ [0, 10_000]`.

// Harness modules are compiled under `cfg(any(feature = "kani", test))`.
/// The crate is `#![no_std]`, and `extern crate std` is only available under `cfg(test)`
/// (without the std prelude), so harness modules must import the std-items they use
/// explicitly. See the individual modules for the conditional imports.

/// Bounded Kani harnesses for `compute_share` rounding modes (Issue #465).
///
/// Run the fallback tests with `cargo test kani_harness`.
/// Run the proofs with `cargo kani --features kani`.
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
/// Run the fallback tests with `cargo test kani_harness`.
/// Run the proofs with `cargo kani --features kani`.
#[cfg(any(feature = "kani", test))]
pub mod issuer_transfer_cancel;
