#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env,
};

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset_admin = Address::generate(&env);
    let payout_asset = crate::test_utils::create_token(&env, &payout_asset_admin);
    crate::test_utils::mint_tokens(&env, &payout_asset, &issuer, 1_000_000);

    client.register_offering(&issuer, &symbol_short!("def"), &token, &5_000, &payout_asset, &0);

    (env, client, issuer, token, payout_asset)
}

#[test]
fn claim_uses_historical_share_for_unclaimed_periods() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &2_500);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &2);

    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(payout, 75_000);
}

#[test]
fn zeroing_share_does_not_burn_already_accrued_claims() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &0);

    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(payout, 50_000);
}

#[test]
fn get_claimable_uses_historical_share_schedule() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&_env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &4_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &1_000);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &2);

    assert_eq!(client.get_claimable(&issuer, &symbol_short!("def"), &token, &holder), 50_000);
}

#[test]
fn delay_barrier_preserves_pre_change_accrual() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);
    client.set_claim_delay(&issuer, &symbol_short!("def"), &token, &100);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    env.ledger().with_mut(|li| li.timestamp = 1_050);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &2_500);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &2);

    env.ledger().with_mut(|li| li.timestamp = 1_100);
    assert_eq!(client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0), 50_000);

    env.ledger().with_mut(|li| li.timestamp = 1_150);
    assert_eq!(client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0), 25_000);
}

#[test]
fn holder_statement_page_paginates_in_period_order() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &2_500);
    for period_id in 1_u64..=5_u64 {
        RevoraRevenueShare::test_insert_period(
            env.clone(),
            issuer.clone(),
            symbol_short!("def"),
            token.clone(),
            period_id,
            100_000,
        );
    }

    let (page_one, next_one) = client.get_holder_statement_page(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &0,
        &2,
    );
    assert_eq!(page_one.len(), 2);
    assert_eq!(page_one.get(0).unwrap().period_id, 1);
    assert_eq!(page_one.get(1).unwrap().period_id, 2);
    assert_eq!(page_one.get(0).unwrap().claimable_amount, 25_000);
    assert_eq!(page_one.get(1).unwrap().claimable_amount, 25_000);
    assert_eq!(next_one, Some(2));

    let (page_two, next_two) = client.get_holder_statement_page(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &2,
        &2,
    );
    assert_eq!(page_two.len(), 2);
    assert_eq!(page_two.get(0).unwrap().period_id, 3);
    assert_eq!(page_two.get(1).unwrap().period_id, 4);
    assert_eq!(next_two, Some(4));

    let (page_three, next_three) = client.get_holder_statement_page(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &4,
        &2,
    );
    assert_eq!(page_three.len(), 1);
    assert_eq!(page_three.get(0).unwrap().period_id, 5);
    assert_eq!(next_three, None);
}

#[test]
fn holder_statement_page_cursor_past_end_returns_empty() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &1_000);
    RevoraRevenueShare::test_insert_period(
        env.clone(),
        issuer.clone(),
        symbol_short!("def"),
        token.clone(),
        1,
        50_000,
    );

    let (page, next) = client.get_holder_statement_page(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &99,
        &10,
    );
    assert_eq!(page.len(), 0);
    assert_eq!(next, None);
}

#[test]
fn holder_statement_page_cursor_is_stable_with_delay_barrier() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &10_000);
    client.set_claim_delay(&issuer, &symbol_short!("def"), &token, &100);

    env.ledger().with_mut(|li| li.timestamp = 1_000);
    RevoraRevenueShare::test_insert_period(
        env.clone(),
        issuer.clone(),
        symbol_short!("def"),
        token.clone(),
        1,
        10_000,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_050);
    RevoraRevenueShare::test_insert_period(
        env.clone(),
        issuer.clone(),
        symbol_short!("def"),
        token.clone(),
        2,
        20_000,
    );

    env.ledger().with_mut(|li| li.timestamp = 1_100);
    let (page_one, next_one) = client.get_holder_statement_page(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &0,
        &10,
    );
    let (page_two, next_two) = client.get_holder_statement_page(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &0,
        &10,
    );

    assert_eq!(page_one.len(), 1);
    assert_eq!(page_one.get(0).unwrap().period_id, 1);
    assert_eq!(page_one.get(0).unwrap().claimable_amount, 10_000);
    assert_eq!(next_one, Some(1));

    assert_eq!(page_two, page_one);
    assert_eq!(next_two, next_one);
}
