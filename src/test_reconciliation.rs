#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::{Address as _, Events}, Address, Env, String, vec};
use crate::{RevoraRevenueShare, RevoraRevenueShareClient, EventIndexTopicV2, EVENT_INDEXED_V2};

#[test]
fn test_reconciliation_completeness() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("test");

    // 1. register_offering
    client.register_offering(&issuer, &ns, &token, &5000u32);
    
    // 2. set_fee_configuration
    client.set_fee_configuration(&issuer, &ns, &token, &1000u32);

    // 3. set_min_revenue_threshold
    client.set_min_revenue_threshold(&issuer, &ns, &token, &1000000000i128);

    // 4. set_rounding_mode
    client.set_rounding_mode(&issuer, &ns, &token, &1u32);

    // 5. set_concentration_limit
    client.set_concentration_limit(&issuer, &ns, &token, &1000u32, &true);

    // 6. set_claim_delay
    client.set_claim_delay(&issuer, &ns, &token, &86400u64);

    // 7. init_multisig
    let owners = vec![&env, Address::generate(&env), Address::generate(&env)];
    client.init_multisig(&issuer, &owners, &2u32);

    // 8. set_offering_metadata
    let metadata = String::from_str(&env, "ipfs://Qm...");
    client.set_offering_metadata(&issuer, &ns, &token, &metadata);

    // Verify events
    let events = env.events().all();
    let v2_events: Vec<_> = events
        .iter()
        .filter(|e| e.topics.get(0).unwrap() == EVENT_INDEXED_V2.into_val(&env))
        .collect();

    // Expecting at least 8 v2 events from the operations above
    assert!(v2_events.len() >= 8);
}
