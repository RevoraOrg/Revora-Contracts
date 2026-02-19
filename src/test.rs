#![cfg(test)]
use soroban_sdk::{testutils::Address as _, testutils::Events, Address, Env};

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

    // At least two events emitted (register + report)
    assert!(env.events().all().len() >= 2);
}

#[test]
fn pause_blocks_state_changing_calls() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // initialize admin (no safety)
    client.initialize(&admin, &None::<Address>);

    // pause as admin
    client.pause_admin(&admin);

    // register_offering should panic when paused
    // (split into a separate test with should_panic below)
    client.unpause_admin(&admin);
    client.register_offering(&issuer, &token, &1_000);
}

#[test]
#[should_panic]
fn register_offering_panics_when_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.initialize(&admin, &None::<Address>);
    client.pause_admin(&admin);
    client.register_offering(&issuer, &token, &1_000);
}

#[test]
#[should_panic]
fn report_revenue_panics_when_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.initialize(&admin, &None::<Address>);
    client.pause_admin(&admin);
    client.report_revenue(&issuer, &token, &1_000_000, &1);
}

#[test]
fn pause_toggle_emits_events_and_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>);

    // pause twice
    client.pause_admin(&admin);
    client.pause_admin(&admin); // idempotent: should not panic

    // unpause twice
    client.unpause_admin(&admin);
    client.unpause_admin(&admin);

    // expect pause/unpause events (>=2)
    assert!(env.events().all().len() >= 2);
}
