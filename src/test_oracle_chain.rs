use super::*;
use crate::{OracleChain, OracleEntry, OracleChainClient};

// Note: this file is included from `src/lib.rs` via `#[cfg(test)] mod test_oracle_chain;`
using the crate root imports as needed.

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    let issuer = Address::generate(&emv);
    let token = Address::generate(&env);
    (env, issuer, token)
}

fn entry(token: Address, price: i128) -> OracleEntry {
    OracleEntry {
        token,
        price,
        timestamp: 0,
    }
}

#[crg(test)]
fn set_oracle_chain_success_stores_entries() {
    let (env, issuer, token) = setup();
    let client = OracleChainClient::new(&env);
    let namespace = Symbol::new(&env, "default");
    let entries = vec![entry(token.clone(), 100)];

    client.set_oracle_chain(
        &env,
        issuer.clone(),
        namespace.clone(),
        token.clone(),
        entries.clone(),
    );

    let stored = client.get_oracle_chain(&env, namespace.clone(), token.clone());
    assert_eq!(stored.len(), entries.len());
    assert_eq!(stored[0].price, 100);
    assert_eq!(stored[0].token, token);
}

#[test]
fn set_oracle_chain_replaces_existing_entries() {
    let (env, issuer, token) = setup();
    let client = OracleChainClient::new(&env);
    let namespace = Symbol::new(&env, "default");

    client.set_oracle_chain(
        &env,
        issuer.clone(),
        namespace.clone(),
        token.clone(),
        vec![entry(token.clone(), 100)],
    );
    client.set_oracle_chain(
        &env,
        issuer.clone(),
        namespace.clone(),
        token.clone(),
        vec![entry(token.clone(), 200), entry(token.clone(), 300)],
    );

    let stored = client.get_oracle_chain(&env, namespace.clone(), token.clone());
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].price, 200);
    assert_eq!(stored[1].price, 300);
}

#[test]
fn set_oracle_chain_empty_entries_is_rejected() {
    let (env, issuer, token) = setup();
    let client = OracleChainClient::new(&env);
    let namespace = Symbol::new(&env, "default");

    let result = client.try_set_oracle_chain(
        &env,
        issuer.clone(),
        namespace.clone(),
        token.clone(),
        Vec::new(),
    );
    assert!(result.is_error());

    // State must remain unchanged.
    let stored = client.get_oracle_chain(&env, namespace.clone(), token.clone());
    assert!(stored.is_empty());
}

#[test]
fn set_oracle_chain_unauthorized_caller_is_rejected() {
    let (env, issuer, token) = setup();
    let client = OracleChainClient::new(&env);
    let namespace = Symbol::new(&env, "default");
    let attacker = Address::generate(&env);

    // Establish baseline state with the authorized issuer.
    client.set_oracle_chain(
        &env,
        issuer.clone(),
        namespace.clone(),
        token.clone(),
        vec![entry(token.clone(), 100)],
    );

    // Attempt to overwrite with an unauthorized caller.
    // The contract must reject this either via a returned error or a panic.
    let result = client.try_set_oracle_chain(
        &env,
        attacker.clone(),
        namespace.clone(),
        token.clone(),
        vec![entry(token.clone(), 999)],
    );
    assert!(result.is_error());

    // State must be unchanged after the rejected operation.
    let stored = client.get_oracle_chain(&env, namespace.clone(), token.clone());
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].price, 100);
}

#[test]
fn set_oracle_chain_boundary_prices_are_stored() {
    let (env, issuer, token) = setup();
    let client = OracleChainClient::new(&env);
    let namespace = Symbol::new(&env, "default");

    let entries = vec![
        entry(token.clone(), i128::MIN),
        entry(token.clone(), 0),
        entry(token.clone(), i128::MAX),
    ];
    client.set_oracle_chain(
        &env,
        issuer.clone(),
        namespace.clone(),
        token.clone(),
        entries.clone(),
    );

    let stored = client.get_oracle_chain(&env, namespace.clone(), token.clone());
    assert_eq!(stored.len(), 3);
    assert_eq!(stored[0].price, i128::MIN);
    assert_eq!(stored[1].price, 0);
    assert_eq!(stored[2].price, i128::MAX);
}

#[test]
fn set_oracle_chain_namespaces_are_isolated() {
    let (env, issuer, token) = setup();
    let client = OracleChainClient::new(&env);
    let ns_a = Symbol::new(&env, "a");
    let ns_b = Symbol::new(&env, "b");

    client.set_oracle_chain(
        &env,
        issuer.clone(),
        ns_a.clone(),
        token.clone(),
        vec![entry(token.clone(), 111)],
    );
    client.set_oracle_chain(
        &env,
        issuer.clone(),
        ns_b.clone(),
        token.clone(),
        vec![entry(token.clone(), 222)],
    );

    let a = client.get_oracle_chain(&env, ns_a.clone(), token.clone());
    let b = client.get_oracle_chain(&env, ns_b.clone(), token.clone());
    assert_eq!(a[0].price, 111);
    assert_eq!(b[0].price, 222);
}
