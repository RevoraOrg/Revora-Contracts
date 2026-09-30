//! Adversarial state and authorization coverage for `pause_admin`.

use soroban_sdk::{
    testutils::{Address as _, Events as _},
    Address, Env,
};

use revora_contracts::{PauseState, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &contract_id)
}

fn initialized(env: &Env) -> (RevoraRevenueShareClient<'_>, Address) {
    env.mock_all_auths();
    let client = make_client(env);
    let admin = Address::generate(env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    (client, admin)
}

#[test]
fn admin_soft_pauses_and_emits_both_compatibility_events() {
    let env = Env::default();
    let (client, admin) = initialized(&env);
    let events_before = env.events().all().len();

    client.pause_admin(&admin);

    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
    assert!(client.is_paused());
    assert_eq!(env.events().all().len(), events_before + 2);
}

#[test]
fn authenticated_non_admin_is_rejected_without_state_or_event_changes() {
    let env = Env::default();
    let (client, _admin) = initialized(&env);
    let attacker = Address::generate(&env);
    let events_before = env.events().all().len();

    assert_eq!(client.try_pause_admin(&attacker), Err(Ok(RevoraError::NotAuthorized)),);

    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());
    assert_eq!(env.events().all().len(), events_before);
}

#[test]
fn rejected_soft_pause_cannot_downgrade_an_existing_hard_pause() {
    let env = Env::default();
    let (client, admin) = initialized(&env);
    let attacker = Address::generate(&env);
    client.hard_pause_admin(&admin);
    let events_before = env.events().all().len();

    assert_eq!(client.try_pause_admin(&attacker), Err(Ok(RevoraError::NotAuthorized)),);

    assert_eq!(client.get_pause_state(), PauseState::HardPaused);
    assert!(client.is_paused());
    assert_eq!(env.events().all().len(), events_before);
}

#[test]
fn uninitialized_contract_rejects_pause_without_creating_state() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let caller = Address::generate(&env);
    let events_before = env.events().all().len();

    assert_eq!(client.try_pause_admin(&caller), Err(Ok(RevoraError::NotInitialized)),);

    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());
    assert_eq!(env.events().all().len(), events_before);
}

#[test]
fn repeated_admin_pause_is_state_idempotent_and_observable() {
    let env = Env::default();
    let (client, admin) = initialized(&env);
    let events_before = env.events().all().len();

    client.pause_admin(&admin);
    client.pause_admin(&admin);

    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
    assert_eq!(env.events().all().len(), events_before + 4);
}
