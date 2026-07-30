//! # Tests for multi-issuer per-action permission matrix (Issue #544)
//!
//! Covers the role-based access control system:
//!
//! | Test Case                              | Description |
//! |----------------------------------------|-------------|
//! | `grant_role_succeeds`                  | Primary issuer can grant a role |
//! | `grant_role_idempotent`                | Granting same role twice is a no-op |
//! | `revoke_role_succeeds`                 | Primary issuer can revoke a role |
//! | `revoke_nonexistent_role_noop`         | Revoking non-existent role is a no-op |
//! | `non_issuer_cannot_grant_role`         | Non-issuer cannot grant roles |
//! | `compliance_role_required_for_blacklist` | Blacklist requires Compliance role when grants exist |
//! | `treasury_role_required_for_deposit`   | Deposit requires Treasury role when grants exist |
//! | `role_check_noop_when_no_grants`       | Role check is skipped when no grants exist |
//! | `events_emitted`                       | Events emitted on grant and revoke |
//! | `cross_offering_isolation`             | Role grants are scoped per offering |

#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Val, Vec,
};

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, Role, RoleGrant};

// ── Shared helpers ────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn setup_offering(env: &Env) -> (RevoraRevenueShareClient<'_>, Address, Address) {
    env.mock_all_auths();
    let client = make_client(env);
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let ns = symbol_short!("def");
    let payout = Address::generate(env);
    client.register_offering(&issuer, &Vec::new(env), &1u32, &ns, &token, &1_000, &payout, &0);
    (client, issuer, token)
}

// ── Grant role ─────────────────────────────────────────────────────────────────

#[test]
fn grant_role_succeeds() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let addr = Address::generate(&env);

    client.grant_role(&issuer, &symbol_short!("def"), &token, &Role::Compliance, &addr);
}

#[test]
fn grant_role_idempotent() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let addr = Address::generate(&env);

    // Grant twice — should not error
    client.grant_role(&issuer, &symbol_short!("def"), &token, &Role::Treasury, &addr);
    client.grant_role(&issuer, &symbol_short!("def"), &token, &Role::Treasury, &addr);
}

#[test]
fn non_issuer_cannot_grant_role() {
    let env = Env::default();
    let (client, _issuer, token) = setup_offering(&env);
    let fake_issuer = Address::generate(&env);
    let addr = Address::generate(&env);

    let result = client.try_grant_role(
        &fake_issuer,
        &symbol_short!("def"),
        &token,
        &Role::Compliance,
        &addr,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

// ── Revoke role ───────────────────────────────────────────────────────────────

#[test]
fn revoke_role_succeeds() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let addr = Address::generate(&env);

    client.grant_role(&issuer, &symbol_short!("def"), &token, &Role::Operations, &addr);
    client.revoke_role(&issuer, &symbol_short!("def"), &token, &Role::Operations, &addr);
}

#[test]
fn revoke_nonexistent_role_noop() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let addr = Address::generate(&env);

    // Revoke without grant — should succeed (no-op)
    client.revoke_role(&issuer, &symbol_short!("def"), &token, &Role::Compliance, &addr);
}

// ── Role enforcement ──────────────────────────────────────────────────────────

#[test]
fn compliance_role_required_for_blacklist() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let compliance_officer = Address::generate(&env);
    let non_compliance = Address::generate(&env);
    let investor = Address::generate(&env);

    // Grant Compliance role to compliance_officer
    client.grant_role(
        &issuer,
        &symbol_short!("def"),
        &token,
        &Role::Compliance,
        &compliance_officer,
    );

    // Compliance officer can blacklist
    client.blacklist_add(&compliance_officer, &issuer, &symbol_short!("def"), &token, &investor);

    // Non-compliance address cannot blacklist when grants exist
    let result = client.try_blacklist_add(
        &non_compliance,
        &issuer,
        &symbol_short!("def"),
        &token,
        &investor,
    );
    assert_eq!(result, Err(Ok(RevoraError::RoleNotGranted)));
}

#[test]
fn treasury_role_required_for_deposit() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let treasury_agent = Address::generate(&env);
    let non_treasury = Address::generate(&env);

    // Grant Treasury role to treasury_agent only
    client.grant_role(&issuer, &symbol_short!("def"), &token, &Role::Treasury, &treasury_agent);

    // Non-treasury caller should be rejected with RoleNotGranted
    let payment_token = Address::generate(&env);
    let result = client.try_deposit_revenue(
        &non_treasury,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_i128,
        &1u64,
    );
    // When grants exist, non-Treasury holders are rejected
    // (The exact error depends on whether other checks fire first,
    // but RoleNotGranted should be the primary rejection)
    assert_eq!(
        result,
        Err(Ok(RevoraError::RoleNotGranted)),
        "Non-treasury role holder must be rejected with RoleNotGranted"
    );
}

#[test]
fn role_check_noop_when_no_grants() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let investor = Address::generate(&env);
    let caller = Address::generate(&env);

    // No role grants exist — blacklist should work normally for any authorized issuer
    // Non-issuer should still be rejected by existing auth checks
    let result =
        client.try_blacklist_add(&caller, &issuer, &symbol_short!("def"), &token, &investor);
    // Should fail with NotAuthorized (caller is not the issuer), not RoleNotGranted
    assert_eq!(result, Err(Ok(RevoraError::NotAuthorized)));
}

// ── Event emission ────────────────────────────────────────────────────────────

#[test]
fn events_emitted_on_grant_and_revoke() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let addr = Address::generate(&env);

    let before = env.events().all().len();

    client.grant_role(&issuer, &symbol_short!("def"), &token, &Role::Compliance, &addr);
    client.revoke_role(&issuer, &symbol_short!("def"), &token, &Role::Compliance, &addr);

    let events = env.events().all();
    let role_grt_sym = symbol_short!("role_grt");
    let role_rvk_sym = symbol_short!("role_rvk");

    let mut found_grant = false;
    let mut found_revoke = false;

    for i in before..events.len() {
        let (_, topics, data) = events.get(i).unwrap();
        let topics_vec: soroban_sdk::Vec<Val> = topics.clone().into_val(&env);
        let topic_sym: soroban_sdk::Symbol = topics_vec.get(0).unwrap().into_val(&env);

        if topic_sym == role_grt_sym {
            let data_vec: soroban_sdk::Vec<Val> = data.clone().into_val(&env);
            let ev_role: Role = data_vec.get(0).unwrap().into_val(&env);
            let ev_addr: Address = data_vec.get(1).unwrap().into_val(&env);
            assert_eq!(ev_role, Role::Compliance);
            assert_eq!(ev_addr, addr);
            found_grant = true;
        }

        if topic_sym == role_rvk_sym {
            let data_vec: soroban_sdk::Vec<Val> = data.clone().into_val(&env);
            let ev_role: Role = data_vec.get(0).unwrap().into_val(&env);
            let ev_addr: Address = data_vec.get(1).unwrap().into_val(&env);
            assert_eq!(ev_role, Role::Compliance);
            assert_eq!(ev_addr, addr);
            found_revoke = true;
        }
    }

    assert!(found_grant, "role_grt event must be emitted");
    assert!(found_revoke, "role_rvk event must be emitted");
}

// ── Cross-offering isolation ──────────────────────────────────────────────────

#[test]
fn cross_offering_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(env);

    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout = Address::generate(&env);
    let ns = symbol_short!("def");
    let addr = Address::generate(&env);
    let investor = Address::generate(&env);

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token_a, &1_000, &payout, &0);
    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token_b, &1_000, &payout, &0);

    // Grant Compliance on offering A only
    client.grant_role(&issuer, &ns, &token_a, &Role::Compliance, &addr);

    // addr can blacklist on offering A
    client.blacklist_add(&addr, &issuer, &ns, &token_a, &investor);

    // addr cannot blacklist on offering B (grants are scoped per-offering)
    let result = client.try_blacklist_add(&addr, &issuer, &ns, &token_b, &Address::generate(&env));
    assert_eq!(result, Err(Ok(RevoraError::RoleNotGranted)));
}
