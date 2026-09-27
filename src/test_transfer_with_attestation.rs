//! # Tests for `transfer_with_attestation` and `verify_attestation_digest`
//!
//! Covers every guard in `transfer_with_attestation` (see numbered guards in the implementation):
//!
//! | Guard | Tested by |
//! |-------|-----------|
//! | 1 — global freeze / pause            | `transfer_blocked_when_frozen`, `transfer_blocked_when_paused` |
//! | 2 — dual-party auth (host panic)     | `[#ignore]` tests documented below |
//! | 3 — self-transfer no-op              | `self_transfer_is_noop` |
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
//! | attestation nonce/expiry             | `expired_attestation_rejected`, `replayed_attestation_nonce_rejected`, `attestation_used_at_exact_expiry` |

#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Bytes, BytesN, Env, IntoVal, Val, Vec,
};

use crate::{DataKey, OfferingId, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, SignedAttestation};

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

/// Default nonce for test attestations.
fn test_nonce() -> u64 {
    1
}

/// Far-future expiry timestamp for tests.
fn test_expires_at() -> u64 {
    u64::MAX
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
    client.register_offering(&issuer,
        &Vec::new(&env),
        &1u32,
        &Vec::new(env),
        &1u32,
        &ns,
        &token,
        &1_000,
        &payout,
        &0);
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
    client.set_holder_share(issuer, &symbol_short!("def"), token, holder, &bps, &1);
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractPaused)));
}

// ── Guard 3: self-transfer is a no-op ────────────────────────────────────────

#[test]
fn self_transfer_is_noop() {
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
        &test_nonce(),
        &test_expires_at(),
    );
    // Self-transfer is allowed as a no-op (returns Ok, state unchanged)
    assert_eq!(result, Ok(Ok(())));
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
        &test_nonce(),
        &test_expires_at(),
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidShareBps)));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 1_000);
}

// ── Guard 4: offering existence and issuer identity ──────────────────────────

#[test]
fn unknown_offering_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env.clone());
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
    let client = make_client(&env.clone());
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    // Re-create with a known contract_id so we can inspect storage
    let client2 = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    client2.register_offering(&issuer,
        &Vec::new(&env),
        &1u32,
        &Vec::new(&env),
        &1u32,
        &ns,
        &token,
        &1_000,
        &payout,
        &0);
    client2.set_holder_share(&issuer, &ns, &token, &from, &4_000, &1);
    client2.set_holder_share(&issuer, &ns, &token, &to, &2_000, &1);

    let total_before = read_total(&env, &contract_id, &issuer, &token);
    assert_eq!(total_before, 6_000);

    client2.transfer_with_attestation(&issuer, &ns, &token, &from, &to, &1_500u32, &symbol_short!("def"), &attest(&env),
        &test_network_id(&env, 0x01), &test_nonce(), &test_expires_at());

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
        &test_nonce(),
        &test_expires_at(),
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

/// The `xfer_att` event must carry (from, to, shares_bps, attest_hash, nonce, expires_at)
/// as its data payload and (EVENT_XFER_ATT, issuer, namespace, token) as its topic.
#[test]
fn event_payload_correct() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 5_000);

    let hash = BytesN::from_array(&env, &[0x42u8; 32]);
    let before = env.events().all().len();
    let nonce = 7u64;
    let expires_at = u64::MAX;

    client.transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &from, &to, &2_000u32, &symbol_short!("def"),
        &hash, &test_network_id(&env, 0x01), &nonce, &expires_at,
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

            // Verify data: (from, to, shares_bps, attest_hash, nonce, expires_at)
            let data_vec: soroban_sdk::Vec<Val> = data.clone().into_val(&env);
            let ev_from: Address = data_vec.get(0).unwrap().into_val(&env);
            let ev_to: Address = data_vec.get(1).unwrap().into_val(&env);
            let ev_bps: u32 = data_vec.get(2).unwrap().into_val(&env);
            let ev_hash: BytesN<32> = data_vec.get(3).unwrap().into_val(&env);
            let ev_nonce: u64 = data_vec.get(4).unwrap().into_val(&env);
            let ev_expires_at: u64 = data_vec.get(5).unwrap().into_val(&env);

            assert_eq!(ev_from, from);
            assert_eq!(ev_to, to);
            assert_eq!(ev_bps, 2_000u32);
            assert_eq!(ev_hash, hash);
            assert_eq!(ev_nonce, nonce);
            assert_eq!(ev_expires_at, expires_at);
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
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
        &test_nonce(),
        &test_expires_at(),
    );
    assert_eq!(result, Err(Ok(RevoraError::NetworkIdMismatch)));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 1_000);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 0);
}

// ── Attestation nonce/expiry validation (issue #561) ──────────────────────────

/// An expired attestation must be rejected.
#[test]
fn expired_attestation_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    // Set ledger timestamp to 1000, expiry to 500 (already expired)
    env.ledger().set_timestamp(1000);
    let expiry = 500u64;

    let result = client.try_transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &from, &to, &500u32, &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
        &test_nonce(),
        &expiry,
    );
    assert_eq!(result, Err(Ok(RevoraError::SignatureExpired)));
    // State must be unchanged
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 1_000);
}

/// Reusing a consumed nonce must be rejected.
#[test]
fn replayed_attestation_nonce_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 2_000);

    let nonce = 42u64;
    let expires_at = u64::MAX;

    // First use should succeed
    let r1 = client.try_transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &from, &to, &500u32, &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
        &nonce,
        &expires_at,
    );
    assert_eq!(r1, Ok(Ok(())));

    // Second use with same nonce (and different `to`) should fail as replay
    let to2 = Address::generate(&env);
    let r2 = client.try_transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &from, &to2, &500u32, &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
        &nonce,
        &expires_at,
    );
    assert_eq!(r2, Err(Ok(RevoraError::SignatureReplay)));
}

/// A nonce consumed by one `from` address must not block a different `from` address
/// from using the same nonce value.
#[test]
fn nonce_is_per_signer() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let charlie = Address::generate(&env);
    set_share(&client, &issuer, &token, &alice, 2_000);
    set_share(&client, &issuer, &token, &bob, 2_000);

    let nonce = 99u64;
    let expires_at = u64::MAX;

    // Alice uses nonce 99
    let r1 = client.try_transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &alice, &charlie, &500u32, &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
        &nonce,
        &expires_at,
    );
    assert_eq!(r1, Ok(Ok(())));

    // Bob uses nonce 99 — should succeed (per-signer scoping)
    let to2 = Address::generate(&env);
    let r2 = client.try_transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &bob, &to2, &500u32, &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
        &nonce,
        &expires_at,
    );
    assert_eq!(r2, Ok(Ok(())));
}

/// Attestation used at the exact expiry second must succeed (boundary condition).
#[test]
fn attestation_used_at_exact_expiry() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    set_share(&client, &issuer, &token, &from, 1_000);

    // Set ledger timestamp to exactly equal the expiry
    let now = 5000u64;
    env.ledger().set_timestamp(now);
    let nonce = 7u64;

    let result = client.try_transfer_with_attestation(
        &issuer, &symbol_short!("def"), &token, &from, &to, &500u32, &symbol_short!("def"),
        &attest(&env),
        &test_network_id(&env, 0x01),
        &nonce,
        &now,
    );
    // now == expires_at, transfer should succeed (expires_at is an inclusive upper bound)
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from), 500);
    assert_eq!(client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to), 500);
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
        &test_network_id(&env, 0x01), &1u64, &u64::MAX);
    // B→C: 1000
    client.transfer_with_attestation(&issuer, &ns, &token, &b, &c, &1_000u32, &symbol_short!("def"), &attest(&env),
        &test_network_id(&env, 0x01), &2u64, &u64::MAX);

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
        &test_network_id(&env, 0x01), &1u64, &u64::MAX);
    client.transfer_with_attestation(&issuer, &ns, &token, &from, &to2, &3_000u32, &symbol_short!("def"), &attest(&env),
        &test_network_id(&env, 0x01), &2u64, &u64::MAX);

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
    let client = make_client(&env.clone());

    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout = Address::generate(&env);
    let ns = symbol_short!("def");
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    client.register_offering(&issuer,
        &Vec::new(&env),
        &1u32,
        &Vec::new(&env),
        &1u32,
        &ns,
        &token_a,
        &1_000,
        &payout,
        &0);
    client.register_offering(&issuer,
        &Vec::new(&env),
        &1u32,
        &Vec::new(&env),
        &1u32,
        &ns,
        &token_b,
        &1_000,
        &payout,
        &0);

    // Set shares in both offerings
    client.set_holder_share(&issuer, &ns, &token_a, &from, &4_000, &1);
    client.set_holder_share(&issuer, &ns, &token_b, &from, &6_000, &1);

    // Transfer on offering A only
    client.transfer_with_attestation(&issuer, &ns, &token_a, &from, &to, &2_000u32, &symbol_short!("def"), &attest(&env),
        &test_network_id(&env, 0x01), &1u64, &u64::MAX);

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
        &test_nonce(),
        &test_expires_at(),
    );
}

// ── Network-id domain separator tests (closes #578) ──────────────────────────
//
// These tests exercise `verify_attestation_digest`, the read-only helper that
// binds a `SignedAttestation` to the current Stellar network.  A testnet
// attestation must be rejected on mainnet, and vice-versa.

/// Helper: build a `SignedAttestation` whose `network_id` matches the default
/// test environment's network id and whose `digest` is the canonically
/// computed attestation digest for the supplied parameters.
fn make_signed_attestation(
    env: &Env,
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    token: &Address,
    from: &Address,
    to: &Address,
    amount_bps: u32,
) -> SignedAttestation {
    let network_id: BytesN<32> = env.ledger().network_id();
    let digest = client.compute_attestation_digest(
        issuer,
        &symbol_short!("def"),
        token,
        from,
        to,
        &amount_bps,
    );
    SignedAttestation { network_id, digest }
}

/// 1. Correct network_id + correct digest → `Ok(())`
///
/// The golden-path: an attestation produced for the current chain passes
/// `verify_attestation_digest` without error.
#[test]
fn verify_attestation_correct_network_id() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let attestation = make_signed_attestation(&env, &client, &issuer, &token, &from, &to, 500);

    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(result, Ok(Ok(())));
}

/// 2. Mainnet network_id submitted to a testnet contract → `NetworkIdMismatch`
///
/// The test environment uses the default network_id (`[0u8; 32]`).  We
/// construct an attestation whose `network_id` is the sha-256 of the mainnet
/// passphrase.  The contract must reject it.
#[test]
fn verify_attestation_mainnet_id_on_testnet_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // sha256("Public Global Stellar Network ; September 2015") — mainnet id
    let mainnet_id: [u8; 32] = [
        0xe9, 0x27, 0xf1, 0x28, 0x74, 0x20, 0x77, 0x64,
        0x06, 0xfe, 0x3b, 0x21, 0x95, 0x70, 0x6f, 0x49,
        0x1b, 0x04, 0x2a, 0xb9, 0x7f, 0xa3, 0x57, 0x6b,
        0xbc, 0x40, 0x85, 0x58, 0xb1, 0x7d, 0x52, 0xd4,
    ];

    // Compute the correct digest using the real (testnet) network_id, then
    // wrap it in an attestation with a *different* (mainnet) network_id.
    let correct_digest = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );

    let attestation = SignedAttestation {
        network_id: BytesN::from_array(&env, &mainnet_id),
        digest: correct_digest,
    };

    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(result, Err(Ok(RevoraError::NetworkIdMismatch)));
}

/// 3. Testnet network_id submitted to a mainnet contract → `NetworkIdMismatch`
///
/// The contract's environment is configured with a mainnet-like network_id.
/// An attestation carrying the testnet network_id must be rejected.
#[test]
fn verify_attestation_testnet_id_on_mainnet_rejected() {
    let env = Env::default();

    // Reconfigure the environment to simulate a mainnet node by giving it a
    // non-zero network_id (distinct from the default `[0u8; 32]` used above).
    let simulated_mainnet_id: [u8; 32] = [
        0xe9, 0x27, 0xf1, 0x28, 0x74, 0x20, 0x77, 0x64,
        0x06, 0xfe, 0x3b, 0x21, 0x95, 0x70, 0x6f, 0x49,
        0x1b, 0x04, 0x2a, 0xb9, 0x7f, 0xa3, 0x57, 0x6b,
        0xbc, 0x40, 0x85, 0x58, 0xb1, 0x7d, 0x52, 0xd4,
    ];
    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: 0,
        protocol_version: 20,
        sequence_number: 1,
        network_id: simulated_mainnet_id,
        base_reserve: 10,
        min_temp_entry_ttl: 10,
        min_persistent_entry_ttl: 10,
        max_entry_ttl: 6_312_000,
    });

    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // sha256("Test SDF Network ; September 2015") — testnet id
    let testnet_id: [u8; 32] = [
        0xce, 0xe0, 0x30, 0x2d, 0x59, 0x84, 0x4d, 0x32,
        0xbd, 0xca, 0x91, 0x5c, 0x82, 0x03, 0xdd, 0x44,
        0xb3, 0x3f, 0xbb, 0x7e, 0xdc, 0x19, 0x05, 0x1e,
        0xa3, 0x7a, 0xbe, 0xdf, 0x28, 0xec, 0xd4, 0x72,
    ];

    // Compute the correct digest using the mainnet network_id (env is now
    // mainnet), then wrap it in an attestation with the *testnet* network_id.
    let correct_digest = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );

    let attestation = SignedAttestation {
        network_id: BytesN::from_array(&env, &testnet_id),
        digest: correct_digest,
    };

    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(result, Err(Ok(RevoraError::NetworkIdMismatch)));
}

/// 4. Unknown / arbitrary network_id → `NetworkIdMismatch`
///
/// Any `network_id` value that does not match `env.ledger().network_id()` is
/// rejected regardless of whether the digest itself is correct.
#[test]
fn verify_attestation_unknown_network_id_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Arbitrary / unknown network_id — all 0xde bytes, not a real network.
    let unknown_id: [u8; 32] = [0xde; 32];

    let correct_digest = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );

    let attestation = SignedAttestation {
        network_id: BytesN::from_array(&env, &unknown_id),
        digest: correct_digest,
    };

    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(result, Err(Ok(RevoraError::NetworkIdMismatch)));
}

/// 5. Correct network_id but wrong digest → `NetworkIdMismatch`
///
/// Even when `network_id` matches the current chain, if the digest does not
/// correspond to the canonical preimage for the supplied parameters the
/// attestation is rejected.  This prevents forgery of attestation hashes.
#[test]
fn verify_attestation_wrong_digest_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let network_id: BytesN<32> = env.ledger().network_id();

    // Deliberately wrong digest — all 0xba bytes.
    let wrong_digest = BytesN::from_array(&env, &[0xba; 32]);

    let attestation = SignedAttestation {
        network_id,
        digest: wrong_digest,
    };

    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(result, Err(Ok(RevoraError::NetworkIdMismatch)));
}

/// 6. Round-trip: `compute_attestation_digest` → `verify_attestation_digest` → `Ok`
///
/// An attestation produced by `compute_attestation_digest` and wrapped in a
/// `SignedAttestation` with the current chain's network_id must pass
/// `verify_attestation_digest` without error, and the returned digest must
/// change when any parameter changes (no-aliasing property).
#[test]
fn attestation_compute_verify_round_trip() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let network_id: BytesN<32> = env.ledger().network_id();

    // ── Round-trip for amount_bps = 500 ──────────────────────────────────────
    let digest_500 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );

    let attestation_500 = SignedAttestation {
        network_id: network_id.clone(),
        digest: digest_500.clone(),
    };

    let result = client.try_verify_attestation_digest(
        &attestation_500,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(result, Ok(Ok(())), "round-trip must succeed for amount_bps=500");

    // ── No-aliasing: digest changes when amount_bps changes ──────────────────
    let digest_1000 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &1_000u32,
    );
    assert_ne!(
        digest_500, digest_1000,
        "digests for different amount_bps must differ (no aliasing)"
    );

    // Using digest_500 with amount_bps=1000 must fail
    let mismatched_attestation = SignedAttestation {
        network_id: network_id.clone(),
        digest: digest_500,
    };
    let result_mismatch = client.try_verify_attestation_digest(
        &mismatched_attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &1_000u32,
    );
    assert_eq!(
        result_mismatch,
        Err(Ok(RevoraError::NetworkIdMismatch)),
        "using digest for 500 bps against params with 1000 bps must be rejected"
    );

    // ── No-aliasing: digest changes when `from` changes ──────────────────────
    let from2 = Address::generate(&env);
    let digest_from2 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from2,
        &to,
        &500u32,
    );
    assert_ne!(
        digest_1000, digest_from2,
        "digests for different `from` addresses must differ"
    );
}

// ── Focused boundary and adversarial tests for `verify_attestation_digest` ───
//
// These tests exercise the boundary and adversarial parameter space not covered
// by the round-trip / network-id suite above.  They are grouped by concern:
//
//   A — amount_bps boundary values (0, 10 000, u32::MAX)
//   B — full parameter no-aliasing (each of issuer / namespace / token / to)
//   C — parameter position sensitivity (swap from ↔ to)
//   D — first-check-wins: both fields wrong → NetworkIdMismatch at step 1
//   E — all-zeros digest with correct network_id → NetworkIdMismatch at step 2
//   F — state invariance: the function is read-only; no storage mutations

// ── A: amount_bps boundary values ────────────────────────────────────────────

/// A1. amount_bps = 0 (minimum boundary) → `Ok(())`
///
/// Zero basis-points is a valid amount_bps (no revenue share).  The digest must
/// be computed and verified without error.
#[test]
fn verify_attestation_amount_bps_zero_accepted() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let attestation = make_signed_attestation(&env, &client, &issuer, &token, &from, &to, 0);

    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &0u32,
    );
    assert_eq!(result, Ok(Ok(())), "amount_bps=0 (min boundary) must be accepted");
}

/// A2. amount_bps = 10 000 (maximum valid offering bps) → `Ok(())`
///
/// 10 000 bps represents a 100% revenue share and is the upper valid boundary
/// for an offering's share allocation.  The round-trip must succeed.
#[test]
fn verify_attestation_amount_bps_max_valid_accepted() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let attestation = make_signed_attestation(&env, &client, &issuer, &token, &from, &to, 10_000);

    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &10_000u32,
    );
    assert_eq!(result, Ok(Ok(())), "amount_bps=10 000 (max valid) must be accepted");
}

/// A3. amount_bps = u32::MAX (extreme boundary) → `Ok(())`
///
/// `verify_attestation_digest` has no upper bound on amount_bps; it only checks
/// the digest.  An attestation produced for u32::MAX must round-trip successfully.
#[test]
fn verify_attestation_amount_bps_u32_max_accepted() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let attestation =
        make_signed_attestation(&env, &client, &issuer, &token, &from, &to, u32::MAX);

    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &u32::MAX,
    );
    assert_eq!(result, Ok(Ok(())), "amount_bps=u32::MAX (extreme boundary) must be accepted");
}

/// A4. Digest computed for amount_bps=0 must not verify for amount_bps=1.
///
/// Confirms that the BE-4-byte encoding of amount_bps participates fully in the
/// preimage and that the zero value is not a special alias for any nonzero value.
#[test]
fn verify_attestation_amount_bps_zero_vs_one_no_alias() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let network_id: BytesN<32> = env.ledger().network_id();

    let digest_0 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &0u32,
    );
    let digest_1 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &1u32,
    );
    assert_ne!(digest_0, digest_1, "digests for bps=0 and bps=1 must differ");

    // Using digest for 0 against params with 1 must be rejected.
    let attestation = SignedAttestation {
        network_id: network_id.clone(),
        digest: digest_0,
    };
    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &1u32,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::NetworkIdMismatch)),
        "digest for bps=0 must not verify against bps=1"
    );
}

// ── B: full parameter no-aliasing ────────────────────────────────────────────
//
// For each of issuer / namespace / token / to, changing only that parameter must
// produce a different digest and must cause verify to return NetworkIdMismatch
// when the old digest is presented with the new parameters.

/// B1. Changing the issuer alone changes the digest.
#[test]
fn verify_attestation_issuer_no_alias() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let network_id: BytesN<32> = env.ledger().network_id();

    let digest_original = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );

    // Distinct issuer address.
    let issuer2 = Address::generate(&env);
    let digest_issuer2 = client.compute_attestation_digest(
        &issuer2,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_ne!(
        digest_original, digest_issuer2,
        "changing issuer must produce a different digest"
    );

    // Presenting the original digest against issuer2's params must fail.
    let attestation = SignedAttestation {
        network_id,
        digest: digest_original,
    };
    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer2,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::NetworkIdMismatch)),
        "digest for issuer_1 must not verify against issuer_2"
    );
}

/// B2. Changing the namespace alone changes the digest.
#[test]
fn verify_attestation_namespace_no_alias() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let network_id: BytesN<32> = env.ledger().network_id();

    let digest_def = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    let digest_alt = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("alt"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_ne!(
        digest_def, digest_alt,
        "changing namespace must produce a different digest"
    );

    // Presenting digest_def against the alt namespace must fail.
    let attestation = SignedAttestation {
        network_id,
        digest: digest_def,
    };
    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("alt"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::NetworkIdMismatch)),
        "digest for namespace 'def' must not verify against namespace 'alt'"
    );
}

/// B3. Changing the token alone changes the digest.
#[test]
fn verify_attestation_token_no_alias() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let network_id: BytesN<32> = env.ledger().network_id();

    let digest_token1 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );

    let token2 = Address::generate(&env);
    let digest_token2 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token2,
        &from,
        &to,
        &500u32,
    );
    assert_ne!(
        digest_token1, digest_token2,
        "changing token must produce a different digest"
    );

    // Presenting digest for token against token2 must fail.
    let attestation = SignedAttestation {
        network_id,
        digest: digest_token1,
    };
    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token2,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::NetworkIdMismatch)),
        "digest for token_1 must not verify against token_2"
    );
}

/// B4. Changing the `to` address alone changes the digest.
#[test]
fn verify_attestation_to_address_no_alias() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to1 = Address::generate(&env);
    let to2 = Address::generate(&env);

    let network_id: BytesN<32> = env.ledger().network_id();

    let digest_to1 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to1,
        &500u32,
    );
    let digest_to2 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to2,
        &500u32,
    );
    assert_ne!(
        digest_to1, digest_to2,
        "changing `to` must produce a different digest"
    );

    // Presenting digest for to1 against to2 must fail.
    let attestation = SignedAttestation {
        network_id,
        digest: digest_to1,
    };
    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to2,
        &500u32,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::NetworkIdMismatch)),
        "digest for to_1 must not verify against to_2"
    );
}

// ── C: parameter position sensitivity (from ↔ to swap) ──────────────────────

/// C1. Swapping `from` and `to` produces a different digest.
///
/// An attestation for (from=A, to=B) must not pass verification for (from=B,
/// to=A).  This ensures the positional encoding of from/to is unambiguous and
/// prevents a receiver from forging an attestation where they appear as sender.
#[test]
fn verify_attestation_from_to_swap_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let addr_a = Address::generate(&env);
    let addr_b = Address::generate(&env);

    let network_id: BytesN<32> = env.ledger().network_id();

    // Digest for (from=A, to=B).
    let digest_ab = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &addr_a,
        &addr_b,
        &500u32,
    );
    // Digest for (from=B, to=A).
    let digest_ba = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &addr_b,
        &addr_a,
        &500u32,
    );
    assert_ne!(
        digest_ab, digest_ba,
        "from/to swap must produce a different digest (position sensitivity)"
    );

    // Presenting digest_ab against the swapped parameters must fail.
    let attestation_ab = SignedAttestation {
        network_id: network_id.clone(),
        digest: digest_ab,
    };
    let result = client.try_verify_attestation_digest(
        &attestation_ab,
        &issuer,
        &symbol_short!("def"),
        &token,
        &addr_b, // swapped
        &addr_a, // swapped
        &500u32,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::NetworkIdMismatch)),
        "attestation for (from=A,to=B) must not verify against (from=B,to=A)"
    );

    // And the reverse: digest_ba against the original (from=A, to=B) must fail.
    let attestation_ba = SignedAttestation {
        network_id,
        digest: digest_ba,
    };
    let result_rev = client.try_verify_attestation_digest(
        &attestation_ba,
        &issuer,
        &symbol_short!("def"),
        &token,
        &addr_a,
        &addr_b,
        &500u32,
    );
    assert_eq!(
        result_rev,
        Err(Ok(RevoraError::NetworkIdMismatch)),
        "attestation for (from=B,to=A) must not verify against (from=A,to=B)"
    );
}

// ── D: first-check-wins — both fields wrong → NetworkIdMismatch at step 1 ───

/// D1. When both `network_id` and `digest` are wrong, the function returns
///     `NetworkIdMismatch` (step 1 check wins before step 2 is reached).
///
/// This test ensures the guard ordering is stable: the network_id gate fires
/// first, so an attacker cannot learn whether their digest is correct by
/// providing a mismatched network_id.
#[test]
fn verify_attestation_both_fields_wrong_returns_network_id_mismatch() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Deliberately wrong on both fields: unknown network_id and garbage digest.
    let wrong_network_id = BytesN::from_array(&env, &[0xde; 32]);
    let wrong_digest = BytesN::from_array(&env, &[0xad; 32]);

    let attestation = SignedAttestation {
        network_id: wrong_network_id,
        digest: wrong_digest,
    };

    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::NetworkIdMismatch)),
        "both fields wrong must still return NetworkIdMismatch (step 1 fires first)"
    );
}

// ── E: all-zeros digest with correct network_id → step-2 rejection ───────────

/// E1. A `SignedAttestation` with the correct `network_id` but an all-zeros
///     digest (BytesN<32> = [0x00; 32]) must be rejected.
///
/// This is the zero-value boundary for the digest field: SHA-256 never produces
/// all-zeros naturally, so this tests that the comparison is strict and that the
/// zero digest cannot pass as a wildcard.
#[test]
fn verify_attestation_zero_digest_with_correct_network_id_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let network_id: BytesN<32> = env.ledger().network_id();

    // Correct network_id, but zero digest — should fail at step 2.
    let zero_digest = BytesN::from_array(&env, &[0x00u8; 32]);
    let attestation = SignedAttestation {
        network_id,
        digest: zero_digest,
    };

    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::NetworkIdMismatch)),
        "all-zeros digest must not pass verification even with a correct network_id"
    );
}

// ── F: state invariance (read-only) ──────────────────────────────────────────
//
// `verify_attestation_digest` must never mutate contract storage.  We verify
// this by reading holder-share state before and after both a successful call
// and a failing call, asserting they are identical.

/// F1. A successful `verify_attestation_digest` call must not alter any
///     storage entry (holder shares are unchanged).
#[test]
fn verify_attestation_success_does_not_mutate_state() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Pre-condition: give `from` a share so we have a concrete storage entry.
    set_share(&client, &issuer, &token, &from, 3_000);
    let share_before = client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from);
    let to_share_before = client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to);

    let attestation = make_signed_attestation(&env, &client, &issuer, &token, &from, &to, 500);
    let result = client.try_verify_attestation_digest(
        &attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(result, Ok(Ok(())), "successful verify must return Ok(())");

    // Post-condition: storage must be identical to pre-condition.
    assert_eq!(
        client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from),
        share_before,
        "from holder's share must be unchanged after successful verify"
    );
    assert_eq!(
        client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to),
        to_share_before,
        "to holder's share must be unchanged after successful verify"
    );
}

/// F2. A rejected `verify_attestation_digest` call (wrong network_id) must not
///     alter any storage entry.
#[test]
fn verify_attestation_failure_does_not_mutate_state() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Pre-condition: give `from` a share so we have a concrete storage entry.
    set_share(&client, &issuer, &token, &from, 4_000);
    let share_before = client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from);
    let to_share_before = client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to);

    // Construct an attestation that will be rejected (wrong network_id).
    let wrong_network_id = BytesN::from_array(&env, &[0xff; 32]);
    let correct_digest = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    let bad_attestation = SignedAttestation {
        network_id: wrong_network_id,
        digest: correct_digest,
    };

    let result = client.try_verify_attestation_digest(
        &bad_attestation,
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::NetworkIdMismatch)),
        "call with wrong network_id must be rejected"
    );

    // Post-condition: storage must be identical to pre-condition.
    assert_eq!(
        client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from),
        share_before,
        "from holder's share must be unchanged after failed verify"
    );
    assert_eq!(
        client.get_holder_share(&issuer, &symbol_short!("def"), &token, &to),
        to_share_before,
        "to holder's share must be unchanged after failed verify"
    );
}

/// F3. Multiple consecutive failed calls (wrong digest) must not accumulate any
///     state changes — all shares remain at their initial values.
#[test]
fn verify_attestation_repeated_failures_do_not_mutate_state() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    set_share(&client, &issuer, &token, &from, 2_500);
    let share_before = client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from);

    let network_id: BytesN<32> = env.ledger().network_id();

    // Five consecutive calls, each with a correct network_id but a wrong digest.
    for i in 0u8..5u8 {
        let bad_digest = BytesN::from_array(&env, &[i; 32]);
        let attestation = SignedAttestation {
            network_id: network_id.clone(),
            digest: bad_digest,
        };
        let result = client.try_verify_attestation_digest(
            &attestation,
            &issuer,
            &symbol_short!("def"),
            &token,
            &from,
            &to,
            &500u32,
        );
        assert_eq!(
            result,
            Err(Ok(RevoraError::NetworkIdMismatch)),
            "iteration {i}: wrong digest must be rejected"
        );
    }

    assert_eq!(
        client.get_holder_share(&issuer, &symbol_short!("def"), &token, &from),
        share_before,
        "from holder's share must be unchanged after five failed verify calls"
    );
}
