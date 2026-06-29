use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, IntoVal,
};

use crate::vesting::{RevoraVesting, RevoraVestingClient, VESTING_EVENT_SCHEMA_VERSION};

fn setup(env: &Env) -> (RevoraVestingClient, Address, Address, Address) {
    let contract_id = env.register_contract(None, RevoraVesting);
    let client = RevoraVestingClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let beneficiary = Address::generate(env);
    let token_id = crate::test_utils::create_token(env, &admin);
    (client, admin, beneficiary, token_id)
}

fn mint_tokens(env: &Env, payment_token: &Address, recipient: &Address, amount: &i128) {
    soroban_sdk::token::StellarAssetClient::new(env, payment_token).mint(recipient, amount);
}

fn balance(env: &Env, payment_token: &Address, who: &Address) -> i128 {
    soroban_sdk::token::Client::new(env, payment_token).balance(who)
}

fn has_event_symbol(env: &Env, symbol: soroban_sdk::Symbol) -> bool {
    let symbol_val = symbol.into_val(env);
    env.events().all().iter().any(|event| event.1.contains(symbol_val))
}

#[test]
fn initialize_sets_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _b, _t) = setup(&env);
    client.initialize_vesting(&admin);
}

#[test]
fn create_schedule_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    let total = 1_000_000_i128;
    let start = 1000_u64;
    let cliff = 500_u64;
    let duration = 2000_u64;

    let idx =
        client.create_schedule(&admin, &beneficiary, &token_id, &total, &start, &cliff, &duration);
    assert_eq!(idx, 0);

    let schedule = client.get_schedule(&admin, &0);
    assert_eq!(schedule.beneficiary, beneficiary);
    assert_eq!(schedule.total_amount, total);
    assert_eq!(schedule.claimed_amount, 0);
    assert_eq!(schedule.start_time, start);
    assert_eq!(schedule.cliff_time, start + cliff);
    assert_eq!(schedule.end_time, start + duration);
    assert!(!schedule.cancelled);
}

#[test]
fn get_claimable_before_cliff_is_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    let total = 1_000_000_i128;
    let start = 1000_u64;
    let cliff = 500_u64;
    let duration = 2000_u64;
    client.create_schedule(&admin, &beneficiary, &token_id, &total, &start, &cliff, &duration);

    crate::test_utils::set_timestamp(&env, start + 100);
    let claimable = client.get_claimable_vesting(&admin, &0);
    assert_eq!(claimable, 0);
}

#[test]
fn cancel_schedule() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    client.create_schedule(&admin, &beneficiary, &token_id, &1_000_000, &1000, &100, &2000);

    client.cancel_schedule(&admin, &beneficiary, &0);
    let schedule = client.get_schedule(&admin, &0);
    assert!(schedule.cancelled);
}

#[test]
fn multiple_schedules_same_beneficiary() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    client.create_schedule(&admin, &beneficiary, &token_id, &100, &1000, &0, &1000);
    client.create_schedule(&admin, &beneficiary, &token_id, &200, &2000, &0, &1000);
    assert_eq!(client.get_schedule_count(&admin), 2);
}

#[test]
fn zero_duration_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    let r = client.try_create_schedule(&admin, &beneficiary, &token_id, &1000, &1000, &0, &0);
    assert!(r.is_err());
}

#[test]
fn cliff_longer_than_duration_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    let r = client.try_create_schedule(&admin, &beneficiary, &token_id, &1000, &1000, &2000, &1000);
    assert!(r.is_err());
}

#[test]
fn negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    let r = client.try_create_schedule(&admin, &beneficiary, &token_id, &0, &1000, &0, &1000);
    assert!(r.is_err());
    let r2 = client.try_create_schedule(&admin, &beneficiary, &token_id, &-10, &1000, &0, &1000);
    assert!(r2.is_err());
}

#[test]
fn double_initialize_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _b, _t) = setup(&env);
    client.initialize_vesting(&admin);
    let r = client.try_initialize_vesting(&admin);
    assert!(r.is_err());
}

#[test]
fn test_claim_vesting_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    // Mint tokens to the contract
    crate::test_utils::mint_tokens(&env, &token_id, &client.address, 1000);

    let start = 1000;
    client.create_schedule(&admin, &beneficiary, &token_id, &1000, &start, &0, &1000);

    crate::test_utils::set_timestamp(&env, 1500);
    let claimed = client.claim_vesting(&beneficiary, &admin, &0);
    assert_eq!(claimed, 500);

    crate::test_utils::set_timestamp(&env, 2500);
    let claimed2 = client.claim_vesting(&beneficiary, &admin, &0);
    assert_eq!(claimed2, 500);

    let r = client.try_claim_vesting(&beneficiary, &admin, &0);
    assert!(r.is_err());
}

#[test]
fn cancel_schedule_already_cancelled() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    client.create_schedule(&admin, &beneficiary, &token_id, &1000, &1000, &100, &2000);

    client.cancel_schedule(&admin, &beneficiary, &0);
    let r = client.try_cancel_schedule(&admin, &beneficiary, &0);
    assert!(r.is_err());
}

#[test]
fn try_cancel_schedule_wrong_beneficiary() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    let wrong_beneficiary = Address::generate(&env);
    client.initialize_vesting(&admin);
    client.create_schedule(&admin, &beneficiary, &token_id, &1000, &1000, &100, &2000);

    let r = client.try_cancel_schedule(&admin, &wrong_beneficiary, &0);
    assert!(r.is_err());
}

#[test]
fn amend_schedule_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    let start = 1000;
    client.create_schedule(&admin, &beneficiary, &token_id, &1000, &start, &0, &1000);

    // Amend: Increase total amount and double duration
    client.amend_schedule(&admin, &beneficiary, &0, &2000, &start, &0, &2000);

    let schedule = client.get_schedule(&admin, &0);
    assert_eq!(schedule.total_amount, 2000);
    assert_eq!(schedule.end_time, start + 2000);
}

#[test]
fn amend_schedule_partially_claimed_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    // Mint tokens to the contract
    crate::test_utils::mint_tokens(&env, &token_id, &client.address, 5000);

    let start = 1000;
    client.create_schedule(&admin, &beneficiary, &token_id, &1000, &start, &0, &1000);

    // Claim 500 at t=1500
    crate::test_utils::set_timestamp(&env, 1500);
    client.claim_vesting(&beneficiary, &admin, &0);

    // Amend: Reduce total to 800 (still > 500 claimed)
    client.amend_schedule(&admin, &beneficiary, &0, &800, &start, &0, &1000);

    let schedule = client.get_schedule(&admin, &0);
    assert_eq!(schedule.total_amount, 800);
    assert_eq!(schedule.claimed_amount, 500);
}

#[test]
fn amend_schedule_too_low_amount_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    crate::test_utils::mint_tokens(&env, &token_id, &client.address, 1000);

    client.create_schedule(&admin, &beneficiary, &token_id, &1000, &1000, &0, &1000);

    crate::test_utils::set_timestamp(&env, 1500);
    client.claim_vesting(&beneficiary, &admin, &0); // claimed 500

    // Try to reduce total to 400 (claimed is 500)
    let r = client.try_amend_schedule(&admin, &beneficiary, &0, &400, &1000, &0, &1000);
    assert!(r.is_err());
}

#[test]
fn amend_schedule_invalid_params_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    client.create_schedule(&admin, &beneficiary, &token_id, &1000, &1000, &0, &1000);

    // Zero duration
    let r = client.try_amend_schedule(&admin, &beneficiary, &0, &1000, &1000, &0, &0);
    assert!(r.is_err());

    // Cliff > Duration
    let r2 = client.try_amend_schedule(&admin, &beneficiary, &0, &1000, &1000, &2000, &1000);
    assert!(r2.is_err());
}

#[test]
fn amend_cancelled_schedule_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    client.create_schedule(&admin, &beneficiary, &token_id, &1000, &1000, &0, &1000);

    client.cancel_schedule(&admin, &beneficiary, &0);

    let r = client.try_amend_schedule(&admin, &beneficiary, &0, &2000, &1000, &0, &1000);
    assert!(r.is_err());
}

#[test]
fn amend_non_existent_schedule_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, _token_id) = setup(&env);
    client.initialize_vesting(&admin);

    let r = client.try_amend_schedule(&admin, &beneficiary, &99, &1000, &1000, &0, &1000);
    assert!(r.is_err());
}

#[test]
fn partial_claim_cursor_advances_and_full_claim_keeps_history_append_only() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_id);
    token_client.mint(&client.address, &1000);

    let start = 1000;
    client.create_schedule(&admin, &beneficiary, &token_id, &1000, &start, &0, &1000);

    env.ledger().with_mut(|l| l.timestamp = 1500);
    let first_claim = client.claim_vesting_partial(&beneficiary, &admin, &0, &200);
    assert_eq!(first_claim, 200);
    assert_eq!(client.get_partial_claim_count(&admin, &0), 1);
    assert_eq!(client.get_partial_claim_record(&admin, &0, &0), Some((1500, 200)));
    assert_eq!(balance(&env, &token_id, &beneficiary), 200);

    env.ledger().with_mut(|l| l.timestamp = 2000);
    let second_claim = client.claim_vesting_partial(&beneficiary, &admin, &0, &300);
    assert_eq!(second_claim, 300);
    assert_eq!(client.get_partial_claim_count(&admin, &0), 2);
    assert_eq!(client.get_partial_claim_record(&admin, &0, &1), Some((2000, 300)));
    assert_eq!(client.get_partial_claim_record(&admin, &0, &0), Some((1500, 200)));
    assert_eq!(client.get_claimable_vesting(&admin, &0), 500);

    let schedule = client.get_schedule(&admin, &0);
    assert_eq!(schedule.claimed_amount, 500);

    let full_claim = client.claim_vesting(&beneficiary, &admin, &0);
    assert_eq!(full_claim, 500);
    assert_eq!(balance(&env, &token_id, &beneficiary), 1000);
    assert_eq!(client.get_partial_claim_count(&admin, &0), 2);
    assert_eq!(client.get_partial_claim_record(&admin, &0, &0), Some((1500, 200)));
    assert_eq!(client.get_partial_claim_record(&admin, &0, &1), Some((2000, 300)));
    assert_eq!(client.get_claimable_vesting(&admin, &0), 0);
}

#[test]
fn partial_claim_rejects_invalid_amounts_and_before_cliff() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_id);
    token_client.mint(&client.address, &1000);

    let start = 1000;
    client.create_schedule(&admin, &beneficiary, &token_id, &1000, &start, &250, &1000);

    let zero_amount = client.try_claim_vesting_partial(&beneficiary, &admin, &0, &0);
    assert!(zero_amount.is_err());
    assert_eq!(client.get_partial_claim_count(&admin, &0), 0);

    env.ledger().with_mut(|l| l.timestamp = 1100);
    let before_cliff = client.try_claim_vesting_partial(&beneficiary, &admin, &0, &50);
    assert!(before_cliff.is_err());
    assert_eq!(client.get_partial_claim_count(&admin, &0), 0);

    env.ledger().with_mut(|l| l.timestamp = 1500);
    let claimable = client.get_claimable_vesting(&admin, &0);
    assert_eq!(claimable, 500);

    let too_large = client.try_claim_vesting_partial(&beneficiary, &admin, &0, &600);
    assert!(too_large.is_err());
    assert_eq!(client.get_partial_claim_count(&admin, &0), 0);
}

#[test]
fn vesting_event_schema_version_is_stable_and_partial_claim_emits_v1_events() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    assert_eq!(client.get_event_schema_version(), VESTING_EVENT_SCHEMA_VERSION);

    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_id);
    token_client.mint(&client.address, &1000);

    let start = 1000;
    client.create_schedule(&admin, &beneficiary, &token_id, &1000, &start, &0, &1000);

    env.ledger().with_mut(|l| l.timestamp = 1500);
    let before = env.events().all().len();
    let claimed = client.claim_vesting_partial(&beneficiary, &admin, &0, &250);
    assert_eq!(claimed, 250);
    assert!(env.events().all().len() >= before + 2);
    assert!(has_event_symbol(&env, symbol_short!("vest_pcl")));
    assert!(has_event_symbol(&env, symbol_short!("vst_pcl1")));
}
