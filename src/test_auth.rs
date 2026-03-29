#![cfg(test)]
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, String as SdkString, Vec};

use crate::{RevoraRevenueShare, RevoraRevenueShareClient, RoundingMode};

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn init_admin_safety(env: &Env, client: &RevoraRevenueShareClient) -> (Address, Address) {
    let admin = Address::generate(env);
    let safety = Address::generate(env);
    client.initialize(&admin, &Some(safety.clone()), &None::<bool>);
    (admin, safety)
}

fn setup_offering(env: &Env, client: &RevoraRevenueShareClient) -> (Address, Address) {
    env.mock_all_auths();
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);
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
    assert!(!client.is_paused());
    client.pause_admin(&admin);
    assert!(client.is_paused());
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
    assert!(client.is_paused());
    client.unpause_admin(&admin);
    assert!(!client.is_paused());
}

#[test]
fn pause_safety_unauthorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, safety) = init_admin_safety(&env, &client);
    env.mock_all_auths();
    let attacker = Address::generate(&env);
    assert!(client.try_pause_safety(&attacker).is_err());
    assert!(!client.is_paused());
    client.pause_safety(&safety);
    assert!(client.is_paused());
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
    assert!(client.is_paused());
    client.unpause_safety(&safety);
    assert!(!client.is_paused());
}

#[test]
fn set_testnet_mode_missing_auth() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    assert!(client.try_set_testnet_mode(&true).is_err());
    assert!(!client.is_testnet_mode());
}

#[test]
fn set_platform_fee_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    assert!(client.try_set_platform_fee(&1_000).is_err());
    assert_eq!(client.get_platform_fee(), 0);
}

#[test]
fn freeze_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    assert!(client.try_freeze().is_err());
    assert!(!client.is_frozen());
}

#[test]
fn freeze_offering_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    let (issuer, token) = setup_offering(&env, &client);

    assert!(client
        .try_freeze_offering(&Address::generate(&env), &issuer, &symbol_short!("def"), &token)
        .is_err());
    assert!(!client.is_offering_frozen(&issuer, &symbol_short!("def"), &token));
}

#[test]
fn unfreeze_offering_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (admin, _safety) = init_admin_safety(&env, &client);
    let (issuer, token) = setup_offering(&env, &client);

    client.freeze_offering(&issuer, &issuer, &symbol_short!("def"), &token);
    assert!(client.is_offering_frozen(&issuer, &symbol_short!("def"), &token));

    let attacker = Address::generate(&env);
    assert!(client
        .try_unfreeze_offering(&attacker, &issuer, &symbol_short!("def"), &token)
        .is_err());
    assert!(client.is_offering_frozen(&issuer, &symbol_short!("def"), &token));

    client.unfreeze_offering(&admin, &issuer, &symbol_short!("def"), &token);
    assert!(!client.is_offering_frozen(&issuer, &symbol_short!("def"), &token));
}

#[test]
fn set_admin_missing_auth() {
    let env = Env::default();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    assert!(client.try_set_admin(&admin).is_err());
    assert!(client.get_admin().is_none());
}

#[test]
fn set_admin_success() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    assert_eq!(client.get_admin(), Some(admin));
}

#[test]
fn register_offering_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    assert!(client
        .try_register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0)
        .is_err());
    assert_eq!(client.get_offering_count(&issuer, &symbol_short!("def")), 0);
    assert_eq!(client.get_payment_token(&issuer, &symbol_short!("def"), &token), None);
}

#[test]
fn report_revenue_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    assert!(client
        .try_report_revenue(&attacker, &symbol_short!("def"), &token, &token, &100, &1u64, &false)
        .is_err());
    assert!(client.get_audit_summary(&issuer, &symbol_short!("def"), &token).is_none());
}

#[test]
fn deposit_revenue_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    let payment_token = Address::generate(&env);
    assert!(client
        .try_deposit_revenue(&attacker, &symbol_short!("def"), &token, &payment_token, &100, &1u64)
        .is_err());
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 0);
}

#[test]
fn set_holder_share_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    let holder = Address::generate(&env);
    assert!(client
        .try_set_holder_share(&attacker, &symbol_short!("def"), &token, &holder, &100u32)
        .is_err());
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder), 0);
}

#[test]
fn set_concentration_limit_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    assert!(client
        .try_set_concentration_limit(&attacker, &symbol_short!("def"), &token, &1_000u32, &true)
        .is_err());
    assert!(client.get_concentration_limit(&issuer, &symbol_short!("def"), &token).is_none());
}

#[test]
fn set_rounding_mode_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    assert!(client
        .try_set_rounding_mode(&attacker, &symbol_short!("def"), &token, &RoundingMode::RoundHalfUp)
        .is_err());
    assert_eq!(
        client.get_rounding_mode(&issuer, &symbol_short!("def"), &token),
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
        .try_set_min_revenue_threshold(&attacker, &symbol_short!("def"), &token, &123i128)
        .is_err());
    assert_eq!(client.get_min_revenue_threshold(&issuer, &symbol_short!("def"), &token), 0);
}

#[test]
fn set_claim_delay_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    assert!(client.try_set_claim_delay(&attacker, &symbol_short!("def"), &token, &100u64).is_err());
    assert_eq!(client.get_claim_delay(&issuer, &symbol_short!("def"), &token), 0);
}

#[test]
fn set_offering_metadata_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    let meta: SdkString = SdkString::from_str(&env, "m");
    assert!(client
        .try_set_offering_metadata(&attacker, &symbol_short!("def"), &token, &meta)
        .is_err());
    assert!(client.get_offering_metadata(&issuer, &symbol_short!("def"), &token).is_none());
}

#[test]
fn blacklist_add_wrong_caller_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    let investor = Address::generate(&env);
    assert!(client
        .try_blacklist_add(&attacker, &issuer, &symbol_short!("def"), &token, &investor)
        .is_err());
    assert!(!client.is_blacklisted(&issuer, &symbol_short!("def"), &token, &investor));
    let bl: Vec<Address> = client.get_blacklist(&issuer, &symbol_short!("def"), &token);
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
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);
    client.blacklist_add(&issuer, &issuer, &symbol_short!("def"), &token, &investor);
    let attacker = Address::generate(&env);
    assert!(client
        .try_blacklist_remove(&attacker, &issuer, &symbol_short!("def"), &token, &investor)
        .is_err());
    assert!(client.is_blacklisted(&issuer, &symbol_short!("def"), &token, &investor));
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
    client.register_offering(&issuer_a, &symbol_short!("def"), &token_a, &1_000, &token_a, &0);
    client.register_offering(&issuer_b, &symbol_short!("def"), &token_b, &1_000, &token_b, &0);
    assert!(client
        .try_set_holder_share(&issuer_b, &symbol_short!("def"), &token_a, &holder, &1_000u32)
        .is_err());
    assert_eq!(client.get_holder_share(&issuer_a, &symbol_short!("def"), &token_a, &holder), 0);
}

#[test]
fn claim_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let holder = Address::generate(&env);
    let token = Address::generate(&env);
    let issuer = Address::generate(&env);
    assert!(client.try_claim(&holder, &issuer, &symbol_short!("def"), &token, &0).is_err());
}
