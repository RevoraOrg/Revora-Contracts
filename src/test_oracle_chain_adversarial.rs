//! Adversarial coverage for `RevoraRevenueShare::set_oracle_chain` (Issue #1058).
//!
//! The existing `oracle_chain_tests` module proves the happy path (a chain is
//! stored, a stale entry falls back, the chain outranks the legacy single
//! oracle). This fixture targets the edges that decide whether an admin can
//! *corrupt* the price feed configuration:
//!
//! * the bound is exact — `MAX_ORACLE_CHAIN_LEN` entries are accepted, one more
//!   is rejected with `LimitReached`;
//! * a rejected write is atomic — it does not truncate, append to, or otherwise
//!   mutate the chain that was already stored;
//! * unknown offerings are rejected with `OfferingNotFound` and store nothing;
//! * an empty vector is a *clear*, not a no-op, and `get_oracle_chain` then
//!   returns `None`;
//! * a second `set_oracle_chain` replaces the previous chain instead of
//!   appending to it;
//! * entry order and payloads (`oracle`, symbols, `max_age_secs`) round-trip
//!   verbatim, including the `0` and `u64::MAX` sentinels for the staleness
//!   check;
//! * duplicate oracle addresses are stored as-is (no silent de-duplication);
//! * chains are isolated per `(issuer, namespace, token)`.

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol, Vec};

/// Mirror of the contract's `MAX_ORACLE_CHAIN_LEN`. Kept as a literal so this
/// fixture fails loudly (rather than silently passing) if the bound changes.
const CHAIN_MAX: u32 = 10;
const BPS_50_PCT: u32 = 5_000;

fn new_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn deploy(env: &Env) -> RevoraRevenueShareClient<'static> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn register(
    env: &Env,
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) {
    let payout_asset = Address::generate(env);
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1,
        namespace,
        token,
        &BPS_50_PCT,
        &payout_asset,
        &0,
        &Symbol::new(env, "USD"),
        &0,
    );
}

fn entry(env: &Env, oracle: &Address, max_age_secs: u64) -> OracleEntry {
    OracleEntry {
        oracle: oracle.clone(),
        revenue_symbol: Symbol::new(env, "EUR"),
        payout_symbol: Symbol::new(env, "USD"),
        max_age_secs,
    }
}

fn chain(env: &Env, oracles: &[Address], max_age_secs: u64) -> Vec<OracleEntry> {
    let mut entries = Vec::new(env);
    for oracle in oracles {
        entries.push_back(entry(env, oracle, max_age_secs));
    }
    entries
}

fn distinct_oracles(env: &Env, count: u32) -> Vec<Address> {
    let mut oracles = Vec::new(env);
    for _ in 0..count {
        oracles.push_back(Address::generate(env));
    }
    oracles
}

fn setup(env: &Env) -> (RevoraRevenueShareClient<'static>, Address, Symbol, Address) {
    let client = deploy(env);
    let issuer = Address::generate(env);
    let ns = Symbol::new(env, "ns");
    let token = Address::generate(env);
    register(env, &client, &issuer, &ns, &token);
    (client, issuer, ns, token)
}

#[test]
fn exactly_max_entries_are_accepted() {
    let env = new_env();
    let (client, issuer, ns, token) = setup(&env);
    let oracles = distinct_oracles(&env, CHAIN_MAX);
    let entries = chain(&env, &oracles.slice(..), 60);

    client.set_oracle_chain(&issuer, &ns, &token, &entries);

    let stored = client.get_oracle_chain(&issuer, &ns, &token).unwrap();
    assert_eq!(stored.len(), CHAIN_MAX);
}

#[test]
fn one_entry_over_max_is_rejected() {
    let env = new_env();
    let (client, issuer, ns, token) = setup(&env);
    let oracles = distinct_oracles(&env, CHAIN_MAX + 1);
    let entries = chain(&env, &oracles.slice(..), 60);

    let result = client.try_set_oracle_chain(&issuer, &ns, &token, &entries);

    assert_eq!(result, Err(Ok(RevoraError::LimitReached)));
    assert!(client.get_oracle_chain(&issuer, &ns, &token).is_none());
}

#[test]
fn rejected_over_limit_write_leaves_the_stored_chain_untouched() {
    let env = new_env();
    let (client, issuer, ns, token) = setup(&env);
    let kept_oracle = Address::generate(&env);
    let kept = chain(&env, &[kept_oracle.clone()], 60);
    client.set_oracle_chain(&issuer, &ns, &token, &kept);

    let oracles = distinct_oracles(&env, CHAIN_MAX + 1);
    let too_many = chain(&env, &oracles.slice(..), 60);
    assert_eq!(
        client.try_set_oracle_chain(&issuer, &ns, &token, &too_many),
        Err(Ok(RevoraError::LimitReached))
    );

    let stored = client.get_oracle_chain(&issuer, &ns, &token).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored.get(0).unwrap().oracle, kept_oracle);
    assert_eq!(stored.get(0).unwrap().max_age_secs, 60);
}

#[test]
fn empty_chain_clears_a_previous_configuration() {
    let env = new_env();
    let (client, issuer, ns, token) = setup(&env);
    let oracles = distinct_oracles(&env, 2);
    client.set_oracle_chain(&issuer, &ns, &token, &chain(&env, &oracles.slice(..), 60));
    assert!(client.get_oracle_chain(&issuer, &ns, &token).is_some());

    let empty: Vec<OracleEntry> = Vec::new(&env);
    client.set_oracle_chain(&issuer, &ns, &token, &empty);

    assert!(client.get_oracle_chain(&issuer, &ns, &token).is_none());
}

#[test]
fn unknown_offering_is_rejected_and_stores_nothing() {
    let env = new_env();
    let client = deploy(&env);
    let issuer = Address::generate(&env);
    let ns = Symbol::new(&env, "ns");
    let token = Address::generate(&env);
    let oracle = Address::generate(&env);

    let result = client.try_set_oracle_chain(&issuer, &ns, &token, &chain(&env, &[oracle], 60));

    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    assert!(client.get_oracle_chain(&issuer, &ns, &token).is_none());
}

#[test]
fn an_unregistered_issuer_cannot_install_a_chain() {
    let env = new_env();
    let client = deploy(&env);
    let ns = Symbol::new(&env, "ns");
    let token = Address::generate(&env);
    let owner = Address::generate(&env);
    register(&env, &client, &owner, &ns, &token);

    let intruder = Address::generate(&env);
    let oracle = Address::generate(&env);
    let result = client.try_set_oracle_chain(&intruder, &ns, &token, &chain(&env, &[oracle], 60));

    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    // The owner's offering still has no chain.
    assert!(client.get_oracle_chain(&owner, &ns, &token).is_none());
}

#[test]
fn second_write_replaces_rather_than_appends() {
    let env = new_env();
    let (client, issuer, ns, token) = setup(&env);
    let first = distinct_oracles(&env, 3);
    let second = distinct_oracles(&env, 2);

    client.set_oracle_chain(&issuer, &ns, &token, &chain(&env, &first.slice(..), 60));
    client.set_oracle_chain(&issuer, &ns, &token, &chain(&env, &second.slice(..), 120));

    let stored = client.get_oracle_chain(&issuer, &ns, &token).unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored.get(0).unwrap().oracle, second.get(0).unwrap());
    assert_eq!(stored.get(1).unwrap().oracle, second.get(1).unwrap());
    assert_eq!(stored.get(0).unwrap().max_age_secs, 120);
}

#[test]
fn entry_order_and_payloads_round_trip_verbatim() {
    let env = new_env();
    let (client, issuer, ns, token) = setup(&env);
    let oracles = distinct_oracles(&env, 4);

    let mut entries = Vec::new(&env);
    for (i, oracle) in oracles.iter().enumerate() {
        entries.push_back(OracleEntry {
            oracle: oracle.clone(),
            revenue_symbol: Symbol::new(&env, "EUR"),
            payout_symbol: Symbol::new(&env, "USD"),
            max_age_secs: 10 + i as u64,
        });
    }
    client.set_oracle_chain(&issuer, &ns, &token, &entries);

    let stored = client.get_oracle_chain(&issuer, &ns, &token).unwrap();
    assert_eq!(stored.len(), entries.len());
    for i in 0..entries.len() {
        assert_eq!(stored.get(i).unwrap(), entries.get(i).unwrap());
    }
}

#[test]
fn boundary_max_age_sentinels_are_preserved() {
    let env = new_env();
    let (client, issuer, ns, token) = setup(&env);
    let disabled = Address::generate(&env);
    let far_future = Address::generate(&env);

    let mut entries = Vec::new(&env);
    entries.push_back(entry(&env, &disabled, 0));
    entries.push_back(entry(&env, &far_future, u64::MAX));
    client.set_oracle_chain(&issuer, &ns, &token, &entries);

    let stored = client.get_oracle_chain(&issuer, &ns, &token).unwrap();
    assert_eq!(stored.get(0).unwrap().max_age_secs, 0);
    assert_eq!(stored.get(1).unwrap().max_age_secs, u64::MAX);
}

#[test]
fn duplicate_oracle_entries_are_not_de_duplicated() {
    let env = new_env();
    let (client, issuer, ns, token) = setup(&env);
    let oracle = Address::generate(&env);

    let entries = chain(&env, &[oracle.clone(), oracle.clone(), oracle.clone()], 60);
    client.set_oracle_chain(&issuer, &ns, &token, &entries);

    let stored = client.get_oracle_chain(&issuer, &ns, &token).unwrap();
    assert_eq!(stored.len(), 3);
    assert_eq!(stored.get(0).unwrap().oracle, oracle);
    assert_eq!(stored.get(2).unwrap().oracle, oracle);
}

#[test]
fn chains_are_isolated_per_token_and_namespace() {
    let env = new_env();
    let client = deploy(&env);
    let issuer = Address::generate(&env);
    let ns_a = Symbol::new(&env, "aaa");
    let ns_b = Symbol::new(&env, "bbb");
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    register(&env, &client, &issuer, &ns_a, &token_a);
    register(&env, &client, &issuer, &ns_a, &token_b);
    register(&env, &client, &issuer, &ns_b, &token_a);

    let oracle_a = Address::generate(&env);
    let oracle_b = Address::generate(&env);
    let oracle_c = Address::generate(&env);
    client.set_oracle_chain(
        &issuer,
        &ns_a,
        &token_a,
        &chain(&env, &[oracle_a.clone()], 60),
    );
    client.set_oracle_chain(
        &issuer,
        &ns_a,
        &token_b,
        &chain(&env, &[oracle_b.clone()], 60),
    );
    client.set_oracle_chain(
        &issuer,
        &ns_b,
        &token_a,
        &chain(&env, &[oracle_c.clone()], 60),
    );

    assert_eq!(
        client
            .get_oracle_chain(&issuer, &ns_a, &token_a)
            .unwrap()
            .get(0)
            .unwrap()
            .oracle,
        oracle_a
    );
    assert_eq!(
        client
            .get_oracle_chain(&issuer, &ns_a, &token_b)
            .unwrap()
            .get(0)
            .unwrap()
            .oracle,
        oracle_b
    );
    assert_eq!(
        client
            .get_oracle_chain(&issuer, &ns_b, &token_a)
            .unwrap()
            .get(0)
            .unwrap()
            .oracle,
        oracle_c
    );
}
