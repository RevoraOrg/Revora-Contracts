//! Adversarial coverage for `get_min_revenue_threshold` (issue #1126).
//!
//! `get_min_revenue_threshold` is the read side of `DataKey2::MinRevenueThreshold`.
//! It is keyed by the stable offering identity `(issuer, namespace, token)` and
//! deliberately permissionless: it never calls `require_auth` and returns `0` for
//! unknown or never-configured keys.
//!
//! The tests below pin down the success path, the `i128` boundary values, the
//! cross-key isolation guarantees, and the invariant that every rejected
//! `set_min_revenue_threshold` call leaves both the stored value and the event
//! stream untouched.
//!
//! Exercised matrix:
//! | Case                                     | Expected outcome                             |
//! |------------------------------------------|----------------------------------------------|
//! | read with no mocked authorization        | `0`, no auth panic                           |
//! | never-registered offering                | `0`, no panic                                |
//! | registered but never configured          | `0`                                          |
//! | set then get                             | exact round-trip + `min_rev` event           |
//! | explicit `0`                             | threshold disabled (`0`)                     |
//! | `1` / `i128::MAX`                        | stored and returned verbatim                 |
//! | `-1` / `i128::MIN`                       | `InvalidAmount`, previous value preserved    |
//! | foreign / co-issuer                      | `OfferingNotFound`, no state change          |
//! | unknown offering + negative amount       | `OfferingNotFound` (existence checked first) |
//! | different namespace / token / issuer     | isolated thresholds                          |
//! | rejected write                           | no event, no storage mutation                |
//! | repeated reads / identical writes        | deterministic / idempotent                   |

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Val, Vec as SdkVec,
};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Register the contract plus one offering in namespace `def`.
fn setup() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &SdkVec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token,
        &1_000,
        &payout,
        &0,
        &symbol_short!(""),
        &0,
    );
    (env, contract_id, issuer, token, payout)
}

/// Register an additional offering for `issuer` in `namespace` with `token`.
fn register_extra_offering(
    env: &Env,
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) {
    let payout = Address::generate(env);
    client.register_offering(
        issuer,
        &SdkVec::new(env),
        &1u32,
        namespace,
        token,
        &1_000,
        &payout,
        &0,
        &symbol_short!(""),
        &0,
    );
}

/// First topic of every event emitted since the `start` event-index watermark.
fn event_topics_since(env: &Env, start: u32) -> alloc::vec::Vec<Symbol> {
    let events = env.events().all();
    let mut out = alloc::vec::Vec::new();
    for i in start..events.len() {
        let (_, topics, _) = events.get(i).unwrap();
        let v: SdkVec<Val> = topics.clone().into_val(env);
        let sym: Symbol = v.get(0).unwrap().into_val(env);
        out.push(sym);
    }
    out
}

fn read_threshold(
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) -> i128 {
    client.get_min_revenue_threshold(issuer, namespace, token)
}

// ── success paths ─────────────────────────────────────────────────────────────

/// The getter is read-only and permissionless: it must not require any
/// authorization. This test deliberately never enables authorization mocking, so
/// any `require_auth` on the read path would abort the invocation.
#[test]
fn read_is_permissionless_without_mocked_authorization() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    assert_eq!(client.get_min_revenue_threshold(&issuer, &symbol_short!("ns"), &token), 0);
}

/// Never-registered offering: the getter answers `0` instead of erroring out.
#[test]
fn never_registered_offering_reads_zero() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    assert_eq!(
        read_threshold(&client, &issuer, &symbol_short!("ghost"), &token),
        0,
        "unknown namespace must read 0"
    );
    assert_eq!(
        read_threshold(&client, &Address::generate(&env), &symbol_short!("def"), &token),
        0,
        "unknown issuer must read 0"
    );
}

/// Registered but never configured offering: default is `0` ("no threshold").
#[test]
fn registered_but_unset_offering_reads_zero() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    assert_eq!(read_threshold(&client, &issuer, &symbol_short!("def"), &token), 0);
}

/// Happy path: a stored threshold round-trips verbatim and emits `min_rev`.
#[test]
fn set_then_get_round_trips_and_emits_min_rev_event() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    let before = env.events().all().len();
    client.set_min_revenue_threshold(&issuer, &ns, &token, &2_500);

    assert!(
        event_topics_since(&env, before).contains(&symbol_short!("min_rev")),
        "successful set must emit min_rev"
    );
    assert_eq!(read_threshold(&client, &issuer, &ns, &token), 2_500);
}

/// Explicit `0` disables a previously configured threshold.
#[test]
fn explicit_zero_disables_a_configured_threshold() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_min_revenue_threshold(&issuer, &ns, &token, &4_000);
    assert_eq!(read_threshold(&client, &issuer, &ns, &token), 4_000);

    client.set_min_revenue_threshold(&issuer, &ns, &token, &0);
    assert_eq!(read_threshold(&client, &issuer, &ns, &token), 0);
}

/// `1` is the smallest strictly-positive threshold (`0` is the disable sentinel).
#[test]
fn one_is_the_smallest_positive_threshold() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_min_revenue_threshold(&issuer, &ns, &token, &1);
    assert_eq!(read_threshold(&client, &issuer, &ns, &token), 1);
}

/// `i128::MAX` must survive the round-trip unchanged: the getter performs no
/// saturating or clamping arithmetic.
#[test]
fn i128_max_round_trips_verbatim() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_min_revenue_threshold(&issuer, &ns, &token, &i128::MAX);
    assert_eq!(read_threshold(&client, &issuer, &ns, &token), i128::MAX);
}

// ── failure paths and state preservation ──────────────────────────────────────

/// `-1` is rejected with `InvalidAmount`; the previous value and the event
/// stream are both untouched.
#[test]
fn negative_one_is_rejected_and_previous_value_is_preserved() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    client.set_min_revenue_threshold(&issuer, &ns, &token, &777);

    let before = env.events().all().len();
    let res = client.try_set_min_revenue_threshold(&issuer, &ns, &token, &-1);

    assert_eq!(res, Err(Ok(RevoraError::InvalidAmount)));
    assert_eq!(env.events().all().len(), before, "rejected write must not emit events");
    assert_eq!(read_threshold(&client, &issuer, &ns, &token), 777);
}

/// `i128::MIN` is rejected with `InvalidAmount` (no negation-overflow panic) and
/// the previous value is preserved.
#[test]
fn i128_min_is_rejected_and_previous_value_is_preserved() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    client.set_min_revenue_threshold(&issuer, &ns, &token, &1_000);

    let res = client.try_set_min_revenue_threshold(&issuer, &ns, &token, &i128::MIN);

    assert_eq!(res, Err(Ok(RevoraError::InvalidAmount)));
    assert_eq!(read_threshold(&client, &issuer, &ns, &token), 1_000);
}

/// A rejected write on a never-configured offering must not create state: the
/// getter keeps reporting the `0` default.
#[test]
fn rejected_negative_on_unset_offering_keeps_the_zero_default() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    let res = client.try_set_min_revenue_threshold(&issuer, &ns, &token, &-1);

    assert_eq!(res, Err(Ok(RevoraError::InvalidAmount)));
    assert_eq!(read_threshold(&client, &issuer, &ns, &token), 0);
}

/// A stranger cannot write: the offering is resolved by
/// `(issuer, namespace, token)`, so a foreign address sees `OfferingNotFound`.
#[test]
fn foreign_issuer_cannot_set_threshold() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let stranger = Address::generate(&env);
    client.set_min_revenue_threshold(&issuer, &ns, &token, &4_321);

    let res = client.try_set_min_revenue_threshold(&stranger, &ns, &token, &5_000);

    assert_eq!(res, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(read_threshold(&client, &issuer, &ns, &token), 4_321, "owner value intact");
    assert_eq!(read_threshold(&client, &stranger, &ns, &token), 0, "stranger key stays unset");
}

/// A co-issuer is authorized on the offering but is not its owner: acting as
/// `issuer` resolves to a different offering identity and is rejected without
/// touching the primary issuer's threshold.
#[test]
fn co_issuer_cannot_set_threshold_for_primary_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let primary = Address::generate(&env);
    let co = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    let ns = symbol_short!("def");
    client.initialize(&primary, &None::<Address>, &None::<bool>);

    let mut co_issuers = SdkVec::new(&env);
    co_issuers.push_back(co.clone());
    client.register_offering(
        &primary,
        &co_issuers,
        &2u32,
        &ns,
        &token,
        &1_000,
        &payout,
        &0,
        &symbol_short!(""),
        &0,
    );

    let res = client.try_set_min_revenue_threshold(&co, &ns, &token, &5_000);

    assert_eq!(res, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(read_threshold(&client, &primary, &ns, &token), 0, "primary value untouched");
}

/// Offering existence is validated before the amount matrix, so an unknown
/// offering reports `OfferingNotFound` even for a negative amount.
#[test]
fn unknown_offering_takes_precedence_over_amount_validation() {
    let (env, contract_id, issuer, _token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let ghost_token = Address::generate(&env);

    let res = client.try_set_min_revenue_threshold(&issuer, &ns, &ghost_token, &-1);

    assert_eq!(res, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(read_threshold(&client, &issuer, &ns, &ghost_token), 0);
}

/// A rejected write must not emit a spurious `min_rev` event.
#[test]
fn rejected_write_emits_no_min_rev_event() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    client.set_min_revenue_threshold(&issuer, &ns, &token, &100);

    let before = env.events().all().len();
    assert_eq!(
        client.try_set_min_revenue_threshold(&issuer, &ns, &token, &-5),
        Err(Ok(RevoraError::InvalidAmount))
    );

    let topics = event_topics_since(&env, before);
    assert!(topics.is_empty(), "rejected set must emit nothing, got {topics:?}");
}

// ── key isolation ─────────────────────────────────────────────────────────────

/// Thresholds are scoped by namespace: two namespaces of the same issuer never
/// leak into each other.
#[test]
fn namespace_scoping_isolates_thresholds() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns_a = symbol_short!("aaa");
    let ns_b = symbol_short!("bbb");
    register_extra_offering(&env, &client, &issuer, &ns_a, &token);
    register_extra_offering(&env, &client, &issuer, &ns_b, &token);

    client.set_min_revenue_threshold(&issuer, &ns_a, &token, &111);

    assert_eq!(read_threshold(&client, &issuer, &ns_a, &token), 111);
    assert_eq!(read_threshold(&client, &issuer, &ns_b, &token), 0);
    assert_eq!(read_threshold(&client, &issuer, &symbol_short!("def"), &token), 0);

    client.set_min_revenue_threshold(&issuer, &ns_b, &token, &222);
    assert_eq!(read_threshold(&client, &issuer, &ns_a, &token), 111);
    assert_eq!(read_threshold(&client, &issuer, &ns_b, &token), 222);
}

/// Thresholds are scoped by offering token within the same namespace.
#[test]
fn token_scoping_isolates_thresholds() {
    let (env, contract_id, issuer, token_a, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let token_b = Address::generate(&env);
    register_extra_offering(&env, &client, &issuer, &ns, &token_b);

    client.set_min_revenue_threshold(&issuer, &ns, &token_a, &333);

    assert_eq!(read_threshold(&client, &issuer, &ns, &token_a), 333);
    assert_eq!(read_threshold(&client, &issuer, &ns, &token_b), 0);

    client.set_min_revenue_threshold(&issuer, &ns, &token_b, &444);
    assert_eq!(read_threshold(&client, &issuer, &ns, &token_a), 333);
    assert_eq!(read_threshold(&client, &issuer, &ns, &token_b), 444);
}

/// Thresholds are scoped by issuer even when namespace and token collide.
#[test]
fn issuer_scoping_isolates_thresholds() {
    let (env, contract_id, issuer_a, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let issuer_b = Address::generate(&env);
    register_extra_offering(&env, &client, &issuer_b, &ns, &token);

    client.set_min_revenue_threshold(&issuer_a, &ns, &token, &555);

    assert_eq!(read_threshold(&client, &issuer_a, &ns, &token), 555);
    assert_eq!(read_threshold(&client, &issuer_b, &ns, &token), 0);

    client.set_min_revenue_threshold(&issuer_b, &ns, &token, &666);
    assert_eq!(read_threshold(&client, &issuer_a, &ns, &token), 555);
    assert_eq!(read_threshold(&client, &issuer_b, &ns, &token), 666);
}

// ── determinism ───────────────────────────────────────────────────────────────

/// Repeated reads of the same key are deterministic.
#[test]
fn repeated_reads_are_deterministic() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    client.set_min_revenue_threshold(&issuer, &ns, &token, &9_999);

    let a = read_threshold(&client, &issuer, &ns, &token);
    let b = read_threshold(&client, &issuer, &ns, &token);
    let c = read_threshold(&client, &issuer, &ns, &token);

    assert_eq!((a, b, c), (9_999, 9_999, 9_999));
}

/// Writing the same value twice is idempotent and observable after each write.
#[test]
fn identical_writes_are_idempotent() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_min_revenue_threshold(&issuer, &ns, &token, &500);
    assert_eq!(read_threshold(&client, &issuer, &ns, &token), 500);
    client.set_min_revenue_threshold(&issuer, &ns, &token, &500);
    assert_eq!(read_threshold(&client, &issuer, &ns, &token), 500);
}

/// Unrelated per-offering configuration (rounding mode, claim delay) must not
/// disturb the stored threshold.
#[test]
fn threshold_is_unaffected_by_unrelated_offering_configuration() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    client.set_min_revenue_threshold(&issuer, &ns, &token, &9_000);

    client.set_rounding_mode(&issuer, &ns, &token, &RoundingMode::RoundHalfUp);
    client.set_claim_delay(&issuer, &ns, &token, &3_600u64);

    assert_eq!(read_threshold(&client, &issuer, &ns, &token), 9_000);
    assert_eq!(client.get_rounding_mode(&issuer, &ns, &token), RoundingMode::RoundHalfUp);
    assert_eq!(client.get_claim_delay(&issuer, &ns, &token), 3_600);
}
