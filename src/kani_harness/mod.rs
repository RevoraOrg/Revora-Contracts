//! Bounded Kani verification harnesses for `compute_share` rounding modes (Issue #465).
//!
//! Enabled only with `--features kani` so default `cargo test` / CI are unaffected.
//! Proofs exhaustively cover `amount ∈ [-2^32, 2^32]` and `bps ∈ [0, 10_000]`.

#[cfg(feature = "kani")]
pub mod compute_share;
