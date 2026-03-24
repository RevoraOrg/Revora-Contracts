use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};

use crate::vesting::{RevoraVesting, RevoraVestingClient};

fn setup(env: &Env) -> (RevoraVestingClient<'_>, Address, Address, Address) {
    let contract_id = env.register_contract(None, RevoraVesting);
    let client = RevoraVestingClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let beneficiary = Address::generate(env);
    let token_id = env.register_stellar_asset_contract(admin.clone());
    (client, admin, beneficiary, token_id)
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

    env.ledger().with_mut(|l| l.timestamp = start + 100);
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

/// **Issue #171:** Cliff boundary — no vesting at timestamps strictly before `cliff_time`;
/// at exactly `cliff_time`, vested is still 0; linear uses `elapsed = now - cliff_time`.
#[test]
fn claimable_at_exact_cliff_timestamp_is_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    let total = 1_000_000_i128;
    let start = 1_000_u64;
    let cliff = 500_u64;
    let duration = 2_000_u64;
    client.create_schedule(&admin, &beneficiary, &token_id, &total, &start, &cliff, &duration);

    env.ledger().with_mut(|l| l.timestamp = start + cliff);
    assert_eq!(client.get_claimable_vesting(&admin, &0), 0);
}

#[test]
fn claimable_one_second_after_cliff_matches_linear_slice() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    let total = 1_000_000_i128;
    let start = 1_000_u64;
    let cliff = 500_u64;
    let duration = 2_000_u64;
    client.create_schedule(&admin, &beneficiary, &token_id, &total, &start, &cliff, &duration);

    env.ledger().with_mut(|l| l.timestamp = start + cliff + 1);
    let vesting_secs = duration - cliff;
    let expected = (total as u128).saturating_mul(1).checked_div(vesting_secs as u128).unwrap() as i128;
    assert_eq!(client.get_claimable_vesting(&admin, &0), expected);
}

#[test]
fn claimable_last_second_before_end_one_step_below_full() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    let total = 1_000_000_i128;
    let start = 1_000_u64;
    let cliff = 500_u64;
    let duration = 2_000_u64;
    client.create_schedule(&admin, &beneficiary, &token_id, &total, &start, &cliff, &duration);

    let end = start + duration;
    env.ledger().with_mut(|l| l.timestamp = end - 1);
    let vesting_secs = duration - cliff;
    let elapsed = (end - 1) - (start + cliff);
    let expected = (total as u128).saturating_mul(elapsed as u128).checked_div(vesting_secs as u128).unwrap() as i128;
    assert_eq!(client.get_claimable_vesting(&admin, &0), expected);
    assert!(expected < total);
}

#[test]
fn cliff_equals_duration_unlocks_full_amount_only_at_end() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    let total = 10_000_i128;
    let start = 100_u64;
    let cliff = 1_000_u64;
    let duration = 1_000_u64;
    client.create_schedule(&admin, &beneficiary, &token_id, &total, &start, &cliff, &duration);

    let cliff_time = start + cliff;
    let end = start + duration;
    assert_eq!(cliff_time, end);

    env.ledger().with_mut(|l| l.timestamp = end - 1);
    assert_eq!(client.get_claimable_vesting(&admin, &0), 0);

    env.ledger().with_mut(|l| l.timestamp = end);
    assert_eq!(client.get_claimable_vesting(&admin, &0), total);
}
