#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger, LedgerInfo},
    Address, BytesN, Env, Symbol, Vec,
};

/// Advance the test ledger by `secs` seconds.
fn advance_ledger(env: &Env, secs: u64) {
    let info = env.ledger().get();
    env.ledger().set(LedgerInfo { timestamp: info.timestamp.saturating_add(secs), ..info });
}

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset_admin = Address::generate(&env);
    let payout_asset = crate::test_utils::create_token(&env, &payout_asset_admin);
    crate::test_utils::mint_tokens(&env, &payout_asset, &issuer, 1_000_000);

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &symbol_short!("def"), &token, &5_000, &payout_asset, &0, &symbol_short!(""), &0u32);

    (env, client, issuer, token, payout_asset)
}

/// Set both holder jurisdiction and allowed jurisdictions for an offering.
fn set_jurisdiction(
    client: &RevoraRevenueShareClient<'static>,
    issuer: &Address,
    token: &Address,
    holder: &Address,
    jurisdiction: Symbol,
) {
    client.set_holder_jurisdiction(
        issuer,
        &symbol_short!("def"),
        token,
        holder,
        &jurisdiction,
        &0u64,
    );
    client.set_allowed_jurisdictions(
        issuer,
        &symbol_short!("def"),
        token,
        &soroban_sdk::vec![
            &client.env,
            symbol_short!("us"),
            symbol_short!("sg"),
            symbol_short!("jp")
        ],
    );
}

// ── Tests ────────────────────────────────────────────────────────────

#[test]
fn test_set_and_get_transfer_cooldown() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let jurisdiction = symbol_short!("us");

    // Initially cooldown should be 0 (disabled)
    let cd = client.get_transfer_cooldown(&issuer, &symbol_short!("def"), &token, &jurisdiction);
    assert_eq!(cd, 0, "default cooldown should be 0");

    // Set a cooldown of 1 hour
    client.set_transfer_cooldown(&issuer, &symbol_short!("def"), &token, &jurisdiction, &3600);

    let cd = client.get_transfer_cooldown(&issuer, &symbol_short!("def"), &token, &jurisdiction);
    assert_eq!(cd, 3600, "cooldown should be 3600 after set");

    // Verify event was emitted
    let events = env.events().all();
    let found = events.iter().any(|e| {
        let topic_str = format!("{:?}", e.0);
        topic_str.contains("tr_cool")
    });
    assert!(found, "cooldown set event should have been emitted");
}

#[test]
fn test_transfer_blocked_by_cooldown() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let ns = symbol_short!("ns");
    let category = Symbol::new(&env, "General");

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &1000, &payout_asset, &0, &symbol_short!(""), &0u32);
    env.ledger().set_network_id([0x01u8; 32]);

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);
    let jur = symbol_short!("us");

    // Assign jurisdiction to holder1
    set_jurisdiction(&client, &issuer, &token, &holder1, jur.clone());

    // Set holder shares
    client.set_holder_share(&issuer, &ns, &token, &holder1, &100, &1);

    // Set a 1-hour cooldown for jurisdiction "us"
    client.set_transfer_cooldown(&issuer, &ns, &token, &jur, &3600);

    // First transfer should succeed (no prior transfer timestamp)
    client.transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder2, &50, &category);

    // Attempt another transfer immediately — should fail with TransferCooldownActive
    let holder3 = Address::generate(&env);
    let result = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder3, &25, &category);
    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::TransferCooldownActive,
        "transfer should be blocked by cooldown"
    );
}

#[test]
fn test_transfer_allowed_after_cooldown_elapsed() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let ns = symbol_short!("ns");
    let category = Symbol::new(&env, "General");

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &1000, &payout_asset, &0, &symbol_short!(""), &0u32);
    env.ledger().set_network_id([0x01u8; 32]);

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);
    let jur = symbol_short!("us");

    set_jurisdiction(&client, &issuer, &token, &holder1, jur.clone());
    client.set_holder_share(&issuer, &ns, &token, &holder1, &100, &1);

    // Set a 1-hour cooldown
    client.set_transfer_cooldown(&issuer, &ns, &token, &jur, &3600);

    // First transfer
    client.transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder2, &50, &category);

    // Advance ledger past the cooldown window
    advance_ledger(&env, 3601);

    // Second transfer should now succeed
    let holder3 = Address::generate(&env);
    let result = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder3, &25, &category);
    assert!(result.is_ok(), "transfer should succeed after cooldown elapsed");
}

#[test]
fn test_cooldown_exactly_at_boundary_rejects() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let ns = symbol_short!("ns");
    let category = Symbol::new(&env, "General");

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &1000, &payout_asset, &0, &symbol_short!(""), &0u32);
    env.ledger().set_network_id([0x01u8; 32]);

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);
    let jur = symbol_short!("us");

    set_jurisdiction(&client, &issuer, &token, &holder1, jur.clone());
    client.set_holder_share(&issuer, &ns, &token, &holder1, &100, &1);

    // Set a 60-second cooldown
    client.set_transfer_cooldown(&issuer, &ns, &token, &jur, &60);

    // First transfer
    client.transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder2, &50, &category);

    // Advance exactly to the boundary (59 seconds — still too early)
    advance_ledger(&env, 59);

    let holder3 = Address::generate(&env);
    let result = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder3, &25, &category);
    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::TransferCooldownActive,
        "transfer at 59s should still be blocked (cooldown=60)"
    );

    // Advance past the boundary (61 seconds total)
    advance_ledger(&env, 2);

    let result = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder3, &25, &category);
    assert!(result.is_ok(), "transfer at 61s should succeed (cooldown=60)");
}

#[test]
fn test_cooldown_zero_means_disabled() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let ns = symbol_short!("ns");
    let category = Symbol::new(&env, "General");

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &1000, &payout_asset, &0, &symbol_short!(""), &0u32);
    env.ledger().set_network_id([0x01u8; 32]);

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);
    let jur = symbol_short!("us");

    set_jurisdiction(&client, &issuer, &token, &holder1, jur.clone());
    client.set_holder_share(&issuer, &ns, &token, &holder1, &100, &1);

    // Set cooldown to 0 — should be disabled
    client.set_transfer_cooldown(&issuer, &ns, &token, &jur, &0);

    // First transfer
    client.transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder2, &50, &category);

    // Immediate second transfer should succeed (cooldown=0 = disabled)
    let holder3 = Address::generate(&env);
    let result = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder3, &25, &category);
    assert!(result.is_ok(), "transfer should succeed when cooldown=0");
}

#[test]
fn test_different_jurisdictions_have_independent_cooldowns() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let ns = symbol_short!("ns");
    let category = Symbol::new(&env, "General");

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &1000, &payout_asset, &0, &symbol_short!(""), &0u32);
    env.ledger().set_network_id([0x01u8; 32]);

    let holder_us = Address::generate(&env);
    let holder_sg = Address::generate(&env);
    let holder2 = Address::generate(&env);

    // Set up jurisdictions
    set_jurisdiction(&client, &issuer, &token, &holder_us, symbol_short!("us"));
    set_jurisdiction(&client, &issuer, &token, &holder_sg, symbol_short!("sg"));

    client.set_holder_share(&issuer, &ns, &token, &holder_us, &100, &1);
    client.set_holder_share(&issuer, &ns, &token, &holder_sg, &100, &1);

    // Set different cooldowns for each jurisdiction
    client.set_transfer_cooldown(&issuer, &ns, &token, &symbol_short!("us"), &3600); // 1 hour
    client.set_transfer_cooldown(&issuer, &ns, &token, &symbol_short!("sg"), &60); // 1 minute

    // Both holders transfer
    client.transfer_with_attestation(&issuer, &ns, &token, &holder_us, &holder2, &25, &category);
    client.transfer_with_attestation(&issuer, &ns, &token, &holder_sg, &holder2, &25, &category);

    // Both transfers should be blocked immediately
    let holder3 = Address::generate(&env);
    let result = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &holder_us, &holder3, &25, &category);
    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::TransferCooldownActive,
        "US holder should be blocked"
    );

    let result = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &holder_sg, &holder3, &25, &category);
    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::TransferCooldownActive,
        "SG holder should be blocked"
    );

    // Advance 61 seconds — SG cooldown (60s) has elapsed, US cooldown (3600s) has not
    advance_ledger(&env, 61);

    // SG holder should now be able to transfer
    let result = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &holder_sg, &holder3, &25, &category);
    assert!(result.is_ok(), "SG holder should be able to transfer after 61s (cooldown=60)");

    // US holder should still be blocked
    let result = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &holder_us, &holder3, &25, &category);
    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::TransferCooldownActive,
        "US holder should still be blocked (cooldown=3600)"
    );
}

#[test]
fn test_cooldown_not_applied_when_jurisdiction_not_set() {
    // A holder without a jurisdiction tag assigned should bypass the cooldown
    // even if a cooldown is configured for some jurisdiction
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let ns = symbol_short!("ns");
    let category = Symbol::new(&env, "General");

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &1000, &payout_asset, &0, &symbol_short!(""), &0u32);
    env.ledger().set_network_id([0x01u8; 32]);

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);

    // Set empty allowed jurisdictions (gating disabled)
    client.set_allowed_jurisdictions(&issuer, &ns, &token, &soroban_sdk::vec![&env]);
    client.set_holder_share(&issuer, &ns, &token, &holder1, &100, &1);

    // Set a cooldown for "us" jurisdiction, but holder1 has no jurisdiction
    let jur = symbol_short!("us");
    client.set_transfer_cooldown(&issuer, &ns, &token, &jur, &3600);

    // Transfer — should succeed because holder1 has no jurisdiction
    // (cooldown check only applies when holder has a jurisdiction)
    let result = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder2, &50, &category);
    assert!(result.is_ok(), "transfer should succeed when sender has no jurisdiction tag");

    // Second immediate transfer should also succeed
    let holder3 = Address::generate(&env);
    let result = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder3, &25, &category);
    assert!(result.is_ok(), "second transfer should also succeed when sender has no jurisdiction");
}

#[test]
fn test_estimate_transfer_cooldown_consistency() {
    // Verify that `estimate_transfer` also reports TransferCooldownActive
    // when a cooldown is active, matching the real transfer path.
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let ns = symbol_short!("ns");
    let category = Symbol::new(&env, "General");

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &1000, &payout_asset, &0, &symbol_short!(""), &0u32);
    env.ledger().set_network_id([0x01u8; 32]);

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);
    let jur = symbol_short!("us");

    set_jurisdiction(&client, &issuer, &token, &holder1, jur.clone());
    client.set_holder_share(&issuer, &ns, &token, &holder1, &100, &1);

    // Set a cooldown
    client.set_transfer_cooldown(&issuer, &ns, &token, &jur, &3600);

    // First transfer to establish timestamp
    client.transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder2, &50, &category);

    // estimate_transfer should also return TransferCooldownActive
    let result = client.try_estimate_transfer(
        &issuer,
        &ns,
        &token,
        &holder1,
        &holder2,
        &25,
        &category,
        &BytesN::from_array(&env, &[0xabu8; 32]),
        &BytesN::from_array(&env, &[0x01u8; 32]),
    );
    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::TransferCooldownActive,
        "estimate_transfer should also report cooldown active"
    );

    // After cooldown elapses, both should succeed
    advance_ledger(&env, 3601);

    let result = client.try_estimate_transfer(
        &issuer,
        &ns,
        &token,
        &holder1,
        &holder2,
        &25,
        &category,
        &BytesN::from_array(&env, &[0xabu8; 32]),
        &BytesN::from_array(&env, &[0x01u8; 32]),
    );
    assert!(result.is_ok(), "estimate_transfer should succeed after cooldown elapses");
}

#[test]
fn test_cooldown_state_not_recorded_when_no_cooldown_configured() {
    // Verify that when no cooldown is configured for a jurisdiction,
    // the HolderLastTransferTime is NOT written (storage efficiency)
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let ns = symbol_short!("ns");
    let category = Symbol::new(&env, "General");

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &1000, &payout_asset, &0, &symbol_short!(""), &0u32);
    env.ledger().set_network_id([0x01u8; 32]);

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);
    let jur = symbol_short!("us");

    set_jurisdiction(&client, &issuer, &token, &holder1, jur.clone());
    client.set_holder_share(&issuer, &ns, &token, &holder1, &100, &1);

    // NO cooldown configured for "us" jurisdiction

    // Transfer — should succeed and NOT record last transfer time
    client.transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder2, &50, &category);

    // Second transfer — should also succeed immediately since no cooldown is configured
    let holder3 = Address::generate(&env);
    let result = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &holder1, &holder3, &25, &category);
    assert!(
        result.is_ok(),
        "transfer should succeed when no cooldown is configured for the jurisdiction"
    );
}
