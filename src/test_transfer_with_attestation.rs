//! # Tests for `transfer_with_attestation`
//!
//! Covers every guard in the function (see numbered guards in the implementation):
//!
//! | Guard | Tested by |
//! |-------|-----------|
//! | 1 — global freeze / pause            | `transfer_blocked_when_frozen`, `transfer_blocked_when_paused` |
//! | 2 — dual-party auth (host panic)     | `[#ignore]` tests documented below |
//! | 3 — self-transfer rejection          | `self_transfer_rejected` |
//! | 10 — zero shares rejected            | `zero_shares_rejected` |
//! | 4 — offering existence / issuer      | `unknown_offering_rejected`, `wrong_issuer_rejected` |
//! | 5 — offering frozen                  | `transfer_blocked_when_offering_frozen` |
//! | 6 — blacklist (from or to)           | `blacklisted_from_rejected`, `blacklisted_to_rejected` |
//! | 7 — whitelist enforcement            | `whitelist_unlisted_from_rejected`, `whitelist_unlisted_to_rejected`, `whitelist_both_listed_succeeds` |
//! | 8 — insufficient shares              | `insufficient_shares_rejected` |
//! | 9 — recipient cap                    | `recipient_share_cap_rejected` |
//! | event emission                       | `event_payload_correct` |
//! | storage invariants                   | `shares_updated_correctly`, `share_total_invariant` |
//! | happy path                           | `happy_path_full_transfer`, `happy_path_partial_transfer` |

#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, BytesN, Env, IntoVal, Val, Vec,
};

use crate::{DataKey, OfferingId, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};

// ── Shared helpers ────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Deterministic 32-byte hash for use as attestation hash.
fn attest(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0xabu8; 32])
}

/// Deterministic 32-byte network ID for tests.
fn test_network_id(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

/// Register an offering with a single issuer (1-of-1 quorum) and return
/// (client, issuer, token).
fn setup_offering(env: &Env) -> (RevoraRevenueShareClient<'_>, Address, Address) {
    env.mock_all_auths();
    env.ledger().set_network_id([0x01u8; 32]);
    let client = make_client(env);
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let ns = symbol_short!("def");
    let payout = Address::generate(env);
    client.register_offering(&issuer, &Vec::new(env), &1u32, &ns, &token, &1_000, &payout, &0);
    (client, issuer, token)
}

/// Set `holder`'s share to `bps` for the default offering.
fn set_share(
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    token: &Address,
    holder: &Address,
    bps: u32,
) {
    client.set_holder_share(issuer, &symbol_short!("def"), token, holder, &bps);
}

/// Read the `HolderShareTotal` for the default offering directly from storage.
fn read_total(env: &Env, contract_id: &Address, issuer: &Address, token: &Address) -> u32 {
    let offering_id = OfferingId {
        issuer: issuer.clone(),
        namespace: symbol_short!("def"),
        token: token.clone(),
    };
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::HolderShareTotal(offering_id))
            .unwrap_or(0u32)
    })
}

// ── Guard 1: global freeze / pause ───────────────────────────────────────────

#[test]
fn transfer_blocked_when_frozen() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    client.freeze();

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));
    // State must be unchanged
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 1_000);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 0);
}

#[test]
fn transfer_blocked_when_paused() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    let admin = Address::generate(&env);
    client.set_admin(&admin);
    client.pause_admin(&admin);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractPaused)));
}

// ── Guard 3: self-transfer rejection ─────────────────────────────────────────

#[test]
fn self_transfer_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let holder = Address::generate(&env);
    set_share(&client, &issuer, &token, &holder, 2_000);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &holder,
        &500u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidTransferParticipants)));
    // Share is unchanged
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder), 2_000);
}

// ── Guard 10: zero-shares rejected ───────────────────────────────────────────

#[test]
fn zero_shares_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &0u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidShareBps)));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 1_000);
}

// ── Guard 4: offering existence and issuer identity ──────────────────────────

#[test]
fn unknown_offering_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // No offering registered
    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn wrong_issuer_rejected() {
    let env = Env::default();
    let (client, _real_issuer, token) = setup_offering(&env);
    let fake_issuer = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let result = client.try_transfer_with_attestation(
        &fake_issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

// ── Guard 5: offering-level freeze ───────────────────────────────────────────

#[test]
fn transfer_blocked_when_offering_frozen() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    // Freeze the offering
    client.freeze_offering(&issuer, &issuer, &symbol_short!("def"), &token);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingFrozen)));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 1_000);
}

// ── Guard 6: blacklist checks ────────────────────────────────────────────────

#[test]
fn blacklisted_from_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    client.blacklist_add(&issuer, &issuer, &symbol_short!("def"), &token, &from);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::HolderBlacklisted)));
    // Share unchanged
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 1_000);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 0);
}

#[test]
fn blacklisted_to_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    client.blacklist_add(&issuer, &issuer, &symbol_short!("def"), &token, &to);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::HolderBlacklisted)));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 1_000);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 0);
}

// ── Guard 7: whitelist (allowlist) enforcement ───────────────────────────────

/// When a whitelist is active, a `from` that is not listed is rejected even if `to` is.
#[test]
fn whitelist_unlisted_from_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    // Enable whitelist: only `to` is listed; `from` is not
    client.whitelist_add(&issuer, &issuer, &symbol_short!("def"), &token, &to);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::NotAuthorized)));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 1_000);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 0);
}

/// When a whitelist is active, a `to` that is not listed is rejected even if `from` is.
#[test]
fn whitelist_unlisted_to_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    // Enable whitelist: only `from` is listed; `to` is not
    client.whitelist_add(&issuer, &issuer, &symbol_short!("def"), &token, &from);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::NotAuthorized)));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 1_000);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 0);
}

/// When both parties are whitelisted the transfer proceeds.
#[test]
fn whitelist_both_listed_succeeds() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    client.whitelist_add(&issuer, &issuer, &symbol_short!("def"), &token, &from);
    client.whitelist_add(&issuer, &issuer, &symbol_short!("def"), &token, &to);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 500);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 500);
}

/// When whitelist is disabled (empty), transfers are not gated by it.
#[test]
fn no_whitelist_transfer_unrestricted() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    // Whitelist is empty (disabled) — no whitelist check should fire
    assert!(!client.is_whitelist_enabled(&issuer, &symbol_short!("def"), &token));

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Ok(Ok(())));
}

// ── Guard 8: insufficient shares ─────────────────────────────────────────────

#[test]
fn insufficient_shares_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    // from has 500 bps but tries to transfer 600
    set_share(&client, &issuer, &token, &from, 500);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &600u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidShareBps)));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 500);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 0);
}

/// `from` with zero share cannot transfer anything.
#[test]
fn zero_holder_share_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    // from has no share (default 0)

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &1u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidShareBps)));
}

// ── Guard 9: recipient share cap ─────────────────────────────────────────────

#[test]
fn recipient_share_cap_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    // from has 3_000; to already has 8_000; transfer 3_000 would push to to 11_000
    set_share(&client, &issuer, &token, &from, 3_000);
    set_share(&client, &issuer, &token, &to, 8_000);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &3_000u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidShareBps)));
    // State unchanged
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 3_000);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 8_000);
}

/// Transfer that brings `to` to exactly 10_000 bps is allowed (boundary).
#[test]
fn recipient_share_at_cap_boundary_allowed() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 5_000);
    set_share(&client, &issuer, &token, &to, 5_000);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &5_000u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 0);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 10_000);
}

// ── Happy path tests ──────────────────────────────────────────────────────────

/// Full share transfer: `from` surrenders all their shares to `to`.
#[test]
fn happy_path_full_transfer() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 4_000);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &4_000u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 0);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 4_000);
}

/// Partial share transfer: `from` retains some shares.
#[test]
fn happy_path_partial_transfer() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 6_000);
    set_share(&client, &issuer, &token, &to, 1_000);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &2_500u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 3_500);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 3_500);
}

/// Transfer of 1 bps (minimum granularity).
#[test]
fn minimum_granularity_one_bps() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 100);

    let result = client.try_transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &1u32,
        &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 99);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 1);
}

// ── Storage invariants ────────────────────────────────────────────────────────

/// `HolderShareTotal` must not change after a peer-to-peer transfer because
/// the total BPS across all holders is invariant (redistribution, not creation).
#[test]
fn share_total_invariant_after_transfer() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    // Re-create with a known contract_id so we can inspect storage
    let client2 = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    client2.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &1_000, &payout, &0);
    client2.set_holder_share(&issuer, &ns, &token, &from, &4_000);
    client2.set_holder_share(&issuer, &ns, &token, &to, &2_000);

    let total_before = read_total(&env, &contract_id, &issuer, &token);
    assert_eq!(total_before, 6_000);

    client2.transfer_with_attestation(&issuer, &ns, &token, &from, &to, &1_500u32, &symbol_short!("def"), &attest(&env),
        &test_network_id(&env, 0x01));

    let total_after = read_total(&env, &contract_id, &issuer, &token);
    assert_eq!(total_after, 6_000, "HolderShareTotal must be invariant across peer-to-peer transfer");

    assert_eq!(client2.get_holder_share(&issuer, &ns, &token, &from), 2_500);
    assert_eq!(client2.get_holder_share(&issuer, &ns, &token, &to), 3_500);
}

/// A subsequent `set_holder_share` after a transfer must see the correct totals
/// and enforce the 10_000 bps cap correctly.
#[test]
fn subsequent_set_holder_share_respects_post_transfer_state() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let charlie = Address::generate(&env);

    // alice=4000, bob=3000 (total=7000)
    set_share(&client, &issuer, &token, &alice, 4_000);
    set_share(&client, &issuer, &token, &bob, 3_000);

    // Transfer 2000 from alice to bob → alice=2000, bob=5000 (total=7000)
    client.transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &alice, &bob, &2_000u32, &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &alice), 2_000);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &bob), 5_000);

    // charlie can get up to 3000 bps (10000 - 7000)
    assert!(client.try_set_holder_share(&issuer, &symbol_short!("def"), &token, &charlie, &3_000u32).is_ok());

    // charlie cannot get 3001 — total would be 10001
    let r = client.try_set_holder_share(&issuer, &symbol_short!("def"), &token, &charlie, &3_001u32);
    assert_eq!(r, Err(Ok(RevoraError::InvalidShareBps)));
}

// ── Event emission ────────────────────────────────────────────────────────────

/// The `xfer_att` event must carry (from, to, shares_bps, attest_hash) as its
/// data payload and (EVENT_XFER_ATT, issuer, namespace, token) as its topic.
#[test]
fn event_payload_correct() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 5_000);

    let hash = BytesN::from_array(&env, &[0x42u8; 32]);
    let before = env.events().all().len();

    client.transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &from, &to, &2_000u32, &hash, &test_network_id(&env, 0x01),
    );

    let events = env.events().all();
    assert!(events.len() > before, "at least one event must be emitted");

    // Find the xfer_att event
    let xfer_att_sym = symbol_short!("xfer_att");
    let mut found = false;
    for i in before..events.len() {
        let (_, topics, data) = events.get(i).unwrap();
        let topics_vec: soroban_sdk::Vec<Val> = topics.clone().into_val(&env);
        let topic_sym: soroban_sdk::Symbol = topics_vec.get(0).unwrap().into_val(&env);
        if topic_sym == xfer_att_sym {
            // Verify topic contains issuer, namespace, token
            let ev_issuer: Address = topics_vec.get(1).unwrap().into_val(&env);
            let ev_ns: soroban_sdk::Symbol = topics_vec.get(2).unwrap().into_val(&env);
            let ev_token: Address = topics_vec.get(3).unwrap().into_val(&env);
            assert_eq!(ev_issuer, issuer);
            assert_eq!(ev_ns, symbol_short!("def"));
            assert_eq!(ev_token, token);

            // Verify data: (from, to, shares_bps, attest_hash)
            let data_vec: soroban_sdk::Vec<Val> = data.clone().into_val(&env);
            let ev_from: Address = data_vec.get(0).unwrap().into_val(&env);
            let ev_to: Address = data_vec.get(1).unwrap().into_val(&env);
            let ev_bps: u32 = data_vec.get(2).unwrap().into_val(&env);
            let ev_hash: BytesN<32> = data_vec.get(3).unwrap().into_val(&env);

            assert_eq!(ev_from, from);
            assert_eq!(ev_to, to);
            assert_eq!(ev_bps, 2_000u32);
            assert_eq!(ev_hash, hash);
            found = true;
            break;
        }
    }
    assert!(found, "xfer_att event must be emitted with correct payload");
}

/// Each transfer emits exactly one `xfer_att` event.
#[test]
fn exactly_one_xfer_att_event_per_transfer() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 5_000);

    let xfer_att_sym = symbol_short!("xfer_att");
    let before = env.events().all().len();

    client.transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &from, &to, &500u32, &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );

    let events = env.events().all();
    let count = (before..events.len())
        .filter(|&i| {
            let (_, topics, _) = events.get(i as u32).unwrap();
            let tv: soroban_sdk::Vec<Val> = topics.into_val(&env);
            let s: soroban_sdk::Symbol = tv.get(0).unwrap().into_val(&env);
            s == xfer_att_sym
        })
        .count();
    assert_eq!(count, 1, "exactly one xfer_att event per transfer");
}

// ── Attestation hash validation ───────────────────────────────────────────────

/// All-zeros attestation hash is accepted (contract does not inspect content).
#[test]
fn zero_attest_hash_accepted() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    let zero_hash = BytesN::from_array(&env, &[0u8; 32]);
    let result = client.try_transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &from, &to, &500u32, &symbol_short!("def"),
        &zero_hash,
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Ok(Ok(())));
}

/// All-ones attestation hash is accepted.
#[test]
fn all_ones_attest_hash_accepted() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    let ones_hash = BytesN::from_array(&env, &[0xffu8; 32]);
    let result = client.try_transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &from, &to, &500u32, &symbol_short!("def"),
        &ones_hash,
        &test_network_id(&env, 0x01),
    );
    assert_eq!(result, Ok(Ok(())));
}

#[test]
fn matching_network_id_is_accepted() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    let network_id = test_network_id(&env, 0x42);
    env.ledger().set_network_id([0x42u8; 32]);

    let result = client.try_transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &from, &to, &500u32, &symbol_short!("def"),
        &attest(&env),
        &network_id,
    );
    assert_eq!(result, Ok(Ok(())));
}

#[test]
fn mismatched_network_id_is_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    env.ledger().set_network_id([0x42u8; 32]);
    let mismatched_network_id = test_network_id(&env, 0x43);

    let result = client.try_transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &from, &to, &500u32, &symbol_short!("def"),
        &attest(&env),
        &mismatched_network_id,
    );
    assert_eq!(result, Err(Ok(RevoraError::NetworkIdMismatch)));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 1_000);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 0);
}

// ── Multi-hop and chained transfers ──────────────────────────────────────────

/// A→B then B→C chains correctly; total shares remain invariant.
#[test]
fn chained_transfers_maintain_total() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    set_share(&client, &issuer, &token, &a, 6_000);

    let ns = symbol_short!("def");

    // A→B: 2000
    client.transfer_with_attestation(&issuer, &ns, &token, &a, &b, &2_000u32, &symbol_short!("def"), &attest(&env),
        &test_network_id(&env, 0x01));
    // B→C: 1000
    client.transfer_with_attestation(&issuer, &ns, &token, &b, &c, &1_000u32, &symbol_short!("def"), &attest(&env),
        &test_network_id(&env, 0x01));

    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &a), 4_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &b), 1_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &c), 1_000);
    // Total = 6000 (invariant)
}

/// Multiple transfers from the same `from` address are correctly accumulated.
#[test]
fn multiple_transfers_from_same_holder() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to1 = Address::generate(&env);
    let to2 = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 9_000);

    let ns = symbol_short!("def");
    client.transfer_with_attestation(&issuer, &ns, &token, &from, &to1, &3_000u32, &symbol_short!("def"), &attest(&env),
        &test_network_id(&env, 0x01));
    client.transfer_with_attestation(&issuer, &ns, &token, &from, &to2, &3_000u32, &symbol_short!("def"), &attest(&env),
        &test_network_id(&env, 0x01));

    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &from), 3_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &to1), 3_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &to2), 3_000);
}

// ── Cross-offering isolation ──────────────────────────────────────────────────

/// A transfer on offering A must not affect offering B, even when the same
/// holder addresses appear in both.
#[test]
fn transfer_does_not_affect_other_offerings() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout = Address::generate(&env);
    let ns = symbol_short!("def");
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token_a, &1_000, &payout, &0);
    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token_b, &1_000, &payout, &0);

    // Set shares in both offerings
    client.set_holder_share(&issuer, &ns, &token_a, &from, &4_000);
    client.set_holder_share(&issuer, &ns, &token_b, &from, &6_000);

    // Transfer on offering A only
    client.transfer_with_attestation(&issuer, &ns, &token_a, &from, &to, &2_000u32, &symbol_short!("def"), &attest(&env),
        &test_network_id(&env, 0x01));

    // Offering A shares updated
    assert_eq!(client.get_holder_share(&issuer, &ns, &token_a, &from), 2_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token_a, &to), 2_000);

    // Offering B shares unchanged
    assert_eq!(client.get_holder_share(&issuer, &ns, &token_b, &from), 6_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token_b, &to), 0);
}

// ── Auth layer documentation ──────────────────────────────────────────────────

/// `transfer_with_attestation` without auth mock causes a host panic (non-unwinding
/// in no_std). These tests are ignored in the standard test harness because
/// `try_*` cannot catch a non-unwinding host abort.
///
/// On-network, missing auth surfaces as a transaction-level failure, not a
/// `RevoraError` discriminant.
#[test]
#[ignore = "require_auth causes non-unwinding panic in no_std; use mock_all_auths to test auth paths"]
fn transfer_without_from_auth_causes_host_panic() {
    let env = Env::default();
    // Intentionally no mock_all_auths
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);
    // This will abort the host — cannot be caught by try_
    let _ = client.try_transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &from, &to, &500u32, &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
    );
}
