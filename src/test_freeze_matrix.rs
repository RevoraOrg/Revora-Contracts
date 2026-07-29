
//! # Emergency Holder Freeze Tests
//!
//! Comprehensive tests for the emergency holder freeze feature:
//! - Successful freeze and claim blocking
//! - Successful unfreeze with matching reason
//! - Unfreeze failure with mismatched reason
//! - Unauthorized freeze/unfreeze attempts
//! - is_holder_frozen correctness
//! - Event emission (frz_set, frz_clr)
//! - OFAC attestation auto-freeze with idempotency (auto_frz event)

#![cfg(test)]

use crate::{FreezeReason, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, BytesN, Env, Symbol, Val, Vec,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

// ─── Shared helpers ───────────────────────────────────────────────────────────

/// Assert that a `try_*` result is exactly `ContractFrozen` and nothing else.
///
/// This is the single source of truth for the "frozen" assertion.  All matrix
/// tests call this helper so that a future error-code change only needs to be
/// updated here.
fn assert_frozen_err<T: core::fmt::Debug>(
    result: Result<T, Result<RevoraError, soroban_sdk::InvokeError>>,
) {
    match result {
        Err(Ok(RevoraError::ContractFrozen)) => {} // expected
        other => panic!("expected ContractFrozen, got {:?}", other),
    }
}

/// Build a fresh client, initialize with admin + safety, register one offering,
/// freeze the contract, and return everything needed by the tests.
fn frozen_setup(
    env: &Env,
) -> (
    RevoraRevenueShareClient<'_>,
    Address, // admin
    Address, // issuer (== admin for simplicity)
    Address, // token
    Address, // payout_asset
) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let safety = Address::generate(env);
    client.initialize(&admin, &Some(safety.clone()), &None::<bool>);

    let issuer = admin.clone();
    let token = Address::generate(env);
    let payout_asset = Address::generate(env);
    client.register_offering(&issuer, &symbol_short!("ns"), &token, &1_000u32, &payout_asset, &0i128, &symbol_short!(""), &0);

    // Freeze the contract — all subsequent mutating calls must return ContractFrozen.
    client.freeze();

    (client, admin, issuer, token, payout_asset)
}

// ─── 1. Issuer / offering registration ───────────────────────────────────────

#[test]
fn frozen_register_offering_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, _, payout_asset) = frozen_setup(&env);
    let new_token = Address::generate(&env);
    let result = client.try_register_offering(
        &issuer,
        &symbol_short!("ns2"),
        &new_token,
        &500u32,
        &payout_asset,
        &0i128,
        &symbol_short!(""),
        &0);
    assert_frozen_err(result);
    // Verify no partial write: offering must not exist.
    assert!(client.get_offering(&issuer, &symbol_short!("ns2"), &new_token).is_none());
}

// ─── 2. Revenue reporting ─────────────────────────────────────────────────────

#[test]
fn frozen_report_revenue_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, payout_asset) = frozen_setup(&env);
    let result = client.try_report_revenue(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &payout_asset,
        &10_000i128,
        &1u64,
        &false,
    );
    assert_frozen_err(result);
    // No audit summary should have been written.
    assert!(client.get_audit_summary(&issuer, &symbol_short!("ns"), &token).is_none());
}

// ─── 3. Revenue deposit ───────────────────────────────────────────────────────

#[test]
fn frozen_deposit_revenue_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, payout_asset) = frozen_setup(&env);
    let result = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &payout_asset,
        &10_000i128,
        &1u64,
    );
    assert_frozen_err(result);
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("ns"), &token), 0);
}

#[test]
fn frozen_deposit_revenue_with_snapshot_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, payout_asset) = frozen_setup(&env);
    let result = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &payout_asset,
        &10_000i128,
        &1u64,
        &1u64,
    );
    assert_frozen_err(result);
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("ns"), &token), 0);
}

// ─── 4. Holder share management ──────────────────────────────────────────────

#[test]
fn frozen_set_holder_share_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let holder = Address::generate(&env);
    let result =
        client.try_set_holder_share(&issuer, &symbol_short!("ns"), &token, &holder, &500u32);
    assert_frozen_err(result);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("ns"), &token, &holder), 0);
}

// ─── 5. Blacklist management ──────────────────────────────────────────────────

#[test]
fn frozen_blacklist_add_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let investor = Address::generate(&env);
    let result =
        client.try_blacklist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);
    assert_frozen_err(result);
    assert!(!client.is_blacklisted(&issuer, &symbol_short!("ns"), &token, &investor));
}

#[test]
fn frozen_blacklist_remove_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let investor = Address::generate(&env);
    let result =
        client.try_blacklist_remove(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);
    assert_frozen_err(result);
}

// ─── 6. Whitelist management ──────────────────────────────────────────────────

#[test]
fn frozen_whitelist_add_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let investor = Address::generate(&env);
    let result =
        client.try_whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);
    assert_frozen_err(result);
    assert!(!client.is_whitelisted(&issuer, &symbol_short!("ns"), &token, &investor));
}

#[test]
fn frozen_whitelist_remove_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let investor = Address::generate(&env);
    let result =
        client.try_whitelist_remove(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);
    assert_frozen_err(result);
}

// ─── 7. Concentration limit ───────────────────────────────────────────────────

#[test]
fn frozen_set_concentration_limit_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let result = client.try_set_concentration_limit(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &5_000u32,
        &true,
        &0u64,
    );
    assert_frozen_err(result);
    assert!(client.get_concentration_limit(&issuer, &symbol_short!("ns"), &token).is_none());
}

#[test]
fn frozen_report_concentration_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let result =
        client.try_report_concentration(&issuer, &symbol_short!("ns"), &token, &3_000u32);
    assert_frozen_err(result);
    assert_eq!(client.get_current_concentration(&issuer, &symbol_short!("ns"), &token), 0);
}

// ─── 8. Rounding mode ────────────────────────────────────────────────────────

#[test]
fn frozen_set_rounding_mode_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let result = client.try_set_rounding_mode(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &RoundingMode::RoundHalfUp,
    );
    assert_frozen_err(result);
    // Default rounding mode must be unchanged.
    assert_eq!(
        client.get_rounding_mode(&issuer, &symbol_short!("ns"), &token),
        RoundingMode::Truncation
    );
}

// ─── 9. Investment constraints ────────────────────────────────────────────────

#[test]
fn frozen_set_investment_constraints_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let result = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &100i128,
        &10_000i128,
    );
    assert_frozen_err(result);
    assert!(client.get_investment_constraints(&issuer, &symbol_short!("ns"), &token).is_none());
}

// ─── 10. Minimum revenue threshold ───────────────────────────────────────────

#[test]
fn frozen_set_min_revenue_threshold_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let result =
        client.try_set_min_revenue_threshold(&issuer, &symbol_short!("ns"), &token, &500i128);
    assert_frozen_err(result);
    assert_eq!(client.get_min_revenue_threshold(&issuer, &symbol_short!("ns"), &token), 0);
}

// ─── 11. Claim delay ─────────────────────────────────────────────────────────

#[test]
fn frozen_set_claim_delay_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let result = client.try_set_claim_delay(&issuer, &symbol_short!("ns"), &token, &3600u64);
    assert_frozen_err(result);
    assert_eq!(client.get_claim_delay(&issuer, &symbol_short!("ns"), &token), 0);
}

// ─── 12. Report / claim windows ──────────────────────────────────────────────

#[test]
fn frozen_set_report_window_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let result =
        client.try_set_report_window(&issuer, &symbol_short!("ns"), &token, &100u64, &200u64);
    assert_frozen_err(result);
    assert!(client.get_report_window(&issuer, &symbol_short!("ns"), &token).is_none());
}

#[test]
fn frozen_set_claim_window_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let result =
        client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &100u64, &200u64);
    assert_frozen_err(result);
    assert!(client.get_claim_window(&issuer, &symbol_short!("ns"), &token).is_none());
}

// ─── 13. Snapshot configuration ──────────────────────────────────────────────

#[test]
fn frozen_set_snapshot_config_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let result = client.try_set_snapshot_config(&issuer, &symbol_short!("ns"), &token, &true);
    assert_frozen_err(result);
    assert!(!client.get_snapshot_config(&issuer, &symbol_short!("ns"), &token));
}

#[test]
fn frozen_commit_snapshot_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let hash = BytesN::<32>::from_array(&env, &[0u8; 32]);
    let result =
        client.try_commit_snapshot(&issuer, &symbol_short!("ns"), &token, &1u64, &hash);
    assert_frozen_err(result);
    assert!(client.get_snapshot_entry(&issuer, &symbol_short!("ns"), &token, &1u64).is_none());
}

#[test]
fn frozen_apply_snapshot_shares_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let holder = Address::generate(&env);
    let holders: Vec<(Address, u32)> = {
        let mut v = Vec::new(&env);
        v.push_back((holder.clone(), 1_000u32));
        v
    };
    let result = client.try_apply_snapshot_shares(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &1u64,
        &0u32,
        &holders,
    );
    assert_frozen_err(result);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("ns"), &token, &holder), 0);
}

// ─── 14. Meta-delegate ────────────────────────────────────────────────────────

#[test]
fn frozen_set_meta_delegate_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let delegate = Address::generate(&env);
    let result =
        client.try_set_meta_delegate(&issuer, &symbol_short!("ns"), &token, &delegate);
    assert_frozen_err(result);
    assert!(client.get_meta_delegate(&issuer, &symbol_short!("ns"), &token).is_none());
}

// ─── 15. Admin rotation ───────────────────────────────────────────────────────

#[test]
fn frozen_propose_admin_rotation_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, _, _, _) = frozen_setup(&env);
    let new_admin = Address::generate(&env);
    let result = client.try_propose_admin_rotation(&new_admin);
    assert_frozen_err(result);
    assert!(client.get_pending_admin_rotation().is_none());
}

#[test]
fn frozen_finalize_admin_rotation_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, _, _, _) = frozen_setup(&env);
    let new_admin = Address::generate(&env);
    // finalize_admin_rotation checks frozen before checking pending state
    let result = client.try_finalize_admin_rotation(&new_admin);
    assert_frozen_err(result);
}

#[test]
fn frozen_cancel_admin_rotation_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, _, _, _) = frozen_setup(&env);
    let result = client.try_cancel_admin_rotation();
    assert_frozen_err(result);
}

// ─── 16. Offering-scoped freeze controls ─────────────────────────────────────

#[test]
fn frozen_freeze_offering_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    // freeze_offering itself checks global freeze first (fail-closed)
    let result =
        client.try_freeze_offering(&issuer, &issuer, &symbol_short!("ns"), &token);
    assert_frozen_err(result);
}

#[test]
fn frozen_unfreeze_offering_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
    let result =
        client.try_unfreeze_offering(&issuer, &issuer, &symbol_short!("ns"), &token);
    assert_frozen_err(result);
}

// ─── 17. Audit repair ────────────────────────────────────────────────────────

#[test]
fn frozen_repair_audit_summary_returns_contract_frozen() {
    let env = Env::default();
    let (client, admin, issuer, token, _) = frozen_setup(&env);
    let result = client.try_repair_audit_summary(
        &admin,
        &issuer,
        &symbol_short!("ns"),
        &token,
    );
    assert_frozen_err(result);
}

// ─── 18. Migration ────────────────────────────────────────────────────────────

#[test]
fn frozen_migrate_returns_contract_frozen() {
    let env = Env::default();
    let (client, _, _, _, _) = frozen_setup(&env);
    let result = client.try_migrate();
    assert_frozen_err(result);
}

// ─── 19. Intentional exceptions — claim is NOT blocked ───────────────────────

/// `claim` must succeed (or fail for a business reason, never ContractFrozen).
/// This test verifies the intentional exception: holders can always exit.
#[test]
fn frozen_claim_is_not_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("test");
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("ns"), &token, &1_000u32, &payout_asset, &0i128, &symbol_short!(""), &0);

    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &symbol_short!("ns"), &token, &holder, &1_000u32);

    // Freeze the contract.
    client.freeze();
    assert!(client.is_frozen());

    // claim must NOT return ContractFrozen — it should return NoPendingClaims
    // (no periods deposited) rather than ContractFrozen.
    let result = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50u32);
    match result {
        Err(Ok(RevoraError::ContractFrozen)) => {
            panic!("claim must not be blocked by global freeze")
        }
        _ => {} // any other result (including NoPendingClaims) is acceptable
    }
}

/// After a frozen `report_revenue` call, the audit summary must remain absent.
#[test]
fn frozen_report_revenue_no_partial_write() {
    let env = Env::default();
    let (client, _, issuer, token, payout_asset) = frozen_setup(&env);

    let _ = client.try_report_revenue(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &payout_asset,
        &1_000i128,
        &1u64,
        &false,
    );

    // Audit summary must not have been created.
    assert!(client.get_audit_summary(&issuer, &symbol_short!("ns"), &token).is_none());
    // Revenue index must be zero.
    assert_eq!(client.get_revenue_by_period(&issuer, &symbol_short!("ns"), &token, &1u64), 0);
}

/// After a frozen `set_holder_share`, the holder's share must remain 0.
#[test]
fn frozen_set_holder_share_no_partial_write() {
    let env = Env::default();
    let (client, _, issuer, token, _) = frozen_setup(&env);
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

// ─── #605 set_freeze reason tests ────────────────────────────────────────────

/// Helper: fresh env + initialized client (not yet frozen).
fn unfrozen_client(
    env: &Env,
) -> (
    RevoraRevenueShareClient<'_>,
    Address, // admin
) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let safety = Address::generate(env);
    client.initialize(&admin, &Some(safety), &None::<bool>);
    (client, admin)
}

/// `set_freeze` persists the reason and emits a `frz_set` event carrying it.
#[test]
fn set_freeze_records_reason_and_emits_event() {
    let env = Env::default();
    let (client, _admin) = unfrozen_client(&env);

    // No reason stored before freeze.
    assert!(client.get_freeze_reason().is_none());

    client.set_freeze(&FreezeReason::LegalHold);

    // Contract must be frozen.
    assert!(client.is_frozen());
    // Reason must be persisted.
    assert_eq!(client.get_freeze_reason(), Some(FreezeReason::LegalHold));

    // A frz_set event must have been emitted.
    let events = env.events().all();
    let found = events.iter().any(|e| {
        let (_, topics, _) = e;
        topics.len() >= 1 && {
            let t0: Symbol = topics.get(0).unwrap().into_val(&env);
            t0 == symbol_short!("frz_set")
        }
    });
    assert!(found, "expected frz_set event after set_freeze");

    // A frz_rsn (freeze_reason_v1) event must have been emitted with reason and target.
    let found_rsn = events.iter().any(|e| {
        let (_, topics, _) = e;
        topics.len() >= 1 && {
            let t0: Symbol = topics.get(0).unwrap().into_val(&env);
            t0 == symbol_short!("frz_rsn")
        }
    });
    assert!(found_rsn, "expected frz_rsn event after set_freeze");
}

/// Sequential `set_freeze` calls with different reasons overwrite the stored reason.
#[test]
fn set_freeze_sequential_different_reasons() {
    let env = Env::default();
    let (client, _admin) = unfrozen_client(&env);

    client.set_freeze(&FreezeReason::DisputeOpen);
    assert_eq!(client.get_freeze_reason(), Some(FreezeReason::DisputeOpen));

    client.set_freeze(&FreezeReason::SanctionsMatch);
    assert_eq!(client.get_freeze_reason(), Some(FreezeReason::SanctionsMatch));
}

/// `freeze()` (the no-arg legacy entrypoint) must record `Compliance` as the reason.
#[test]
fn default_freeze_sets_compliance_reason() {
    let env = Env::default();
    let (client, _admin) = unfrozen_client(&env);

    client.freeze();

    assert!(client.is_frozen());
    assert_eq!(
        client.get_freeze_reason(),
        Some(FreezeReason::Compliance),
        "freeze() must record Compliance as the default reason"
    );

    // freeze() via set_freeze must also emit frz_rsn
    let events = env.events().all();
    let found_rsn = events.iter().any(|e| {
        let (_, topics, _) = e;
        topics.len() >= 1 && {
            let t0: Symbol = topics.get(0).unwrap().into_val(&env);
            t0 == symbol_short!("frz_rsn")
        }
    });
    assert!(found_rsn, "expected frz_rsn event from freeze() (default set_freeze)");
}

// ─── OFAC Attestation Auto-Freeze Tests ─────────────────────────────────────────

/// Helper to set up an offering with a holder for OFAC attestation tests.
fn ofac_setup(env: &Env) -> (RevoraRevenueShareClient<'_>, Address, Address, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let ns = symbol_short!("ofac");
    let payout = Address::generate(env);
    
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &Vec::new(env), &1u32, &ns, &token, &1_000, &payout, &0);
    
    (client, admin, issuer, token)
}

/// OFAC attestation should auto-freeze the targeted holder.
#[test]
fn ofac_attestation_auto_freezes_holder() {
    let env = Env::default();
    let (client, _admin, issuer, token) = ofac_setup(&env);
    let holder = Address::generate(&env);
    let ns = symbol_short!("ofac");
    
    // Set up holder with shares
    client.set_holder_share(&issuer, &ns, &token, &holder, &1_000);
    
    // Submit OFAC attestation
    let attestation_hash = BytesN::from_array(&env, &[0x01u8; 32]);
    client.process_ofac_attestation(&attestation_hash, &issuer, &ns, &token, &holder);
    
    // Verify holder is frozen
    assert!(client.is_holder_frozen(&issuer, &ns, &token, &holder));
}

/// OFAC attestation replay should be idempotent - same hash should not re-freeze.
#[test]
fn ofac_attestation_replay_is_idempotent() {
    let env = Env::default();
    let (client, _admin, issuer, token) = ofac_setup(&env);
    let holder = Address::generate(&env);
    let ns = symbol_short!("ofac");
    
    client.set_holder_share(&issuer, &ns, &token, &holder, &1_000);
    
    let attestation_hash = BytesN::from_array(&env, &[0x02u8; 32]);
    
    // First attestation should freeze
    client.process_ofac_attestation(&attestation_hash, &issuer, &ns, &token, &holder);
    assert!(client.is_holder_frozen(&issuer, &ns, &token, &holder));
    
    // Replay with same hash should succeed (idempotent) but not change state
    let result = client.try_process_ofac_attestation(&attestation_hash, &issuer, &ns, &token, &holder);
    assert!(result.is_ok());
    assert!(client.is_holder_frozen(&issuer, &ns, &token, &holder)); // Still frozen
}

/// OFAC attestation should emit auto_frz event with correct payload.
#[test]
fn ofac_attestation_emits_auto_frz_event() {
    let env = Env::default();
    let (client, _admin, issuer, token) = ofac_setup(&env);
    let holder = Address::generate(&env);
    let ns = symbol_short!("ofac");
    
    client.set_holder_share(&issuer, &ns, &token, &holder, &1_000);
    
    let attestation_hash = BytesN::from_array(&env, &[0x03u8; 32]);
    let before = env.events().all().len();
    
    client.process_ofac_attestation(&attestation_hash, &issuer, &ns, &token, &holder);
    
    // Find and verify auto_frz event
    let events = env.events().all();
    let auto_frz_sym = symbol_short!("auto_frz");
    let mut found = false;
    
    for i in before..events.len() {
        let (_, topics, data) = events.get(i).unwrap();
        let topics_vec: soroban_sdk::Vec<Val> = topics.clone().into_val(&env);
        let topic_sym: soroban_sdk::Symbol = topics_vec.get(0).unwrap().into_val(&env);
        
        if topic_sym == auto_frz_sym {
            // Verify topic contains issuer, namespace, token
            let ev_issuer: Address = topics_vec.get(1).unwrap().into_val(&env);
            let ev_ns: soroban_sdk::Symbol = topics_vec.get(2).unwrap().into_val(&env);
            let ev_token: Address = topics_vec.get(3).unwrap().into_val(&env);
            assert_eq!(ev_issuer, issuer);
            assert_eq!(ev_ns, ns);
            assert_eq!(ev_token, token);
            
            // Verify data: (holder, attestation_hash)
            let data_vec: soroban_sdk::Vec<Val> = data.clone().into_val(&env);
            let ev_holder: Address = data_vec.get(0).unwrap().into_val(&env);
            let ev_hash: BytesN<32> = data_vec.get(1).unwrap().into_val(&env);
            assert_eq!(ev_holder, holder);
            assert_eq!(ev_hash, attestation_hash);
            
            found = true;
            break;
        }
    }
    assert!(found, "auto_frz event must be emitted with correct payload");
}

/// OFAC attestation should fail when contract is globally frozen.
#[test]
fn ofac_attestation_blocked_when_contract_frozen() {
    let env = Env::default();
    let (client, admin, issuer, token) = ofac_setup(&env);
    let holder = Address::generate(&env);
    let ns = symbol_short!("ofac");
    
    client.set_holder_share(&issuer, &ns, &token, &holder, &1_000);
    
    // Freeze the contract globally
    client.freeze();
    
    let attestation_hash = BytesN::from_array(&env, &[0x04u8; 32]);
    let result = client.try_process_ofac_attestation(&attestation_hash, &issuer, &ns, &token, &holder);
    
    assert!(result.is_err());
    // Holder should NOT be frozen (operation failed)
    assert!(!client.is_holder_frozen(&issuer, &ns, &token, &holder));
}

/// Multiple different OFAC attestations should each freeze independently.
#[test]
fn multiple_ofac_attestations_freeze_independently() {
    let env = Env::default();
    let (client, _admin, issuer, token) = ofac_setup(&env);
    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);
    let ns = symbol_short!("ofac");
    
    client.set_holder_share(&issuer, &ns, &token, &holder1, &500);
    client.set_holder_share(&issuer, &ns, &token, &holder2, &500);
    
    let hash1 = BytesN::from_array(&env, &[0x05u8; 32]);
    let hash2 = BytesN::from_array(&env, &[0x06u8; 32]);
    
    // Freeze holder1 with first attestation
    client.process_ofac_attestation(&hash1, &issuer, &ns, &token, &holder1);
    assert!(client.is_holder_frozen(&issuer, &ns, &token, &holder1));
    assert!(!client.is_holder_frozen(&issuer, &ns, &token, &holder2));
    
    // Freeze holder2 with second attestation
    client.process_ofac_attestation(&hash2, &issuer, &ns, &token, &holder2);
    assert!(client.is_holder_frozen(&issuer, &ns, &token, &holder1));
    assert!(client.is_holder_frozen(&issuer, &ns, &token, &holder2));
}

/// OFAC attestation should use SanctionsMatch as freeze reason.
#[test]
fn ofac_attestation_uses_sanctions_match_reason() {
    let env = Env::default();
    let (client, _admin, issuer, token) = ofac_setup(&env);
    let holder = Address::generate(&env);
    let ns = symbol_short!("ofac");
    
    client.set_holder_share(&issuer, &ns, &token, &holder, &1_000);
    
    let attestation_hash = BytesN::from_array(&env, &[0x07u8; 32]);
    client.process_ofac_attestation(&attestation_hash, &issuer, &ns, &token, &holder);
    
    // Verify frozen with correct reason by checking unfreeze requires matching reason
    let result = client.try_emergency_unfreeze_holder(
        &issuer,
        &issuer,
        &ns,
        &token,
        &holder,
        &FreezeReason::SanctionsMatch,
    );
    assert!(result.is_ok());
    assert!(!client.is_holder_frozen(&issuer, &ns, &token, &holder));
}
