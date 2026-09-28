//! Adversarial coverage for the `get_investment_constraints` read API (#1124).
//!
//! `get_investment_constraints(env, issuer, namespace, token)` is the public
//! view that off-chain orchestrators rely on when they pre-screen an investor
//! against a per-offering stake window.  It is a pure O(1) persistent read of
//! `DataKey2::InvestmentConstraints(OfferingId { issuer, namespace, token })`
//! that performs **no** auth check and **no** state mutation, therefore its
//! adversarial surface is:
//!
//! 1. Keying — a read must only ever surface the bounds stored for the exact
//!    `(issuer, namespace, token)` triple.  Any other triple must resolve to
//!    `None`, and a configured offering must never leak bounds into a
//!    neighbouring namespace, token, issuer, or contract instance.
//! 2. Absence vs. explicit zeros — `None` ("never configured") and
//!    `Some({ min: 0, max: 0 })` ("explicitly unlimited") are distinct
//!    off-chain signals and must not collapse into one another.
//! 3. Numeric fidelity — bounds are `i128` and are stored verbatim, so the
//!    `i128::MAX` edge must round-trip without truncation or saturation.
//! 4. Rejection isolation — a rejected `set_investment_constraints` (negative
//!    bound, inverted range, foreign issuer, missing auth, frozen contract)
//!    must leave the previously stored bounds intact and must not publish a
//!    configuration event.
//!
//! Test matrix (issue #1124):
//! | Case                                       | Expected outcome                    |
//! |--------------------------------------------|-------------------------------------|
//! | offering never registered                  | `None`                              |
//! | offering registered, constraints never set | `None`                              |
//! | configured `min < max`                     | `Some` with exact bounds            |
//! | configured `min == 0 && max == 0`          | `Some { 0, 0 }` (not `None`)        |
//! | configured `max == i128::MAX`              | `Some` with `i128::MAX` verbatim    |
//! | configured, read with other namespace      | `None` (incl. empty & long Symbol)  |
//! | configured, read with other token          | `None`                              |
//! | configured, read with other issuer         | `None` (no cross-tenant leak)       |
//! | read against a second contract instance    | `None`                              |
//! | read with no auth entries available        | succeeds (public view, no auth)     |
//! | repeated reads                             | identical value, no events          |
//! | rejected negative / inverted-range write   | `InvalidAmount`, bounds unchanged   |
//! | rejected foreign-issuer write              | `OfferingNotFound`, bounds unchanged|
//! | rejected write with no auth entries        | error, bounds unchanged, no event   |
//! | rejected write while globally frozen       | `ContractFrozen`, bounds unchanged  |
//! | read while globally frozen                 | still returns `Some`                |

#![cfg(test)]

use crate::{
    InvestmentConstraintsConfig, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Val, Vec as SdkVec,
};

/// Event topic published by `set_investment_constraints` (`EVENT_INV_CONSTRAINTS`).
const EVENT_INV_CFG: Symbol = symbol_short!("inv_cfg");

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Register one offering for `(issuer, namespace, token)` with no supply cap and
/// no minimum revenue threshold, so the only state under test is the investment
/// constraints record.
fn register_offering(
    client: &RevoraRevenueShareClient<'static>,
    env: &Env,
    issuer: &Address,
    namespace: Symbol,
    token: &Address,
) {
    let payout_asset = Address::generate(env);
    client.register_offering(
        issuer,
        &SdkVec::new(env),
        &1u32,
        &namespace,
        token,
        &1_000u32,
        &payout_asset,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
}

/// Default fixture: initialized contract with a single offering registered under
/// the `("def")` namespace.
fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    register_offering(&client, &env, &issuer, symbol_short!("def"), &token);

    (env, client, issuer, token)
}

/// Count `inv_cfg` events in the ledger log.  Used to prove that a read never
/// publishes and a rejected write never announces a new configuration.
fn count_inv_cfg_events(env: &Env) -> usize {
    let topic: Val = EVENT_INV_CFG.into_val(env);
    env.events().all().iter().filter(|(_, topics, _)| topics.contains(topic)).count()
}

/// Shorthand for the value `get_investment_constraints` must return.
fn bounds(min: i128, max: i128) -> Option<InvestmentConstraintsConfig> {
    Some(InvestmentConstraintsConfig { min_stake: min, max_stake: max })
}

// ── Absence / default ─────────────────────────────────────────────────────────

/// A triple that was never registered resolves to `None` instead of panicking
/// or inventing a default configuration.
#[test]
fn get_investment_constraints_returns_none_for_unregistered_offering() {
    let (env, client, _issuer, _token) = setup();

    let unknown_issuer = Address::generate(&env);
    let unknown_token = Address::generate(&env);

    let same_ns_unknown_token =
        client.get_investment_constraints(&unknown_issuer, &symbol_short!("def"), &unknown_token);
    assert_eq!(same_ns_unknown_token, None, "unknown issuer/token must read as None");

    let same_token_unknown_ns =
        client.get_investment_constraints(&unknown_issuer, &symbol_short!("ns"), &unknown_token);
    assert_eq!(same_token_unknown_ns, None, "unknown namespace must read as None");
}

/// "Never configured" (`None`) must stay distinguishable from an explicit
/// unlimited configuration (`Some { min: 0, max: 0 }`).
#[test]
fn get_investment_constraints_distinguishes_unset_from_explicit_zero_bounds() {
    let (_env, client, issuer, token) = setup();

    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        None,
        "a registered offering without constraints must read as None"
    );

    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &0i128, &0i128);
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(0, 0),
        "an explicit unlimited configuration must read as Some {{ min: 0, max: 0 }}, never None"
    );
}

// ── Happy path / numeric fidelity ─────────────────────────────────────────────

/// The configured bounds are returned field-by-field, with no clamping,
/// rounding, or field swapping.
#[test]
fn get_investment_constraints_returns_exact_configured_bounds() {
    let (_env, client, issuer, token) = setup();

    client.set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000i128,
        &10_000i128,
    );

    let observed = client.get_investment_constraints(&issuer, &symbol_short!("def"), &token);
    assert_eq!(observed, bounds(1_000, 10_000));

    let config = observed.expect("a configured offering must return Some");
    assert_eq!(config.min_stake, 1_000, "min_stake must be returned verbatim");
    assert_eq!(config.max_stake, 10_000, "max_stake must be returned verbatim");
}

/// `i128::MAX` is the largest representable bound; it must survive the storage
/// round-trip unchanged (no truncation, no saturation).
#[test]
fn get_investment_constraints_round_trips_i128_max_bounds() {
    let (_env, client, issuer, token) = setup();

    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &0i128, &i128::MAX);
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(0, i128::MAX),
        "max_stake = i128::MAX must round-trip verbatim"
    );

    client.set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &i128::MAX,
        &i128::MAX,
    );
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(i128::MAX, i128::MAX),
        "min_stake = max_stake = i128::MAX must round-trip verbatim"
    );
}

/// A write fully replaces the previous record: no stale field survives, and the
/// read reflects only the latest configuration.
#[test]
fn get_investment_constraints_reflects_only_latest_configuration() {
    let (_env, client, issuer, token) = setup();

    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &100i128, &1_000i128);
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(100, 1_000)
    );

    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &200i128, &2_000i128);
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(200, 2_000),
        "an update must replace both fields atomically"
    );

    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &0i128, &0i128);
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(0, 0),
        "relaxing back to unlimited must be visible, not merged with previous bounds"
    );
}

// ── Key scoping / isolation ───────────────────────────────────────────────────

/// Namespace is part of the storage key: a neighbouring namespace (including
/// the empty symbol and a `Symbol` longer than 9 characters) must not observe
/// the configured bounds.
#[test]
fn get_investment_constraints_is_scoped_by_namespace() {
    let (env, client, issuer, token) = setup();
    let long_namespace = Symbol::new(&env, "long_namespace_sym");

    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &50i128, &500i128);

    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("alt"), &token),
        None,
        "a different short namespace must not see the bounds"
    );
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!(""), &token),
        None,
        "the empty namespace must not see the bounds"
    );
    assert_eq!(
        client.get_investment_constraints(&issuer, &long_namespace, &token),
        None,
        "a namespace longer than 9 characters must not see the bounds"
    );
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(50, 500),
        "the owning namespace must still observe its own bounds"
    );
}

/// Token is part of the storage key: a second offering on the same namespace
/// keeps an independent constraints record.
#[test]
fn get_investment_constraints_is_scoped_by_token() {
    let (env, client, issuer, token_a) = setup();
    let token_b = Address::generate(&env);
    register_offering(&client, &env, &issuer, symbol_short!("def"), &token_b);

    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token_a, &10i128, &20i128);

    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token_b),
        None,
        "a sibling token on the same namespace must not inherit the bounds"
    );
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token_a),
        bounds(10, 20),
        "the offering that was configured must still observe its bounds"
    );
}

/// Issuer is part of the storage key: two issuers may run offerings that share a
/// namespace and token, and neither may observe the other's bounds.
#[test]
fn get_investment_constraints_does_not_leak_across_issuers() {
    let (env, client, issuer_a, token) = setup();
    let issuer_b = Address::generate(&env);
    register_offering(&client, &env, &issuer_b, symbol_short!("def"), &token);

    client.set_investment_constraints(&issuer_a, &symbol_short!("def"), &token, &11i128, &22i128);

    assert_eq!(
        client.get_investment_constraints(&issuer_b, &symbol_short!("def"), &token),
        None,
        "another issuer's offering must not expose the first issuer's bounds"
    );
    assert_eq!(
        client.get_investment_constraints(&issuer_a, &symbol_short!("def"), &token),
        bounds(11, 22),
        "the owning issuer must still observe its own bounds"
    );
}

/// Storage is per contract instance: a freshly deployed contract must not read
/// bounds persisted by another deployment.
#[test]
fn get_investment_constraints_isolated_across_contract_instances() {
    let (env, client, issuer, token) = setup();
    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &7i128, &77i128);

    let other_id = env.register_contract(None, RevoraRevenueShare);
    let other_client = RevoraRevenueShareClient::new(&env, &other_id);

    assert_eq!(
        other_client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        None,
        "a second deployment must start with no constraints for the same triple"
    );
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(7, 77),
        "the original deployment keeps its own bounds"
    );
}

// ── Authorization surface ─────────────────────────────────────────────────────

/// The read API is a public view: it declares no auth requirement, so it must
/// keep serving callers with no authorization entries available at all.
#[test]
fn get_investment_constraints_serves_callers_without_auth() {
    let (env, client, issuer, token) = setup();
    let stranger = Address::generate(&env);
    let unconfigured_token = Address::generate(&env);

    client.set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000i128,
        &2_000i128,
    );

    // Revoke every mocked authorization: any `require_auth` on the read path
    // would now trap, so a successful read proves the view is unauthenticated.
    env.mock_auths(&[]);

    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(1_000, 2_000),
        "configured bounds must be readable without any authorization"
    );
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &unconfigured_token),
        None,
        "an unconfigured token must read as None without any authorization"
    );
    assert_eq!(
        client.get_investment_constraints(&stranger, &symbol_short!("def"), &token),
        None,
        "an unrelated caller supplies its own key triple and still resolves cleanly"
    );
}

/// A read is side-effect free: repeated calls are stable and publish no events.
#[test]
fn get_investment_constraints_reads_are_idempotent_and_side_effect_free() {
    let (env, client, issuer, token) = setup();
    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &300i128, &900i128);

    let events_before = env.events().all().len();
    let inv_cfg_before = count_inv_cfg_events(&env);

    for _ in 0..3 {
        assert_eq!(
            client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
            bounds(300, 900)
        );
    }

    assert_eq!(env.events().all().len(), events_before, "reads must not publish events");
    assert_eq!(count_inv_cfg_events(&env), inv_cfg_before, "reads must not publish inv_cfg");
}

// ── Rejection isolation: stored state must remain unchanged ───────────────────

/// A negative bound is rejected with `InvalidAmount` and leaves the record
/// untouched — first write (stays `None`), then writes over a good record.
#[test]
fn get_investment_constraints_unchanged_after_rejected_negative_bounds() {
    let (_env, client, issuer, token) = setup();

    let first_reject = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &-1i128,
        &100i128,
    );
    assert_eq!(first_reject, Err(Ok(RevoraError::InvalidAmount)));
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        None,
        "a rejected first write must not create a partial record"
    );

    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &100i128, &500i128);

    let negative_min = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &-1i128,
        &100i128,
    );
    assert_eq!(negative_min, Err(Ok(RevoraError::InvalidAmount)));

    let negative_max = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &100i128,
        &-1i128,
    );
    assert_eq!(negative_max, Err(Ok(RevoraError::InvalidAmount)));

    let both_negative = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &-5i128,
        &-5i128,
    );
    assert_eq!(both_negative, Err(Ok(RevoraError::InvalidAmount)));

    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(100, 500),
        "rejected negative bounds must not modify the stored configuration"
    );
}

/// `min > max` is rejected with `InvalidAmount`; the previous bounds survive and
/// the inclusive `min == max` boundary stays storable.
#[test]
fn get_investment_constraints_unchanged_after_rejected_inverted_range() {
    let (_env, client, issuer, token) = setup();

    let inverted_first = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000i128,
        &1i128,
    );
    assert_eq!(inverted_first, Err(Ok(RevoraError::InvalidAmount)));
    assert_eq!(client.get_investment_constraints(&issuer, &symbol_short!("def"), &token), None);

    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &100i128, &500i128);

    let inverted = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &600i128,
        &500i128,
    );
    assert_eq!(inverted, Err(Ok(RevoraError::InvalidAmount)));
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(100, 500),
        "a rejected inverted range must not modify the stored configuration"
    );

    let equal_bounds = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &500i128,
        &500i128,
    );
    assert!(equal_bounds.is_ok(), "min == max is the inclusive valid boundary");
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(500, 500),
        "the boundary configuration must become visible"
    );
}

/// A caller that is not the offering's issuer cannot write bounds: the write is
/// rejected deterministically and the issuer's configuration is preserved.
#[test]
fn get_investment_constraints_unchanged_after_rejected_foreign_issuer_write() {
    let (env, client, issuer, token) = setup();
    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &250i128, &750i128);

    let foreign_issuer = Address::generate(&env);
    let rejected = client.try_set_investment_constraints(
        &foreign_issuer,
        &symbol_short!("def"),
        &token,
        &1i128,
        &9_999i128,
    );
    assert_eq!(
        rejected,
        Err(Ok(RevoraError::OfferingNotFound)),
        "a non-issuer has no offering to configure"
    );

    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(250, 750),
        "a rejected foreign-issuer write must not modify the issuer's bounds"
    );
    assert_eq!(
        client.get_investment_constraints(&foreign_issuer, &symbol_short!("def"), &token),
        None,
        "the rejected caller must not gain a visible record"
    );
}

/// Without any authorization entry the write is rejected, no `inv_cfg` event is
/// published, and the stored bounds are preserved.
#[test]
fn get_investment_constraints_unchanged_after_write_without_authorization() {
    let (env, client, issuer, token) = setup();
    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &400i128, &800i128);

    let events_before = env.events().all().len();
    let inv_cfg_before = count_inv_cfg_events(&env);

    // Revoke all mocked auths so the issuer quorum can no longer be met.
    env.mock_auths(&[]);
    let rejected = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000i128,
        &9_000i128,
    );
    assert!(rejected.is_err(), "an unauthorized write must be rejected");

    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(400, 800),
        "an unauthorized write must not modify the stored bounds"
    );
    assert_eq!(env.events().all().len(), events_before, "a rejected write must publish no events");
    assert_eq!(
        count_inv_cfg_events(&env),
        inv_cfg_before,
        "a rejected write must not publish inv_cfg"
    );
}

/// While the contract is globally frozen, writes fail with `ContractFrozen`, the
/// stored bounds are unchanged, and the read API keeps serving (reads are not
/// gated by the freeze guard).
#[test]
fn get_investment_constraints_unchanged_after_rejected_write_while_frozen() {
    let (env, client, issuer, token) = setup();
    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &500i128, &5_000i128);

    client.freeze();
    assert!(client.is_frozen(), "the contract must be frozen for this case");

    let rejected = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &6_000i128,
        &7_000i128,
    );
    assert_eq!(rejected, Err(Ok(RevoraError::ContractFrozen)));
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        bounds(500, 5_000),
        "a write rejected by the freeze guard must not modify the stored bounds"
    );
}
