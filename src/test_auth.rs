#![cfg(test)]
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, String as SdkString, Vec,
};

use crate::{RevoraRevenueShare, RevoraRevenueShareClient, RoundingMode};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn init_admin_safety(env: &Env, client: &RevoraRevenueShareClient) -> (Address, Address) {
    let admin = Address::generate(env);
    let safety = Address::generate(env);
    client.initialize(&admin, &Some(safety.clone()));
    (admin, safety)
}

fn setup_offering(env: &Env, client: &RevoraRevenueShareClient) -> (Address, Address) {
    env.mock_all_auths();
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    client.register_offering(&issuer, &token, &1_000, &token);
    (issuer, token)
}

#[test]
fn pause_admin_unauthorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (admin, _safety) = init_admin_safety(&env, &client);
    env.mock_all_auths();
    let attacker = Address::generate(&env);
    assert!(client.try_pause_admin(&attacker).is_err());
    assert!(!RevoraRevenueShareClient::is_paused(&env));
    client.pause_admin(&admin);
    assert!(RevoraRevenueShareClient::is_paused(&env));
}

#[test]
fn unpause_admin_unauthorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (admin, _safety) = init_admin_safety(&env, &client);
    env.mock_all_auths();
    client.pause_admin(&admin);
    let attacker = Address::generate(&env);
    assert!(client.try_unpause_admin(&attacker).is_err());
    assert!(RevoraRevenueShareClient::is_paused(&env));
    client.unpause_admin(&admin);
    assert!(!RevoraRevenueShareClient::is_paused(&env));
}

#[test]
fn pause_safety_unauthorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, safety) = init_admin_safety(&env, &client);
    env.mock_all_auths();
    let attacker = Address::generate(&env);
    assert!(client.try_pause_safety(&attacker).is_err());
    assert!(!RevoraRevenueShareClient::is_paused(&env));
    client.pause_safety(&safety);
    assert!(RevoraRevenueShareClient::is_paused(&env));
}

#[test]
fn unpause_safety_unauthorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, safety) = init_admin_safety(&env, &client);
    env.mock_all_auths();
    client.pause_safety(&safety);
    let attacker = Address::generate(&env);
    assert!(client.try_unpause_safety(&attacker).is_err());
    assert!(RevoraRevenueShareClient::is_paused(&env));
    client.unpause_safety(&safety);
    assert!(!RevoraRevenueShareClient::is_paused(&env));
}

#[test]
fn set_testnet_mode_missing_auth() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    assert!(client.try_set_testnet_mode(&true).is_err());
    assert!(!RevoraRevenueShareClient::is_testnet_mode(&env));
}

#[test]
fn set_platform_fee_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    assert!(client.try_set_platform_fee(&1_000).is_err());
    assert_eq!(RevoraRevenueShareClient::get_platform_fee(&env), 0);
}

#[test]
fn freeze_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    assert!(client.try_freeze().is_err());
    assert!(!RevoraRevenueShareClient::is_frozen(&env));
}

#[test]
fn set_admin_missing_auth() {
    let env = Env::default();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    assert!(client.try_set_admin(&admin).is_err());
    assert!(RevoraRevenueShareClient::get_admin(&env).is_none());
}

#[test]
fn set_admin_success() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    client.set_admin(&admin).unwrap();
    assert_eq!(RevoraRevenueShareClient::get_admin(&env), Some(admin));
}

#[test]
fn register_offering_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    assert!(client
        .try_register_offering(&issuer, &token, &1_000, &token)
        .is_err());
    assert_eq!(
        RevoraRevenueShareClient::get_offering_count(&env, &issuer),
        0
    );
}

#[test]
fn report_revenue_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    assert!(client
        .try_report_revenue(&attacker, &token, &token, &100, &1u64, &false)
        .is_err());
    assert!(RevoraRevenueShareClient::get_audit_summary(&env, &issuer, &token).is_none());
}

#[test]
fn deposit_revenue_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (_issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    let payment_token = Address::generate(&env);
    assert!(client
        .try_deposit_revenue(&attacker, &token, &payment_token, &100, &1u64)
        .is_err());
    assert_eq!(RevoraRevenueShareClient::get_period_count(&env, &token), 0);
}

#[test]
fn set_holder_share_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (_issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    let holder = Address::generate(&env);
    assert!(client
        .try_set_holder_share(&attacker, &token, &holder, &100u32)
        .is_err());
    assert_eq!(
        RevoraRevenueShareClient::get_holder_share(&env, &token, &holder),
        0
    );
}

#[test]
fn set_concentration_limit_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    assert!(client
        .try_set_concentration_limit(&attacker, &token, &1_000u32, &true)
        .is_err());
    assert!(RevoraRevenueShareClient::get_concentration_limit(&env, &issuer, &token).is_none());
}

#[test]
fn set_rounding_mode_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    assert!(client
        .try_set_rounding_mode(&attacker, &token, &RoundingMode::RoundHalfUp)
        .is_err());
    assert_eq!(
        RevoraRevenueShareClient::get_rounding_mode(&env, &issuer, &token),
        RoundingMode::Truncation
    );
}

#[test]
fn set_min_revenue_threshold_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    assert!(client
        .try_set_min_revenue_threshold(&attacker, &token, &123i128)
        .is_err());
    assert_eq!(
        RevoraRevenueShareClient::get_min_revenue_threshold(&env, &issuer, &token),
        0
    );
}

#[test]
fn set_claim_delay_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    assert!(client
        .try_set_claim_delay(&attacker, &token, &100u64)
        .is_err());
    assert_eq!(RevoraRevenueShareClient::get_claim_delay(&env, &token), 0);
}

#[test]
fn set_offering_metadata_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    let meta: SdkString = "m".into();
    assert!(client
        .try_set_offering_metadata(&attacker, &token, &meta)
        .is_err());
    assert!(RevoraRevenueShareClient::get_offering_metadata(&env, &issuer, &token).is_none());
}

#[test]
fn blacklist_add_wrong_caller_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (_issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    let investor = Address::generate(&env);
    assert!(client
        .try_blacklist_add(&attacker, &token, &investor)
        .is_err());
    assert!(!RevoraRevenueShareClient::is_blacklisted(
        &env, &token, &investor
    ));
    let bl: Vec<Address> = RevoraRevenueShareClient::get_blacklist(&env, &token);
    assert_eq!(bl.len(), 0);
}

#[test]
fn blacklist_remove_wrong_caller_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &token);
    client.blacklist_add(&issuer, &token, &investor).unwrap();
    let attacker = Address::generate(&env);
    assert!(client
        .try_blacklist_remove(&attacker, &token, &investor)
        .is_err());
    assert!(RevoraRevenueShareClient::is_blacklisted(
        &env, &token, &investor
    ));
}

#[test]
fn cross_offering_confusion_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let holder = Address::generate(&env);
    client.register_offering(&issuer_a, &token_a, &1_000, &token_a);
    client.register_offering(&issuer_b, &token_b, &1_000, &token_b);
    assert!(client
        .try_set_holder_share(&issuer_b, &token_a, &holder, &1_000u32)
        .is_err());
    assert_eq!(
        RevoraRevenueShareClient::get_holder_share(&env, &token_a, &holder),
        0
    );
}

#[test]
fn claim_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let holder = Address::generate(&env);
    let token = Address::generate(&env);
    assert!(client.try_claim(&holder, &token, &0u32).is_err());
}
