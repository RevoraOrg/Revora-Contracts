//! Adversarial coverage for `get_platform_fee` (`src/lib.rs`, Revora-Contracts).
//!
//! `get_platform_fee(env) -> u32` is the read-only accessor for the single global
//! platform-fee BPS value written by `set_platform_fee`. It is the value that
//! `calculate_platform_fee` consumes and that off-chain fee accounting reads, so
//! the accessor must be:
//!
//! * **Defaulted to zero** — a contract that has never configured a fee charges none.
//! * **Exact** — it returns the last successfully written value, no clamping and no
//!   silent fallback to a "sane" default.
//! * **Write-gated** — the maximum accepted value is `MAX_PLATFORM_FEE_BPS` (5 000);
//!   every larger value is rejected with `InvalidRevenueShareBps` and must leave the
//!   readable value untouched.
//! * **Global** — it is a distinct storage slot from the per-asset overrides, which
//!   live under their own keys and are read by `get_platform_fee_per_asset`.
//!
//! `set_platform_fee` takes no caller argument and gates on `admin.require_auth()`,
//! so caller identity cannot be varied from the outside; the repo's auth suite
//! documents that the un-mocked `require_auth` path panics rather than returning a
//! typed error. The guards that *do* return typed errors (uninitialized, over-cap)
//! are what the tests below assert on.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Vec,
};

/// The highest fee the contract accepts, mirroring `MAX_PLATFORM_FEE_BPS`.
const MAX_FEE_BPS: u32 = 5_000;
/// Denominator used by `calculate_platform_fee`.
const BPS_DENOMINATOR: i128 = 10_000;

struct Ctx {
    env: Env,
    client: RevoraRevenueShareClient<'static>,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    Ctx { env, client }
}

/// Decode the `fee_set` payloads emitted at or after `start_idx`.
fn fee_set_values(env: &Env, start_idx: u32) -> Vec<u32> {
    let all = env.events().all();
    let mut out = Vec::new(env);
    let mut i = start_idx;
    while i < all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        if !topics.is_empty() {
            let t0: Symbol = topics.get(0).unwrap().into_val(env);
            if t0 == crate::EVENT_PLATFORM_FEE_SET {
                let value: u32 = data.into_val(env);
                out.push_back(value);
            }
        }
        i += 1;
    }
    out
}

fn fee_set_count(env: &Env, start_idx: u32) -> u32 {
    fee_set_values(env, start_idx).len()
}

// ── Defaults ─────────────────────────────────────────────────────────────────

#[test]
fn the_platform_fee_defaults_to_zero() {
    let c = setup();

    // No configuration has happened, so no fee may be charged.
    assert_eq!(c.client.get_platform_fee(), 0);
}

#[test]
fn a_freshly_initialized_contract_still_charges_no_fee() {
    let c = setup();

    // `initialize` must not seed a non-zero platform fee behind the operator's back.
    assert_eq!(c.client.get_platform_fee(), 0);
    assert_eq!(c.client.calculate_platform_fee(&1_000_000), 0);
}

#[test]
fn the_getter_is_a_pure_read() {
    let c = setup();
    c.client.set_platform_fee(&250).unwrap();
    let before = c.env.events().all().len();

    let first = c.client.get_platform_fee();
    let second = c.client.get_platform_fee();

    assert_eq!(first, second);
    assert_eq!(first, 250);
    // Reading a fee must not publish events or shift any state.
    assert_eq!(c.env.events().all().len(), before);
}

// ── Round-trip fidelity ──────────────────────────────────────────────────────

#[test]
fn the_getter_returns_exactly_what_was_written() {
    let c = setup();

    for value in [0u32, 1, 7, 250, 2_500, MAX_FEE_BPS - 1, MAX_FEE_BPS] {
        c.client.set_platform_fee(&value).unwrap();
        assert_eq!(c.client.get_platform_fee(), value, "round-trip must be lossless");
    }
}

#[test]
fn the_getter_tracks_the_latest_write_not_the_first() {
    let c = setup();

    c.client.set_platform_fee(&1_000).unwrap();
    c.client.set_platform_fee(&0).unwrap();
    c.client.set_platform_fee(&42).unwrap();

    assert_eq!(c.client.get_platform_fee(), 42);
}

#[test]
fn zero_is_an_accepted_explicit_configuration() {
    let c = setup();
    c.client.set_platform_fee(&900).unwrap();

    c.client.set_platform_fee(&0).unwrap();

    // Explicitly disabling the fee must be observable, not treated as "unset".
    assert_eq!(c.client.get_platform_fee(), 0);
    assert_eq!(fee_set_count(&c.env, 0), 2);
}

// ── Boundary and invalid values ──────────────────────────────────────────────

#[test]
fn the_maximum_fee_is_accepted_at_the_boundary() {
    let c = setup();

    c.client.set_platform_fee(&MAX_FEE_BPS).unwrap();

    assert_eq!(c.client.get_platform_fee(), MAX_FEE_BPS);
}

#[test]
fn one_bps_above_the_maximum_is_rejected_and_leaves_the_fee_unchanged() {
    let c = setup();
    c.client.set_platform_fee(&250).unwrap();
    let events_before = c.env.events().all().len();

    let res = c.client.try_set_platform_fee(&(MAX_FEE_BPS + 1));

    assert_eq!(res, Err(Ok(RevoraError::InvalidRevenueShareBps)));
    // Rejected writes must not change the readable fee...
    assert_eq!(c.client.get_platform_fee(), 250);
    // ...and must not emit a `fee_set` event that indexers would act on.
    assert_eq!(fee_set_count(&c.env, events_before), 0);
}

#[test]
fn a_wildly_over_cap_fee_is_rejected_without_clamping() {
    let c = setup();
    c.client.set_platform_fee(&1).unwrap();

    let res = c.client.try_set_platform_fee(&u32::MAX);

    assert_eq!(res, Err(Ok(RevoraError::InvalidRevenueShareBps)));
    // A "clamp to 5 000" implementation would report 5 000 here; it must report 1.
    assert_eq!(c.client.get_platform_fee(), 1);
}

#[test]
fn a_rejected_write_is_rejected_before_the_event_log_is_touched() {
    let c = setup();
    let before = c.env.events().all().len();

    assert_eq!(
        c.client.try_set_platform_fee(&(MAX_FEE_BPS + 1)),
        Err(Ok(RevoraError::InvalidRevenueShareBps))
    );

    assert_eq!(c.env.events().all().len(), before);
    assert_eq!(c.client.get_platform_fee(), 0);
}

#[test]
fn setting_the_fee_before_initialization_returns_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let res = client.try_set_platform_fee(&100);

    assert_eq!(res, Err(Ok(RevoraError::NotInitialized)));
    assert_eq!(client.get_platform_fee(), 0);
    assert_eq!(fee_set_count(&env, 0), 0);
}

// ── Events ───────────────────────────────────────────────────────────────────

#[test]
fn every_accepted_write_emits_the_new_fee_exactly_once() {
    let c = setup();
    let before = c.env.events().all().len();

    c.client.set_platform_fee(&300).unwrap();
    c.client.set_platform_fee(&0).unwrap();
    c.client.set_platform_fee(&MAX_FEE_BPS).unwrap();

    let emitted = fee_set_values(&c.env, before);
    assert_eq!(emitted.len(), 3, "one fee_set event per accepted write");
    assert_eq!(emitted.get(0).unwrap(), 300);
    assert_eq!(emitted.get(1).unwrap(), 0);
    assert_eq!(emitted.get(2).unwrap(), MAX_FEE_BPS);
    assert_eq!(c.client.get_platform_fee(), MAX_FEE_BPS);
}

// ── Interaction with the per-asset overrides ─────────────────────────────────

#[test]
fn the_global_fee_is_independent_of_per_asset_overrides() {
    let c = setup();
    let usdc = Address::generate(&c.env);
    let xlm = Address::generate(&c.env);

    c.client.set_platform_fee(&300).unwrap();
    c.client.set_platform_fee_per_asset(&usdc, &1_234).unwrap();

    // The global getter must not read, merge, or be shadowed by the per-asset slot.
    assert_eq!(c.client.get_platform_fee(), 300);
    assert_eq!(c.client.get_platform_fee_per_asset(&usdc), 1_234);
    assert_eq!(c.client.get_platform_fee_per_asset(&xlm), 0);

    // ...and the reverse direction holds too.
    c.client.set_platform_fee_per_asset(&xlm, &2).unwrap();
    assert_eq!(c.client.get_platform_fee(), 300);
}

#[test]
fn per_asset_overrides_are_rejected_above_the_cap_without_touching_the_global_fee() {
    let c = setup();
    let asset = Address::generate(&c.env);
    c.client.set_platform_fee(&400).unwrap();

    let res = c.client.try_set_platform_fee_per_asset(&asset, &(MAX_FEE_BPS + 1));

    assert_eq!(res, Err(Ok(RevoraError::InvalidRevenueShareBps)));
    assert_eq!(c.client.get_platform_fee_per_asset(&asset), 0);
    assert_eq!(c.client.get_platform_fee(), 400);
}

// ── The fee the getter exposes is the fee that is charged ────────────────────

#[test]
fn calculate_platform_fee_matches_the_value_exposed_by_the_getter() {
    let c = setup();
    c.client.set_platform_fee(&250).unwrap();

    let readable = c.client.get_platform_fee();
    assert_eq!(readable, 250);
    assert_eq!(c.client.calculate_platform_fee(&BPS_DENOMINATOR), readable as i128);
    assert_eq!(c.client.calculate_platform_fee(&1_000), 25);
    // Sub-unit remainders truncate toward zero rather than rounding up.
    assert_eq!(c.client.calculate_platform_fee(&3), 0);
    assert_eq!(c.client.calculate_platform_fee(&0), 0);
}

#[test]
fn calculate_platform_fee_at_the_maximum_bps_charges_half() {
    let c = setup();
    c.client.set_platform_fee(&MAX_FEE_BPS).unwrap();

    assert_eq!(c.client.calculate_platform_fee(&BPS_DENOMINATOR), 5_000);
    assert_eq!(c.client.calculate_platform_fee(&0), 0);
}
