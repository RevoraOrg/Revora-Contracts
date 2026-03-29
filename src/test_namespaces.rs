#![cfg(test)]

use crate::{AggregatedMetrics, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// @dev Verifies that registering the same token under different namespaces isolates their state.
#[test]
fn test_namespace_isolation() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);

    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token = Address::generate(&env); // Same token for both!
    let ns_1 = symbol_short!("ns1");
    let ns_2 = symbol_short!("ns2");

    // Issuer A registers in ns1
    client.register_offering(&issuer_a, &ns_1, &token, &1000, &token, &0);
    // Issuer B registers in ns2 with SAME token
    client.register_offering(&issuer_b, &ns_2, &token, &2000, &token, &0);

    // Set holder shares differently
    let holder = Address::generate(&env);
    client.set_holder_share(&issuer_a, &ns_1, &token, &holder, &500);
    client.set_holder_share(&issuer_b, &ns_2, &token, &holder, &1500);

    // Verify they are isolated
    assert_eq!(client.get_holder_share(&issuer_a, &ns_1, &token, &holder), 500);
    assert_eq!(client.get_holder_share(&issuer_b, &ns_2, &token, &holder), 1500);

    // We need to manage the token (mint some to the issuer)
    // Actually, in mock_all_auths, the transfer will succeed if we don't check balances?
    // No, soroban-sdk mock_all_auths doesn't mock balances.
    // But we are using the `token` Address directly. We should probably use a proper token client.

    // For simplicity in this isolation test, let's just check metadata/config which are simple set/get
    client.set_claim_delay(&issuer_a, &ns_1, &token, &3600);
    client.set_claim_delay(&issuer_b, &ns_2, &token, &7200);

    assert_eq!(client.get_claim_delay(&issuer_a, &ns_1, &token), 3600);
    assert_eq!(client.get_claim_delay(&issuer_b, &ns_2, &token), 7200);
}

/// @dev Verifies that a single issuer can register the same token in multiple namespaces isolated from each other.
#[test]
fn test_same_issuer_different_namespaces() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns_1 = symbol_short!("prod");
    let ns_2 = symbol_short!("stg");

    client.register_offering(&issuer, &ns_1, &token, &1000, &token, &0);
    client.register_offering(&issuer, &ns_2, &token, &2000, &token, &0);

    client.set_snapshot_config(&issuer, &ns_1, &token, &true);
    client.set_snapshot_config(&issuer, &ns_2, &token, &false);

    assert!(client.get_snapshot_config(&issuer, &ns_1, &token));
    assert!(!client.get_snapshot_config(&issuer, &ns_2, &token));
}

/// @dev Verifies that blacklisting an investor in one namespace does not affect their standing in another.
#[test]
fn test_cross_namespace_blacklist_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns_1 = symbol_short!("ns1");
    let ns_2 = symbol_short!("ns2");
    let investor = Address::generate(&env);

    client.register_offering(&issuer, &ns_1, &token, &1000, &token, &0);
    client.register_offering(&issuer, &ns_2, &token, &1000, &token, &0);

    // Blacklist in NS 1
    client.blacklist_add(&issuer, &issuer, &ns_1, &token, &investor);
    
    // Verify isolated
    assert!(client.is_blacklisted(&issuer, &ns_1, &token, &investor));
    assert!(!client.is_blacklisted(&issuer, &ns_2, &token, &investor));
    
    assert_eq!(client.get_blacklist(&issuer, &ns_1, &token).len(), 1);
    assert_eq!(client.get_blacklist(&issuer, &ns_2, &token).len(), 0);
}

/// @dev Verifies that attempting to access state of an unregistered namespace fails securely.
#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")] // OfferingNotFound
fn test_unregistered_namespace_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns_ghost = symbol_short!("ghost");

    // Attempt to set delay on non-existent offering
    client.set_claim_delay(&issuer, &ns_ghost, &token, &3600);
}

/// @dev Verifies that an issuer cannot access or modify offerings they do not own, even within the same namespace.
#[test]
fn test_unauthorized_issuer_access_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer_real = Address::generate(&env);
    let issuer_attacker = Address::generate(&env);
    let token = Address::generate(&env);
    let ns_1 = symbol_short!("ns1");

    client.register_offering(&issuer_real, &ns_1, &token, &1000, &token, &0);

    // Attacker tries to blacklist for real issuer's offering
    // Note: mock_all_auths will allow the call to reach the contract, 
    // but the contract should check that issuer_attacker is not current_issuer.
    
    let res = client.try_blacklist_add(&issuer_attacker, &issuer_real, &ns_1, &token, &Address::generate(&env));
    
    // Should fail with NotAuthorized (#10) or OfferingNotFound (if we strictly check issuer in ID)
    // Actually our implementation returns NotAuthorized if issuer matches but caller doesn't, 
    // but here the issuer_real in the ID matches the real one, but the caller is attacker.
    assert!(res.is_err());
}

/// @dev Verifies that transferring an offering ownership maintains namespace isolation while correctly updating authorization.
#[test]
fn test_transfer_maintains_namespace_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token_1 = Address::generate(&env);
    let ns_1 = symbol_short!("ns1");

    client.register_offering(&issuer_a, &ns_1, &token_1, &1000, &token_1, &0);
    client.set_claim_delay(&issuer_a, &ns_1, &token_1, &3600);

    // Transfer to Issuer B
    client.propose_issuer_transfer(&issuer_a, &ns_1, &token_1, &issuer_b);
    client.accept_issuer_transfer(&issuer_a, &ns_1, &token_1);

    // Verify config preserved
    assert_eq!(client.get_claim_delay(&issuer_a, &ns_1, &token_1), 3600);

    // Verify Issuer B now has control (e.g. can change delay)
    client.set_claim_delay(&issuer_b, &ns_1, &token_1, &7200);
    assert_eq!(client.get_claim_delay(&issuer_a, &ns_1, &token_1), 7200);

    // Verify Issuer A NO LONGER has control
    let res = client.try_set_claim_delay(&issuer_a, &ns_1, &token_1, &9999);
    assert!(res.is_err());
}


/// @dev Verifies that double-registration of the exact same (issuer, namespace, token) is rejected to prevent state clobbering.
#[test]
fn test_duplicate_registration_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns1");

    client.register_offering(&issuer, &ns, &token, &1000, &token, &0);
    
    // Exact same registration should fail
    let res = client.try_register_offering(&issuer, &ns, &token, &1000, &token, &0);
    assert!(res.is_err());
}

/// @dev Verifies that aggregated platform and issuer metrics correctly sum across namespace boundaries.
#[test]
fn test_aggregation_across_namespaces() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token1 = Address::generate(&env);
    let token2 = Address::generate(&env);
    let ns_1 = symbol_short!("prod");
    let ns_2 = symbol_short!("stg");

    client.register_offering(&issuer, &ns_1, &token1, &1000, &token1, &0);
    client.register_offering(&issuer, &ns_2, &token2, &1000, &token2, &0);
    
    // Report revenue in both namespaces
    client.report_revenue(&issuer, &ns_1, &token1, &token1, &50000, &1, &false);
    client.report_revenue(&issuer, &ns_2, &token2, &token2, &25000, &1, &false);

    let metrics = client.get_issuer_aggregation(&issuer);
    assert_eq!(metrics.total_reported_revenue, 75000);
    assert_eq!(metrics.offering_count, 2);
}
