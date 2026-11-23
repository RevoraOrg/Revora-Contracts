//! Adversarial coverage for [`FreezeReason::to_bitmask`] (issue #1025).
//!
//! `to_bitmask` is the single primitive behind every freeze-mask read and
//! write: `emergency_freeze_holder`, `clear_freeze_reason`, `is_holder_frozen`,
//! `get_holder_freeze_reasons`, the OFAC auto-freeze path and the legacy
//! single-reason key migration all derive their bits from it.  A bit that
//! collides with another variant, or a discriminant that shifts outside `u32`,
//! silently corrupts freeze state for a holder.
//!
//! Two layers are covered:
//! 1. Pure bit-matrix invariants — documented table lock-in, single-bit
//!    output, pairwise disjointness, shift-range boundaries, purity.
//! 2. Contract surface — the same primitive reached through storage, where a
//!    wrong value becomes a wrong bit: replayed freezes, mismatched clears,
//!    unauthorized callers and legacy-key migration, each asserting that a
//!    rejected call leaves the mask unchanged.

extern crate alloc;

use super::*;
use crate::test_utils;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Val, Vec as SdkVec,
};

// ── pure bit-matrix helpers ──────────────────────────────────────────────────

/// Every variant the bitmask contract documents, in discriminant order.
const ALL_REASONS: [FreezeReason; 8] = [
    FreezeReason::Compliance,
    FreezeReason::LegalHold,
    FreezeReason::DisputeOpen,
    FreezeReason::SanctionsMatch,
    FreezeReason::Sanctions,
    FreezeReason::CourtOrder,
    FreezeReason::IssuerDispute,
    FreezeReason::Manual,
];

/// Exhaustive view over `FreezeReason`.
///
/// A new variant that is not added here (and to `ALL_REASONS` and the
/// documented table) breaks this match, so new bits cannot ship untested.
fn discriminant_of(reason: FreezeReason) -> u32 {
    match reason {
        FreezeReason::Compliance => 0,
        FreezeReason::LegalHold => 1,
        FreezeReason::DisputeOpen => 2,
        FreezeReason::SanctionsMatch => 3,
        FreezeReason::Sanctions => 4,
        FreezeReason::CourtOrder => 5,
        FreezeReason::IssuerDispute => 6,
        FreezeReason::Manual => 7,
    }
}

// ── pure bit-matrix tests ────────────────────────────────────────────────────

/// Lock-in test for the bitmask table in the `FreezeReason` doc comment.
/// These values are persisted, so a change here is a storage migration.
#[test]
fn to_bitmask_matches_documented_table() {
    assert_eq!(FreezeReason::Compliance.to_bitmask(), 1);
    assert_eq!(FreezeReason::LegalHold.to_bitmask(), 2);
    assert_eq!(FreezeReason::DisputeOpen.to_bitmask(), 4);
    assert_eq!(FreezeReason::SanctionsMatch.to_bitmask(), 8);
    assert_eq!(FreezeReason::Sanctions.to_bitmask(), 16);
    assert_eq!(FreezeReason::CourtOrder.to_bitmask(), 32);
    assert_eq!(FreezeReason::IssuerDispute.to_bitmask(), 64);
    assert_eq!(FreezeReason::Manual.to_bitmask(), 128);
}

/// `to_bitmask` must set exactly one bit — never `0` (which the storage layer
/// reads as "not frozen") and never a multi-bit value (which would freeze
/// reasons the caller never named).
#[test]
fn to_bitmask_sets_exactly_one_bit() {
    for reason in ALL_REASONS {
        let mask = reason.to_bitmask();
        assert_ne!(mask, 0, "{:?} masks to the unfrozen sentinel", reason);
        assert_eq!(mask.count_ones(), 1, "{:?} is not a single bit", reason);
    }
}

/// No two variants may share a bit.  The legacy `Sanctions` / `SanctionsMatch`
/// pair is the historical aliasing hazard this guards against.
#[test]
fn to_bitmask_variants_are_pairwise_disjoint() {
    for (i, a) in ALL_REASONS.iter().enumerate() {
        for b in ALL_REASONS.iter().skip(i + 1) {
            assert_eq!(a.to_bitmask() & b.to_bitmask(), 0, "{:?} collides with {:?}", a, b);
        }
    }
    let union = ALL_REASONS.iter().fold(0u32, |acc, r| acc | r.to_bitmask());
    let sum: u32 = ALL_REASONS.iter().map(|r| r.to_bitmask()).sum();
    assert_eq!(union, 0xFF);
    assert_eq!(union, sum, "OR diverges from SUM: reason bits overlap");
}

/// Boundary test on `self`: `to_bitmask` is `1u32 << (self as u32)`, so the bit
/// index must round-trip to the discriminant.  A reordered enum fails here
/// before persisted masks are silently reinterpreted.
#[test]
fn to_bitmask_bit_index_round_trips_to_discriminant() {
    let mut seen = [0u32; 8];
    for (i, reason) in ALL_REASONS.iter().enumerate() {
        let d = discriminant_of(*reason);
        let mask = reason.to_bitmask();
        assert_eq!(mask, 1u32 << d);
        assert_eq!(mask.trailing_zeros(), d);
        assert_eq!(*reason as u32, d, "enum discriminant drifted from the documented table");
        seen[i] = *reason as u32;
    }
    assert_eq!(seen, [0, 1, 2, 3, 4, 5, 6, 7], "discriminants must be gapless");
}

/// Shift-range boundary: `1u32 << d` panics on shift overflow once `d >= 32`,
/// so every supported discriminant must stay inside `u32` width and the whole
/// set must keep clear of the high half-word the stored `u32` also carries.
#[test]
fn to_bitmask_discriminants_stay_inside_u32_shift_range() {
    let max_discriminant = ALL_REASONS.iter().map(|r| *r as u32).max().unwrap_or(0);
    assert!(
        max_discriminant < u32::BITS,
        "discriminant {} would overflow the u32 shift in to_bitmask",
        max_discriminant
    );
    assert_eq!(max_discriminant, 7);
    for reason in ALL_REASONS {
        assert_eq!(reason.to_bitmask() & 0xFFFF_FF00, 0, "{:?} escaped the low byte", reason);
        assert!(reason.to_bitmask() < (1u32 << 31));
    }
    // The last bit a 32-variant enum could ever occupy is still representable
    // — this is what the "do not exceed 32 variants" note in the enum guards.
    assert_eq!(1u32 << 31, u32::MAX / 2 + 1);
}

/// `to_bitmask` takes `self` by value on a `Copy` type: it must be pure and
/// deterministic, must not consume its input, and each mask must identify
/// exactly one variant.
#[test]
fn to_bitmask_is_pure_and_repeatable() {
    for reason in ALL_REASONS {
        let first = reason.to_bitmask();
        let second = reason.to_bitmask();
        let copied = reason;
        assert_eq!(first, second);
        assert_eq!(first, copied.to_bitmask());
        assert_eq!(reason, copied, "to_bitmask must not consume its input");
        let matches = ALL_REASONS.iter().filter(|r| r.to_bitmask() == first).count();
        assert_eq!(matches, 1, "mask {} identifies more than one variant", first);
    }
}

// ── contract-surface fixtures ────────────────────────────────────────────────

struct Ctx {
    env: Env,
    contract_id: Address,
    client: RevoraRevenueShareClient<'static>,
    /// Contract admin (can freeze any holder of any offering).
    admin: Address,
    /// Issuer that owns the two registered offerings; not the admin.
    issuer: Address,
    /// Neither admin nor issuer.
    stranger: Address,
    namespace: Symbol,
    token: Address,
    other_token: Address,
    holder: Address,
}

/// Initialize with an admin that is *not* the offering issuer so that the
/// issuer-only and admin-only authorization branches stay distinguishable.
fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let namespace = symbol_short!("def");
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let other_token = Address::generate(&env);
    // `register_offering` cross-checks `display_decimals` against the payout
    // token's on-chain `decimals()`, so the payout asset must be a real token
    // contract and the decimals must be read rather than assumed.
    let payout = test_utils::create_token(&env, &admin);
    let payout_decimals = soroban_sdk::token::Client::new(&env, &payout).decimals();
    for share_token in [token.clone(), other_token.clone()] {
        client.register_offering(
            &issuer,
            &SdkVec::new(&env),
            &1u32,
            &namespace,
            &share_token,
            &1_000u32,
            &payout,
            &0i128,
            &symbol_short!("USD"),
            &payout_decimals,
        );
    }

    let stranger = Address::generate(&env);
    let holder = Address::generate(&env);

    Ctx { env, contract_id, client, admin, issuer, stranger, namespace, token, other_token, holder }
}

/// Result shape emitted by generated `try_*` client methods for a contract fn
/// declared as `Result<(), RevoraError>`: the outer `Ok` arm holds the decoded
/// success value, the outer `Err` arm the decoded contract error.
type TryVoid =
    Result<Result<(), soroban_sdk::ConversionError>, Result<RevoraError, soroban_sdk::InvokeError>>;

/// Unwrap the `Result<RevoraError, InvokeError>` reported by generated `try_*`
/// client methods so tests assert exact contract error codes instead of
/// "something failed".
fn contract_error(err: Result<RevoraError, soroban_sdk::InvokeError>) -> RevoraError {
    match err {
        Ok(e) => e,
        Err(host) => panic!("host failure instead of a contract error: {:?}", host),
    }
}

/// Collapse a `TryVoid` into `Result<(), RevoraError>`.
fn flatten(res: TryVoid) -> Result<(), RevoraError> {
    match res {
        Ok(ok) => {
            ok.expect("contract returned an undecodable success value");
            Ok(())
        }
        Err(err) => Err(contract_error(err)),
    }
}

impl Ctx {
    fn freeze(&self, caller: &Address, reason: FreezeReason) -> Result<(), RevoraError> {
        self.freeze_on(caller, &self.token, &self.holder, reason)
    }

    fn freeze_on(
        &self,
        caller: &Address,
        token: &Address,
        holder: &Address,
        reason: FreezeReason,
    ) -> Result<(), RevoraError> {
        flatten(self.client.try_emergency_freeze_holder(
            caller,
            &self.issuer,
            &self.namespace,
            token,
            holder,
            &reason,
        ))
    }

    fn clear(&self, caller: &Address, reason: FreezeReason) -> Result<(), RevoraError> {
        self.clear_on(caller, &self.holder, reason)
    }

    fn clear_on(
        &self,
        caller: &Address,
        holder: &Address,
        reason: FreezeReason,
    ) -> Result<(), RevoraError> {
        flatten(self.client.try_clear_freeze_reason(
            caller,
            &self.issuer,
            &self.namespace,
            &self.token,
            holder,
            &reason,
        ))
    }

    fn mask(&self, token: &Address, holder: &Address) -> u32 {
        self.client.get_holder_freeze_reasons(&self.issuer, &self.namespace, token, holder)
    }

    fn frozen(&self, token: &Address, holder: &Address) -> bool {
        self.client.is_holder_frozen(&self.issuer, &self.namespace, token, holder)
    }

    /// Reach into contract storage directly (legacy-key fixtures).
    fn with_storage<T>(&self, f: impl FnOnce(&Env) -> T) -> T {
        let env = self.env.clone();
        let contract_id = self.contract_id.clone();
        let mut out = None;
        env.as_contract(&contract_id, || {
            out = Some(f(&env));
        });
        out.expect("storage closure must run")
    }
}

/// Count published events whose first topic equals `topic`.
fn count_topic(env: &Env, topic: Symbol) -> u32 {
    let all = env.events().all();
    let mut hits = 0u32;
    for i in 0..all.len() {
        let (_contract, topics, _data): (Address, SdkVec<Val>, Val) = all.get(i).unwrap();
        let first: Symbol = topics.get(0).unwrap().into_val(env);
        if first == topic {
            hits += 1;
        }
    }
    hits
}

// ── contract-surface tests ───────────────────────────────────────────────────

/// A replayed freeze for the same reason must not add a second copy of the bit.
#[test]
fn freezing_the_same_reason_twice_keeps_one_bit() {
    let ctx = setup();
    ctx.freeze(&ctx.issuer, FreezeReason::CourtOrder).unwrap();
    let after_first = ctx.mask(&ctx.token, &ctx.holder);
    assert_eq!(after_first, FreezeReason::CourtOrder.to_bitmask());

    ctx.freeze(&ctx.issuer, FreezeReason::CourtOrder).unwrap();

    let mask = ctx.mask(&ctx.token, &ctx.holder);
    assert_eq!(mask, after_first);
    assert_eq!(mask.count_ones(), 1, "replay doubled the reason bit");
    // Idempotent no-op: no duplicate audit event.
    assert_eq!(count_topic(&ctx.env, EVENT_FRZ_SET), 1);
}

/// Every variant must round-trip through storage as exactly its own bit, and
/// only that same reason may clear it.
#[test]
fn every_reason_round_trips_through_storage_as_its_bit() {
    let ctx = setup();
    let mut union = 0u32;
    for reason in ALL_REASONS {
        let holder = Address::generate(&ctx.env);
        ctx.freeze_on(&ctx.issuer, &ctx.token, &holder, reason).unwrap();
        assert_eq!(ctx.mask(&ctx.token, &holder), reason.to_bitmask());
        assert!(ctx.frozen(&ctx.token, &holder));

        // A different reason cannot clear it, then the right one can.
        let wrong =
            ALL_REASONS.iter().find(|r| r.to_bitmask() != reason.to_bitmask()).copied().unwrap();
        assert_eq!(
            ctx.clear_on(&ctx.issuer, &holder, wrong).unwrap_err(),
            RevoraError::FreezeReasonMismatch
        );
        assert_eq!(ctx.mask(&ctx.token, &holder), reason.to_bitmask());

        ctx.clear_on(&ctx.issuer, &holder, reason).unwrap();
        assert_eq!(ctx.mask(&ctx.token, &holder), 0);
        assert!(!ctx.frozen(&ctx.token, &holder));
        union |= reason.to_bitmask();
    }
    assert_eq!(union, 0xFF);
}

/// Distinct reasons compose into the union of their bits, which — because the
/// bits are disjoint — is also their arithmetic sum.
#[test]
fn multiple_reasons_compose_into_disjoint_union() {
    let ctx = setup();
    let reasons = [
        FreezeReason::Compliance,
        FreezeReason::SanctionsMatch,
        FreezeReason::CourtOrder,
        FreezeReason::Manual,
    ];
    for reason in reasons {
        ctx.freeze(&ctx.issuer, reason).unwrap();
    }

    let mask = ctx.mask(&ctx.token, &ctx.holder);
    let expected_or = reasons.iter().fold(0u32, |acc, r| acc | r.to_bitmask());
    let expected_sum: u32 = reasons.iter().map(|r| r.to_bitmask()).sum();
    assert_eq!(mask, expected_or);
    assert_eq!(expected_or, expected_sum, "reason bits overlap");
    assert_eq!(mask.count_ones(), reasons.len() as u32);
    assert_eq!(mask, 1 | 8 | 32 | 128);
    assert!(ctx.frozen(&ctx.token, &ctx.holder));
}

/// Clearing a reason whose bit is not set must fail with
/// `FreezeReasonMismatch` and leave every other bit intact.
#[test]
fn clearing_an_absent_reason_is_rejected_and_state_unchanged() {
    let ctx = setup();
    ctx.freeze(&ctx.issuer, FreezeReason::CourtOrder).unwrap();
    ctx.freeze(&ctx.issuer, FreezeReason::Manual).unwrap();
    let before = ctx.mask(&ctx.token, &ctx.holder);

    assert_eq!(
        ctx.clear(&ctx.issuer, FreezeReason::SanctionsMatch).unwrap_err(),
        RevoraError::FreezeReasonMismatch
    );

    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), before);
    assert_eq!(before & FreezeReason::SanctionsMatch.to_bitmask(), 0);
    assert!(ctx.frozen(&ctx.token, &ctx.holder));
    // Rejected calls must emit no unfreeze event.
    assert_eq!(count_topic(&ctx.env, EVENT_FRZ_CLR), 0);
    assert_eq!(count_topic(&ctx.env, EVENT_FREEZE_REASON_CLEARED), 0);
}

/// Clearing a reason twice fails the second time, and a partially cleared mask
/// equals the bit of only the surviving reason.
#[test]
fn partial_clear_leaves_exactly_the_remaining_bits() {
    let ctx = setup();
    ctx.freeze(&ctx.issuer, FreezeReason::CourtOrder).unwrap();
    ctx.freeze(&ctx.issuer, FreezeReason::Manual).unwrap();

    ctx.clear(&ctx.issuer, FreezeReason::CourtOrder).unwrap();

    let mask = ctx.mask(&ctx.token, &ctx.holder);
    assert_eq!(mask, FreezeReason::Manual.to_bitmask());
    assert_eq!(mask & FreezeReason::CourtOrder.to_bitmask(), 0);
    assert_eq!(mask.count_ones(), 1);
    assert_eq!(mask.trailing_zeros(), FreezeReason::Manual as u32);
    assert!(ctx.frozen(&ctx.token, &ctx.holder));
    assert_eq!(count_topic(&ctx.env, EVENT_FREEZE_REASON_CLEARED), 1);

    assert_eq!(
        ctx.clear(&ctx.issuer, FreezeReason::CourtOrder).unwrap_err(),
        RevoraError::FreezeReasonMismatch
    );
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), mask);
}

/// Clearing the last bit must return to the `0` "not frozen" sentinel, and a
/// further clear must be rejected rather than wrap the mask negative.
#[test]
fn clearing_last_reason_returns_zero_sentinel_and_rejects_repeats() {
    let ctx = setup();
    ctx.freeze(&ctx.issuer, FreezeReason::Manual).unwrap();
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), 128);

    ctx.clear(&ctx.issuer, FreezeReason::Manual).unwrap();
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), 0);
    assert!(!ctx.frozen(&ctx.token, &ctx.holder));
    assert_eq!(count_topic(&ctx.env, EVENT_FRZ_CLR), 1);

    assert_eq!(
        ctx.clear(&ctx.issuer, FreezeReason::Manual).unwrap_err(),
        RevoraError::HolderFrozen,
        "clearing an unfrozen holder must not wrap the mask"
    );
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), 0);
}

/// A caller who is neither the offering's issuer nor the admin must be rejected
/// with `NotAuthorized` before any mask write — on both mutating paths.
#[test]
fn unauthorized_caller_cannot_write_the_mask() {
    let ctx = setup();

    assert_eq!(
        ctx.freeze(&ctx.stranger, FreezeReason::CourtOrder).unwrap_err(),
        RevoraError::NotAuthorized
    );
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), 0);
    assert!(!ctx.frozen(&ctx.token, &ctx.holder));
    assert_eq!(count_topic(&ctx.env, EVENT_FRZ_SET), 0);

    // Same for the clear path once a legitimate freeze exists.
    ctx.freeze(&ctx.issuer, FreezeReason::CourtOrder).unwrap();
    let before = ctx.mask(&ctx.token, &ctx.holder);
    assert_eq!(
        ctx.clear(&ctx.stranger, FreezeReason::CourtOrder).unwrap_err(),
        RevoraError::NotAuthorized
    );
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), before);
    assert!(ctx.frozen(&ctx.token, &ctx.holder));
    assert_eq!(count_topic(&ctx.env, EVENT_FRZ_CLR), 0);
}

/// Both accepted authorization paths write the identical bit: the mask is a
/// function of the reason only, never of who wrote it.
#[test]
fn issuer_and_admin_write_identical_bits() {
    let ctx = setup();
    let holder_a = Address::generate(&ctx.env);
    let holder_b = Address::generate(&ctx.env);

    ctx.freeze_on(&ctx.issuer, &ctx.token, &holder_a, FreezeReason::DisputeOpen).unwrap();
    ctx.freeze_on(&ctx.admin, &ctx.token, &holder_b, FreezeReason::DisputeOpen).unwrap();

    let a = ctx.mask(&ctx.token, &holder_a);
    let b = ctx.mask(&ctx.token, &holder_b);
    assert_eq!(a, b);
    assert_eq!(a, FreezeReason::DisputeOpen.to_bitmask());

    // The admin may clear a freeze the issuer wrote, but only with the
    // matching bit.
    assert_eq!(
        ctx.clear_on(&ctx.admin, &holder_a, FreezeReason::Manual).unwrap_err(),
        RevoraError::FreezeReasonMismatch
    );
    ctx.clear_on(&ctx.admin, &holder_a, FreezeReason::DisputeOpen).unwrap();
    assert_eq!(ctx.mask(&ctx.token, &holder_a), 0);
    assert_eq!(ctx.mask(&ctx.token, &holder_b), FreezeReason::DisputeOpen.to_bitmask());
}

/// `OfferingNotFound` is raised before any mask write, so freezing against an
/// unregistered token cannot create a phantom bit.
#[test]
fn freeze_on_unknown_offering_is_rejected_and_writes_nothing() {
    let ctx = setup();
    let ghost_token = Address::generate(&ctx.env);

    assert_eq!(
        ctx.freeze_on(&ctx.issuer, &ghost_token, &ctx.holder, FreezeReason::CourtOrder)
            .unwrap_err(),
        RevoraError::OfferingNotFound
    );
    assert_eq!(ctx.mask(&ghost_token, &ctx.holder), 0);
    assert!(!ctx.frozen(&ghost_token, &ctx.holder));
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), 0);
    assert_eq!(count_topic(&ctx.env, EVENT_FRZ_SET), 0);

    assert_eq!(
        ctx.client
            .try_clear_freeze_reason(
                &ctx.issuer,
                &ctx.issuer,
                &ctx.namespace,
                &ghost_token,
                &ctx.holder,
                &FreezeReason::CourtOrder,
            )
            .unwrap_err()
            .unwrap(),
        RevoraError::OfferingNotFound
    );
}

/// With the contract globally frozen the reason bits are irrelevant: both
/// mutating calls fail closed and an existing mask survives untouched.
#[test]
fn global_freeze_blocks_mask_writes_and_preserves_existing_mask() {
    let ctx = setup();
    ctx.freeze(&ctx.issuer, FreezeReason::CourtOrder).unwrap();
    let before = ctx.mask(&ctx.token, &ctx.holder);

    ctx.client.try_freeze().expect("global freeze should succeed");

    assert_eq!(
        ctx.freeze(&ctx.issuer, FreezeReason::Manual).unwrap_err(),
        RevoraError::ContractFrozen
    );
    assert_eq!(
        ctx.clear(&ctx.issuer, FreezeReason::CourtOrder).unwrap_err(),
        RevoraError::ContractFrozen
    );
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), before);
    assert_eq!(before, FreezeReason::CourtOrder.to_bitmask());
    assert!(ctx.frozen(&ctx.token, &ctx.holder));
}

/// Masks are per-holder: a bit written for one holder must not leak to another.
#[test]
fn mask_is_isolated_per_holder() {
    let ctx = setup();
    let other_holder = Address::generate(&ctx.env);

    ctx.freeze(&ctx.issuer, FreezeReason::LegalHold).unwrap();
    assert!(!ctx.frozen(&ctx.token, &other_holder));
    assert_eq!(ctx.mask(&ctx.token, &other_holder), 0);

    ctx.freeze_on(&ctx.issuer, &ctx.token, &other_holder, FreezeReason::Sanctions).unwrap();

    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), FreezeReason::LegalHold.to_bitmask());
    assert_eq!(ctx.mask(&ctx.token, &other_holder), FreezeReason::Sanctions.to_bitmask());
    // Legacy `Sanctions` (bit 4) must never be read as `SanctionsMatch` (bit 3).
    assert_eq!(ctx.mask(&ctx.token, &other_holder) & FreezeReason::SanctionsMatch.to_bitmask(), 0);

    // Clearing the other holder's reason leaves this holder's bit set.
    ctx.clear_on(&ctx.issuer, &other_holder, FreezeReason::Sanctions).unwrap();
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), FreezeReason::LegalHold.to_bitmask());
    assert!(ctx.frozen(&ctx.token, &ctx.holder));
}

/// The same reason bit on two offerings must stay independently clearable.
#[test]
fn mask_is_isolated_per_offering() {
    let ctx = setup();
    let reason = FreezeReason::SanctionsMatch;

    ctx.freeze(&ctx.issuer, reason).unwrap();
    ctx.freeze_on(&ctx.issuer, &ctx.other_token, &ctx.holder, reason).unwrap();
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), reason.to_bitmask());
    assert_eq!(ctx.mask(&ctx.other_token, &ctx.holder), reason.to_bitmask());

    ctx.clear(&ctx.issuer, reason).unwrap();

    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), 0);
    assert!(!ctx.frozen(&ctx.token, &ctx.holder));
    assert_eq!(ctx.mask(&ctx.other_token, &ctx.holder), reason.to_bitmask());
    assert!(ctx.frozen(&ctx.other_token, &ctx.holder));
}

/// Legacy storage wrote a single `FreezeReason` under `EmergencyFreeze`; the
/// read path converts it through `to_bitmask`, so the mask, the freeze
/// predicate and the clear path must all agree on that one bit.
#[test]
fn legacy_single_reason_key_is_read_through_to_bitmask() {
    let ctx = setup();
    let reason = FreezeReason::IssuerDispute;
    let offering = OfferingId {
        issuer: ctx.issuer.clone(),
        namespace: ctx.namespace.clone(),
        token: ctx.token.clone(),
    };
    let legacy_key = DataKey2::EmergencyFreeze(offering.clone(), ctx.holder.clone());
    let mask_key = DataKey2::HolderFreezeMask(offering, ctx.holder.clone());

    ctx.with_storage(|env| env.storage().persistent().set(&legacy_key, &reason));

    assert!(ctx.frozen(&ctx.token, &ctx.holder));
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), reason.to_bitmask());
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), 1u32 << 6);

    // A mismatched clear is rejected and must leave the legacy key in place.
    assert_eq!(
        ctx.clear(&ctx.issuer, FreezeReason::Compliance).unwrap_err(),
        RevoraError::FreezeReasonMismatch
    );
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), reason.to_bitmask());
    assert!(ctx.with_storage(|env| env.storage().persistent().has(&legacy_key)));
    assert!(!ctx.with_storage(|env| env.storage().persistent().has(&mask_key)));

    // The matching clear resolves through the same bit and drops both keys.
    ctx.clear(&ctx.issuer, reason).unwrap();
    assert_eq!(ctx.mask(&ctx.token, &ctx.holder), 0);
    assert!(!ctx.frozen(&ctx.token, &ctx.holder));
    assert!(!ctx.with_storage(|env| env.storage().persistent().has(&legacy_key)));
    assert!(!ctx.with_storage(|env| env.storage().persistent().has(&mask_key)));
}

/// A legacy single reason plus a new freeze must OR into two bits and delete
/// the legacy key rather than overwrite it.
#[test]
fn legacy_reason_merges_with_new_mask_bits() {
    let ctx = setup();
    let offering = OfferingId {
        issuer: ctx.issuer.clone(),
        namespace: ctx.namespace.clone(),
        token: ctx.token.clone(),
    };
    let legacy_key = DataKey2::EmergencyFreeze(offering, ctx.holder.clone());

    ctx.with_storage(|env| {
        env.storage().persistent().set(&legacy_key, &FreezeReason::LegalHold);
    });

    ctx.freeze(&ctx.issuer, FreezeReason::Compliance).unwrap();

    let mask = ctx.mask(&ctx.token, &ctx.holder);
    assert_eq!(mask, FreezeReason::LegalHold.to_bitmask() | FreezeReason::Compliance.to_bitmask());
    assert_eq!(mask.count_ones(), 2);
    assert_eq!(mask, 3);
    assert!(!ctx.with_storage(|env| env.storage().persistent().has(&legacy_key)));

    ctx.clear(&ctx.issuer, FreezeReason::LegalHold).unwrap();
    let mask_after_clear = ctx.mask(&ctx.token, &ctx.holder);
    assert_eq!(mask_after_clear & FreezeReason::LegalHold.to_bitmask(), 0);
    assert_eq!(mask_after_clear, FreezeReason::Compliance.to_bitmask());
    assert!(ctx.frozen(&ctx.token, &ctx.holder));
}
