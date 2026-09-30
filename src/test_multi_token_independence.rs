extern crate alloc;

use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

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
    let decimals = soroban_sdk::token::Client::new(&client.env, &payout_asset)
        .try_decimals()
        .ok()
        .and_then(|d| d.ok())
        .unwrap_or(0);
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

    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // Payment tokens
    // Payment tokens must be real token contracts: deposit_revenue
    // transfers the amount from the issuer.
    let pay_token_x = crate::test_utils::create_token(&env, &issuer);
    let pay_token_y = crate::test_utils::create_token(&env, &issuer);

    // Register Offerings
    register_offering(&client, &issuer, &namespace, &token_a);
    register_offering(&client, &issuer, &namespace, &token_b);

    // Deposit tokenX to A
    let amount_a = 1000;
    client.deposit_revenue(&issuer, &namespace, &token_a, &pay_token_x, &amount_a, &1);

    // Deposit tokenY to B
    let amount_b = 2000;
    client.deposit_revenue(&issuer, &namespace, &token_b, &pay_token_y, &amount_b, &1);

    // Assert get_payment_token returns correct for each
    assert_eq!(client.get_payment_token(&issuer, &namespace, &token_a), Some(pay_token_x.clone()));
    assert_eq!(client.get_payment_token(&issuer, &namespace, &token_b), Some(pay_token_y.clone()));

    // Assert cross-deposit fails (tokenY into A)
    let res =
        client.try_deposit_revenue(&issuer, &namespace, &token_a, &pay_token_y, &amount_b, &2);
    match res {
        Ok(_) => panic!("cross-token deposit must be rejected"),
        Err(Ok(err)) => assert_eq!(err, RevoraError::PaymentTokenMismatch),
        Err(Err(host)) => panic!("host failure instead of a contract error: {:?}", host),
    }
}
