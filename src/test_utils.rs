#![cfg(test)]
// `setup_context` returns several values; callers may not use all of them.
// Suppress only the specific lint rather than silencing all warnings.
#![allow(dead_code)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

/// Core test utilities avoiding self-referential struct lifetime errors.
pub fn setup_context() -> (Env, RevoraRevenueShareClient, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    (env, client, contract_id, issuer, token, payout_asset)
}

pub fn create_token(env: &Env, admin: &Address) -> Address {
    env.register_stellar_asset_contract_v2(admin.clone()).address()
}

pub fn mint_tokens(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

pub fn get_balance(env: &Env, token: &Address, who: &Address) -> i128 {
    TokenClient::new(env, token).balance(who)
}

pub fn advance_past(env: &Env, ledger: u32) {
    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: 12345,
        protocol_version: 20,
        sequence_number: ledger + 1,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 10,
        min_persistent_entry_ttl: 10,
        max_entry_ttl: 6_312_000,
    });
}

pub fn set_timestamp(env: &Env, timestamp: u64) {
    env.ledger().with_mut(|l| l.timestamp = timestamp);
}
