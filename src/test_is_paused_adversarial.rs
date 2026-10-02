#![cfg(test)]

use crate::{PauseState, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    (env, contract, admin)
}

#[test]
fn returns_false_for_fresh_and_initialized_contracts() {
    let env = Env::default();
    let contract = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract);
    assert!(!client.is_paused());

    let (env, contract, _admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract);
    assert!(!client.is_paused());
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
}

#[test]
fn returns_true_for_soft_and_hard_pause_and_false_after_unpause() {
    let (env, contract, admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract);

    client.pause_admin(&admin);
    assert!(client.is_paused());
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);

    client.unpause_admin(&admin);
    assert!(!client.is_paused());
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);

    client.hard_pause_admin(&admin);
    assert!(client.is_paused());
    assert_eq!(client.get_pause_state(), PauseState::HardPaused);
    client.unpause_admin(&admin);
    assert!(!client.is_paused());
}

#[test]
fn repeated_queries_are_read_only_and_do_not_change_pause_state() {
    let (env, contract, admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract);
    client.pause_admin(&admin);
    assert!(client.is_paused());
    assert!(client.is_paused());
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
}
