#![cfg(test)]

use crate::{
    proptest_helpers::{any_test_operation, arb_valid_operation_sequence, TestOperation},
    RevoraRevenueShare, RevoraRevenueShareClient,
};
use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

// Simple oracle for tracking expected holder shares
fn verify_share_conservation(
    env: &Env,
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
    assert!(
        total_bps <= 10_000,
        "Share conservation violated! Total BPS = {}",
        total_bps
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 10_000,
        max_local_rng: None,
        ..ProptestConfig::default()
    })]
    
    #[test]
    fn prop_share_conservation(env in Env::default(), seq in arb_valid_operation_sequence(20usize)) {
        env.mock_all_auths();
        let contract_id = env.register_contract(None, RevoraRevenueShare);
        let client = RevoraRevenueShareClient::new(&env, &contract_id);
        
        let admin = Address::generate(&env);
        client.initialize(&admin, &None::<Address>, &None::<bool>);
        
        // Track generated issuers, tokens and holders so we can query them
        let mut active_offerings: Vec<(Address, Symbol, Address)> = vec![];
        let mut all_holders: Vec<Address> = vec![];

        for op in seq {
            match op {
                TestOperation::RegisterOffering { issuer, namespace, token, bps, payout_asset, supply_cap } => {
                    if let Ok(_) = client.try_register_offering(&issuer, &namespace, &token, &bps, &payout_asset, &supply_cap) {
                        active_offerings.push((issuer, namespace, token));
                    }
                }
                TestOperation::SetHolderShare { issuer, namespace, token, holder, share_bps } => {
                    let _ = client.try_set_holder_share(&issuer, &namespace, &token, &holder, &share_bps);
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
