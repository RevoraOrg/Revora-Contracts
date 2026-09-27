#![cfg(test)]
use super::*;
use soroban_sdk::{
    testutils::Address as _,
    Address, Env, Symbol,
};

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    // Register stellar asset so decimals check passes
    let payment_token = env.register_stellar_asset_contract_v2(issuer.clone()).address();
    let token = Address::generate(&env);
    client.register_offering(
        &issuer,
        &Vec::from_array(&env, []),
        &1u32, // quorum
        &symbol_short!("ns"), // namespace
        &token,
        &10_000u32, // revenue_share_bps
        &payment_token,
        &0i128, // supply cap
        &symbol_short!("USD"), // denomination_symbol
        &7u32, // display_decimals
    );
    (env, client, issuer, token, payment_token)
}

#[test]
fn test_get_denomination_metadata_valid() {
    let (_env, client, issuer, token, _) = setup_offering();
    let namespace = symbol_short!("ns");

    let result = client.get_denomination_metadata(&issuer, &namespace, &token);
    assert!(result.is_some());
    let (sym, decimals) = result.unwrap();
    assert_eq!(sym, symbol_short!("USD"));
    assert_eq!(decimals, 7);
}

#[test]
fn test_get_denomination_metadata_not_found() {
    let (env, client, issuer, token, _) = setup_offering();
    
    // Different namespace
    let result = client.get_denomination_metadata(&issuer, &symbol_short!("other"), &token);
    assert!(result.is_none());

    // Different issuer
    let other_issuer = Address::generate(&env);
    let result2 = client.get_denomination_metadata(&other_issuer, &symbol_short!("ns"), &token);
    assert!(result2.is_none());

    // Different token
    let other_token = Address::generate(&env);
    let result3 = client.get_denomination_metadata(&issuer, &symbol_short!("ns"), &other_token);
    assert!(result3.is_none());
}

#[test]
fn test_get_denomination_metadata_unauthorized_does_not_mutate() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let namespace = symbol_short!("ns");
    
    let result = client.get_denomination_metadata(&issuer, &namespace, &token);
    assert!(result.is_none());
}
