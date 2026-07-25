
#![cfg(test)]
extern crate std;

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, Symbol,
};

fn setup_test() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address, Symbol) {
    let env = Env::default();
    env.mock_all_auths();

    let client = RevoraRevenueShareClient::new(&env, &env.register_contract(None, RevoraRevenueShare));

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let namespace = symbol_short!("ns");

    client.initialize(&admin, &None, &None);
    client.register_offering(&issuer, &namespace, &token, &10_000, &payout_asset, &0);

    (env, client, admin, issuer, token, namespace)
}

#[test]
fn test_set_and_get_restriction() {
    let (env, client, admin, ..) = setup_test();
    let category = symbol_short!("reg_d");

    client.set_transfer_restriction(&admin, &category, &99);
    assert_eq!(client.get_transfer_restriction(&category), 99);
}

#[test]
fn test_set_and_get_holder_category() {
    let (env, client, admin, ..) = setup_test();
    let holder = Address::generate(&env);
    let category = symbol_short!("accred");

    client.set_holder_category(&admin, &holder, &category);
    assert_eq!(client.get_holder_category(&holder), Some(category));
}

#[test]
fn test_cap_is_enforced() {
    let (env, client, admin, issuer, token, namespace) = setup_test();
    let category = symbol_short!("reg_s");
    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);

    client.set_transfer_restriction(&admin, &category, &1);
    client.set_holder_category(&admin, &holder1, &category);
    client.set_holder_category(&admin, &holder2, &category);

    // First holder should succeed
    let res1 = client.try_set_holder_share(&issuer, &namespace, &token, &holder1, &100);
    assert_eq!(res1, Ok(Ok(())));
    assert_eq!(client.get_category_holder_count(&category), 1);

    // Second holder should fail
    let res2 = client.try_set_holder_share(&issuer, &namespace, &token, &holder2, &100);
    assert_eq!(res2, Err(Ok(RevoraError::CategoryCapReached)));
    assert_eq!(client.get_category_holder_count(&category), 1);
}

#[test]
fn test_counter_decrements_on_zero_share() {
    let (env, client, admin, issuer, token, namespace) = setup_test();
    let category = symbol_short!("reg_d");
    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);

    client.set_transfer_restriction(&admin, &category, &1);
    client.set_holder_category(&admin, &holder1, &category);
    client.set_holder_category(&admin, &holder2, &category);

    // Add holder1, count should be 1
    client.set_holder_share(&issuer, &namespace, &token, &holder1, &100);
    assert_eq!(client.get_category_holder_count(&category), 1);

    // Remove holder1, count should be 0
    client.set_holder_share(&issuer, &namespace, &token, &holder1, &0);
    assert_eq!(client.get_category_holder_count(&category), 0);

    // Add holder2, should succeed, count should be 1
    client.set_holder_share(&issuer, &namespace, &token, &holder2, &100);
    assert_eq!(client.get_category_holder_count(&category), 1);
}

#[test]
fn test_holder_share_oscillation() {
    let (env, client, admin, issuer, token, namespace) = setup_test();
    let category = symbol_short!("qualified");
    let holder = Address::generate(&env);

    client.set_transfer_restriction(&admin, &category, &1);
    client.set_holder_category(&admin, &holder, &category);

    // 0 -> 100: count becomes 1
    client.set_holder_share(&issuer, &namespace, &token, &holder, &100);
    assert_eq!(client.get_category_holder_count(&category), 1);

    // 100 -> 200: count remains 1
    client.set_holder_share(&issuer, &namespace, &token, &holder, &200);
    assert_eq!(client.get_category_holder_count(&category), 1);

    // 200 -> 0: count becomes 0
    client.set_holder_share(&issuer, &namespace, &token, &holder, &0);
    assert_eq!(client.get_category_holder_count(&category), 0);

    // 0 -> 50: count becomes 1 again
    client.set_holder_share(&issuer, &namespace, &token, &holder, &50);
    assert_eq!(client.get_category_holder_count(&category), 1);
}

#[test]
fn test_multiple_offerings_single_holder() {
    let (env, client, admin, issuer, token1, namespace) = setup_test();
    let category = symbol_short!("multi_off");
    let holder = Address::generate(&env);

    let token2 = Address::generate(&env);
    let payout_asset2 = Address::generate(&env);
    client.register_offering(&issuer, &namespace, &token2, &10_000, &payout_asset2, &0);

    client.set_transfer_restriction(&admin, &category, &1);
    client.set_holder_category(&admin, &holder, &category);

    // Add to offering 1: count becomes 1
    client.set_holder_share(&issuer, &namespace, &token1, &holder, &100);
    assert_eq!(client.get_category_holder_count(&category), 1);

    // Add to offering 2: count remains 1
    client.set_holder_share(&issuer, &namespace, &token2, &holder, &100);
    assert_eq!(client.get_category_holder_count(&category), 1);

    // Remove from offering 1: count remains 1
    client.set_holder_share(&issuer, &namespace, &token1, &holder, &0);
    assert_eq!(client.get_category_holder_count(&category), 1);

    // Remove from offering 2: count becomes 0
    client.set_holder_share(&issuer, &namespace, &token2, &holder, &0);
    assert_eq!(client.get_category_holder_count(&category), 0);
}

#[test]
fn test_unrestricted_category_holder() {
    let (env, client, admin, issuer, token, namespace) = setup_test();
    let restricted_cat = symbol_short!("restrict");
    let unrestricted_cat = symbol_short!("unrestrict");
    let holder = Address::generate(&env);

    client.set_transfer_restriction(&admin, &restricted_cat, &0); // 0 means no limit set
    client.set_holder_category(&admin, &holder, &unrestricted_cat);

    // Should succeed as holder is not in a restricted category
    client.set_holder_share(&issuer, &namespace, &token, &holder, &100);
    assert_eq!(client.get_category_holder_count(&restricted_cat), 0);
    assert_eq!(client.get_category_holder_count(&unrestricted_cat), 0); // Not tracked
}

#[test]
fn test_holder_with_no_category() {
    let (env, client, admin, issuer, token, namespace) = setup_test();
    let category = symbol_short!("reg_d");
    let holder = Address::generate(&env);

    client.set_transfer_restriction(&admin, &category, &1);

    // Holder has no category, should succeed
    client.set_holder_share(&issuer, &namespace, &token, &holder, &100);
    assert_eq!(client.get_category_holder_count(&category), 0);
}

#[test]
fn test_setting_share_to_zero_for_uncategorized_holder() {
    let (env, client, admin, issuer, token, namespace) = setup_test();
    let holder = Address::generate(&env);

    // Give a share, then remove it. Should not panic or error.
    client.set_holder_share(&issuer, &namespace, &token, &holder, &100);
    client.set_holder_share(&issuer, &namespace, &token, &holder, &0);

    // Check that no counters were affected
    let any_category = symbol_short!("any");
    assert_eq!(client.get_category_holder_count(&any_category), 0);
}
