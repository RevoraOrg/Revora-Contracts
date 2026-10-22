//! # Cross-Contract Vesting Test
//!
//! Verifies that the revenue-share contract delegates vesting checks to the
//! separate [`revora-vesting`](revora_vesting) contract at runtime instead of
//! linking vesting entry points into its own cdylib.
//!
//! `convert_class` must still block holders with unvested vesting schedules by
//! querying the configured vesting contract via [`VestingClient`].

#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env,
};

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, ShareClass, VestingClient};
use revora_vesting::{VestingContract, VestingContractClient, VestingCurve};

fn setup(env: &Env) -> (RevoraRevenueShareClient<'static>, Address, Address, Address, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(env);
    let issuer = Address::generate(env);
    let holder = Address::generate(env);
    let token = Address::generate(env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    (client, admin, issuer, holder, token)
}

fn register_vesting_schedule(
    env: &Env,
    issuer: &Address,
    beneficiary: &Address,
    token: &Address,
    total_amount: i128,
    cliff_ts: u64,
    start_ts: u64,
    end_ts: u64,
    cliff_secs: u64,
) -> Address {
    let vesting_id = env.register_contract(None, VestingContract);
    let vesting = VestingContractClient::new(env, &vesting_id);
    vesting.vesting_register(
        issuer,
        beneficiary,
        token,
        &total_amount,
        &cliff_ts,
        &start_ts,
        &end_ts,
        &VestingCurve::Linear,
        &cliff_secs,
    );
    vesting_id
}

#[test]
fn convert_class_blocks_unvested_holder_via_vesting_contract() {
    let env = Env::default();
    let (client, admin, issuer, holder, token) = setup(&env);

    // An unvested schedule (query time before start) lives in the vesting contract.
    let vesting_id =
        register_vesting_schedule(&env, &issuer, &holder, &token, 1000, 1000, 1000, 2000, 0);
    env.ledger().with_mut(|l| l.timestamp = 500);

    client.set_vesting_contract(&admin, &vesting_id);

    let namespace = symbol_short!("def");
    let result = client.try_convert_class(
        &issuer,
        &namespace,
        &token,
        &holder,
        &ShareClass::A,
        &ShareClass::B,
        &100u32,
    );
    assert_eq!(result, Err(Ok(RevoraError::UnvestedConversionBlocked)));
}

#[test]
fn convert_class_allows_fully_vested_holder_via_vesting_contract() {
    let env = Env::default();
    let (client, admin, issuer, holder, token) = setup(&env);

    // A fully vested schedule (query time after end) lives in the vesting
    // contract, so the vesting check must not block the conversion.
    let vesting_id =
        register_vesting_schedule(&env, &issuer, &holder, &token, 1000, 100, 100, 200, 0);
    env.ledger().with_mut(|l| l.timestamp = 300);

    client.set_vesting_contract(&admin, &vesting_id);

    let namespace = symbol_short!("def");
    // No conversion ratio is configured, so the call passes the vesting check
    // and is rejected later with `ConversionNotApproved` rather than
    // `UnvestedConversionBlocked`.
    let result = client.try_convert_class(
        &issuer,
        &namespace,
        &token,
        &holder,
        &ShareClass::A,
        &ShareClass::B,
        &100u32,
    );
    assert_eq!(result, Err(Ok(RevoraError::ConversionNotApproved)));
}
