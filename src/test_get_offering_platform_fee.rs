//! Focused adversarial tests for `get_offering_platform_fee` (Issue #1049).
//!
//! `get_offering_platform_fee` is a pure O(1) persistent-storage read of the
//! per-offering [`PlatformFeeModel`] stored under `DataKey2::OfferingPlatformFee`.
//! It performs no auth, no mutation, and no event emission, so the adversarial
//! surface is the *storage-key identity*: the getter must return exactly the
//! model stored under the exact `(issuer, namespace, token)` triple — and
//! nothing else — for every axis that could collide:
//!
//! | # | Scenario | Expected |
//! |---|----------|----------|
//! | 1 | Unknown offering (every component unregistered) | `None` |
//! | 2 | Registered offering, fee never configured | `None` |
//! | 3 | Wrong issuer, right namespace/token | `None` |
//! | 4 | Wrong namespace, right issuer/token | `None` |
//! | 5 | Wrong token, right issuer/namespace | `None` |
//! | 6 | Same (issuer, token), colliding namespace key | `None` |
//! | 7 | Happy path round-trip: exact model is returned | `Some(model)` |
//! | 8 | Multi-offering isolation (6-tuple independence) | only exact key hits |
//! | 9 | Read is non-mutating (storage len unchanged) | unchanged |
//! |10 | Read emits no events | no new events |
//! |11 | Fee update via overwrite is observable through the getter | latest wins |
//! |12 | Rejected setter leaves getter untouched (`FeeExceedsHolderShare`) | `None` |
//! |13 | Rejected setter leaves getter untouched (`OfferingNotFound`) | `None` |
//! |14 | Deterministic repeat reads return equal models | equal |
//! |15 | Boundary fee values (`0` and `10_000` bps) round-trip | exact model |
//! |16 | Auth-failure rejection leaves getter untouched | unchanged |
//!
//! The storage-unchanged assertions after rejected `set_offering_platform_fee`
//! calls close the "state must not move on rejected operations" requirement of
//! the audit item.

#![cfg(test)]

use crate::{PlatformFeeModel, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

// ── Helpers ─────────────────────────────────────────────────────────────────────

struct Ctx {
    env: Env,
    client: RevoraRevenueShareClient<'static>,
    admin: Address,
    issuer: Address,
    ns: Symbol,
    token: Address,
    payout: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("def");
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns,
        &token,
        &2_500,
        &payout,
        &0,
        &symbol_short!(""),
        &0u32,
    );
    Ctx { env, client, admin, issuer, ns, token, payout }
}

/// Persistent storage footprint of the contract (used to prove a rejected or
/// read-only operation did not move state).
fn persistent_len(c: &Ctx) -> usize {
    c.env.storage().persistent().len()
}

/// Count events emitted at or after `start_idx`.
fn events_since(c: &Ctx, start_idx: u32) -> u32 {
    c.env.events().all().len() - start_idx
}

fn model(fee_bps: u32, treasury: &Address) -> PlatformFeeModel {
    PlatformFeeModel { fee_bps, treasury: treasury.clone() }
}

// ── 1–2. Miss paths ─────────────────────────────────────────────────────────────

#[test]
fn getter_returns_none_for_unknown_offering() {
    let c = setup();
    // All three components are random: no offering can match this triple.
    assert_eq!(
        c.client.get_offering_platform_fee(
            &Address::generate(&c.env),
            &c.ns,
            &Address::generate(&c.env)
        ),
        None
    );
}

#[test]
fn getter_returns_none_when_fee_never_configured() {
    let c = setup();
    // Offering exists (registered in setup) but no fee model was ever set.
    assert_eq!(c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token), None);
}

// ── 3–6. Key-identity: every near-miss triple must miss ─────────────────────────

#[test]
fn getter_isolates_issuer() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &500, &treasury);

    let other_issuer = Address::generate(&c.env);
    assert_eq!(c.client.get_offering_platform_fee(&other_issuer, &c.ns, &c.token), None);
    // The exact key still resolves.
    assert_eq!(
        c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token).unwrap().fee_bps,
        500
    );
}

#[test]
fn getter_isolates_namespace() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &500, &treasury);

    // Same offering identity semantics as the setter: namespace is part of the key.
    for other_ns in ["DEF", "de", "defx"] {
        assert_eq!(
            c.client.get_offering_platform_fee(&c.issuer, &Symbol::new(&c.env, other_ns), &c.token),
            None,
            "namespace {other_ns} must not alias the stored key"
        );
    }
}

#[test]
fn getter_isolates_token() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &500, &treasury);

    assert_eq!(
        c.client.get_offering_platform_fee(&c.issuer, &c.ns, &Address::generate(&c.env)),
        None
    );
}

#[test]
fn getter_isolates_namespace_case_collision() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &500, &treasury);

    // Two namespaces differing only by case are distinct storage keys: writes to
    // one must be invisible to reads of the other, in both directions.
    let upper = Symbol::new(&c.env, "DEF");
    c.client.set_offering_platform_fee(&c.issuer, &upper, &c.token, &9_000, &treasury);

    assert_eq!(
        c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token).unwrap().fee_bps,
        500
    );
    assert_eq!(
        c.client.get_offering_platform_fee(&c.issuer, &upper, &c.token).unwrap().fee_bps,
        9_000
    );
}

// ── 7–8. Hits: exact-key round-trip and multi-offering isolation ────────────────

#[test]
fn getter_round_trips_exact_model() {
    let c = setup();
    let treasury = Address::generate(&c.env);

    assert_eq!(c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token), None);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &421, &treasury);

    assert_eq!(
        c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token),
        Some(model(421, &treasury))
    );
}

#[test]
fn getter_distinguishes_all_offerings_independently() {
    let c = setup();
    let t1 = Address::generate(&c.env);
    let t2 = Address::generate(&c.env);
    let ns2 = Symbol::new(&c.env, "abc");
    let token2 = Address::generate(&c.env);
    let issuer2 = Address::generate(&c.env);

    c.client.register_offering(
        &issuer2,
        &Vec::new(&c.env),
        &1u32,
        &ns2,
        &token2,
        &1_000,
        &c.payout,
        &0,
        &symbol_short!(""),
        &0u32,
    );
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &111, &t1);
    c.client.set_offering_platform_fee(&c.issuer, &ns2, &token2, &222, &t2);
    c.client.set_offering_platform_fee(&issuer2, &ns2, &token2, &333, &t1);

    // Every configured 6-tuple resolves to its own model...
    assert_eq!(
        c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token),
        Some(model(111, &t1))
    );
    assert_eq!(c.client.get_offering_platform_fee(&c.issuer, &ns2, &token2), Some(model(222, &t2)));
    assert_eq!(c.client.get_offering_platform_fee(&issuer2, &ns2, &token2), Some(model(333, &t1)));
    // ...and every cross-combination misses.
    assert_eq!(c.client.get_offering_platform_fee(&c.issuer, &c.ns, &token2), None);
    assert_eq!(c.client.get_offering_platform_fee(&c.issuer, &ns2, &c.token), None);
    assert_eq!(c.client.get_offering_platform_fee(&issuer2, &c.ns, &c.token), None);
    assert_eq!(c.client.get_offering_platform_fee(&issuer2, &ns2, &c.token), None);
    assert_eq!(c.client.get_offering_platform_fee(&issuer2, &c.ns, &token2), None);
}

// ── 9–10. Purity: reads do not mutate state or emit events ──────────────────────

#[test]
fn getter_does_not_change_storage_len() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &500, &treasury);

    let len_before = persistent_len(&c);
    for _ in 0..3 {
        let _ = c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token);
    }
    assert_eq!(persistent_len(&c), len_before, "read must not extend persistent storage");
}

#[test]
fn getter_emits_no_events() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &500, &treasury);

    let before = c.env.events().all().len();
    let _ = c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token);
    let _ = c.client.get_offering_platform_fee(&Address::generate(&c.env), &c.ns, &c.token);
    assert_eq!(events_since(&c, before), 0, "read must be event-silent");
}

// ── 11. Overwrite visibility ─────────────────────────────────────────────────────

#[test]
fn getter_reflects_latest_overwrite() {
    let c = setup();
    let t1 = Address::generate(&c.env);
    let t2 = Address::generate(&c.env);

    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &100, &t1);
    assert_eq!(
        c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token),
        Some(model(100, &t1))
    );

    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &10_000, &t2);
    assert_eq!(
        c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token),
        Some(model(10_000, &t2))
    );
}

// ── 12–13. Rejected writes leave the getter untouched ────────────────────────────

#[test]
fn rejected_fee_exceeds_holder_share_leaves_state_unchanged() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    let holder = Address::generate(&c.env);

    // Aggregate holder share = 7_000; any fee above 3_000 exceeds the 10_000 budget.
    c.client.set_holder_share(&c.issuer, &c.ns, &c.token, &holder, &7_000, &1);
    let before = persistent_len(&c);
    let res = c.client.try_set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &3_001, &treasury);
    assert_eq!(res, Err(Ok(RevoraError::FeeExceedsHolderShare)));

    // State did not move: storage footprint and getter result are both unchanged.
    assert_eq!(persistent_len(&c), before);
    assert_eq!(c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token), None);
}

#[test]
fn rejected_unknown_offering_leaves_state_unchanged() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    let unknown_token = Address::generate(&c.env);

    let before = persistent_len(&c);
    let res =
        c.client.try_set_offering_platform_fee(&c.issuer, &c.ns, &unknown_token, &100, &treasury);
    assert_eq!(res, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(persistent_len(&c), before);
    assert_eq!(c.client.get_offering_platform_fee(&c.issuer, &c.ns, &unknown_token), None);
}

// ── 14. Determinism ──────────────────────────────────────────────────────────────

#[test]
fn getter_is_deterministic_across_repeated_reads() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &6_789, &treasury);

    let first = c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token);
    for _ in 0..5 {
        assert_eq!(c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token), first);
    }
}

// ── 15. Boundary fee values ──────────────────────────────────────────────────────

#[test]
fn getter_round_trips_boundary_fees() {
    let c = setup();
    let treasury = Address::generate(&c.env);

    // 0 bps: fee disabled, model still stored and observable.
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &0, &treasury);
    assert_eq!(
        c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token),
        Some(model(0, &treasury))
    );

    // 10_000 bps: 100% platform fee is the upper boundary and is allowed.
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &10_000, &treasury);
    assert_eq!(
        c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token),
        Some(model(10_000, &treasury))
    );
}

// ── 16. Auth-failure rejection leaves the getter untouched ───────────────────────

#[test]
fn auth_failure_leaves_state_unchanged() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    let attacker = Address::generate(&c.env);

    // Seed a valid model first.
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &500, &treasury);
    let before = persistent_len(&c);

    // Unset the admin's auth so the setter's require_auth fails, then replay.
    c.env.set_auths(&[]);
    let res = c.client.try_set_offering_platform_fee(&attacker, &c.ns, &c.token, &9_999, &attacker);
    assert!(res.is_err(), "setter must fail without admin auth");

    // The existing model was neither replaced nor deleted.
    assert_eq!(persistent_len(&c), before);
    assert_eq!(
        c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token),
        Some(model(500, &treasury))
    );
}
