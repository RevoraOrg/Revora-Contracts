#![cfg(test)]
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, IntoVal, TryFromVal, Val,
};

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    (env, contract_id, issuer, token)
}

#[test]
fn it_emits_events_on_register_and_report() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.register_offering(&issuer, &token, &1_000); // 10% in bps
    client.report_revenue(&issuer, &token, &1_000_000, &1);

    assert!(env.events().all().len() >= 2);
}

#[test]
fn it_tracks_latest_period_and_allows_equal_period_reports() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.register_offering(&issuer, &token, &1_000);
    client.report_revenue(&issuer, &token, &1_000_000, &1);
    client.report_revenue(&issuer, &token, &2_000_000, &2);
    client.report_revenue(&issuer, &token, &3_000_000, &2);

    let latest = client.latest_accepted_period(&issuer, &token);
    assert_eq!(latest, Some(2));

    let events = env.events().all();
    assert!(events.len() >= 4);
}

#[test]
#[should_panic(expected = "backdated_period")]
fn it_rejects_backdated_periods() {
    let (_env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&_env, &contract_id);

    client.report_revenue(&issuer, &token, &1_000_000, &5);
    client.report_revenue(&issuer, &token, &1_000_000, &3);
}

#[test]
#[should_panic(expected = "period_closed")]
fn it_rejects_reports_for_closed_periods() {
    let (_env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&_env, &contract_id);

    client.report_revenue(&issuer, &token, &1_000_000, &5);
    client.close_period(&issuer, &token, &5);

    client.report_revenue(&issuer, &token, &2_000_000, &5);
}

#[test]
fn it_emits_period_closed_events_and_updates_state() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.report_revenue(&issuer, &token, &1_000_000, &1);
    client.report_revenue(&issuer, &token, &2_000_000, &2);
    client.close_period(&issuer, &token, &2);

    let latest = client.latest_accepted_period(&issuer, &token);
    let closed = client.closed_through_period(&issuer, &token);

    assert_eq!(latest, Some(2));
    assert_eq!(closed, Some(2));

    let events = env.events().all();
    assert!(!events.is_empty());
    let last = events.last().unwrap();
    let expected_data: Val = 2u64.into_val(&env);
    let last_period: u64 = u64::try_from_val(&env, &last.2).unwrap();
    let expected_period: u64 = u64::try_from_val(&env, &expected_data).unwrap();
    assert_eq!(last_period, expected_period);
}

#[test]
fn it_handles_large_period_ids_and_boundary_transitions() {
    let (_env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&_env, &contract_id);

    let large = u64::MAX - 1;

    client.report_revenue(&issuer, &token, &1_000_000, &large);
    client.report_revenue(&issuer, &token, &2_000_000, &u64::MAX);

    let latest = client.latest_accepted_period(&issuer, &token);
    assert_eq!(latest, Some(u64::MAX));
}

