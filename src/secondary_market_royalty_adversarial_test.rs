//! Adversarial / boundary coverage for the per-offering secondary-market royalty
//! setter (#1042): `set_secondary_market_royalty_bps` and its reader
//! `get_secondary_market_royalty_bps`.
//!
//! Focus areas:
//! - valid writes and idempotent overwrites round-trip through the reader,
//! - the lower (`0`) and inclusive upper (`MAX_PLATFORM_FEE_BPS == 5_000`)
//!   boundaries are accepted, and anything above the cap is rejected,
//! - non-owners / unknown offerings and unauthenticated owners are rejected,
//!   with the stored value and event stream proven unchanged,
//! - configuration is isolated per `(offering, asset)` pair.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Vec,
};

/// `EVENT_ROYALTY_CONFIG` (`roy_cfg`) topic emitted on a successful write.
const ROY_CFG: Symbol = symbol_short!("roy_cfg");

struct Ctx {
    env: Env,
    client: RevoraRevenueShareClient<'static>,
    issuer: Address,
    ns: Symbol,
    token: Address,
    asset: Address,
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

    let asset = Address::generate(&env);
    Ctx { env, client, issuer, ns, token, asset }
}

/// A fresh offering defaults to `0`; a valid write is observable and an
/// overwrite tracks the latest writer.
#[test]
fn default_is_zero_then_valid_writes_round_trip() {
    let c = setup();
    let get = |bps_asset: &Address| {
        c.client.get_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, bps_asset)
    };

    assert_eq!(get(&c.asset), 0, "unset royalty must default to 0");

    c.client
        .set_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset, &1);
    assert_eq!(get(&c.asset), 1);

    c.client
        .set_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset, &2_500);
    assert_eq!(get(&c.asset), 2_500, "overwrite must replace the stored value");
}

/// `0` is the accepted lower boundary (means "no royalty").
#[test]
fn zero_royalty_is_accepted_at_lower_boundary() {
    let c = setup();
    c.client
        .set_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset, &0);
    assert_eq!(
        c.client.get_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset),
        0
    );
}

/// The upper bound is inclusive: `MAX_PLATFORM_FEE_BPS` (5_000) is accepted.
#[test]
fn exact_max_royalty_is_accepted() {
    let c = setup();
    c.client
        .set_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset, &5_000);
    assert_eq!(
        c.client.get_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset),
        5_000
    );
}

/// Anything above the cap is rejected with `InvalidRevenueShareBps` and must not
/// disturb the previously stored value.
#[test]
fn above_max_royalty_is_rejected_and_state_unchanged() {
    let c = setup();
    c.client
        .set_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset, &100);

    for bad in [5_001u32, 10_000u32, u32::MAX] {
        let res = c.client.try_set_secondary_market_royalty_bps(
            &c.issuer,
            &c.ns,
            &c.token,
            &c.asset,
            &bad,
        );
        assert_eq!(
            res,
            Err(Ok(RevoraError::InvalidRevenueShareBps)),
            "royalty_bps={bad} must be rejected"
        );
        assert_eq!(
            c.client.get_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset),
            100,
            "state must be unchanged after rejecting royalty_bps={bad}"
        );
    }
}

/// A caller that does not own the offering (or targets an unknown
/// namespace/token) is rejected and creates no phantom configuration.
#[test]
fn non_issuer_caller_is_rejected_and_state_unchanged() {
    let c = setup();
    let stranger = Address::generate(&c.env);

    let res = c.client.try_set_secondary_market_royalty_bps(
        &stranger,
        &c.ns,
        &c.token,
        &c.asset,
        &750,
    );
    assert_eq!(res, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(
        c.client.get_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset),
        0
    );

    let unknown_ns = symbol_short!("nope");
    let res = c.client.try_set_secondary_market_royalty_bps(
        &c.issuer,
        &unknown_ns,
        &c.token,
        &c.asset,
        &750,
    );
    assert_eq!(res, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(
        c.client.get_secondary_market_royalty_bps(&c.issuer, &unknown_ns, &c.token, &c.asset),
        0
    );
}

/// The slot is scoped to one `(issuer, namespace, token)` offering: a second,
/// independently registered issuer cannot write into another offering's royalty
/// slot. The rejected call must neither overwrite the stored value nor emit a
/// `roy_cfg` event.
#[test]
fn foreign_issuer_write_is_rejected_and_state_unchanged() {
    let c = setup();
    c.client
        .set_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset, &250);

    // Register a second offering, owned by a different issuer.
    let other_issuer = Address::generate(&c.env);
    let other_token = Address::generate(&c.env);
    let other_payout = Address::generate(&c.env);
    c.client.register_offering(
        &other_issuer,
        &Vec::new(&c.env),
        &1u32,
        &c.ns,
        &other_token,
        &2_500,
        &other_payout,
        &0,
        &symbol_short!(""),
        &0u32,
    );

    let before = c.env.events().all().len();
    // `other_issuer` is not the issuer of `(ns, token)`, so the ownership lookup
    // rejects the write before any state or event is produced.
    let res = c.client.try_set_secondary_market_royalty_bps(
        &other_issuer,
        &c.ns,
        &c.token,
        &c.asset,
        &900,
    );
    assert_eq!(res, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(
        c.client.get_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset),
        250,
        "rejected foreign-issuer write must not overwrite the stored royalty"
    );
    assert_eq!(
        c.client.get_secondary_market_royalty_bps(&other_issuer, &c.ns, &other_token, &c.asset),
        0,
        "the foreign issuer's own offering must not gain a royalty either"
    );
    assert_eq!(
        c.env.events().all().len(),
        before,
        "rejected foreign-issuer write must not emit roy_cfg"
    );
}

/// The reader is a pure view: it works with auth mocking disabled.
#[test]
fn reader_is_unauthenticated() {
    let c = setup();
    c.client
        .set_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset, &321);

    c.env.set_auths(&[]);
    assert_eq!(
        c.client.get_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset),
        321
    );
}

/// Exactly one `roy_cfg` event is emitted on success, and none on rejection.
#[test]
fn roy_cfg_event_emitted_on_success_only() {
    let c = setup();
    let other_asset = Address::generate(&c.env);

    let before = c.env.events().all().len();
    // Rejected (over-cap) write must not emit anything.
    let _ = c.client.try_set_secondary_market_royalty_bps(
        &c.issuer,
        &c.ns,
        &c.token,
        &c.asset,
        &6_000,
    );
    assert_eq!(c.env.events().all().len(), before, "failed write must not emit");

    c.client
        .set_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &other_asset, &300);

    let all = c.env.events().all();
    let mut count: u32 = 0;
    for i in before..all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        if !topics.is_empty() {
            let t0: Symbol = topics.get(0).unwrap().into_val(&c.env);
            if t0 == ROY_CFG {
                let decoded: u32 = data.into_val(&c.env);
                assert_eq!(decoded, 300);
                count = count.checked_add(1).unwrap();
            }
        }
    }
    assert_eq!(count, 1, "exactly one roy_cfg event expected on success");
}

/// Configuration is keyed by `(offering, asset)`, not by offering alone.
#[test]
fn royalty_is_isolated_per_asset() {
    let c = setup();
    let other_asset = Address::generate(&c.env);

    c.client
        .set_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset, &120);
    c.client
        .set_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &other_asset, &4_999);

    assert_eq!(
        c.client.get_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &c.asset),
        120
    );
    assert_eq!(
        c.client.get_secondary_market_royalty_bps(&c.issuer, &c.ns, &c.token, &other_asset),
        4_999
    );
}
