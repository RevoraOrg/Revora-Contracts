#![cfg(test)]

use crate::{
    DataKey2, JurisdictionMigrationState, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger, LedgerInfo},
    Address, BytesN, Env,
};

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

#[test]
fn holder_jurisdiction_and_allowlist_are_stored_with_audit_event() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);
    let events_before = env.events().all().len();

    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("us"),
        &0u64,
    );
    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us"), symbol_short!("ca")],
    );

    assert_eq!(
        client.get_holder_jurisdiction(&issuer, &symbol_short!("def"), &token, &holder),
        Some(symbol_short!("us"))
    );
    assert_eq!(
        client.get_allowed_jurisdictions(&issuer, &symbol_short!("def"), &token),
        soroban_sdk::vec![&env, symbol_short!("us"), symbol_short!("ca")]
    );
    assert!(env.events().all().len() >= events_before + 2);
}

#[test]
fn set_holder_share_rejects_disallowed_jurisdiction_and_emits_audit_event() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("uk"),
        &0u64,
    );
    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us")],
    );
    let events_before = env.events().all().len();

    let result =
        client.try_set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &2_500u32);
    assert_eq!(result, Err(Ok(RevoraError::JurisdictionDisallowed)));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder), 0);
    assert!(env.events().all().len() > events_before);
}

#[test]
fn removing_a_jurisdiction_does_not_break_existing_holder_claims() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("us"),
        &0u64,
    );
    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us")],
    );
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &4_000, &1);

    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("ca")],
    );

    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);
    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);

    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder), 4_000);
    assert_eq!(payout, 40_000);
    assert_eq!(crate::test_utils::get_balance(&env, &payout_asset, &holder), 40_000);
}

#[test]
fn apply_snapshot_shares_rejects_disallowed_jurisdiction_without_partial_state() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    client.set_snapshot_config(&issuer, &symbol_short!("def"), &token, &true);
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("uk"),
        &0u64,
    );
    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us")],
    );

    let hash = BytesN::from_array(&env, &[7; 32]);
    client.commit_snapshot(&issuer, &symbol_short!("def"), &token, &1, &hash);
    let events_before = env.events().all().len();

    let holders = soroban_sdk::vec![&env, (holder.clone(), 2_500u32)];
    let result =
        client.try_apply_snapshot_shares(&issuer, &symbol_short!("def"), &token, &1, &0, &holders);

    assert_eq!(result, Err(Ok(RevoraError::JurisdictionDisallowed)));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder), 0);
    assert_eq!(client.get_snapshot_holder_count(&issuer, &symbol_short!("def"), &token, &1), 0);
    assert!(env.events().all().len() > events_before);
}

#[test]
fn issuer_transfer_migrates_allowed_jurisdictions() {
    let (env, client, old_issuer, token, _payout_asset) = setup_offering();
    let new_issuer = Address::generate(&env);
    let contract_id = client.address.clone();

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey2::IssuerCount, &1_u32);
        env.storage().persistent().set(&DataKey2::IssuerItem(0), &old_issuer);
        env.storage()
            .persistent()
            .set(&DataKey2::IssuerRegistered(old_issuer.clone()), &true);
        env.storage()
            .persistent()
            .set(&DataKey2::NamespaceCount(old_issuer.clone()), &1_u32);
        env.storage()
            .persistent()
            .set(&DataKey2::NamespaceItem(old_issuer.clone(), 0), &symbol_short!("def"));
        env.storage()
            .persistent()
            .set(&DataKey2::NamespaceRegistered(old_issuer.clone(), symbol_short!("def")), &true);
    });

    client.set_allowed_jurisdictions(
        &old_issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us"), symbol_short!("sg")],
    );
    client.propose_issuer_transfer(&old_issuer, &symbol_short!("def"), &token, &new_issuer);
    client.accept_issuer_transfer(&new_issuer, &symbol_short!("def"), &token);

    assert_eq!(
        client.get_allowed_jurisdictions(&new_issuer, &symbol_short!("def"), &token),
        soroban_sdk::vec![&env, symbol_short!("us"), symbol_short!("sg")]
    );
    assert_eq!(
        client.get_allowed_jurisdictions(&old_issuer, &symbol_short!("def"), &token).len(),
        0
    );
}

// ── Jurisdiction migration tests (#539) ──

/// Helper: advance the test ledger by `secs` seconds.
fn advance_ledger(env: &Env, secs: u64) {
    let info = env.ledger().get();
    env.ledger().set(LedgerInfo {
        timestamp: info.timestamp.saturating_add(secs),
        ..info
    });
}

#[test]
fn jurisdiction_migration_with_future_ts_emits_event_and_stores_state() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);
    let now = env.ledger().timestamp();
    let effective_ts = now + 3600; // 1 hour in the future

    let events_before = env.events().all().len();

    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("ky"),
        &effective_ts,
    );

    // Should NOT have updated the holder's jurisdiction yet (migration pending)
    assert_eq!(
        client.get_holder_jurisdiction(&issuer, &symbol_short!("def"), &token, &holder),
        None
    );

    // Migration state should be stored
    let migration =
        client.get_jurisdiction_migration(&issuer, &symbol_short!("def"), &token, &holder);
    assert!(migration.is_some());
    let m = migration.unwrap();
    assert_eq!(m.old_jurisdiction, symbol_short!("jur_none"));
    assert_eq!(m.new_jurisdiction, symbol_short!("ky"));
    assert_eq!(m.effective_ts, effective_ts);
    // deadline = effective_ts + 7 days (default grace)
    assert_eq!(m.deadline, effective_ts + 7 * 24 * 60 * 60);

    assert!(env.events().all().len() > events_before);
}

#[test]
fn jurisdiction_migration_immediate_sets_jurisdiction_directly() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    // effective_ts = 0 means immediate
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("jp"),
        &0u64,
    );

    assert_eq!(
        client.get_holder_jurisdiction(&issuer, &symbol_short!("def"), &token, &holder),
        Some(symbol_short!("jp"))
    );

    // No migration should be stored
    assert!(client
        .get_jurisdiction_migration(&issuer, &symbol_short!("def"), &token, &holder)
        .is_none());
}

#[test]
fn claim_during_grace_period_succeeds() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);
    let now = env.ledger().timestamp();

    // Set up holder jurisdiction and allowed list
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("us"),
        &0u64,
    );
    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us")],
    );
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &4_000, &1);

    // Deposit revenue
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    // Schedule jurisdiction migration to a disallowed jurisdiction
    let effective_ts = now + 3600 * 24; // 1 day in the future
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("cn"),
        &effective_ts,
    );

    // Claim should succeed - grace period is active (7 days > 1 day)
    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(payout, 40_000);
}

#[test]
fn claim_after_grace_period_with_disallowed_jurisdiction_fails() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);
    let now = env.ledger().timestamp();

    // Set up holder jurisdiction and allowed list
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("us"),
        &0u64,
    );
    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us")],
    );
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &4_000, &1);

    // Deposit revenue
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    // Schedule jurisdiction migration to a disallowed jurisdiction
    let effective_ts = now + 3600; // 1 hour in the future
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("cn"),
        &effective_ts,
    );

    // Advance past the grace period (7 days + 1 hour)
    advance_ledger(&env, 7 * 24 * 3600 + 7200);

    // Claim should now fail
    let result = client.try_claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(result, Err(Ok(RevoraError::JurisdictionMigrationDeadlineExceeded)));
}

#[test]
fn claim_after_grace_period_with_allowed_jurisdiction_succeeds() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);
    let now = env.ledger().timestamp();

    // Set up holder jurisdiction and allowed list (allows both "us" and "uk")
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("us"),
        &0u64,
    );
    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us"), symbol_short!("uk")],
    );
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &4_000, &1);

    // Deposit revenue
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    // Schedule migration to "uk" which is in the allowlist
    let effective_ts = now + 3600;
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("uk"),
        &effective_ts,
    );

    // Advance past the grace period
    advance_ledger(&env, 7 * 24 * 3600 + 7200);

    // Claim should succeed because "uk" is allowed
    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(payout, 40_000);
}

#[test]
fn migration_into_disallowed_jurisdiction_at_exact_grace_end() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);
    let now = env.ledger().timestamp();

    // Set up holder jurisdiction and allowed list
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("us"),
        &0u64,
    );
    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us")],
    );
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &4_000, &1);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    let grace_secs = 60 * 60; // 1 hour grace
    client.set_jurisdiction_grace_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &grace_secs,
    );

    let effective_ts = now + 3600; // 1 hour from now
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("cn"),
        &effective_ts,
    );

    // Advance exactly to deadline (effective_ts + grace_secs)
    advance_ledger(&env, 3600 + grace_secs);

    // At exact deadline, claim should fail (deadline is inclusive)
    let result = client.try_claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(result, Err(Ok(RevoraError::JurisdictionMigrationDeadlineExceeded)));
}

#[test]
fn configurable_grace_period_is_honored() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);
    let now = env.ledger().timestamp();

    // Set a 1-hour grace period
    client.set_jurisdiction_grace_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &(60 * 60),
    );

    let effective_ts = now + 1800; // 30 minutes in the future
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("sg"),
        &effective_ts,
    );

    let migration =
        client.get_jurisdiction_migration(&issuer, &symbol_short!("def"), &token, &holder);
    assert!(migration.is_some());
    let m = migration.unwrap();
    // deadline = effective_ts + 1 hour (configured grace)
    assert_eq!(m.deadline, effective_ts + 60 * 60);

    // Verify grace period getter
    assert_eq!(
        client.get_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token),
        60 * 60
    );
}

#[test]
fn default_grace_period_is_seven_days() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();

    // When not configured, should return 7 days default
    let grace = client.get_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token);
    assert_eq!(grace, 7 * 24 * 60 * 60);
}

#[test]
fn claim_with_no_pending_migration_succeeds_normally() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    // Normal setup without any migration
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("us"),
        &0u64,
    );
    client.set_allowed_jurisdictions(
        &issuer,
        &symbol_short!("def"),
        &token,
        &soroban_sdk::vec![&env, symbol_short!("us")],
    );
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &4_000, &1);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(payout, 40_000);
}

#[test]
fn migration_with_no_allowlist_never_blocks_claims() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let holder = Address::generate(&env);
    let now = env.ledger().timestamp();

    // No allowlist configured (empty = gating disabled)
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("us"),
        &0u64,
    );
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &4_000, &1);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100_000, &1);

    // Schedule migration into any jurisdiction
    let effective_ts = now + 3600;
    client.set_holder_jurisdiction(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &symbol_short!("xx"),
        &effective_ts,
    );

    // Advance past grace period
    advance_ledger(&env, 7 * 24 * 3600 + 7200);

    // Claim should succeed because no allowlist is configured
    let payout = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(payout, 40_000);
}

#[test]
fn get_jurisdiction_migration_returns_none_when_no_migration_pending() {
    let (env, client, issuer, token, _payout_asset) = setup_offering();
    let holder = Address::generate(&env);

    assert!(client
        .get_jurisdiction_migration(&issuer, &symbol_short!("def"), &token, &holder)
        .is_none());
}
