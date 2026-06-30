
//! # Emergency Holder Freeze Tests
//!
//! Comprehensive tests for the emergency holder freeze feature:
//! - Successful freeze and claim blocking
//! - Successful unfreeze with matching reason
//! - Unfreeze failure with mismatched reason
//! - Unauthorized freeze/unfreeze attempts
//! - is_holder_frozen correctness
//! - Event emission (frz_set, frz_clr)

#![cfg(test)]

use crate::{FreezeReason, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, Symbol,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Symbol, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("test");
    let token = Address::generate(&env);
    let payout = env.register_stellar_asset_contract(admin.clone());
    let holder = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &ns, &token, &2500, &payout, &0);

    soroban_sdk::token::StellarAssetClient::new(&env, &payout).mint(&issuer, &1_000_000);
    client.deposit_revenue(&issuer, &ns, &token, &payout, &100_000, &1);
    client.set_holder_share(&issuer, &ns, &token, &holder, &5_000); // 50%

    (env, client, admin, ns, token, issuer, holder)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn emergency_freeze_blocks_claim() {
    let (env, client, _, ns, token, issuer, holder) = setup();

    // Freeze the holder with Sanctions reason
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &ns,
        &token,
        &holder,
        &FreezeReason::Sanctions,
    );

    // Verify is_holder_frozen returns true
    assert!(client.is_holder_frozen(&issuer, &ns, &token, &holder));

    // Try to claim - should fail with HolderFrozen
    let result = client.try_claim(&holder, &issuer, &ns, &token, &10);
    assert!(result.is_err());
}

#[test]
fn emergency_unfreeze_succeeds_with_matching_reason() {
    let (env, client, _, ns, token, issuer, holder) = setup();

    // Freeze with IssuerDispute
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &ns,
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert!(client.is_holder_frozen(&issuer, &ns, &token, &holder));

    // Unfreeze with the same reason
    client.emergency_unfreeze_holder(
        &issuer,
        &issuer,
        &ns,
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert!(!client.is_holder_frozen(&issuer, &ns, &token, &holder));

    // Now claim should work
    let payout = client.claim(&holder, &issuer, &ns, &token, &10);
    assert!(payout > 0);
}

#[test]
fn emergency_unfreeze_fails_with_mismatched_reason() {
    let (env, client, _, ns, token, issuer, holder) = setup();

    // Freeze with CourtOrder
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &ns,
        &token,
        &holder,
        &FreezeReason::CourtOrder,
    );

    // Try to unfreeze with Manual reason - should fail
    let result = client.try_emergency_unfreeze_holder(
        &issuer,
        &issuer,
        &ns,
        &token,
        &holder,
        &FreezeReason::Manual,
    );
    assert!(result.is_err());
    assert!(client.is_holder_frozen(&issuer, &ns, &token, &holder));
}

#[test]
fn unauthorized_freeze_fails() {
    let (env, client, admin, ns, token, issuer, holder) = setup();
    let unauthorized = Address::generate(&env);

    // Unauthorized user tries to freeze - should fail
    let result = client.try_emergency_freeze_holder(
        &unauthorized,
        &issuer,
        &ns,
        &token,
        &holder,
        &FreezeReason::Sanctions,
    );
    assert!(result.is_err());
    assert!(!client.is_holder_frozen(&issuer, &ns, &token, &holder));
}

#[test]
fn admin_can_freeze_and_unfreeze() {
    let (env, client, admin, ns, token, issuer, holder) = setup();

    // Admin freezes the holder
    client.emergency_freeze_holder(
        &admin,
        &issuer,
        &ns,
        &token,
        &holder,
        &FreezeReason::Manual,
    );
    assert!(client.is_holder_frozen(&issuer, &ns, &token, &holder));

    // Admin unfreezes
    client.emergency_unfreeze_holder(
        &admin,
        &issuer,
        &ns,
        &token,
        &holder,
        &FreezeReason::Manual,
    );
    assert!(!client.is_holder_frozen(&issuer, &ns, &token, &holder));
}

#[test]
fn freeze_emits_frz_set_event() {
    let (env, client, _, ns, token, issuer, holder) = setup();
    let before = env.events().all().len();

    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &ns,
        &token,
        &holder,
        &FreezeReason::Sanctions,
    );

    // Check that frz_set event was emitted
    let events = env.events().all();
    let found = events.iter().any(|e| {
        let (_, topics, _) = e;
        topics.len() >= 1 && {
            let t0: Symbol = topics.get(0).unwrap().into_val(&env);
            t0 == symbol_short!("frz_set")
        }
    });
    assert!(found);
}

#[test]
fn unfreeze_emits_frz_clr_event() {
    let (env, client, _, ns, token, issuer, holder) = setup();
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &ns,
        &token,
        &holder,
        &FreezeReason::Sanctions,
    );
    let before = env.events().all().len();

    client.emergency_unfreeze_holder(
        &issuer,
        &issuer,
        &ns,
        &token,
        &holder,
        &FreezeReason::Sanctions,
    );

    // Check that frz_clr event was emitted
    let events = env.events().all();
    let found = events.iter().skip(before).any(|e| {
        let (_, topics, _) = e;
        topics.len() >= 1 && {
            let t0: Symbol = topics.get(0).unwrap().into_val(&env);
            t0 == symbol_short!("frz_clr")
        }
    });
    assert!(found);
}

#[test]
fn freeze_is_scoped_to_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("test");
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout = env.register_stellar_asset_contract(admin.clone());
    let holder = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &ns, &token_a, &2500, &payout, &0);
    client.register_offering(&issuer, &ns, &token_b, &2500, &payout, &0);

    // Freeze holder on token_a offering
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &ns,
        &token_a,
        &holder,
        &FreezeReason::Sanctions,
    );

    // Should be frozen on token_a
    assert!(client.is_holder_frozen(&issuer, &ns, &token_a, &holder));
    // Should NOT be frozen on token_b
    assert!(!client.is_holder_frozen(&issuer, &ns, &token_b, &holder));
}
