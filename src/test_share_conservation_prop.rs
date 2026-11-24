#![cfg(test)]
extern crate alloc;

use crate::{
    proptest_helpers::{arb_valid_operation_sequence, TestOperation},
    RevoraRevenueShare, RevoraRevenueShareClient,
};
use alloc::vec::Vec;
use proptest::prelude::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol};

// Simple oracle for tracking expected holder shares
fn verify_share_conservation(
    _env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    holders: &[Address],
) {
    let mut total_bps = 0u32;
    for holder in holders {
        total_bps += client.get_holder_share(issuer, namespace, token, holder);
    }
    assert!(total_bps <= 10_000, "Share conservation violated! Total BPS = {}", total_bps);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 100,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_share_conservation(seq in arb_valid_operation_sequence(20usize)) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, RevoraRevenueShare);
        let client = RevoraRevenueShareClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize(&admin, &None::<Address>, &None::<bool>);

        // Track generated issuers, tokens and holders so we can query them
        let mut active_offerings: Vec<(Address, Symbol, Address)> = Vec::new();
        let mut all_holders: Vec<Address> = Vec::new();
        let mut nonce = 0u64;

        for op in seq {
            match op {
                TestOperation::RegisterOffering { issuer, namespace, token, bps, payout_asset, supply_cap } => {
                    if client.try_register_offering(
                        &issuer,
                        &soroban_sdk::Vec::new(&env),
                        &1u32,
                        &namespace,
                        &token,
                        &bps,
                        &payout_asset,
                        &supply_cap,
                        &symbol_short!(""),
                        &0u32,
                    ).is_ok() {
                        let offering_tuple = (issuer, namespace, token);
                        if !active_offerings.contains(&offering_tuple) {
                            active_offerings.push(offering_tuple);
                        }
                    }
                }
                TestOperation::SetHolderShare { issuer, namespace, token, holder, share_bps } => {
                    nonce += 1;
                    let _ = client.try_set_holder_share(
                        &issuer,
                        &namespace,
                        &token,
                        &holder,
                        &share_bps,
                        &nonce,
                        &soroban_sdk::Vec::from_array(&client.env, [issuer.clone()]),
                    );
                    if !all_holders.contains(&holder) {
                        all_holders.push(holder);
                    }
                }
                // Add execution for other ops if they exist in TestOperation...
                _ => {}
            }

            // Assert invariant after each successful op
            for (i, ns, t) in &active_offerings {
                verify_share_conservation(&env, &client, i, ns, t, &all_holders);
            }
        }
    }
}
