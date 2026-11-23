#![cfg(test)]

use crate::{DataKey2, OfferingId, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payment_admin = Address::generate(&env);
    let payment_token = crate::test_utils::create_token(&env, &payment_admin);
    crate::test_utils::mint_tokens(&env, &payment_token, &issuer, i128::MAX);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &offering_token,
        &10_000u32,
        &payment_token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    (env, client, issuer, offering_token, payment_token, contract_id)
}

fn offering_id(issuer: &Address, token: &Address) -> OfferingId {
    OfferingId { issuer: issuer.clone(), namespace: symbol_short!("def"), token: token.clone() }
}

#[test]
fn accrual_delta_is_exact_at_the_supported_boundary() {
    assert_eq!(RevoraRevenueShare::accrual_delta_e18(10_000), Ok(1_000_000_000_000_000_000));
    assert!(RevoraRevenueShare::accrual_delta_e18(crate::MAX_DEPOSIT_AMOUNT).is_ok());
    assert_eq!(
        RevoraRevenueShare::accrual_delta_e18(crate::MAX_DEPOSIT_AMOUNT + 1),
        Err(RevoraError::ArithmeticOverflow)
    );
}

#[test]
fn deposit_rejects_index_overflow_before_transfer_or_storage_mutation() {
    let (env, client, issuer, token, payment_token, contract_id) = setup();
    let id = offering_id(&issuer, &token);
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey2::GlobalAccPerShareE18(id), &i128::MAX);
    });

    let result =
        client.try_deposit_revenue(&issuer, &symbol_short!("def"), &token, &payment_token, &1, &1);

    assert_eq!(result, Err(Ok(RevoraError::ArithmeticOverflow)));
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(crate::test_utils::get_balance(&env, &payment_token, &issuer), i128::MAX);
    assert_eq!(crate::test_utils::get_balance(&env, &payment_token, &contract_id), 0);
}

#[test]
fn report_rejects_index_overflow_without_recording_the_report() {
    let (env, client, issuer, token, payment_token, contract_id) = setup();
    let id = offering_id(&issuer, &token);
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey2::AccrualIndex(id), &i128::MAX);
    });

    let result = client.try_report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &1,
        &1,
        &false,
    );

    assert_eq!(result, Err(Ok(RevoraError::ArithmeticOverflow)));
    assert_eq!(client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &1), 0);
}

#[test]
fn report_preserves_exact_arithmetic_at_maximum_safe_amount() {
    let (_env, client, issuer, token, payment_token, _contract_id) = setup();
    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &crate::MAX_DEPOSIT_AMOUNT,
        &1,
        &false,
    );

    assert_eq!(
        client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &1),
        crate::MAX_DEPOSIT_AMOUNT
    );
}
