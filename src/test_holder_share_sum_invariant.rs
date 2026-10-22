//! Tests asserting the aggregate sum invariant of `set_holder_share` (#764).
//!
//! Enforces that `sum(share_bps)` over all holders in an offering never exceeds 10,000 (100%).

#![cfg(test)]

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol,
};

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Symbol, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    let payout = Address::generate(&env);

    client.initialize(&admin);
    // register_offering has a lot of args in some versions, but looking at test_twap_window:
    client.register_offering(&issuer, &namespace, &token, &5_000_u32, &payout, &0_u32);

    (env, client, admin, issuer, namespace, token)
}

#[test]
fn sum_equal_to_10000_is_accepted() {
    let (env, client, _, issuer, ns, token) = setup();
    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);

    let res1 = client.try_set_holder_share(&issuer, &ns, &token, &holder1, &6_000, &1);
    assert!(res1.is_ok(), "First set should succeed");

    let res2 = client.try_set_holder_share(&issuer, &ns, &token, &holder2, &4_000, &1);
    assert!(res2.is_ok(), "Second set bringing sum to exactly 10000 should succeed");
}

#[test]
fn sum_exceeding_10000_is_rejected() {
    let (env, client, _, issuer, ns, token) = setup();
    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder1, &6_000, &1);

    let res2 = client.try_set_holder_share(&issuer, &ns, &token, &holder2, &4_001, &1);
    assert_eq!(
        res2,
        Err(Ok(RevoraError::InvalidShareBps)),
        "Must reject when sum > 10000"
    );
}

#[test]
fn removing_holder_frees_up_bps_capacity() {
    let (env, client, _, issuer, ns, token) = setup();
    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder1, &8_000, &1);

    // Try to set holder2 to 3000, should fail because 8000 + 3000 > 10000
    let res = client.try_set_holder_share(&issuer, &ns, &token, &holder2, &3_000, &1);
    assert_eq!(res, Err(Ok(RevoraError::InvalidShareBps)));

    // Reduce holder1 to 5000
    client.set_holder_share(&issuer, &ns, &token, &holder1, &5_000, &2);

    // Now holder2 can take 5000
    let res_ok = client.try_set_holder_share(&issuer, &ns, &token, &holder2, &5_000, &2);
    assert!(res_ok.is_ok(), "Should succeed after freeing up capacity");
}

#[test]
fn single_holder_exceeding_10000_is_rejected() {
    let (env, client, _, issuer, ns, token) = setup();
    let holder = Address::generate(&env);

    let res = client.try_set_holder_share(&issuer, &ns, &token, &holder, &10_001, &1);
    assert_eq!(
        res,
        Err(Ok(RevoraError::InvalidShareBps)),
        "Single holder cannot exceed 10000"
    );
}
