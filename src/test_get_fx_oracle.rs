//! Adversarial coverage for `get_fx_oracle` (issue #1078).
//!
//! `get_fx_oracle(env, issuer, namespace, token)` is a permissionless read of
//! the per-offering `FxOracleConfig` entry written by `set_fx_oracle`. It has
//! no auth guard and no error path, so the adversarial surface is *what it
//! returns* and *what it must never return*.
//!
//! | Case                                                     | Expected                                        |
//! |----------------------------------------------------------|-------------------------------------------------|
//! | offering registered, no config written                   | `None` (reads never create storage)             |
//! | offering never registered                                | `None` (no error)                               |
//! | config written by the issuer                             | exact round-trip of all four fields             |
//! | second `set_fx_oracle`                                   | replaces wholesale, no field blending           |
//! | different issuer / namespace / token                     | `None` (no cross-tenant leakage)                |
//! | two offerings of one issuer                             | independent configs                             |
//! | `max_oracle_age_secs` = 0, 1, `u64::MAX - 1`, `u64::MAX` | stored verbatim, never clamped or truncated     |
//! | currency symbol at the 32-char limit                    | round-trips; 33 chars is unrepresentable        |
//! | unknown offering                                         | `OfferingNotFound`, nothing stored              |
//! | caller that is not the offering issuer                   | `OfferingNotFound`, stored config unchanged     |
//! | caller without issuer authorization                      | host auth error, stored config unchanged        |
//! | issuer that has since transferred the offering           | `OfferingNotFound`, stored config unchanged     |
//! | paused contract                                          | `ContractPaused`, reads still answer            |
//! | frozen contract                                          | `ContractFrozen`, reads still answer            |
//! | rejected write after a successful one                    | stored config bit-for-bit unchanged             |
//! | contract paused / frozen / no auth available             | getter keeps working (the view is never gated)  |
//! | `set_oracle_chain` (separate storage key)                | legacy config neither created nor cleared       |
//! | repeated reads                                           | identical value, no side effects                |

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, TryFromVal, Vec,
};

/// Longest currency symbol the Soroban `Symbol` encoding accepts.
const MAX_SYMBOL_LEN: usize = 32;
/// 32 valid symbol characters (`a-zA-Z0-9_`).
const SYMBOL_AT_LIMIT: &str = "AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHH";
/// 33 valid symbol characters - one past the limit, so unrepresentable.
const SYMBOL_OVER_LIMIT: &str = "AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHI";

const REVENUE_SYMBOL: &str = "EUR";
const PAYOUT_SYMBOL: &str = "USDC";
const NAMESPACE: &str = "def";

type Fixture =
    (Env, Address, RevoraRevenueShareClient<'static>, Address, Address, Symbol, Address, Address);

// -- helpers -------------------------------------------------------------------

/// Register a single-issuer offering.
///
/// `payout` is a plain (non-token) address on purpose: `register_offering`
/// probes `decimals()` on it and skips the consistency check when the probe
/// fails, which keeps the fixture independent of token contracts.
fn register(
    client: &RevoraRevenueShareClient<'static>,
    env: &Env,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    payout: &Address,
) {
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        namespace,
        token,
        &5_000,
        payout,
        &0,
        &Symbol::new(env, PAYOUT_SYMBOL),
        &0,
    );
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, NAMESPACE);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    register(&client, &env, &issuer, &namespace, &token, &payout);

    (env, contract_id, client, admin, issuer, namespace, token, payout)
}

/// Read back the stored config through the public getter; every
/// "state is unchanged" assertion in this file compares these snapshots.
fn stored_config(
    client: &RevoraRevenueShareClient<'static>,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) -> Option<FxOracleConfig> {
    client.get_fx_oracle(issuer, namespace, token)
}

fn oracle_entry(env: &Env, oracle: &Address, max_age_secs: u64) -> OracleEntry {
    OracleEntry {
        oracle: oracle.clone(),
        revenue_symbol: Symbol::new(env, REVENUE_SYMBOL),
        payout_symbol: Symbol::new(env, PAYOUT_SYMBOL),
        max_age_secs,
    }
}

// -- happy path ----------------------------------------------------------------

/// The getter must return exactly what the issuer stored - no defaults, no
/// clamping, no field reordering.
#[test]
fn get_fx_oracle_round_trips_every_field() {
    let (env, _id, client, _admin, issuer, namespace, token, _payout) = setup();
    let oracle = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, REVENUE_SYMBOL);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &60,
    );

    let config = client.get_fx_oracle(&issuer, &namespace, &token).expect("config must be stored");
    assert_eq!(config.oracle, oracle);
    assert_eq!(config.revenue_symbol, revenue_symbol);
    assert_eq!(config.payout_symbol, payout_symbol);
    assert_eq!(config.max_oracle_age_secs, 60);
    assert_eq!(
        config,
        FxOracleConfig {
            oracle: oracle.clone(),
            revenue_symbol: revenue_symbol.clone(),
            payout_symbol: payout_symbol.clone(),
            max_oracle_age_secs: 60,
        }
    );
}

/// An offering with no config reads as `None`, an offering that was never
/// registered also reads as `None` instead of erroring, and the `try_` wrapper
/// confirms the read is infallible.
#[test]
fn get_fx_oracle_returns_none_when_unset() {
    let (env, _id, client, _admin, issuer, namespace, token, _payout) = setup();

    assert!(stored_config(&client, &issuer, &namespace, &token).is_none());
    assert!(client.try_get_fx_oracle(&issuer, &namespace, &token).is_ok());

    // Repeated reads must not materialise anything either.
    for _ in 0..3 {
        assert!(stored_config(&client, &issuer, &namespace, &token).is_none());
    }

    // Never-registered offering identity.
    let stranger = Address::generate(&env);
    assert!(stored_config(&client, &stranger, &namespace, &token).is_none());
    assert!(client.get_fx_oracle(&stranger, &namespace, &token).is_none());
}

/// Rewriting the config replaces it wholesale: nothing from the previous oracle
/// survives the update.
#[test]
fn get_fx_oracle_overwrite_replaces_previous_config() {
    let (env, _id, client, _admin, issuer, namespace, token, _payout) = setup();
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, REVENUE_SYMBOL);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    client.set_fx_oracle(&issuer, &namespace, &token, &first, &revenue_symbol, &payout_symbol, &60);
    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &second,
        &revenue_symbol,
        &payout_symbol,
        &600,
    );

    let config = stored_config(&client, &issuer, &namespace, &token).unwrap();
    assert_eq!(config.oracle, second);
    assert_ne!(config.oracle, first);
    assert_eq!(config.max_oracle_age_secs, 600);
}

// -- scope / isolation ---------------------------------------------------------

/// Every component of the offering identity is part of the key: a config must
/// never be visible to a different issuer, namespace, or token, and sibling
/// offerings of the same issuer keep independent configs.
#[test]
fn get_fx_oracle_is_scoped_to_issuer_namespace_and_token() {
    let (env, _id, client, _admin, issuer, namespace, token, payout) = setup();
    let oracle = Address::generate(&env);
    let other_oracle = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, REVENUE_SYMBOL);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &60,
    );

    // Sibling offering: same issuer and namespace, different token.
    let sibling_token = Address::generate(&env);
    register(&client, &env, &issuer, &namespace, &sibling_token, &payout);
    let other_namespace = Symbol::new(&env, "alt");
    let other_issuer = Address::generate(&env);

    // Neighbouring identities see nothing.
    assert!(stored_config(&client, &issuer, &namespace, &sibling_token).is_none());
    assert!(stored_config(&client, &issuer, &other_namespace, &token).is_none());
    assert!(stored_config(&client, &other_issuer, &namespace, &token).is_none());

    // The configured offering is unaffected by those lookups.
    let config = stored_config(&client, &issuer, &namespace, &token).unwrap();
    assert_eq!(config.oracle, oracle);
    assert_eq!(config.max_oracle_age_secs, 60);

    // Two offerings of one issuer keep independent configs.
    client.set_fx_oracle(
        &issuer,
        &namespace,
        &sibling_token,
        &other_oracle,
        &revenue_symbol,
        &payout_symbol,
        &u64::MAX,
    );
    assert_eq!(stored_config(&client, &issuer, &namespace, &token).unwrap().oracle, oracle);
    let sibling = stored_config(&client, &issuer, &namespace, &sibling_token).unwrap();
    assert_eq!(sibling.oracle, other_oracle);
    assert_eq!(sibling.max_oracle_age_secs, u64::MAX);
}

// -- boundary values -----------------------------------------------------------

/// `max_oracle_age_secs` is a `u64` window: `0` disables the staleness check
/// and `u64::MAX` is the widest window. Values must be stored verbatim - never
/// clamped, wrapped, or rejected - and changing only the window must not
/// disturb the other fields.
#[test]
fn get_fx_oracle_preserves_max_oracle_age_boundaries() {
    let (env, _id, client, _admin, issuer, namespace, token, _payout) = setup();
    let oracle = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, REVENUE_SYMBOL);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    for max_age in [0u64, 1, 59, 60, 61, u64::MAX - 1, u64::MAX] {
        client.set_fx_oracle(
            &issuer,
            &namespace,
            &token,
            &oracle,
            &revenue_symbol,
            &payout_symbol,
            &max_age,
        );
        let config = stored_config(&client, &issuer, &namespace, &token).unwrap();
        assert_eq!(
            config.max_oracle_age_secs, max_age,
            "max_oracle_age_secs {max_age} must round-trip verbatim"
        );
        assert_eq!(config.oracle, oracle);
        assert_eq!(config.revenue_symbol, revenue_symbol);
        assert_eq!(config.payout_symbol, payout_symbol);
    }
}

/// The currency symbols are bounded by the Soroban `Symbol` encoding: 32
/// characters round-trip, while 33 characters are not representable at all and
/// can therefore never reach this entrypoint's storage.
#[test]
fn get_fx_oracle_rejects_overlong_currency_symbols() {
    let (env, _id, client, _admin, issuer, namespace, token, _payout) = setup();
    let oracle = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, SYMBOL_AT_LIMIT);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    assert_eq!(SYMBOL_AT_LIMIT.len(), MAX_SYMBOL_LEN);
    assert!(Symbol::try_from_val(&env, &SYMBOL_AT_LIMIT).is_ok());
    assert_eq!(SYMBOL_OVER_LIMIT.len(), MAX_SYMBOL_LEN + 1);
    assert!(Symbol::try_from_val(&env, &SYMBOL_OVER_LIMIT).is_err());

    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &60,
    );

    let config = stored_config(&client, &issuer, &namespace, &token).unwrap();
    assert_eq!(config.revenue_symbol, revenue_symbol);
    assert_eq!(config.payout_symbol, payout_symbol);
    assert_eq!(config.max_oracle_age_secs, 60);
}

// -- rejected writes leave state unchanged -------------------------------------

/// Writing a config for an offering that does not exist fails with
/// `OfferingNotFound` and leaves nothing behind for the getter to return.
#[test]
fn set_fx_oracle_for_unknown_offering_writes_nothing() {
    let (env, _id, client, _admin, _issuer, namespace, _token, _payout) = setup();
    let stranger = Address::generate(&env);
    let orphan_token = Address::generate(&env);
    let oracle = Address::generate(&env);

    let result = client.try_set_fx_oracle(
        &stranger,
        &namespace,
        &orphan_token,
        &oracle,
        &Symbol::new(&env, REVENUE_SYMBOL),
        &Symbol::new(&env, PAYOUT_SYMBOL),
        &60,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    assert!(stored_config(&client, &stranger, &namespace, &orphan_token).is_none());
    assert!(client.get_fx_oracle(&stranger, &namespace, &orphan_token).is_none());
}

/// A caller that is not the offering's issuer cannot reconfigure the oracle:
/// the offering lookup is keyed on the supplied issuer, so the write is
/// rejected and the legitimate config survives untouched.
#[test]
fn set_fx_oracle_from_non_issuer_cannot_overwrite_existing_config() {
    let (env, _id, client, _admin, issuer, namespace, token, _payout) = setup();
    let oracle = Address::generate(&env);
    let attacker = Address::generate(&env);
    let attacker_oracle = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, REVENUE_SYMBOL);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &60,
    );
    let stored = stored_config(&client, &issuer, &namespace, &token);
    assert!(stored.is_some(), "the fixture config must be stored");

    let result = client.try_set_fx_oracle(
        &attacker,
        &namespace,
        &token,
        &attacker_oracle,
        &revenue_symbol,
        &payout_symbol,
        &1,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    assert!(stored_config(&client, &attacker, &namespace, &token).is_none());
    assert_eq!(stored_config(&client, &issuer, &namespace, &token), stored);
}

/// Missing authorization is a host-level failure: `set_fx_oracle` calls
/// `issuer.require_auth()`, so with no authorizations available the write
/// cannot happen at all and the previously stored config is unchanged.
#[test]
fn set_fx_oracle_without_issuer_auth_is_rejected() {
    let (env, _id, client, _admin, issuer, namespace, token, _payout) = setup();
    let oracle = Address::generate(&env);
    let replacement = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, REVENUE_SYMBOL);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &60,
    );
    let stored = stored_config(&client, &issuer, &namespace, &token);
    assert!(stored.is_some(), "the fixture config must be stored");

    // Revoke every mocked authorization: any `require_auth()` now fails.
    env.mock_auths(&[]);
    let result = client.try_set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &replacement,
        &revenue_symbol,
        &payout_symbol,
        &1,
    );
    assert!(result.is_err(), "set_fx_oracle must fail without issuer authorization");

    env.mock_all_auths();
    assert_eq!(stored_config(&client, &issuer, &namespace, &token), stored);
    assert_eq!(stored_config(&client, &issuer, &namespace, &token).unwrap().oracle, oracle);
}

/// Once the offering has changed hands the previous issuer is no longer its
/// current issuer and can no longer reconfigure the oracle. The rejected update
/// leaves the stored config exactly as it was.
#[test]
fn set_fx_oracle_by_previous_issuer_after_transfer_is_rejected() {
    let (env, _id, client, _admin, issuer, namespace, token, _payout) = setup();
    let oracle = Address::generate(&env);
    let new_issuer = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, REVENUE_SYMBOL);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &60,
    );
    let stored = stored_config(&client, &issuer, &namespace, &token);
    assert!(stored.is_some(), "the fixture config must be stored");

    client.propose_issuer_transfer(&issuer, &namespace, &token, &new_issuer);
    client.accept_issuer_transfer(&new_issuer, &namespace, &token);

    let result = client.try_set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &1,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(stored_config(&client, &issuer, &namespace, &token), stored);
}

/// A paused contract rejects the write with `ContractPaused`, but the getter
/// keeps answering: reads are never gated by the pause tiers. After resuming,
/// the update applies cleanly, so the rejected call left no partial state.
#[test]
fn paused_contract_rejects_fx_oracle_updates_but_keeps_reads() {
    let (env, _id, client, admin, issuer, namespace, token, _payout) = setup();
    let oracle = Address::generate(&env);
    let other_oracle = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, REVENUE_SYMBOL);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &60,
    );
    let stored = stored_config(&client, &issuer, &namespace, &token);
    assert!(stored.is_some(), "the fixture config must be stored");

    client.pause_admin(&admin);

    let result = client.try_set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &other_oracle,
        &revenue_symbol,
        &payout_symbol,
        &1,
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractPaused)));
    assert_eq!(stored_config(&client, &issuer, &namespace, &token), stored);

    // Still readable while paused...
    assert!(stored_config(&client, &issuer, &namespace, &token).is_some());
    // ...and the write is applied, not partially applied, once resumed.
    client.unpause_admin(&admin);
    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &other_oracle,
        &revenue_symbol,
        &payout_symbol,
        &120,
    );
    let config = stored_config(&client, &issuer, &namespace, &token).unwrap();
    assert_eq!(config.oracle, other_oracle);
    assert_eq!(config.max_oracle_age_secs, 120);
}

/// A frozen contract rejects the write with `ContractFrozen` and the stored
/// config stays readable and unchanged.
///
/// The global freeze flag is seeded directly (`DataKey::Frozen` is what
/// `require_not_frozen` checks) so the fixture does not depend on the
/// admin-gated freeze entrypoint being reachable from the client.
#[test]
fn frozen_contract_rejects_fx_oracle_updates_but_keeps_reads() {
    let (env, contract_id, client, _admin, issuer, namespace, token, _payout) = setup();
    let oracle = Address::generate(&env);
    let other_oracle = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, REVENUE_SYMBOL);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &60,
    );
    let stored = stored_config(&client, &issuer, &namespace, &token);
    assert!(stored.is_some(), "the fixture config must be stored");

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::Frozen, &true);
    });

    let result = client.try_set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &other_oracle,
        &revenue_symbol,
        &payout_symbol,
        &1,
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));
    assert_eq!(stored_config(&client, &issuer, &namespace, &token), stored);
    assert_eq!(stored_config(&client, &issuer, &namespace, &token).unwrap().oracle, oracle);
}

/// Battery of rejected writes followed by a single comparison against the
/// snapshot taken after the single successful write: no rejected path may leave
/// a partial, merged, or mutated config behind.
#[test]
fn rejected_fx_oracle_updates_leave_stored_config_unchanged() {
    let (env, _id, client, admin, issuer, namespace, token, _payout) = setup();
    let oracle = Address::generate(&env);
    let rogue = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, REVENUE_SYMBOL);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &60,
    );
    let stored = stored_config(&client, &issuer, &namespace, &token);
    assert!(stored.is_some(), "the fixture config must be stored");

    // 1. Unknown offering.
    let unknown_token = Address::generate(&env);
    let unknown = client.try_set_fx_oracle(
        &issuer,
        &namespace,
        &unknown_token,
        &rogue,
        &revenue_symbol,
        &payout_symbol,
        &1,
    );
    assert_eq!(unknown, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(stored_config(&client, &issuer, &namespace, &token), stored);

    // 2. Caller that is not the offering issuer.
    let not_issuer = client.try_set_fx_oracle(
        &rogue,
        &namespace,
        &token,
        &rogue,
        &revenue_symbol,
        &payout_symbol,
        &1,
    );
    assert_eq!(not_issuer, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(stored_config(&client, &issuer, &namespace, &token), stored);

    // 3. No authorization available.
    env.mock_auths(&[]);
    let unauthorized = client.try_set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &rogue,
        &revenue_symbol,
        &payout_symbol,
        &1,
    );
    assert!(unauthorized.is_err());
    env.mock_all_auths();
    assert_eq!(stored_config(&client, &issuer, &namespace, &token), stored);

    // 4. Paused contract.
    client.pause_admin(&admin);
    let paused = client.try_set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &rogue,
        &revenue_symbol,
        &payout_symbol,
        &1,
    );
    assert_eq!(paused, Err(Ok(RevoraError::ContractPaused)));
    client.unpause_admin(&admin);
    assert_eq!(stored_config(&client, &issuer, &namespace, &token), stored);
}

// -- interaction with the oracle chain -----------------------------------------

/// The legacy single-oracle config and the chain live under different storage
/// keys: configuring, replacing, or clearing a chain must neither create nor
/// remove the legacy entry the getter reads.
#[test]
fn oracle_chain_updates_do_not_alter_legacy_fx_oracle_config() {
    let (env, _id, client, _admin, issuer, namespace, token, _payout) = setup();
    let oracle = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, REVENUE_SYMBOL);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    // Chain configured first: no legacy config exists yet.
    let mut entries = Vec::new(&env);
    entries.push_back(oracle_entry(&env, &oracle, 30));
    client.set_oracle_chain(&issuer, &namespace, &token, &entries);
    assert!(stored_config(&client, &issuer, &namespace, &token).is_none());
    assert_eq!(client.get_oracle_chain(&issuer, &namespace, &token).unwrap().entries.len(), 1);

    // Both configured: independent values.
    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &60,
    );
    let legacy = stored_config(&client, &issuer, &namespace, &token).unwrap();
    assert_eq!(legacy.max_oracle_age_secs, 60);
    assert_eq!(client.get_oracle_chain(&issuer, &namespace, &token).unwrap().entries.len(), 1);

    // Clearing the chain leaves the legacy config alone.
    client.set_oracle_chain(&issuer, &namespace, &token, &Vec::new(&env));
    assert!(client.get_oracle_chain(&issuer, &namespace, &token).unwrap().entries.is_empty());
    assert_eq!(stored_config(&client, &issuer, &namespace, &token), Some(legacy));
}

// -- determinism and authorization ----------------------------------------------

/// Repeated reads are stable and side-effect free, and the `try_` wrapper
/// agrees with the panicking one.
#[test]
fn get_fx_oracle_is_deterministic_and_side_effect_free() {
    let (env, _id, client, _admin, issuer, namespace, token, _payout) = setup();
    let oracle = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, REVENUE_SYMBOL);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &60,
    );
    let stored = stored_config(&client, &issuer, &namespace, &token);
    assert!(stored.is_some(), "the fixture config must be stored");

    for _ in 0..5 {
        assert_eq!(stored_config(&client, &issuer, &namespace, &token), stored);
    }
    assert!(client.try_get_fx_oracle(&issuer, &namespace, &token).is_ok());
}

/// The read path carries no authorization requirement at all: with no
/// authorizations available the config is still returned, and the read
/// consumes none.
#[test]
fn get_fx_oracle_requires_no_authorization() {
    let (env, _id, client, _admin, issuer, namespace, token, _payout) = setup();
    let oracle = Address::generate(&env);
    let revenue_symbol = Symbol::new(&env, REVENUE_SYMBOL);
    let payout_symbol = Symbol::new(&env, PAYOUT_SYMBOL);

    client.set_fx_oracle(
        &issuer,
        &namespace,
        &token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &60,
    );

    env.mock_auths(&[]);
    let config = stored_config(&client, &issuer, &namespace, &token);
    assert!(config.is_some());
    assert_eq!(config.unwrap().oracle, oracle);
    assert!(env.auths().is_empty(), "get_fx_oracle must not require or consume authorization");
}
