#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Symbol, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "def");
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    client.register_offering(&issuer, &namespace, &token, &5_000, &payout, &0);
    (env, client, issuer, namespace, token, payout)
}

#[test]
fn report_revenue_happy_path_persists_amount() {
    let (_env, client, issuer, ns, token, payout) = setup();
    client.report_revenue(&issuer, &ns, &token, &payout, &1_000, &1, &false);
    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 1_000);
}

#[test]
fn report_revenue_rejects_period_id_zero_and_leaves_state() {
    let (_env, client, issuer, ns, token, payout) = setup();
    let err = client
        .try_report_revenue(&issuer, &ns, &token, &payout, &1_000, &0, &false)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RevoraError::InvalidPeriodId);
    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &0), 0);
}

#[test]
fn report_revenue_rejects_negative_amount() {
    let (_env, client, issuer, ns, token, payout) = setup();
    let r = client.try_report_revenue(&issuer, &ns, &token, &payout, &-1, &1, &false);
    assert!(r.is_err());
    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 0);
}

#[test]
fn report_revenue_duplicate_without_override_leaves_original() {
    let (_env, client, issuer, ns, token, payout) = setup();
    client.report_revenue(&issuer, &ns, &token, &payout, &100, &1, &false);
    let r = client.try_report_revenue(&issuer, &ns, &token, &payout, &999, &1, &false);
    assert!(r.is_err());
    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 100);
}

#[test]
fn report_revenue_override_replaces_existing() {
    let (_env, client, issuer, ns, token, payout) = setup();
    client.report_revenue(&issuer, &ns, &token, &payout, &100, &1, &false);
    client.report_revenue(&issuer, &ns, &token, &payout, &250, &1, &true);
    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 250);
}
