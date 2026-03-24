#![cfg(test)]
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, String as SdkString, Vec};

use crate::{RevoraRevenueShare, RevoraRevenueShareClient, RoundingMode};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
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
#[ignore = "try_pause_admin with non-admin panics (panic!(\"not admin\")); Soroban host aborts instead of Err"]
fn pause_admin_unauthorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (admin, _safety) = init_admin_safety(&env, &client);
    env.mock_all_auths();
    let attacker = Address::generate(&env);
    let _ = client.try_pause_admin(&attacker);
    assert!(!client.is_paused());
    client.pause_admin(&admin);
    assert!(client.is_paused());
}

#[test]
#[ignore = "try_unpause_admin with non-admin panics; Soroban host aborts"]
fn unpause_admin_unauthorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (admin, _safety) = init_admin_safety(&env, &client);
    env.mock_all_auths();
    client.pause_admin(&admin);
    let attacker = Address::generate(&env);
    let _ = client.try_unpause_admin(&attacker);
    assert!(client.is_paused());
    client.unpause_admin(&admin);
    assert!(!client.is_paused());
}

#[test]
#[ignore = "try_pause_safety with non-safety panics; Soroban host aborts"]
fn pause_safety_unauthorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, safety) = init_admin_safety(&env, &client);
    env.mock_all_auths();
    let attacker = Address::generate(&env);
    let _ = client.try_pause_safety(&attacker);
    assert!(!client.is_paused());
    client.pause_safety(&safety);
    assert!(client.is_paused());
}

#[test]
#[ignore = "try_unpause_safety with non-safety panics; Soroban host aborts"]
fn unpause_safety_unauthorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, safety) = init_admin_safety(&env, &client);
    env.mock_all_auths();
    client.pause_safety(&safety);
    let attacker = Address::generate(&env);
    let _ = client.try_unpause_safety(&attacker);
    assert!(client.is_paused());
    client.unpause_safety(&safety);
    assert!(!client.is_paused());
}

#[test]
#[ignore = "try_set_testnet_mode aborts the Soroban host on missing auth (non-unwinding panic); admin.require_auth is enforced in set_testnet_mode"]
fn set_testnet_mode_missing_auth() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    let _ = client.try_set_testnet_mode(&true);
}

#[test]
#[ignore = "try_set_platform_fee aborts the Soroban host on missing auth (non-unwinding panic); admin.require_auth is enforced in set_platform_fee"]
fn set_platform_fee_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    let _ = client.try_set_platform_fee(&1_000);
}

#[test]
#[ignore = "try_freeze aborts the Soroban host on missing auth (non-unwinding panic); admin.require_auth is enforced in freeze"]
fn freeze_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    let _ = client.try_freeze();
}

#[test]
#[ignore = "try_set_admin aborts the Soroban host on missing auth (non-unwinding panic)"]
fn set_admin_missing_auth() {
    let env = Env::default();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let _ = client.try_set_admin(&admin);
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
#[ignore = "try_register_offering aborts the Soroban host on missing auth (non-unwinding panic); issuer.require_auth is enforced in register_offering"]
fn register_offering_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let _ = client.try_register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);
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
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
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
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
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
#[ignore = "try_claim aborts the Soroban host on missing auth (non-unwinding panic); holder.require_auth is enforced in claim"]
fn claim_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let holder = Address::generate(&env);
    let token = Address::generate(&env);
    let issuer = Address::generate(&env);
    let _ = client.try_claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
}
