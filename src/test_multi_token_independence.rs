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
    namespace: &Symbol,
    token: &Address,
) {
    // The payout asset must be a real token contract: `register_offering`
    // cross-checks `display_decimals` against its on-chain `decimals()`.
    let payout_asset = crate::test_utils::create_token(&client.env, issuer);
    let decimals = soroban_sdk::token::Client::new(&client.env, &payout_asset).decimals();
    client.register_offering(
        issuer,
        &Vec::new(&client.env),
        &1u32,
        namespace,
        token,
        &5000u32,
        &payout_asset,
        &0i128,
        &symbol_short!("USD"),
        &decimals,
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
    match res {
        Ok(_) => panic!("cross-token deposit must be rejected"),
        Err(Ok(err)) => assert_eq!(err, RevoraError::PaymentTokenMismatch),
        Err(Err(host)) => panic!("host failure instead of a contract error: {:?}", host),
    }
}
