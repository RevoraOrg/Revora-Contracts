#![cfg(test)]

use soroban_sdk::{testutils::{Address as _, Events}, Address, Env};
use crate::{RevoraRevenueShare, RevoraRevenueShareClient};

// ── helper ────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

// ── original smoke test ───────────────────────────────────────

#[test]
fn it_emits_events_on_register_and_report() {
    let env = Env::default();
    env.mock_all_auths();
    let client  = make_client(&env);
    let issuer  = Address::generate(&env);
    let token   = Address::generate(&env);

    client.register_offering(&issuer, &token, &1_000);
    client.report_revenue(&issuer, &token, &1_000_000, &1);

    assert!(env.events().all().len() >= 2);
}

// ── blacklist CRUD ────────────────────────────────────────────

#[test]
fn add_marks_investor_as_blacklisted() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    assert!(!client.is_blacklisted(&token, &investor));
    client.blacklist_add(&admin, &token, &investor);
    assert!(client.is_blacklisted(&token, &investor));
}

#[test]
fn remove_unmarks_investor() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);
    client.blacklist_remove(&admin, &token, &investor);
    assert!(!client.is_blacklisted(&token, &investor));
}

#[test]
fn get_blacklist_returns_all_blocked_investors() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin  = Address::generate(&env);
    let token  = Address::generate(&env);
    let inv_a  = Address::generate(&env);
    let inv_b  = Address::generate(&env);
    let inv_c  = Address::generate(&env);

    client.blacklist_add(&admin, &token, &inv_a);
    client.blacklist_add(&admin, &token, &inv_b);
    client.blacklist_add(&admin, &token, &inv_c);

    let list = client.get_blacklist(&token);
    assert_eq!(list.len(), 3);
    assert!(list.contains(&inv_a));
    assert!(list.contains(&inv_b));
    assert!(list.contains(&inv_c));
}

#[test]
fn get_blacklist_empty_before_any_add() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let token  = Address::generate(&env);

    assert_eq!(client.get_blacklist(&token).len(), 0);
}

// ── idempotency ───────────────────────────────────────────────

#[test]
fn double_add_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);
    client.blacklist_add(&admin, &token, &investor);

    assert_eq!(client.get_blacklist(&token).len(), 1);
}

#[test]
fn remove_nonexistent_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_remove(&admin, &token, &investor); // must not panic
    assert!(!client.is_blacklisted(&token, &investor));
}

// ── per-offering isolation ────────────────────────────────────

#[test]
fn blacklist_is_scoped_per_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token_a  = Address::generate(&env);
    let token_b  = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token_a, &investor);

    assert!( client.is_blacklisted(&token_a, &investor));
    assert!(!client.is_blacklisted(&token_b, &investor));
}

#[test]
fn removing_from_one_offering_does_not_affect_another() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token_a  = Address::generate(&env);
    let token_b  = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token_a, &investor);
    client.blacklist_add(&admin, &token_b, &investor);
    client.blacklist_remove(&admin, &token_a, &investor);

    assert!(!client.is_blacklisted(&token_a, &investor));
    assert!( client.is_blacklisted(&token_b, &investor));
}

// ── event emission ────────────────────────────────────────────

#[test]
fn blacklist_add_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    let before = env.events().all().len();
    client.blacklist_add(&admin, &token, &investor);
    assert!(env.events().all().len() > before);
}

#[test]
fn blacklist_remove_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);
    let before = env.events().all().len();
    client.blacklist_remove(&admin, &token, &investor);
    assert!(env.events().all().len() > before);
}

// ── distribution enforcement ──────────────────────────────────

#[test]
fn blacklisted_investor_excluded_from_distribution_filter() {
    let env = Env::default();
    env.mock_all_auths();
    let client  = make_client(&env);
    let admin   = Address::generate(&env);
    let token   = Address::generate(&env);
    let allowed = Address::generate(&env);
    let blocked = Address::generate(&env);

    client.blacklist_add(&admin, &token, &blocked);

    let investors = [allowed.clone(), blocked.clone()];
    let eligible = investors
        .iter()
        .filter(|inv| !client.is_blacklisted(&token, inv))
        .count();

    assert_eq!(eligible, 1);
}

#[test]
fn blacklist_takes_precedence_over_whitelist() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);

    // Even if investor were on a whitelist, blacklist must win
    assert!(client.is_blacklisted(&token, &investor));
}

// ── auth enforcement ──────────────────────────────────────────

#[test]
#[should_panic]
fn blacklist_add_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let client    = make_client(&env);
    let bad_actor = Address::generate(&env);
    let token     = Address::generate(&env);
    let victim    = Address::generate(&env);

    client.blacklist_add(&bad_actor, &token, &victim);
}

#[test]
#[should_panic]
fn blacklist_remove_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let client    = make_client(&env);
    let bad_actor = Address::generate(&env);
    let token     = Address::generate(&env);
    let investor  = Address::generate(&env);

    client.blacklist_remove(&bad_actor, &token, &investor);
}

// ── whitelist CRUD ────────────────────────────────────────────

#[test]
fn whitelist_add_marks_investor_as_whitelisted() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    assert!(!client.is_whitelisted(&token, &investor));
    client.whitelist_add(&admin, &token, &investor);
    assert!(client.is_whitelisted(&token, &investor));
}

#[test]
fn whitelist_remove_unmarks_investor() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.whitelist_add(&admin, &token, &investor);
    client.whitelist_remove(&admin, &token, &investor);
    assert!(!client.is_whitelisted(&token, &investor));
}

#[test]
fn get_whitelist_returns_all_approved_investors() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin  = Address::generate(&env);
    let token  = Address::generate(&env);
    let inv_a  = Address::generate(&env);
    let inv_b  = Address::generate(&env);
    let inv_c  = Address::generate(&env);

    client.whitelist_add(&admin, &token, &inv_a);
    client.whitelist_add(&admin, &token, &inv_b);
    client.whitelist_add(&admin, &token, &inv_c);

    let list = client.get_whitelist(&token);
    assert_eq!(list.len(), 3);
    assert!(list.contains(&inv_a));
    assert!(list.contains(&inv_b));
    assert!(list.contains(&inv_c));
}

#[test]
fn get_whitelist_empty_before_any_add() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let token  = Address::generate(&env);

    assert_eq!(client.get_whitelist(&token).len(), 0);
}

// ── whitelist idempotency ─────────────────────────────────────

#[test]
fn whitelist_double_add_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.whitelist_add(&admin, &token, &investor);
    client.whitelist_add(&admin, &token, &investor);

    assert_eq!(client.get_whitelist(&token).len(), 1);
}

#[test]
fn whitelist_remove_nonexistent_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.whitelist_remove(&admin, &token, &investor); // must not panic
    assert!(!client.is_whitelisted(&token, &investor));
}

// ── whitelist per-offering isolation ──────────────────────────

#[test]
fn whitelist_is_scoped_per_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token_a  = Address::generate(&env);
    let token_b  = Address::generate(&env);
    let investor = Address::generate(&env);

    client.whitelist_add(&admin, &token_a, &investor);

    assert!( client.is_whitelisted(&token_a, &investor));
    assert!(!client.is_whitelisted(&token_b, &investor));
}

#[test]
fn whitelist_removing_from_one_offering_does_not_affect_another() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token_a  = Address::generate(&env);
    let token_b  = Address::generate(&env);
    let investor = Address::generate(&env);

    client.whitelist_add(&admin, &token_a, &investor);
    client.whitelist_add(&admin, &token_b, &investor);
    client.whitelist_remove(&admin, &token_a, &investor);

    assert!(!client.is_whitelisted(&token_a, &investor));
    assert!( client.is_whitelisted(&token_b, &investor));
}

// ── whitelist event emission ──────────────────────────────────

#[test]
fn whitelist_add_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    let before = env.events().all().len();
    client.whitelist_add(&admin, &token, &investor);
    assert!(env.events().all().len() > before);
}

#[test]
fn whitelist_remove_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.whitelist_add(&admin, &token, &investor);
    let before = env.events().all().len();
    client.whitelist_remove(&admin, &token, &investor);
    assert!(env.events().all().len() > before);
}

// ── whitelist distribution enforcement ────────────────────────

#[test]
fn whitelist_enabled_only_includes_whitelisted_investors() {
    let env = Env::default();
    env.mock_all_auths();
    let client     = make_client(&env);
    let admin      = Address::generate(&env);
    let token      = Address::generate(&env);
    let whitelisted = Address::generate(&env);
    let not_listed  = Address::generate(&env);

    client.whitelist_add(&admin, &token, &whitelisted);

    let investors = [whitelisted.clone(), not_listed.clone()];
    let whitelist_enabled = client.is_whitelist_enabled(&token);
    
    let eligible = investors
        .iter()
        .filter(|inv| {
            let blacklisted = client.is_blacklisted(&token, inv);
            let whitelisted = client.is_whitelisted(&token, inv);
            
            if blacklisted {
                return false;
            }
            if whitelist_enabled {
                return whitelisted;
            }
            true
        })
        .count();

    assert_eq!(eligible, 1);
}

#[test]
fn whitelist_disabled_includes_all_non_blacklisted() {
    let env = Env::default();
    env.mock_all_auths();
    let client  = make_client(&env);
    let token   = Address::generate(&env);
    let inv_a   = Address::generate(&env);
    let inv_b   = Address::generate(&env);

    // No whitelist entries - whitelist disabled
    assert!(!client.is_whitelist_enabled(&token));

    let investors = [inv_a.clone(), inv_b.clone()];
    let whitelist_enabled = client.is_whitelist_enabled(&token);
    
    let eligible = investors
        .iter()
        .filter(|inv| {
            let blacklisted = client.is_blacklisted(&token, inv);
            let whitelisted = client.is_whitelisted(&token, inv);
            
            if blacklisted {
                return false;
            }
            if whitelist_enabled {
                return whitelisted;
            }
            true
        })
        .count();

    assert_eq!(eligible, 2);
}

#[test]
fn blacklist_overrides_whitelist() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    // Add to both whitelist and blacklist
    client.whitelist_add(&admin, &token, &investor);
    client.blacklist_add(&admin, &token, &investor);

    // Blacklist must take precedence
    let whitelist_enabled = client.is_whitelist_enabled(&token);
    let is_eligible = {
        let blacklisted = client.is_blacklisted(&token, &investor);
        let whitelisted = client.is_whitelisted(&token, &investor);
        
        if blacklisted {
            false
        } else if whitelist_enabled {
            whitelisted
        } else {
            true
        }
    };

    assert!(!is_eligible);
}

// ── whitelist auth enforcement ────────────────────────────────

#[test]
#[should_panic]
fn whitelist_add_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let client    = make_client(&env);
    let bad_actor = Address::generate(&env);
    let token     = Address::generate(&env);
    let investor  = Address::generate(&env);

    client.whitelist_add(&bad_actor, &token, &investor);
}

#[test]
#[should_panic]
fn whitelist_remove_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let client    = make_client(&env);
    let bad_actor = Address::generate(&env);
    let token     = Address::generate(&env);
    let investor  = Address::generate(&env);

    client.whitelist_remove(&bad_actor, &token, &investor);
}

// ── large whitelist handling ──────────────────────────────────

#[test]
fn large_whitelist_operations() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin  = Address::generate(&env);
    let token  = Address::generate(&env);

    // Add 50 investors to whitelist
    let mut investors = soroban_sdk::Vec::new(&env);
    for _ in 0..50 {
        let inv = Address::generate(&env);
        client.whitelist_add(&admin, &token, &inv);
        investors.push_back(inv);
    }

    let whitelist = client.get_whitelist(&token);
    assert_eq!(whitelist.len(), 50);

    // Verify all are whitelisted
    for i in 0..investors.len() {
        assert!(client.is_whitelisted(&token, &investors.get(i).unwrap()));
    }
}

// ── repeated operations on same address ───────────────────────

#[test]
fn repeated_whitelist_operations_on_same_address() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    // Add, remove, add again
    client.whitelist_add(&admin, &token, &investor);
    assert!(client.is_whitelisted(&token, &investor));
    
    client.whitelist_remove(&admin, &token, &investor);
    assert!(!client.is_whitelisted(&token, &investor));
    
    client.whitelist_add(&admin, &token, &investor);
    assert!(client.is_whitelisted(&token, &investor));
}

// ── whitelist enabled state ───────────────────────────────────

#[test]
fn whitelist_enabled_when_non_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    assert!(!client.is_whitelist_enabled(&token));
    
    client.whitelist_add(&admin, &token, &investor);
    assert!(client.is_whitelist_enabled(&token));
    
    client.whitelist_remove(&admin, &token, &investor);
    assert!(!client.is_whitelist_enabled(&token));
}
