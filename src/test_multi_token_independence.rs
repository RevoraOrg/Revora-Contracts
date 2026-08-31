extern crate alloc;

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env,
};

fn setup_test() -> (Env, RevoraRevenueShareClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    (env, client, admin)
}

fn register_offering(
    client: &RevoraRevenueShareClient<'static>,
    issuer: &Address,
    namespace: Symbol,
    token: &Address,
) {
    let payout_asset = Address::generate(&client.env);
    client.register_offering(
        issuer,
        &namespace,
        token,
        &5000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
}

#[test]
fn test_multi_token_offering_independence() {
    let (env, client, issuer) = setup_test();
    let namespace = symbol_short!("ns");
    
    let tokenA = Address::generate(&env);
    let tokenB = Address::generate(&env);
    
    // Payment tokens
    let payTokenX = Address::generate(&env);
    let payTokenY = Address::generate(&env);
    
    // Register Offerings
    register_offering(&client, &issuer, namespace, &tokenA);
    register_offering(&client, &issuer, namespace, &tokenB);
    
    // Deposit tokenX to A
    let amountA = 1000;
    client.deposit_revenue(&issuer, &namespace, &tokenA, &payTokenX, &amountA, &1);
    
    // Deposit tokenY to B
    let amountB = 2000;
    client.deposit_revenue(&issuer, &namespace, &tokenB, &payTokenY, &amountB, &1);
    
    // Assert get_payment_token returns correct for each
    assert_eq!(client.get_payment_token(&issuer, &namespace, &tokenA), Some(payTokenX));
    assert_eq!(client.get_payment_token(&issuer, &namespace, &tokenB), Some(payTokenY));
    
    // Assert cross-deposit fails (tokenY into A)
    let res = client.try_deposit_revenue(&issuer, &namespace, &tokenA, &payTokenY, &amountB, &2);
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().unwrap(), RevoraError::PaymentTokenMismatch as u32);
}
