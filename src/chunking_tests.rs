#![cfg(test)]
#![allow(dead_code, unused_variables, unused_imports)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, token, Address, Env, Vec};

// Minimal helpers duplicated from src/test.rs so these chunking tests can live separately.
fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn setup() -> (Env, RevoraRevenueShareClient, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, crate::RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    (env, client, issuer)
}

fn create_payment_token(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract(admin.clone());
    (token_id, admin)
}

fn mint_tokens(env: &Env, payment_token: &Address, recipient: &Address, amount: &i128) {
    token::StellarAssetClient::new(env, payment_token).mint(recipient, amount);
}

fn setup_with_offering(
) -> (Env, RevoraRevenueShareClient, Address, Address, Address, Address) {
    let (env, client, issuer) = setup();
    let token = Address::generate(&env);
    let (payment_token, pt_admin) = create_payment_token(&env);
    // Register offering and fund issuer so deposit_revenue can transfer tokens
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &payment_token, &0);
    mint_tokens(&env, &payment_token, &issuer, &100_000i128);
    (env, client, issuer, token, payment_token, pt_admin)
}

#[test]
fn get_revenue_range_chunk_matches_full_sum() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1000u32, &token, &0i128);

    // Report revenue for periods 1..=10
    for p in 1u64..=10u64 {
        client.report_revenue(&issuer, &symbol_short!("def"), &token, &token, &100i128, &p, &false);
    }

    // Full sum
    let full = client.get_revenue_range(&issuer, &symbol_short!("def"), &token, &1u64, &10u64);

    // Sum in chunks of 3
    let mut cursor = 1u64;
    let mut acc: i128 = 0;
    loop {
        let (partial, next) = client.get_revenue_range_chunk(
            &issuer,
            &symbol_short!("def"),
            &token,
            &cursor,
            &10u64,
            &3u32,
        );
        acc += partial;
        if let Some(n) = next {
            cursor = n;
        } else {
            break;
        }
    }

    assert_eq!(full, acc);
}

#[test]
fn pending_periods_page_and_claimable_chunk_consistent() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    let (payment_token, _pt_admin) = create_payment_token(&env);
    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1000u32,
        &payment_token,
        &0i128,
    );
    // Mint to issuer so deposit_revenue token transfer succeeds
    mint_tokens(&env, &payment_token, &issuer, &100_000i128);

    // Insert periods 1..=8 via the test helper (avoids token transfers in tests)
    for p in 1u64..=8u64 {
        client.test_insert_period(&issuer, &symbol_short!("def"), &token, &p, &1000i128);
    }

    // Set holder share
    let r = client.try_set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &1000u32);
    assert!(r.is_ok());

    // get_pending_periods full
    let full = client.get_pending_periods(&issuer, &symbol_short!("def"), &token, &holder);

    // Page through with limit 3
    let mut cursor = 0u32;
    let mut all = Vec::new(&env);
    loop {
        let (page, next) = client.get_pending_periods_page(
            &issuer,
            &symbol_short!("def"),
            &token,
            &holder,
            &cursor,
            &3u32,
        );
        for i in 0..page.len() {
            all.push_back(page.get(i).unwrap());
        }
        if let Some(n) = next {
            cursor = n;
        } else {
            break;
        }
    }

    // Compare lengths
    assert_eq!(full.len(), all.len());

    // Now check claimable chunk matches full
    let full_claim = client.get_claimable(&issuer, &symbol_short!("def"), &token, &holder);

    // Sum claimable in chunks from index 0, count 2
    let mut idx = 0u32;
    let mut acc: i128 = 0;
    loop {
        let (partial, next) = client.get_claimable_chunk(
            &issuer,
            &symbol_short!("def"),
            &token,
            &holder,
            &idx,
            &2u32,
        );
        acc += partial;
        if let Some(n) = next {
            idx = n;
        } else {
            break;
        }
    }
    assert_eq!(full_claim, acc);
}
