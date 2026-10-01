//! # `get_max_total_supply_shares` — adversarial coverage [issue #1034]
//!
//! `get_max_total_supply_shares(env, issuer, namespace, token) -> i128` is the
//! read side of the per-offering supply ceiling that `set_max_total_supply_shares`
//! writes. The cap is checked by the issuance path, so a wrong read here is a
//! supply-control bug: too high a value lets an issuer mint past the configured
//! ceiling, too low a value blocks legitimate issuance.
//!
//! ## Contract pinned by these tests
//!
//! 1. **Default is `0`** — an offering with no configured cap reads `0`, which is
//!    documented as "no cap" rather than "zero shares allowed".
//! 2. **Round-trip fidelity** — any non-negative `i128` (including `1` and
//!    `i128::MAX`) is returned unchanged.
//! 3. **`0` clears the cap** — writing `0` removes the stored entry, and the
//!    getter reads `0` again.
//! 4. **Negative caps are rejected** — `AmountValidationMatrix` classifies
//!    `MaxTotalSupplyShares` as non-negative only, so a negative write returns
//!    the typed `RevoraError::InvalidAmount` and leaves the previous cap intact.
//! 5. **The cap is scoped** to the full `(issuer, namespace, token)` offering id:
//!    writing one offering never leaks into a sibling offering.
//! 6. **Reads never mutate** — the getter is idempotent and needs no auth.
//! 7. **Frozen contracts reject writes** with the typed `RevoraError::ContractFrozen`
//!    and leave the stored cap untouched.
//!
//! ## Why the auth test is `#[ignore]`d
//!
//! `issuer.require_auth()` is enforced by the host *before* any contract-level
//! check, so in the `no_std` test runtime it aborts the frame instead of
//! returning a typed error (see `src/test_auth.rs`, "Layer 1 panic"). The test is
//! kept for documentation parity with the rest of the suite.

#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Initialize an admin and register one offering owned by a fresh issuer.
///
/// Returns `(client, issuer, namespace, token)`.
fn setup(env: &Env) -> (RevoraRevenueShareClient<'_>, Address, Address, Address) {
    env.mock_all_auths();
    let client = make_client(env);

    let admin = Address::generate(env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(env);
    let namespace = symbol_short!("def");
    let token = Address::generate(env);
    let payout_asset = Address::generate(env);

    client.register_offering(
        &issuer,
        &Vec::new(env),
        &1u32,
        &namespace,
        &token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    (client, issuer, namespace, token)
}

// ── default state ────────────────────────────────────────────────────────────

#[test]
fn defaults_to_zero_for_an_offering_without_a_cap() {
    let env = Env::default();
    let (client, issuer, namespace, token) = setup(&env);

    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token),
        0
    );
}

#[test]
fn defaults_to_zero_for_an_offering_that_was_never_registered() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // An unknown offering id must not error — the issuance path treats a missing
    // entry as "unlimited".
    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &symbol_short!("def"), &token),
        0
    );
}

// ── round-trip ───────────────────────────────────────────────────────────────

#[test]
fn round_trips_a_positive_cap() {
    let env = Env::default();
    let (client, issuer, namespace, token) = setup(&env);

    client.set_max_total_supply_shares(&issuer, &namespace, &token, &10_000);

    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token),
        10_000
    );
}

#[test]
fn boundary_minimum_positive_cap_is_stored() {
    let env = Env::default();
    let (client, issuer, namespace, token) = setup(&env);

    client.set_max_total_supply_shares(&issuer, &namespace, &token, &1);

    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token),
        1
    );
}

#[test]
fn boundary_i128_max_is_stored_verbatim() {
    let env = Env::default();
    let (client, issuer, namespace, token) = setup(&env);

    client.set_max_total_supply_shares(&issuer, &namespace, &token, &i128::MAX);

    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token),
        i128::MAX
    );
}

#[test]
fn a_later_write_replaces_the_previous_cap() {
    let env = Env::default();
    let (client, issuer, namespace, token) = setup(&env);

    client.set_max_total_supply_shares(&issuer, &namespace, &token, &250);
    client.set_max_total_supply_shares(&issuer, &namespace, &token, &999);

    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token),
        999
    );
}

// ── zero means "no cap" ──────────────────────────────────────────────────────

#[test]
fn writing_zero_after_a_positive_cap_clears_it() {
    let env = Env::default();
    let (client, issuer, namespace, token) = setup(&env);

    client.set_max_total_supply_shares(&issuer, &namespace, &token, &250);
    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token),
        250
    );

    client.set_max_total_supply_shares(&issuer, &namespace, &token, &0);

    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token),
        0
    );
}

#[test]
fn writing_zero_to_an_uncapped_offering_is_a_no_op() {
    let env = Env::default();
    let (client, issuer, namespace, token) = setup(&env);

    client.set_max_total_supply_shares(&issuer, &namespace, &token, &0);

    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token),
        0
    );
}

// ── rejected writes leave state unchanged ────────────────────────────────────

#[test]
fn negative_cap_is_rejected_and_the_previous_cap_survives() {
    let env = Env::default();
    let (client, issuer, namespace, token) = setup(&env);

    client.set_max_total_supply_shares(&issuer, &namespace, &token, &42);

    let rejected =
        client.try_set_max_total_supply_shares(&issuer, &namespace, &token, &-1);
    assert_eq!(rejected, Err(Ok(RevoraError::InvalidAmount)));

    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token),
        42
    );
}

#[test]
fn i128_min_cap_is_rejected_and_the_previous_cap_survives() {
    let env = Env::default();
    let (client, issuer, namespace, token) = setup(&env);

    client.set_max_total_supply_shares(&issuer, &namespace, &token, &7);

    let rejected =
        client.try_set_max_total_supply_shares(&issuer, &namespace, &token, &i128::MIN);
    assert_eq!(rejected, Err(Ok(RevoraError::InvalidAmount)));

    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token),
        7
    );
}

#[test]
fn a_rejected_negative_cap_leaves_an_unset_cap_at_zero() {
    let env = Env::default();
    let (client, issuer, namespace, token) = setup(&env);

    let rejected =
        client.try_set_max_total_supply_shares(&issuer, &namespace, &token, &-1_000_000);
    assert!(rejected.is_err());

    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token),
        0
    );
}

#[test]
fn a_frozen_contract_rejects_writes_without_changing_the_cap() {
    let env = Env::default();
    let (client, issuer, namespace, token) = setup(&env);

    client.set_max_total_supply_shares(&issuer, &namespace, &token, &42);

    client.freeze();

    let rejected =
        client.try_set_max_total_supply_shares(&issuer, &namespace, &token, &99);
    assert_eq!(rejected, Err(Ok(RevoraError::ContractFrozen)));

    // Reads still work on a frozen contract and the old cap is intact.
    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token),
        42
    );
}

// ── offering scoping ─────────────────────────────────────────────────────────

#[test]
fn the_cap_is_scoped_per_token() {
    let env = Env::default();
    let (client, issuer, namespace, token_a) = setup(&env);
    let token_b = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token_b,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    client.set_max_total_supply_shares(&issuer, &namespace, &token_a, &100);
    client.set_max_total_supply_shares(&issuer, &namespace, &token_b, &200);

    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token_a),
        100
    );
    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token_b),
        200
    );
}

#[test]
fn the_cap_is_scoped_per_namespace() {
    let env = Env::default();
    let (client, issuer, namespace_a, token) = setup(&env);
    let namespace_b = symbol_short!("alt");

    client.set_max_total_supply_shares(&issuer, &namespace_a, &token, &300);

    // Same issuer + token, different namespace → independent slot, still uncapped.
    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace_b, &token),
        0
    );
    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace_a, &token),
        300
    );
}

#[test]
fn the_cap_is_scoped_per_issuer() {
    let env = Env::default();
    let (client, issuer_a, namespace, token) = setup(&env);
    let issuer_b = Address::generate(&env);

    client.set_max_total_supply_shares(&issuer_a, &namespace, &token, &500);

    assert_eq!(
        client.get_max_total_supply_shares(&issuer_b, &namespace, &token),
        0
    );
    assert_eq!(
        client.get_max_total_supply_shares(&issuer_a, &namespace, &token),
        500
    );
}

// ── read-only guarantees ─────────────────────────────────────────────────────

#[test]
fn repeated_reads_are_idempotent_and_need_no_auth() {
    let env = Env::default();
    let (client, issuer, namespace, token) = setup(&env);

    client.set_max_total_supply_shares(&issuer, &namespace, &token, &1234);

    let first = client.get_max_total_supply_shares(&issuer, &namespace, &token);
    let second = client.get_max_total_supply_shares(&issuer, &namespace, &token);
    let third = client.get_max_total_supply_shares(&issuer, &namespace, &token);

    assert_eq!(first, 1234);
    assert_eq!(second, 1234);
    assert_eq!(third, 1234);
}

// ── authorization (documented, not executed) ─────────────────────────────────

#[test]
#[ignore = "issuer.require_auth causes a non-unwinding host panic in no_std"]
fn unauthorized_write_is_rejected_and_the_cap_is_unchanged() {
    let env = Env::default();
    // Deliberately no `mock_all_auths()`: the issuer's authorization cannot be
    // satisfied, so the host must abort the call.
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("def");
    let token = Address::generate(&env);

    let result = client.try_set_max_total_supply_shares(&issuer, &namespace, &token, &1_000);
    assert!(result.is_err());

    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &namespace, &token),
        0
    );
}
