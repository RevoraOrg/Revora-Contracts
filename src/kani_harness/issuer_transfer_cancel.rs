#![allow(unexpected_cfgs)]
//! Kani bounded verification harness for `cancel_issuer_transfer` (Issue #577).
//!
//! ## What is proved
//!
//! The harnesses below model the **issuer-transfer state machine** as a pure-Rust
//! function that mirrors the on-chain storage transitions.  Kani exhaustively
//! explores every reachable pre-state and asserts:
//!
//! 1. **No orphan `PendingIssuerTransfer` key** — after a successful cancel the key
//!    is absent from the storage model.
//! 2. **Offering issuer field is unchanged** — `cancel_issuer_transfer` must never
//!    mutate the `OfferingIssuer` reverse-lookup or the `OfferingRecord`.
//! 3. **Cancel with no proposal pending** — calling cancel when no transfer is
//!    pending must return `NoTransferPending` and leave all storage keys intact.
//! 4. **Propose → Cancel idempotency** — propose followed by cancel leaves storage
//!    identical to the never-proposed baseline (no residual keys).
//! 5. **Expiry bounds are respected** — the stored `expiry_secs` is always either
//!    `0` (default) or within `[MIN_EXPIRY, MAX_EXPIRY]` after a propose.
//! 6. **Double-cancel is rejected** — a second cancel on an already-cancelled
//!    proposal returns `NoTransferPending`.
//!
//! ## Security notes
//!
//! - All proofs operate on a **pure state model** — no `Env`, no Soroban host.
//!   This lets Kani reason over the full symbolic domain without host stubs.
//! - Auth (`require_auth`, `require_issuer_quorum_auth`) is modelled as a boolean
//!   precondition (`auth_ok`).  The harnesses assume `auth_ok = true` to focus on
//!   storage invariants; auth-failure paths are covered by the integration tests in
//!   `src/test.rs`.
//! - `expiry_secs = 0` is the contract's sentinel for "use the 7-day default"; the
//!   model preserves this semantic and never writes a zero to mean an expired window.
//!
//! ## Cargo test shim
//!
//! Every `#[kani::proof]` is also wrapped in a `#[test]` that calls the same body
//! with fixed concrete inputs so `cargo test` catches basic regressions without the
//! Kani tool-chain.

/// Default expiry (7 days in seconds) mirroring `ISSUER_TRANSFER_EXPIRY_SECS`.
pub const DEFAULT_EXPIRY_SECS: u64 = 7 * 24 * 60 * 60; // 604_800
/// Minimum configurable expiry (1 hour) mirroring `MIN_ISSUER_TRANSFER_EXPIRY_SECS`.
pub const MIN_EXPIRY_SECS: u64 = 60 * 60; // 3_600
/// Maximum configurable expiry (30 days) mirroring `MAX_ISSUER_TRANSFER_EXPIRY_SECS`.
pub const MAX_EXPIRY_SECS: u64 = 30 * 24 * 60 * 60; // 2_592_000

// ── Error codes (mirrors RevoraError discriminants used by issuer-transfer) ──

/// Mirrors `RevoraError::NoTransferPending` (discriminant 13).
pub const ERR_NO_TRANSFER_PENDING: u32 = 13;
/// Mirrors `RevoraError::IssuerTransferPending` (discriminant 12).
pub const ERR_ISSUER_TRANSFER_PENDING: u32 = 12;
/// Mirrors `RevoraError::OfferingNotFound` (discriminant 2 or equivalent).
pub const ERR_OFFERING_NOT_FOUND: u32 = 2;

// ── Minimal state model ───────────────────────────────────────────────────────

/// A symbolic byte-array address reduced to a `u8` id for Kani tractability.
/// Values 1–255 represent distinct on-chain addresses; 0 is the null/unset sentinel.
pub type AddrId = u8;

/// Mirrors the on-chain `PendingTransfer` struct, simplified for bounded proofs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingTransfer {
    /// Proposed new issuer (AddrId, 0 = absent).
    pub new_issuer: AddrId,
    /// Ledger timestamp at proposal creation.
    pub timestamp: u64,
    /// Stored expiry (0 = use DEFAULT_EXPIRY_SECS; otherwise clamped to [MIN, MAX]).
    pub expiry_secs: u64,
}

/// Minimal offering state: just the current primary issuer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfferingState {
    /// Current primary issuer address id.
    pub issuer: AddrId,
}

/// Flat model of the storage keys relevant to issuer transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageModel {
    /// `Some(pt)` ↔ `PendingIssuerTransfer` key is present.
    pub pending: Option<PendingTransfer>,
    /// Current offering state (issuer never changes during cancel).
    pub offering: OfferingState,
    /// Mirrors `OfferingIssuer` reverse-lookup; must equal `offering.issuer`.
    pub offering_issuer_lookup: AddrId,
}

// ── State-machine helpers (pure functions mirroring contract logic) ───────────

/// Error type returned by state-machine operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferError {
    NoTransferPending,
    IssuerTransferPending,
    OfferingNotFound,
    NotAuthorized,
}

/// `propose_issuer_transfer` — pure state transition.
///
/// Preconditions:
/// - `caller == storage.offering.issuer` (issuer auth).
/// - `new_issuer != 0` (valid address).
/// - `new_issuer != caller` (self-transfer would be a no-op / not useful; contract
///    allows it but we model the general case).
pub fn model_propose(
    storage: &mut StorageModel,
    caller: AddrId,
    new_issuer: AddrId,
    timestamp: u64,
    expiry_secs: u64,
) -> Result<(), TransferError> {
    if storage.offering.issuer == 0 {
        return Err(TransferError::OfferingNotFound);
    }
    if caller != storage.offering.issuer {
        return Err(TransferError::NotAuthorized);
    }
    if storage.pending.is_some() {
        return Err(TransferError::IssuerTransferPending);
    }

    // Clamp expiry exactly as the contract does.
    let effective_expiry =
        if expiry_secs == 0 { 0 } else { expiry_secs.clamp(MIN_EXPIRY_SECS, MAX_EXPIRY_SECS) };

    storage.pending =
        Some(PendingTransfer { new_issuer, timestamp, expiry_secs: effective_expiry });
    Ok(())
}

/// `cancel_issuer_transfer` — pure state transition.
///
/// Preconditions:
/// - `caller == storage.offering.issuer` (issuer auth).
pub fn model_cancel(
    storage: &mut StorageModel,
    caller: AddrId,
) -> Result<PendingTransfer, TransferError> {
    if storage.offering.issuer == 0 {
        return Err(TransferError::OfferingNotFound);
    }
    if caller != storage.offering.issuer {
        return Err(TransferError::NotAuthorized);
    }
    match storage.pending.take() {
        Some(pt) => Ok(pt),
        None => Err(TransferError::NoTransferPending),
    }
}

/// Invariant: `OfferingIssuer` reverse-lookup always matches the offering primary issuer.
pub fn assert_issuer_lookup_consistent(storage: &StorageModel) {
    assert_eq!(
        storage.offering_issuer_lookup, storage.offering.issuer,
        "OfferingIssuer lookup must always match the offering primary issuer"
    );
}

/// Invariant: no `PendingIssuerTransfer` key survives after a successful cancel.
pub fn assert_no_orphan_pending(storage: &StorageModel) {
    assert!(storage.pending.is_none(), "PendingIssuerTransfer key must be absent after cancel");
}

// ── Kani proofs ───────────────────────────────────────────────────────────────

#[cfg(kani)]
mod proofs {
    use super::*;

    /// Helper: build a fully symbolic `StorageModel` with a valid offering.
    fn symbolic_storage_with_offering() -> StorageModel {
        let issuer: AddrId = kani::any();
        kani::assume(issuer != 0); // 0 = null sentinel

        StorageModel {
            pending: None,
            offering: OfferingState { issuer },
            offering_issuer_lookup: issuer,
        }
    }

    /// Helper: build a symbolic `PendingTransfer` that satisfies contract invariants.
    fn symbolic_pending(issuer: AddrId) -> PendingTransfer {
        let new_issuer: AddrId = kani::any();
        kani::assume(new_issuer != 0);
        kani::assume(new_issuer != issuer); // self-transfer corner case excluded

        let timestamp: u64 = kani::any();
        let expiry_secs: u64 = kani::any();
        // After propose, expiry is either 0 or in [MIN, MAX].
        let effective_expiry =
            if expiry_secs == 0 { 0 } else { expiry_secs.clamp(MIN_EXPIRY_SECS, MAX_EXPIRY_SECS) };

        PendingTransfer { new_issuer, timestamp, expiry_secs: effective_expiry }
    }

    // ── Proof 1: cancel removes PendingIssuerTransfer key ────────────────────

    /// After a successful cancel the `PendingIssuerTransfer` key must be absent.
    ///
    /// Pre-state: offering exists, pending transfer present.
    /// Post-state: `storage.pending == None`.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_cancel_removes_pending_key() {
        let mut storage = symbolic_storage_with_offering();
        let pending = symbolic_pending(storage.offering.issuer);
        storage.pending = Some(pending);

        let caller = storage.offering.issuer; // authorised caller
        let result = model_cancel(&mut storage, caller);

        assert!(result.is_ok(), "cancel with valid pending must succeed");
        assert_no_orphan_pending(&storage);
    }

    // ── Proof 2: cancel does not mutate offering issuer ───────────────────────

    /// `cancel_issuer_transfer` must never change `offering.issuer` or the
    /// `OfferingIssuer` reverse-lookup.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_cancel_does_not_change_issuer() {
        let mut storage = symbolic_storage_with_offering();
        let pending = symbolic_pending(storage.offering.issuer);
        storage.pending = Some(pending);

        let issuer_before = storage.offering.issuer;
        let lookup_before = storage.offering_issuer_lookup;

        let caller = storage.offering.issuer;
        let _ = model_cancel(&mut storage, caller);

        assert_eq!(
            storage.offering.issuer, issuer_before,
            "offering.issuer must not change after cancel"
        );
        assert_eq!(
            storage.offering_issuer_lookup, lookup_before,
            "OfferingIssuer lookup must not change after cancel"
        );
        assert_issuer_lookup_consistent(&storage);
    }

    // ── Proof 3: cancel with no pending returns NoTransferPending ────────────

    /// When no transfer is pending, `cancel_issuer_transfer` must return
    /// `NoTransferPending` and leave all storage unchanged.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_cancel_no_pending_returns_error() {
        let mut storage = symbolic_storage_with_offering();
        // No pending transfer — storage.pending is None.

        let issuer_before = storage.offering.issuer;

        let caller = storage.offering.issuer;
        let result = model_cancel(&mut storage, caller);

        assert_eq!(
            result,
            Err(TransferError::NoTransferPending),
            "cancel with no pending must return NoTransferPending"
        );
        // Storage must be completely unchanged.
        assert!(storage.pending.is_none());
        assert_eq!(storage.offering.issuer, issuer_before);
        assert_issuer_lookup_consistent(&storage);
    }

    // ── Proof 4: propose → cancel leaves storage identical to baseline ────────

    /// After `propose` followed by `cancel`, the storage must be byte-for-byte
    /// identical to the pre-propose baseline — no residual keys.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_propose_cancel_idempotent_storage() {
        let mut storage = symbolic_storage_with_offering();
        let baseline = storage; // snapshot before any mutation

        let new_issuer: AddrId = kani::any();
        kani::assume(new_issuer != 0);
        kani::assume(new_issuer != storage.offering.issuer);

        let timestamp: u64 = kani::any();
        let expiry_secs: u64 = kani::any();

        let propose_result = model_propose(
            &mut storage,
            storage.offering.issuer,
            new_issuer,
            timestamp,
            expiry_secs,
        );
        kani::assume(propose_result.is_ok()); // only explore the success path

        let caller = storage.offering.issuer;
        let cancel_result = model_cancel(&mut storage, caller);
        assert!(cancel_result.is_ok(), "cancel after successful propose must succeed");

        // Full storage equality with baseline.
        assert_eq!(
            storage, baseline,
            "storage after propose+cancel must equal the never-proposed baseline"
        );
    }

    // ── Proof 5: stored expiry_secs is always 0 or in [MIN, MAX] ─────────────

    /// After `propose`, the stored `expiry_secs` is either `0` (default sentinel)
    /// or within `[MIN_EXPIRY_SECS, MAX_EXPIRY_SECS]`.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_propose_expiry_clamped() {
        let mut storage = symbolic_storage_with_offering();

        let new_issuer: AddrId = kani::any();
        kani::assume(new_issuer != 0);
        kani::assume(new_issuer != storage.offering.issuer);

        let timestamp: u64 = kani::any();
        let expiry_secs: u64 = kani::any();

        let result = model_propose(
            &mut storage,
            storage.offering.issuer,
            new_issuer,
            timestamp,
            expiry_secs,
        );

        if result.is_ok() {
            let pt = storage.pending.unwrap();
            // expiry_secs must be 0 or within the valid range.
            let valid = pt.expiry_secs == 0
                || (pt.expiry_secs >= MIN_EXPIRY_SECS && pt.expiry_secs <= MAX_EXPIRY_SECS);
            assert!(valid, "stored expiry_secs must be 0 or in [MIN_EXPIRY, MAX_EXPIRY]");
        }
    }

    // ── Proof 6: double-cancel returns NoTransferPending ─────────────────────

    /// A second `cancel_issuer_transfer` call on an already-cancelled proposal must
    /// return `NoTransferPending`; no storage mutation occurs on the second call.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_double_cancel_rejected() {
        let mut storage = symbolic_storage_with_offering();
        let pending = symbolic_pending(storage.offering.issuer);
        storage.pending = Some(pending);

        let caller = storage.offering.issuer;

        // First cancel — must succeed.
        let first = model_cancel(&mut storage, caller);
        assert!(first.is_ok(), "first cancel must succeed");
        assert_no_orphan_pending(&storage);

        // Second cancel — must fail.
        let second = model_cancel(&mut storage, caller);
        assert_eq!(
            second,
            Err(TransferError::NoTransferPending),
            "second cancel must return NoTransferPending"
        );
        // Storage still clean.
        assert_no_orphan_pending(&storage);
    }

    // ── Proof 7: cancel by non-issuer is rejected ─────────────────────────────

    /// An unauthorised caller cannot cancel a pending transfer; storage is unchanged.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_cancel_by_non_issuer_rejected() {
        let mut storage = symbolic_storage_with_offering();
        let pending = symbolic_pending(storage.offering.issuer);
        storage.pending = Some(pending);

        let attacker: AddrId = kani::any();
        kani::assume(attacker != storage.offering.issuer);
        kani::assume(attacker != 0);

        let pending_before = storage.pending;
        let issuer_before = storage.offering.issuer;

        let result = model_cancel(&mut storage, attacker);

        assert_eq!(result, Err(TransferError::NotAuthorized), "non-issuer cancel must be rejected");
        // Storage must be completely unchanged.
        assert_eq!(storage.pending, pending_before);
        assert_eq!(storage.offering.issuer, issuer_before);
        assert_issuer_lookup_consistent(&storage);
    }

    // ── Proof 8: cancel returns exactly the stored pending transfer ───────────

    /// The `PendingTransfer` value returned by a successful cancel must exactly
    /// match what was stored by the preceding propose.
    #[kani::proof]
    #[kani::unwind(2)]
    fn proof_cancel_returns_correct_pending_value() {
        let mut storage = symbolic_storage_with_offering();
        let pending = symbolic_pending(storage.offering.issuer);
        storage.pending = Some(pending);

        let caller = storage.offering.issuer;
        let result = model_cancel(&mut storage, caller);

        assert!(result.is_ok());
        let returned = result.unwrap();
        assert_eq!(
            returned, pending,
            "cancel must return the exact PendingTransfer that was stored"
        );
    }
}

// ── Cargo-test shims (concrete inputs; always run in CI) ──────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn base_storage(issuer: AddrId) -> StorageModel {
        StorageModel {
            pending: None,
            offering: OfferingState { issuer },
            offering_issuer_lookup: issuer,
        }
    }

    // ── Test 1: cancel removes pending key ────────────────────────────────────

    /// Concrete analogue of `proof_cancel_removes_pending_key`.
    #[test]
    fn cancel_removes_pending_key() {
        let issuer: AddrId = 1;
        let new_issuer: AddrId = 2;
        let mut storage = base_storage(issuer);
        storage.pending = Some(PendingTransfer { new_issuer, timestamp: 1000, expiry_secs: 0 });

        let result = model_cancel(&mut storage, issuer);
        assert!(result.is_ok());
        assert_no_orphan_pending(&storage);
    }

    // ── Test 2: cancel does not change offering issuer ────────────────────────

    /// `cancel_issuer_transfer` must never mutate the offering's primary issuer.
    #[test]
    fn cancel_does_not_change_issuer() {
        let issuer: AddrId = 3;
        let new_issuer: AddrId = 4;
        let mut storage = base_storage(issuer);
        storage.pending =
            Some(PendingTransfer { new_issuer, timestamp: 500, expiry_secs: MIN_EXPIRY_SECS });

        let _ = model_cancel(&mut storage, issuer);

        assert_eq!(storage.offering.issuer, issuer);
        assert_eq!(storage.offering_issuer_lookup, issuer);
        assert_issuer_lookup_consistent(&storage);
    }

    // ── Test 3: cancel with no pending returns error ──────────────────────────

    /// Cancel on an offering with no pending transfer must return `NoTransferPending`.
    #[test]
    fn cancel_no_pending_returns_error() {
        let issuer: AddrId = 5;
        let mut storage = base_storage(issuer);

        let result = model_cancel(&mut storage, issuer);
        assert_eq!(result, Err(TransferError::NoTransferPending));
        assert!(storage.pending.is_none());
        assert_eq!(storage.offering.issuer, issuer);
    }

    // ── Test 4: propose → cancel idempotency ─────────────────────────────────

    /// `storage` after `propose` then `cancel` must equal the pre-propose snapshot.
    #[test]
    fn propose_cancel_idempotent_storage() {
        let issuer: AddrId = 6;
        let new_issuer: AddrId = 7;
        let mut storage = base_storage(issuer);
        let baseline = storage;

        model_propose(&mut storage, issuer, new_issuer, 1000, 0).unwrap();
        assert!(storage.pending.is_some());

        model_cancel(&mut storage, issuer).unwrap();

        assert_eq!(storage, baseline, "storage after propose+cancel must match baseline");
    }

    // ── Test 5: expiry clamped correctly ─────────────────────────────────────

    /// `expiry_secs` below minimum is clamped up; above maximum is clamped down.
    #[test]
    fn propose_expiry_clamped_to_bounds() {
        let issuer: AddrId = 8;
        let new_issuer: AddrId = 9;

        // Below minimum → clamped to MIN_EXPIRY_SECS.
        let mut s1 = base_storage(issuer);
        model_propose(&mut s1, issuer, new_issuer, 0, 1).unwrap();
        let pt = s1.pending.unwrap();
        assert_eq!(pt.expiry_secs, MIN_EXPIRY_SECS);

        // Above maximum → clamped to MAX_EXPIRY_SECS.
        let mut s2 = base_storage(issuer);
        model_propose(&mut s2, issuer, new_issuer, 0, u64::MAX).unwrap();
        let pt2 = s2.pending.unwrap();
        assert_eq!(pt2.expiry_secs, MAX_EXPIRY_SECS);

        // Zero → stored as 0 (use default sentinel).
        let mut s3 = base_storage(issuer);
        model_propose(&mut s3, issuer, new_issuer, 0, 0).unwrap();
        let pt3 = s3.pending.unwrap();
        assert_eq!(pt3.expiry_secs, 0);

        // Valid in-range value → stored as-is.
        let mid = (MIN_EXPIRY_SECS + MAX_EXPIRY_SECS) / 2;
        let mut s4 = base_storage(issuer);
        model_propose(&mut s4, issuer, new_issuer, 0, mid).unwrap();
        let pt4 = s4.pending.unwrap();
        assert_eq!(pt4.expiry_secs, mid);
    }

    // ── Test 6: double-cancel rejected ───────────────────────────────────────

    /// A second cancel on the same offering returns `NoTransferPending`.
    #[test]
    fn double_cancel_rejected() {
        let issuer: AddrId = 10;
        let new_issuer: AddrId = 11;
        let mut storage = base_storage(issuer);
        storage.pending = Some(PendingTransfer { new_issuer, timestamp: 100, expiry_secs: 0 });

        let first = model_cancel(&mut storage, issuer);
        assert!(first.is_ok());

        let second = model_cancel(&mut storage, issuer);
        assert_eq!(second, Err(TransferError::NoTransferPending));
        assert_no_orphan_pending(&storage);
    }

    // ── Test 7: unauthorised cancel rejected ─────────────────────────────────

    /// A non-issuer caller cannot cancel a pending transfer.
    #[test]
    fn cancel_by_non_issuer_rejected() {
        let issuer: AddrId = 12;
        let new_issuer: AddrId = 13;
        let attacker: AddrId = 99;
        let mut storage = base_storage(issuer);
        storage.pending = Some(PendingTransfer { new_issuer, timestamp: 200, expiry_secs: 0 });

        let result = model_cancel(&mut storage, attacker);
        assert_eq!(result, Err(TransferError::NotAuthorized));
        // Storage unchanged.
        assert!(storage.pending.is_some());
        assert_eq!(storage.offering.issuer, issuer);
    }

    // ── Test 8: cancel returns exact stored value ─────────────────────────────

    /// The `PendingTransfer` value returned equals what was stored.
    #[test]
    fn cancel_returns_correct_pending_value() {
        let issuer: AddrId = 14;
        let new_issuer: AddrId = 15;
        let expected =
            PendingTransfer { new_issuer, timestamp: 9999, expiry_secs: MAX_EXPIRY_SECS };
        let mut storage = base_storage(issuer);
        storage.pending = Some(expected);

        let result = model_cancel(&mut storage, issuer).unwrap();
        assert_eq!(result, expected);
    }

    // ── Test 9: cancel with no offering returns OfferingNotFound ─────────────

    /// Cancel on a non-existent offering (issuer = 0 sentinel) returns `OfferingNotFound`.
    #[test]
    fn cancel_no_offering_returns_not_found() {
        let mut storage = StorageModel {
            pending: Some(PendingTransfer { new_issuer: 2, timestamp: 0, expiry_secs: 0 }),
            offering: OfferingState { issuer: 0 }, // 0 = no offering
            offering_issuer_lookup: 0,
        };
        let result = model_cancel(&mut storage, 1);
        assert_eq!(result, Err(TransferError::OfferingNotFound));
    }

    // ── Test 10: propose rejected when transfer already pending ──────────────

    /// A second `propose` on an offering with an existing pending transfer must fail.
    #[test]
    fn propose_rejected_when_already_pending() {
        let issuer: AddrId = 16;
        let mut storage = base_storage(issuer);

        model_propose(&mut storage, issuer, 17, 100, 0).unwrap();
        let result = model_propose(&mut storage, issuer, 18, 200, 0);
        assert_eq!(result, Err(TransferError::IssuerTransferPending));
    }

    // ── Test 11: cancel preserves offering_issuer_lookup consistency ──────────

    /// `offering_issuer_lookup` must remain consistent with `offering.issuer`
    /// throughout the propose → cancel lifecycle.
    #[test]
    fn issuer_lookup_consistent_through_lifecycle() {
        let issuer: AddrId = 20;
        let new_issuer: AddrId = 21;
        let mut storage = base_storage(issuer);

        assert_issuer_lookup_consistent(&storage);

        model_propose(&mut storage, issuer, new_issuer, 0, 0).unwrap();
        assert_issuer_lookup_consistent(&storage); // still consistent after propose

        model_cancel(&mut storage, issuer).unwrap();
        assert_issuer_lookup_consistent(&storage); // still consistent after cancel
    }

    // ── Test 12: new_issuer field in pending is unaffected by cancel context ──

    /// The `new_issuer` stored in the pending transfer is whatever the proposer set;
    /// cancel reads it out correctly regardless of who the actual new_issuer is.
    #[test]
    fn cancel_returns_new_issuer_as_stored() {
        let issuer: AddrId = 22;
        let expected_new_issuer: AddrId = 200;
        let mut storage = base_storage(issuer);
        storage.pending = Some(PendingTransfer {
            new_issuer: expected_new_issuer,
            timestamp: 42,
            expiry_secs: DEFAULT_EXPIRY_SECS,
        });

        let pt = model_cancel(&mut storage, issuer).unwrap();
        assert_eq!(pt.new_issuer, expected_new_issuer);
    }
}
