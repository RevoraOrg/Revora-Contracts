//! # Reconciliation Event Completeness — Bug Condition Exploration Tests
//!
//! Task 1: Write bug condition exploration tests.
//!
//! These tests MUST FAIL on unfixed code — failure confirms the bug exists.
//! DO NOT fix the production code when these tests fail.
//!
//! Bug condition:
//! - `deposit_revenue` does NOT emit `EVENT_INDEXED_V2` with `event_type = "rv_dep"`
//! - `set_holder_share` does NOT emit `EVENT_INDEXED_V2` with `event_type = "sh_set"`
//!
//! Requirements: 1.1, 1.3

#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, IntoVal};

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};

/// Helper: register a contract and return a client.
fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Helper: create a Stellar Asset Contract for testing token transfers.
/// Returns (token_contract_address, admin_address).
fn create_payment_token(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract(admin.clone());
    (token_id, admin)
}

/// Helper: mint tokens to a recipient.
fn mint_tokens(env: &Env, payment_token: &Address, recipient: &Address, amount: i128) {
    soroban_sdk::token::StellarAssetClient::new(env, payment_token).mint(recipient, &amount);
}

/// Full setup: env, client, issuer, offering token, payment token.
/// Registers an offering and mints payment tokens to the issuer.
fn setup_offering_with_payment_token(
) -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let (payment_token, _pt_admin) = create_payment_token(&env);

    // Register offering (5000 bps = 50% revenue share)
    client.register_offering(&issuer, &symbol_short!("def"), &token, &5_000, &payment_token, &0);

    // Mint payment tokens to the issuer so they can deposit
    mint_tokens(&env, &payment_token, &issuer, 10_000_000);

    (env, client, issuer, token, payment_token)
}

#[cfg(test)]
mod reconciliation_bug_condition {
    use super::*;

    /// Test 1a: deposit_revenue should emit EVENT_INDEXED_V2 with event_type = "rv_dep".
    ///
    /// This test WILL FAIL on unfixed code because `do_deposit_revenue` does not
    /// emit `EVENT_INDEXED_V2` — it only emits `EVENT_REV_DEPOSIT_V2` ("rev_dep2").
    ///
    /// Counterexample: after calling deposit_revenue, scanning env.events().all()
    /// finds no event whose first topic is symbol_short!("ev_idx2").
    ///
    /// Requirements: 1.1
    #[test]
    fn test_1a_deposit_revenue_missing_rv_dep_indexed_v2() {
        let (env, client, issuer, token, payment_token) = setup_offering_with_payment_token();

        // Call deposit_revenue with valid args
        client.deposit_revenue(
            &issuer,
            &symbol_short!("def"),
            &token,
            &payment_token,
            &100_000i128,
            &1u64,
        );

        // Scan env.events().all() for EVENT_INDEXED_V2 ("ev_idx2") with event_type = "rv_dep"
        let all_events = env.events().all();
        let ev_idx2_val: soroban_sdk::Val = symbol_short!("ev_idx2").into_val(&env);
        let rv_dep_val: soroban_sdk::Val = symbol_short!("rv_dep").into_val(&env);

        let found = all_events.iter().any(|(topics, _data)| {
            // topics is a Vec<Val>; first element should be EVENT_INDEXED_V2
            if topics.get(0) != Some(ev_idx2_val) {
                return false;
            }
            // Second element is EventIndexTopicV2 struct — check event_type field
            // The EventIndexTopicV2 is published as the second topic element.
            // We check by looking for rv_dep in the topics Vec.
            topics.iter().any(|v| v == rv_dep_val)
        });

        assert!(
            found,
            "BUG CONFIRMED: deposit_revenue did not emit EVENT_INDEXED_V2 with event_type='rv_dep'. \
             Only EVENT_REV_DEPOSIT_V2 ('rev_dep2') was emitted. \
             Counterexample: deposit_revenue(issuer, 'def', token, payment_token, 100_000, 1) \
             → ev_idx2 absent from env.events().all()"
        );
    }

    /// Test 1b: set_holder_share should emit EVENT_INDEXED_V2 with event_type = "sh_set".
    ///
    /// This test WILL FAIL on unfixed code because `set_holder_share_internal` does not
    /// emit `EVENT_INDEXED_V2` — it only emits `EVENT_SHARE_SET` ("sh_set" legacy symbol).
    ///
    /// Note: "sh_set" is used as BOTH the legacy EVENT_SHARE_SET topic AND the new
    /// EVENT_TYPE_SH_SET. The bug is that no EVENT_INDEXED_V2 ("ev_idx2") is emitted
    /// at all for set_holder_share. This test checks for ev_idx2 in the first topic position.
    ///
    /// Counterexample: after calling set_holder_share, scanning env.events().all()
    /// finds no event whose first topic is symbol_short!("ev_idx2").
    ///
    /// Requirements: 1.3
    #[test]
    fn test_1b_set_holder_share_missing_sh_set_indexed_v2() {
        let (env, client, issuer, token, _payment_token) = setup_offering_with_payment_token();

        let holder = Address::generate(&env);

        // Call set_holder_share with valid args
        client.set_holder_share(
            &issuer,
            &symbol_short!("def"),
            &token,
            &holder,
            &2_500u32, // 25%
        );

        // Scan env.events().all() for EVENT_INDEXED_V2 ("ev_idx2") with event_type = "sh_set"
        let all_events = env.events().all();
        let ev_idx2_val: soroban_sdk::Val = symbol_short!("ev_idx2").into_val(&env);
        let sh_set_val: soroban_sdk::Val = symbol_short!("sh_set").into_val(&env);

        // Check that at least one event has ev_idx2 as its first topic
        let found_indexed_v2 = all_events.iter().any(|(topics, _data)| {
            topics.get(0) == Some(ev_idx2_val)
        });

        // Also verify sh_set appears somewhere in the events (legacy event IS emitted)
        let found_legacy_sh_set = all_events.iter().any(|(topics, _data)| {
            topics.iter().any(|v| v == sh_set_val)
        });

        // The legacy EVENT_SHARE_SET should be present (it IS emitted today)
        assert!(
            found_legacy_sh_set,
            "Unexpected: legacy EVENT_SHARE_SET ('sh_set') was not emitted by set_holder_share"
        );

        // This assertion FAILS on unfixed code — ev_idx2 is never emitted for set_holder_share
        assert!(
            found_indexed_v2,
            "BUG CONFIRMED: set_holder_share did not emit EVENT_INDEXED_V2 ('ev_idx2'). \
             Only legacy EVENT_SHARE_SET ('sh_set') was emitted. \
             Counterexample: set_holder_share(issuer, 'def', token, holder, 2500) \
             → ev_idx2 absent from env.events().all()"
        );
    }
}
