#![cfg(test)]
extern crate std;
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env,
};

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};

#[test]
fn it_emits_events_on_register_and_report() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &1_000); // 10% in bps
    client.report_revenue(&issuer, &token, &1_000_000, &1);

    // In a real test, inspect events / state here.
    assert!(env.events().all().len() >= 2);
}

#[test]
fn enforces_monotonic_period_ordering() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &500);

    // First report establishes latest period = 10.
    client.report_revenue(&issuer, &token, &100, &10);
    assert_eq!(client.latest_period(&issuer, &token), 10);

    // Reporting the same period is allowed (non-decreasing).
    client.report_revenue(&issuer, &token, &200, &10);
    assert_eq!(client.latest_period(&issuer, &token), 10);

    // Back-dated report will panic (covered in a dedicated #[should_panic] test).
    assert_eq!(client.latest_period(&issuer, &token), 10);

    // Forward jump is allowed and updates latest.
    client.report_revenue(&issuer, &token, &400, &11);
    assert_eq!(client.latest_period(&issuer, &token), 11);
}

#[test]
fn closing_a_period_rejects_further_reports() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &1000);
    client.report_revenue(&issuer, &token, &1_000, &1);
    assert_eq!(client.latest_period(&issuer, &token), 1);

    let before = env.events().all().len();
    client.close_period(&issuer, &token, &1);
    assert!(client.is_period_closed(&issuer, &token, &1));
    assert!(env.events().all().len() > before);

    // Another report for the closed period will panic (covered in a dedicated #[should_panic] test).
}

#[test]
fn handles_large_period_values_and_boundaries() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &250);

    let max_minus_one: u64 = u64::MAX - 1;
    client.report_revenue(&issuer, &token, &10, &max_minus_one);
    assert_eq!(client.latest_period(&issuer, &token), max_minus_one);

    client.report_revenue(&issuer, &token, &20, &u64::MAX);
    assert_eq!(client.latest_period(&issuer, &token), u64::MAX);

    // Attempt to go back to MAX-1 will panic (covered in a dedicated #[should_panic] test).
}

#[test]
#[should_panic]
fn backdated_report_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &500);
    client.report_revenue(&issuer, &token, &100, &10);
    // This should panic due to out-of-order period.
    client.report_revenue(&issuer, &token, &300, &9);
}

#[test]
#[should_panic]
fn closed_period_report_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &1000);
    client.report_revenue(&issuer, &token, &1_000, &1);
    client.close_period(&issuer, &token, &1);
    // This should panic because period 1 is closed.
    client.report_revenue(&issuer, &token, &2_000, &1);
}

#[test]
#[should_panic]
fn large_period_backward_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &250);

    let max_minus_one: u64 = u64::MAX - 1;
    client.report_revenue(&issuer, &token, &10, &max_minus_one);
    client.report_revenue(&issuer, &token, &20, &u64::MAX);
    // Going back should panic.
    client.report_revenue(&issuer, &token, &30, &max_minus_one);
}
