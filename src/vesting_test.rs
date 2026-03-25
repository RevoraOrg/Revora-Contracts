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

// ── list_schedules_page ────────────────────────────────────────────────────

#[test]
fn list_schedules_page_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _b, _t) = setup(&env);
    client.initialize_vesting(&admin);

    let (page, cursor) = client.list_schedules_page(&admin, &0, &10);
    assert_eq!(page.len(), 0);
    assert!(cursor.is_none());
}

#[test]
fn list_schedules_page_single() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    client.create_schedule(&admin, &beneficiary, &token_id, &500, &1000, &0, &1000);

    let (page, cursor) = client.list_schedules_page(&admin, &0, &10);
    assert_eq!(page.len(), 1);
    assert_eq!(page.get(0).unwrap().total_amount, 500);
    assert!(cursor.is_none());
}

#[test]
fn list_schedules_page_multiple_returns_cursor() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    for _ in 0..5u32 {
        client.create_schedule(&admin, &beneficiary, &token_id, &100, &1000, &0, &1000);
    }

    // first page: 3 items, cursor points at index 3
    let (page1, cursor1) = client.list_schedules_page(&admin, &0, &3);
    assert_eq!(page1.len(), 3);
    assert_eq!(cursor1, Some(3));

    // second page: 2 items, no further cursor
    let (page2, cursor2) = client.list_schedules_page(&admin, &3, &3);
    assert_eq!(page2.len(), 2);
    assert!(cursor2.is_none());
}

#[test]
fn list_schedules_page_cursor_advances_correctly() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    for i in 0..4u32 {
        client.create_schedule(
            &admin,
            &beneficiary,
            &token_id,
            &((i as i128 + 1) * 100),
            &1000,
            &0,
            &1000,
        );
    }

    let (page1, cursor1) = client.list_schedules_page(&admin, &0, &2);
    assert_eq!(page1.len(), 2);
    let next = cursor1.unwrap();

    let (page2, cursor2) = client.list_schedules_page(&admin, &next, &2);
    assert_eq!(page2.len(), 2);
    assert!(cursor2.is_none());

    // amounts are in insertion order
    assert_eq!(page1.get(0).unwrap().total_amount, 100);
    assert_eq!(page1.get(1).unwrap().total_amount, 200);
    assert_eq!(page2.get(0).unwrap().total_amount, 300);
    assert_eq!(page2.get(1).unwrap().total_amount, 400);
}

#[test]
fn list_schedules_page_zero_limit_uses_max() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    for _ in 0..3u32 {
        client.create_schedule(&admin, &beneficiary, &token_id, &100, &1000, &0, &1000);
    }

    // limit=0 should be clamped to MAX_SCHEDULES_PAGE, returning all 3
    let (page, cursor) = client.list_schedules_page(&admin, &0, &0);
    assert_eq!(page.len(), 3);
    assert!(cursor.is_none());
}

// ── get_beneficiary_schedule_count ────────────────────────────────────────

#[test]
fn get_beneficiary_schedule_count_zero_on_fresh_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, _t) = setup(&env);
    client.initialize_vesting(&admin);

    assert_eq!(client.get_beneficiary_schedule_count(&admin, &beneficiary), 0);
}

#[test]
fn get_beneficiary_schedule_count_increments_on_create() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    client.create_schedule(&admin, &beneficiary, &token_id, &100, &1000, &0, &1000);
    assert_eq!(client.get_beneficiary_schedule_count(&admin, &beneficiary), 1);

    client.create_schedule(&admin, &beneficiary, &token_id, &200, &1000, &0, &1000);
    assert_eq!(client.get_beneficiary_schedule_count(&admin, &beneficiary), 2);
}

// ── list_schedules_for_beneficiary ────────────────────────────────────────

#[test]
fn list_schedules_for_beneficiary_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, _t) = setup(&env);
    client.initialize_vesting(&admin);

    let result = client.list_schedules_for_beneficiary(&admin, &beneficiary);
    assert_eq!(result.len(), 0);
}

#[test]
fn list_schedules_for_beneficiary_single() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    client.create_schedule(&admin, &beneficiary, &token_id, &999, &1000, &0, &1000);

    let result = client.list_schedules_for_beneficiary(&admin, &beneficiary);
    assert_eq!(result.len(), 1);
    assert_eq!(result.get(0).unwrap().total_amount, 999);
}

#[test]
fn list_schedules_for_beneficiary_multiple() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    client.create_schedule(&admin, &beneficiary, &token_id, &100, &1000, &0, &1000);
    client.create_schedule(&admin, &beneficiary, &token_id, &200, &2000, &0, &1000);

    let result = client.list_schedules_for_beneficiary(&admin, &beneficiary);
    assert_eq!(result.len(), 2);
    assert_eq!(result.get(0).unwrap().total_amount, 100);
    assert_eq!(result.get(1).unwrap().total_amount, 200);
}

#[test]
fn list_schedules_for_beneficiary_only_own() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary_a, token_id) = setup(&env);
    let beneficiary_b = Address::generate(&env);
    client.initialize_vesting(&admin);

    client.create_schedule(&admin, &beneficiary_a, &token_id, &111, &1000, &0, &1000);
    client.create_schedule(&admin, &beneficiary_b, &token_id, &222, &1000, &0, &1000);

    let result_a = client.list_schedules_for_beneficiary(&admin, &beneficiary_a);
    let result_b = client.list_schedules_for_beneficiary(&admin, &beneficiary_b);

    assert_eq!(result_a.len(), 1);
    assert_eq!(result_a.get(0).unwrap().total_amount, 111);

    assert_eq!(result_b.len(), 1);
    assert_eq!(result_b.get(0).unwrap().total_amount, 222);
}

#[test]
fn list_schedules_for_beneficiary_multiple_admins_isolated() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin_a, beneficiary, token_id) = setup(&env);
    let admin_b = Address::generate(&env);
    let token_b = env.register_stellar_asset_contract(admin_b.clone());

    client.initialize_vesting(&admin_a);
    // Re-initializing with a different admin would fail; use admin_a for admin_b schedules test
    // by using the same client: admin is the *namespace key* in storage, not contract-wide.
    client.create_schedule(&admin_a, &beneficiary, &token_id, &100, &1000, &0, &1000);
    // admin_b has no schedules registered under this contract
    let result_b = client.list_schedules_for_beneficiary(&admin_b, &beneficiary);
    assert_eq!(result_b.len(), 0);

    let result_a = client.list_schedules_for_beneficiary(&admin_a, &beneficiary);
    assert_eq!(result_a.len(), 1);
    let _ = token_b; // suppress unused warning
}

// ── active_schedules_for_beneficiary ─────────────────────────────────

#[test]
fn list_active_schedules_excludes_cancelled() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    client.create_schedule(&admin, &beneficiary, &token_id, &1000, &1000, &0, &2000);
    client.cancel_schedule(&admin, &beneficiary, &0);

    let active = client.active_schedules_for_beneficiary(&admin, &beneficiary);
    assert_eq!(active.len(), 0);
}

#[test]
fn list_active_schedules_includes_partially_vested() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);
    // cliff = 0, vesting starts at t=1000, ends at t=3000
    client.create_schedule(&admin, &beneficiary, &token_id, &1_000_000, &1000, &0, &2000);

    // advance time to midpoint: claimable > 0, end_time not yet reached
    env.ledger().with_mut(|l| l.timestamp = 2000);

    let active = client.active_schedules_for_beneficiary(&admin, &beneficiary);
    assert_eq!(active.len(), 1);
}

#[test]
fn list_active_schedules_excludes_fully_claimed() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, beneficiary, token_id) = setup(&env);
    client.initialize_vesting(&admin);

    let total: i128 = 1_000_000;
    let start: u64 = 1000;
    let duration: u64 = 2000;
    client.create_schedule(&admin, &beneficiary, &token_id, &total, &start, &0, &duration);

    // advance past end so everything is vested
    env.ledger().with_mut(|l| l.timestamp = start + duration + 1);

    // mint tokens into the contract so the transfer succeeds
    let contract_id = client.address.clone();
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_id);
    token_client.mint(&contract_id, &total);

    // claim everything
    client.claim_vesting(&beneficiary, &admin, &0);

    // schedule is fully claimed; end_time is in the past → not active
    let active = client.active_schedules_for_beneficiary(&admin, &beneficiary);
    assert_eq!(active.len(), 0);
}
