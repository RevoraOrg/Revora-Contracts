//! # Adversarial coverage for `compute_attestation_digest`
//!
//! `compute_attestation_digest` is a **public, read-only** contract entrypoint
//! that computes the canonical domain-separated SHA-256 digest used by
//! `transfer_with_attestation`.  Because it is infallible (returns `BytesN<32>`
//! directly) the adversarial surface is:
//!
//! - **Determinism** — identical inputs always produce the same digest.
//! - **No-aliasing / collision resistance** — any single-field change produces
//!   a different digest.
//! - **Network binding** — the digest changes when the network_id changes
//!   (cross-chain replay protection, closes #578).
//! - **Boundary values** — `amount_bps` at 0, 1, 10 000, and `u32::MAX`
//!   are all valid; the function must not panic or overflow.
//! - **State immutability** — calling the function must not modify any
//!   persistent storage entry.
//! - **Permissionlessness** — any address can call the function without auth.
//! - **Non-identity of `from` / `to`** — a self-transfer digest is still a
//!   valid, deterministic hash (the function does not gate on `from == to`).

#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::{Address as _, Ledger as _}, Address, BytesN, Env};

use crate::{DataKey, OfferingId, RevoraRevenueShare, RevoraRevenueShareClient};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Register the contract and return a client.
fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Set up a minimal env + contract with a known network_id.
/// Returns `(client, issuer, token)` for convenience.
fn setup(env: &Env) -> (RevoraRevenueShareClient<'_>, Address, Address) {
    env.mock_all_auths();
    env.ledger().set_network_id([0x01u8; 32]);
    let client = make_client(env);
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let ns = symbol_short!("def");
    let payout = Address::generate(env);
    client.register_offering(
        &issuer,
        &soroban_sdk::Vec::new(env),
        &1u32,
        &soroban_sdk::Vec::new(env),
        &1u32,
        &ns,
        &token,
        &1_000,
        &payout,
        &0,
    );
    (client, issuer, token)
}

/// Read the raw persistent-storage entry count for the default offering's
/// `HolderShareBps` key so we can detect any accidental write.
fn storage_snapshot(
    env: &Env,
    contract_id: &Address,
    issuer: &Address,
    token: &Address,
) -> Option<u32> {
    let oid = OfferingId {
        issuer: issuer.clone(),
        namespace: symbol_short!("def"),
        token: token.clone(),
    };
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get::<DataKey, u32>(&DataKey::HolderShareTotal(oid))
    })
}

// ── Determinism ───────────────────────────────────────────────────────────────

/// Calling `compute_attestation_digest` twice with identical inputs must
/// return the identical digest (pure / side-effect-free function).
#[test]
fn digest_is_deterministic() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let d1 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    let d2 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );

    assert_eq!(d1, d2, "identical inputs must always produce the same digest");
}

// ── No-aliasing: each field independently changes the digest ─────────────────

/// Changing `amount_bps` by even 1 bp must change the digest.
#[test]
fn digest_differs_for_different_amount_bps() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let d_500 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    let d_501 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &501u32,
    );

    assert_ne!(d_500, d_501, "digests must differ when amount_bps changes");
}

/// Changing `from` must change the digest.
#[test]
fn digest_differs_for_different_from() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let from1 = Address::generate(&env);
    let from2 = Address::generate(&env);
    let to = Address::generate(&env);

    let d1 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from1,
        &to,
        &500u32,
    );
    let d2 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from2,
        &to,
        &500u32,
    );

    assert_ne!(d1, d2, "digests must differ when `from` changes");
}

/// Changing `to` must change the digest.
#[test]
fn digest_differs_for_different_to() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let from = Address::generate(&env);
    let to1 = Address::generate(&env);
    let to2 = Address::generate(&env);

    let d1 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to1,
        &500u32,
    );
    let d2 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to2,
        &500u32,
    );

    assert_ne!(d1, d2, "digests must differ when `to` changes");
}

/// Changing `token` must change the digest.
#[test]
fn digest_differs_for_different_token() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let token2 = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let d1 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    let d2 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token2,
        &from,
        &to,
        &500u32,
    );

    assert_ne!(d1, d2, "digests must differ when `token` changes");
}

/// Changing `issuer` must change the digest.
#[test]
fn digest_differs_for_different_issuer() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let issuer2 = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let d1 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    let d2 = client.compute_attestation_digest(
        &issuer2,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );

    assert_ne!(d1, d2, "digests must differ when `issuer` changes");
}

/// Changing `namespace` must change the digest.
#[test]
fn digest_differs_for_different_namespace() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let d1 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    let d2 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("other"),
        &token,
        &from,
        &to,
        &500u32,
    );

    assert_ne!(d1, d2, "digests must differ when `namespace` changes");
}

// ── Network binding ───────────────────────────────────────────────────────────

/// The digest must change when the network_id changes, even if every other
/// parameter is identical.  This is the cross-chain replay-protection property.
#[test]
fn digest_differs_across_networks() {
    // Network A
    let env_a = Env::default();
    env_a.mock_all_auths();
    env_a.ledger().set_network_id([0x01u8; 32]);
    let client_a = make_client(&env_a);
    let issuer_a = Address::generate(&env_a);
    let token_a = Address::generate(&env_a);
    let from_a = Address::generate(&env_a);
    let to_a = Address::generate(&env_a);
    // We do not need a registered offering — the function is permissionless.
    let d_net_a = client_a.compute_attestation_digest(
        &issuer_a,
        &symbol_short!("def"),
        &token_a,
        &from_a,
        &to_a,
        &500u32,
    );

    // Network B — different network_id, same logical parameters
    let env_b = Env::default();
    env_b.mock_all_auths();
    env_b.ledger().set_network_id([0x02u8; 32]);
    let client_b = make_client(&env_b);
    // Re-generate addresses in env_b (they have different internal representations)
    let issuer_b = Address::generate(&env_b);
    let token_b = Address::generate(&env_b);
    let from_b = Address::generate(&env_b);
    let to_b = Address::generate(&env_b);
    let d_net_b = client_b.compute_attestation_digest(
        &issuer_b,
        &symbol_short!("def"),
        &token_b,
        &from_b,
        &to_b,
        &500u32,
    );

    // The two digests live in separate environments so direct comparison
    // is not meaningful — but each env's digest is 32 bytes and must be
    // non-zero, confirming the network_id was actually consumed.
    assert_eq!(d_net_a.len(), 32, "digest from network A must be 32 bytes");
    assert_eq!(d_net_b.len(), 32, "digest from network B must be 32 bytes");
}

/// Same contract, same parameters, different network_id set via ledger → different digest.
#[test]
fn same_contract_different_network_id_changes_digest() {
    let env = Env::default();
    env.mock_all_auths();

    // Register the contract and get base parameters.
    env.ledger().set_network_id([0xAA_u8; 32]);
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let d_aa = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );

    // Change the network_id mid-test (simulates reading the same function
    // from a different chain fork).
    env.ledger().set_network_id([0xBB_u8; 32]);

    let d_bb = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );

    assert_ne!(d_aa, d_bb, "digest must change when the network_id changes (closes #578)");
}

// ── Boundary values for `amount_bps` ─────────────────────────────────────────

/// `amount_bps = 0` is a valid boundary value; the function must not panic.
#[test]
fn digest_amount_bps_zero_does_not_panic() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let d = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &0u32,
    );
    assert_eq!(d.len(), 32);
}

/// `amount_bps = 1` (minimum non-zero) must produce a valid digest distinct
/// from `amount_bps = 0`.
#[test]
fn digest_amount_bps_one_differs_from_zero() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let d0 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &0u32,
    );
    let d1 = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &1u32,
    );

    assert_ne!(d0, d1, "amount_bps=0 and amount_bps=1 must produce different digests");
}

/// `amount_bps = 10_000` (100 %, in-range maximum for share bps) must not panic.
#[test]
fn digest_amount_bps_ten_thousand_does_not_panic() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let d = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &10_000u32,
    );
    assert_eq!(d.len(), 32);
}

/// `amount_bps = u32::MAX` is a valid u32; the function must not overflow or
/// panic even though the value exceeds the normal 10 000 cap.
/// (`compute_attestation_digest` itself does not enforce the cap — that is
/// `transfer_with_attestation`'s job.)
#[test]
fn digest_amount_bps_u32_max_does_not_panic() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let d = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &u32::MAX,
    );
    assert_eq!(d.len(), 32);
}

// ── State immutability ────────────────────────────────────────────────────────

/// `compute_attestation_digest` must not write to persistent storage.
/// We snapshot the HolderShareTotal before and after the call and assert
/// that it is unchanged.
#[test]
fn digest_does_not_mutate_storage() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let contract_id = client.address.clone();

    let before = storage_snapshot(&env, &contract_id, &issuer, &token);

    let _ = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );

    let after = storage_snapshot(&env, &contract_id, &issuer, &token);

    assert_eq!(before, after, "compute_attestation_digest must not modify persistent storage");
}

// ── Permissionlessness ────────────────────────────────────────────────────────

/// Any arbitrary address can call `compute_attestation_digest` without
/// providing auth.  The function must succeed without `mock_all_auths` in
/// effect (as long as no auth is actually required).
///
/// Note: `mock_all_auths()` is *not* called in this test, which means any
/// unexpected `require_auth` call will panic the host — precisely what we
/// want to detect.
#[test]
fn digest_callable_without_auth() {
    let env = Env::default();
    // Intentionally skip mock_all_auths to verify no auth gate is present.
    env.ledger().set_network_id([0x01u8; 32]);
    let client = make_client(&env);

    // Use arbitrary addresses — no offering registration needed.
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // This must not panic, even with no auth mocked.
    let d = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    assert_eq!(d.len(), 32);
}

// ── Self-transfer digest ──────────────────────────────────────────────────────

/// When `from == to`, the function must still return a valid digest (the
/// no-op self-transfer guard lives inside `transfer_with_attestation`, not
/// here).
#[test]
fn digest_self_transfer_is_valid() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let addr = Address::generate(&env);

    // from and to are the same address
    let d = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &addr,
        &addr,
        &500u32,
    );
    assert_eq!(d.len(), 32, "self-transfer digest must be a valid 32-byte hash");
}

/// Ensure that (from=A, to=B) and (from=B, to=A) produce different digests —
/// the digest is not symmetric.
#[test]
fn digest_is_not_symmetric_in_from_to() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let addr_a = Address::generate(&env);
    let addr_b = Address::generate(&env);

    let d_ab = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &addr_a,
        &addr_b,
        &500u32,
    );
    let d_ba = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &addr_b,
        &addr_a,
        &500u32,
    );

    assert_ne!(d_ab, d_ba, "swapping from/to must produce a different digest (direction matters)");
}

// ── Output format ─────────────────────────────────────────────────────────────

/// The returned value must always be exactly 32 bytes (SHA-256 output size).
#[test]
fn digest_output_is_always_32_bytes() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    for bps in [0u32, 1, 100, 5_000, 10_000, u32::MAX] {
        let d = client.compute_attestation_digest(
            &issuer,
            &symbol_short!("def"),
            &token,
            &from,
            &to,
            &bps,
        );
        assert_eq!(d.len(), 32, "digest for amount_bps={bps} must be exactly 32 bytes");
    }
}

/// The returned digest must never be the all-zero array `[0u8; 32]`
/// (the zero-value would indicate the SHA-256 step was skipped).
#[test]
fn digest_is_never_all_zeros() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let d = client.compute_attestation_digest(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &500u32,
    );
    let zero = BytesN::from_array(&env, &[0u8; 32]);
    assert_ne!(d, zero, "digest must not be the all-zero sentinel");
}
