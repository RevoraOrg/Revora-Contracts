//! Adversarial coverage for [`RevoraRevenueShare::set_min_revenue_threshold`]
//! and its companion reader [`RevoraRevenueShare::get_min_revenue_threshold`].
//!
//! The setter is a per-offering, issuer-gated configuration write. It has three
//! independent rejection gates, and every one of them must leave the previously
//! stored threshold (and the event log) untouched:
//!
//! | Gate                                        | Rejection             | State after |
//! |---------------------------------------------|-----------------------|-------------|
//! | contract frozen                             | `ContractFrozen`      | unchanged   |
//! | offering absent / caller is not the primary | `OfferingNotFound`    | unchanged   |
//! | `min_amount < 0`                            | `InvalidAmount`       | unchanged   |
//!
//! Test matrix:
//!
//! | Case                                          | Expected outcome                              |
//! |-----------------------------------------------|-----------------------------------------------|
//! | never configured                              | getter returns `0`, no `min_rev` event        |
//! | first set                                     | stored, event `(0, min_amount)`               |
//! | update                                        | stored, event `(previous, min_amount)`        |
//! | reset to `0` (disable)                        | stored `0`, event `(previous, 0)`            |
//! | identical value set twice                     | idempotent, event `(v, v)`                    |
//! | `min_amount == i128::MAX`                     | accepted, stored exactly (no overflow)        |
//! | `min_amount == -1` / `i128::MIN`              | `InvalidAmount`, previous value preserved     |
//! | unregistered offering                         | `OfferingNotFound`, no storage written        |
//! | wrong namespace / wrong token                 | `OfferingNotFound`, sibling config untouched  |
//! | co-issuer or unrelated address               | `OfferingNotFound`, config untouched          |
//! | quorum `> 1` satisfied by co-issuer auth      | accepted                                      |
//! | frozen contract                               | `ContractFrozen`, config untouched            |
//! | sibling namespaces / tokens                   | fully independent thresholds                  |
//!
//! Layer-1 host auth (`require_auth`) failures surface as a host panic rather
//! than a `RevoraError`; that layer is documented in `src/test_auth.rs`. This
//! module therefore drives the catchable, contract-level rejection paths.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Val, Vec as SdkVec,
};

const MIN_REV: Symbol = symbol_short!("min_rev");
const REV_BELOW: Symbol = symbol_short!("rev_below");

// ── helpers ──────────────────────────────────────────────────────────────────

/// Initialized contract with a single offering registered in the `def`
/// namespace for `token` by `issuer` (who is also the admin).
fn setup() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    let payout = register(&env, &client, &issuer, &symbol_short!("def"), &token);
    (env, contract_id, issuer, token, payout)
}

/// Initialized contract with **no** offering registered.
fn setup_without_offering() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    (env, contract_id, issuer)
}

/// Register a single-issuer offering and return its payout asset.
fn register(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) -> Address {
    let payout = Address::generate(env);
    client.register_offering(
        issuer,
        &SdkVec::new(env),
        &1u32,
        namespace,
        token,
        &1_000u32,
        &payout,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
    payout
}

/// Every `min_rev` event emitted at or after `start`, decoded as
/// `(previous, current)`. The V2 wrapper prefixes the payload with the schema
/// version, so the raw log entry is `(2u32, (i128, i128))`.
fn min_rev_payloads(env: &Env, start: u32) -> SdkVec<(i128, i128)> {
    let all = env.events().all();
    let mut out = SdkVec::new(env);
    for i in start..all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        if topics.is_empty() {
            continue;
        }
        let topic: Symbol = topics.get(0).unwrap().into_val(env);
        if topic != MIN_REV {
            continue;
        }
        let outer: SdkVec<Val> = data.into_val(env);
        let version: u32 = outer.get(0).unwrap().into_val(env);
        assert_eq!(version, 2u32, "min_rev must carry schema version 2");
        let inner: SdkVec<Val> = outer.get(1).unwrap().into_val(env);
        let previous: i128 = inner.get(0).unwrap().into_val(env);
        let current: i128 = inner.get(1).unwrap().into_val(env);
        out.push_back((previous, current));
    }
    out
}

/// The `min_rev` topic tuple must be `(min_rev, issuer, namespace, token)` so
/// indexers can filter per offering.
fn assert_min_rev_topics(
    env: &Env,
    start: u32,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) {
    let all = env.events().all();
    let mut checked = 0u32;
    for i in start..all.len() {
        let (_, topics, _) = all.get(i).unwrap();
        if topics.is_empty() {
            continue;
        }
        let topic: Symbol = topics.get(0).unwrap().into_val(env);
        if topic != MIN_REV {
            continue;
        }
        assert_eq!(topics.len(), 4, "min_rev must carry 4 topics");
        let t_issuer: Address = topics.get(1).unwrap().into_val(env);
        let t_namespace: Symbol = topics.get(2).unwrap().into_val(env);
        let t_token: Address = topics.get(3).unwrap().into_val(env);
        assert_eq!(&t_issuer, issuer);
        assert_eq!(&t_namespace, namespace);
        assert_eq!(&t_token, token);
        checked += 1;
    }
    assert!(checked > 0, "no min_rev event found in the inspected window");
}

/// Symbols of every event topic emitted at or after `start`.
fn topics_since(env: &Env, start: u32) -> SdkVec<Symbol> {
    let all = env.events().all();
    let mut out = SdkVec::new(env);
    for i in start..all.len() {
        let (_, topics, _) = all.get(i).unwrap();
        if topics.is_empty() {
            continue;
        }
        let topic: Symbol = topics.get(0).unwrap().into_val(env);
        out.push_back(topic);
    }
    out
}

// ── 1. default state ─────────────────────────────────────────────────────────

#[test]
fn threshold_defaults_to_zero_and_emits_nothing() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 0);
    assert_eq!(min_rev_payloads(&env, 0).len(), 0, "reads must not emit min_rev");
}

// ── 2. successful writes ─────────────────────────────────────────────────────

#[test]
fn first_set_persists_threshold_and_reports_previous_zero() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    let start = env.events().all().len();
    client.set_min_revenue_threshold(&issuer, &ns, &token, &5_000i128);

    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 5_000);
    let payloads = min_rev_payloads(&env, start);
    assert_eq!(payloads.len(), 1, "exactly one min_rev event per set");
    assert_eq!(payloads.get(0).unwrap(), (0, 5_000));
    assert_min_rev_topics(&env, start, &issuer, &ns, &token);
}

#[test]
fn update_reports_previous_and_new_threshold() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_min_revenue_threshold(&issuer, &ns, &token, &1_000i128);
    let start = env.events().all().len();
    client.set_min_revenue_threshold(&issuer, &ns, &token, &2_500i128);

    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 2_500);
    let payloads = min_rev_payloads(&env, start);
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads.get(0).unwrap(), (1_000, 2_500));
}

#[test]
fn reset_to_zero_disables_the_threshold() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_min_revenue_threshold(&issuer, &ns, &token, &1_000i128);
    let start = env.events().all().len();
    client.set_min_revenue_threshold(&issuer, &ns, &token, &0i128);

    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 0);
    let payloads = min_rev_payloads(&env, start);
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads.get(0).unwrap(), (1_000, 0), "disabling must report the previous value");

    // A later write must observe 0 as the previous value, i.e. the stored
    // threshold is overwritten rather than accumulated.
    let start = env.events().all().len();
    client.set_min_revenue_threshold(&issuer, &ns, &token, &42i128);
    assert_eq!(min_rev_payloads(&env, start).get(0).unwrap(), (0, 42));
}

#[test]
fn repeated_identical_set_is_idempotent_but_still_observable() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_min_revenue_threshold(&issuer, &ns, &token, &5_000i128);
    let start = env.events().all().len();
    client.set_min_revenue_threshold(&issuer, &ns, &token, &5_000i128);

    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 5_000);
    let payloads = min_rev_payloads(&env, start);
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads.get(0).unwrap(), (5_000, 5_000), "unchanged writes are still announced");
}

#[test]
fn accepts_i128_max_threshold_without_overflow() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_min_revenue_threshold(&issuer, &ns, &token, &i128::MAX);
    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), i128::MAX);

    // The maximum must not wedge the setter: it can still be lowered/disabled.
    let start = env.events().all().len();
    client.set_min_revenue_threshold(&issuer, &ns, &token, &0i128);
    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 0);
    assert_eq!(min_rev_payloads(&env, start).get(0).unwrap(), (i128::MAX, 0));
}

#[test]
fn accepts_one_unit_threshold_boundary() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    let start = env.events().all().len();
    client.set_min_revenue_threshold(&issuer, &ns, &token, &1i128);
    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 1);
    assert_eq!(min_rev_payloads(&env, start).get(0).unwrap(), (0, 1));
}

// ── 3. negative amount matrix (#163) ─────────────────────────────────────────

#[test]
fn rejects_negative_one_and_preserves_previous_threshold() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_min_revenue_threshold(&issuer, &ns, &token, &1_000i128);
    let events_before = env.events().all().len();

    let result = client.try_set_min_revenue_threshold(&issuer, &ns, &token, &-1i128);
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));

    assert_eq!(
        client.get_min_revenue_threshold(&issuer, &ns, &token),
        1_000,
        "a rejected write must not overwrite the stored threshold"
    );
    assert_eq!(min_rev_payloads(&env, events_before).len(), 0, "rejected writes must not emit");
    assert_eq!(
        env.events().all().len(),
        events_before,
        "rejected writes must emit no event at all"
    );
}

#[test]
fn rejects_i128_min_and_leaves_state_untouched() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    let result = client.try_set_min_revenue_threshold(&issuer, &ns, &token, &i128::MIN);
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));

    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 0);
    assert_eq!(min_rev_payloads(&env, 0).len(), 0);
}

// ── 4. offering resolution ───────────────────────────────────────────────────

#[test]
fn rejects_unregistered_offering_and_writes_no_threshold() {
    let (env, contract_id, issuer) = setup_without_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let token = Address::generate(&env);

    let result = client.try_set_min_revenue_threshold(&issuer, &ns, &token, &500i128);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 0);
    assert_eq!(min_rev_payloads(&env, 0).len(), 0);
}

#[test]
fn rejects_wrong_namespace_and_keeps_sibling_namespace() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let def = symbol_short!("def");
    let alt = symbol_short!("alt");

    client.set_min_revenue_threshold(&issuer, &def, &token, &777i128);
    let events_before = env.events().all().len();

    let result = client.try_set_min_revenue_threshold(&issuer, &alt, &token, &999i128);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(client.get_min_revenue_threshold(&issuer, &def, &token), 777);
    assert_eq!(
        client.get_min_revenue_threshold(&issuer, &alt, &token),
        0,
        "no cross-namespace write"
    );
    assert_eq!(min_rev_payloads(&env, events_before).len(), 0);
}

#[test]
fn rejects_wrong_token_and_keeps_registered_token() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let foreign_token = Address::generate(&env);

    client.set_min_revenue_threshold(&issuer, &ns, &token, &111i128);
    let events_before = env.events().all().len();

    let result = client.try_set_min_revenue_threshold(&issuer, &ns, &foreign_token, &222i128);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 111);
    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &foreign_token), 0);
    assert_eq!(min_rev_payloads(&env, events_before).len(), 0);
}

// ── 5. authorization ─────────────────────────────────────────────────────────

#[test]
fn unrelated_address_cannot_configure_threshold() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let attacker = Address::generate(&env);

    client.set_min_revenue_threshold(&issuer, &ns, &token, &4_000i128);
    let events_before = env.events().all().len();

    let result = client.try_set_min_revenue_threshold(&attacker, &ns, &token, &9_999i128);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 4_000);
    assert_eq!(client.get_min_revenue_threshold(&attacker, &ns, &token), 0);
    assert_eq!(min_rev_payloads(&env, events_before).len(), 0);
}

#[test]
fn co_issuer_cannot_configure_threshold_of_primary_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let co_issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    let ns = symbol_short!("def");
    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &SdkVec::from_array(&env, [co_issuer.clone()]),
        &2u32,
        &ns,
        &token,
        &1_000u32,
        &payout,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    client.set_min_revenue_threshold(&issuer, &ns, &token, &500i128);
    let events_before = env.events().all().len();

    // A co-issuer is part of the quorum but is not the offering identity, so it
    // cannot configure the threshold for the primary issuer's offering.
    let result = client.try_set_min_revenue_threshold(&co_issuer, &ns, &token, &123i128);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 500);
    assert_eq!(client.get_min_revenue_threshold(&co_issuer, &ns, &token), 0);
    assert_eq!(min_rev_payloads(&env, events_before).len(), 0);
}

#[test]
fn quorum_above_one_is_satisfied_by_issuer_set_authorizations() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let co_issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");
    client.initialize(&issuer, &None::<Address>, &None::<bool>);

    // 2-of-2 offering: the quorum gate counts authorizations of the whole
    // issuer set, not only of the address passed as `issuer`.
    client.register_offering(
        &issuer,
        &SdkVec::from_array(&env, [co_issuer]),
        &2u32,
        &ns,
        &token,
        &1_000u32,
        &Address::generate(&env),
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    let start = env.events().all().len();
    client.set_min_revenue_threshold(&issuer, &ns, &token, &750i128);

    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 750);
    assert_eq!(min_rev_payloads(&env, start).get(0).unwrap(), (0, 750));
}

// ── 6. frozen contract ───────────────────────────────────────────────────────

#[test]
fn frozen_contract_rejects_update_and_preserves_threshold() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_min_revenue_threshold(&issuer, &ns, &token, &2_500i128);
    client.freeze();
    let events_before = env.events().all().len();

    let result = client.try_set_min_revenue_threshold(&issuer, &ns, &token, &9_999i128);
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));

    assert_eq!(
        client.get_min_revenue_threshold(&issuer, &ns, &token),
        2_500,
        "the freeze guard must run before the write"
    );
    assert_eq!(min_rev_payloads(&env, events_before).len(), 0);
}

// ── 7. storage isolation ─────────────────────────────────────────────────────

#[test]
fn thresholds_are_isolated_per_namespace_and_token() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let def = symbol_short!("def");
    let alt = symbol_short!("alt");
    let other_token = Address::generate(&env);

    register(&env, &client, &issuer, &def, &other_token);
    register(&env, &client, &issuer, &alt, &token);

    client.set_min_revenue_threshold(&issuer, &def, &token, &1i128);
    client.set_min_revenue_threshold(&issuer, &def, &other_token, &2i128);
    client.set_min_revenue_threshold(&issuer, &alt, &token, &3i128);

    assert_eq!(client.get_min_revenue_threshold(&issuer, &def, &token), 1);
    assert_eq!(client.get_min_revenue_threshold(&issuer, &def, &other_token), 2);
    assert_eq!(client.get_min_revenue_threshold(&issuer, &alt, &token), 3);

    // A write against an unregistered combination must not touch any sibling.
    let events_before = env.events().all().len();
    let result = client.try_set_min_revenue_threshold(&issuer, &alt, &other_token, &9_999i128);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(client.get_min_revenue_threshold(&issuer, &def, &token), 1);
    assert_eq!(client.get_min_revenue_threshold(&issuer, &def, &other_token), 2);
    assert_eq!(client.get_min_revenue_threshold(&issuer, &alt, &token), 3);
    assert_eq!(min_rev_payloads(&env, events_before).len(), 0);
}

// ── 8. effect on the reporting gate ──────────────────────────────────────────

#[test]
fn max_threshold_suppresses_lower_reports_and_reset_restores_them() {
    let (env, contract_id, issuer, token, payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_min_revenue_threshold(&issuer, &ns, &token, &i128::MAX);

    let start = env.events().all().len();
    client.report_revenue(&issuer, &ns, &token, &payout, &1_000i128, &1u64, &false);
    let topics = topics_since(&env, start);
    assert!(topics.iter().any(|t| t == REV_BELOW), "a report below the threshold must be skipped");
    assert!(client.get_audit_summary(&issuer, &ns, &token).is_none());

    // Disabling the threshold lets the very same report through unchanged.
    client.set_min_revenue_threshold(&issuer, &ns, &token, &0i128);
    let start = env.events().all().len();
    client.report_revenue(&issuer, &ns, &token, &payout, &1_000i128, &1u64, &false);
    let topics = topics_since(&env, start);
    assert!(!topics.iter().any(|t| t == REV_BELOW));
    let summary = client.get_audit_summary(&issuer, &ns, &token).unwrap();
    assert_eq!(summary.total_revenue, 1_000);
    assert_eq!(summary.report_count, 1);
}
