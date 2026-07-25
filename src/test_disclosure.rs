//! Tests for the `update_disclosure` / `get_disclosure` feature (#485).
//!
//! ## Coverage matrix
//!
//! | Scenario | Expected |
//! |----------|----------|
//! | Happy path: set URI + hash, retrieve via `get_disclosure` | `Ok(())`, values round-trip |
//! | URI exactly 256 bytes | `Ok(())` (boundary allowed) |
//! | URI 257 bytes | `DisclosureUriTooLong` |
//! | Empty URI with non-zero hash | `InconsistentDisclosure` |
//! | Empty URI with zero hash | `Ok(())` (clears or no-ops) |
//! | Overwrite existing disclosure | latest values stored |
//! | Unknown offering | `OfferingNotFound` |
//! | Wrong issuer caller | `OfferingNotFound` |
//! | Event emitted on success | at least one new event |

#![cfg(test)]

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Bytes, BytesN, Env,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payment_token = Address::generate(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("ns"),
        &offering_token,
        &5_000,
        &payment_token,
        &0,
    );

    (env, client, issuer, offering_token, payment_token)
}

fn uri_256(env: &Env) -> Bytes {
    Bytes::from_slice(env, &[b'u'; 256])
}

fn uri_257(env: &Env) -> Bytes {
    Bytes::from_slice(env, &[b'u'; 257])
}

fn zero_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

fn sample_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(
        env,
        &[
            0xde, 0xad, 0xbe, 0xef, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09,
            0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
            0x18, 0x19, 0x1a, 0x1b,
        ],
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn update_disclosure_happy_path() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let uri = Bytes::from_slice(&env, b"ipfs://QmTest");
    let hash = sample_hash(&env);

    client.update_disclosure(&issuer, &ns, &token, &uri, &hash);

    let stored = client.get_disclosure(&issuer, &ns, &token).unwrap();
    assert_eq!(stored.uri, uri);
    assert_eq!(stored.hash, hash);
}

#[test]
fn update_disclosure_uri_exactly_256_bytes_is_allowed() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let uri = uri_256(&env);
    let hash = sample_hash(&env);

    let result = client.try_update_disclosure(&issuer, &ns, &token, &uri, &hash);
    assert!(result.is_ok(), "URI of exactly 256 bytes must be accepted");
}

#[test]
fn update_disclosure_uri_257_bytes_rejected() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let uri = uri_257(&env);
    let hash = sample_hash(&env);

    let result = client.try_update_disclosure(&issuer, &ns, &token, &uri, &hash);
    assert_eq!(result, Err(Ok(RevoraError::DisclosureUriTooLong)));
}

#[test]
fn update_disclosure_empty_uri_nonzero_hash_rejected() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let uri = Bytes::from_slice(&env, b"");
    let hash = sample_hash(&env);

    let result = client.try_update_disclosure(&issuer, &ns, &token, &uri, &hash);
    assert_eq!(result, Err(Ok(RevoraError::InconsistentDisclosure)));
}

#[test]
fn update_disclosure_empty_uri_zero_hash_allowed() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let uri = Bytes::from_slice(&env, b"");
    let hash = zero_hash(&env);

    let result = client.try_update_disclosure(&issuer, &ns, &token, &uri, &hash);
    assert!(result.is_ok(), "empty URI with zero hash must be accepted (clears disclosure)");
}

#[test]
fn update_disclosure_overwrites_existing() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");

    let uri1 = Bytes::from_slice(&env, b"ipfs://first");
    let hash1 = sample_hash(&env);
    client.update_disclosure(&issuer, &ns, &token, &uri1, &hash1);

    let uri2 = Bytes::from_slice(&env, b"https://second.example.com/doc.pdf");
    let hash2 = BytesN::from_array(&env, &[0xaa; 32]);
    client.update_disclosure(&issuer, &ns, &token, &uri2, &hash2);

    let stored = client.get_disclosure(&issuer, &ns, &token).unwrap();
    assert_eq!(stored.uri, uri2);
    assert_eq!(stored.hash, hash2);
}

#[test]
fn get_disclosure_returns_none_when_not_set() {
    let (_env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");

    assert!(client.get_disclosure(&issuer, &ns, &token).is_none());
}

#[test]
fn update_disclosure_unknown_offering_returns_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let uri = Bytes::from_slice(&env, b"ipfs://anything");
    let hash = sample_hash(&env);

    let result = client.try_update_disclosure(&issuer, &symbol_short!("ns"), &token, &uri, &hash);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn update_disclosure_wrong_issuer_returns_not_found() {
    let (env, client, _real_issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let attacker = Address::generate(&env);
    let uri = Bytes::from_slice(&env, b"ipfs://evil");
    let hash = sample_hash(&env);

    let result = client.try_update_disclosure(&attacker, &ns, &token, &uri, &hash);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn update_disclosure_emits_event() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let uri = Bytes::from_slice(&env, b"ipfs://QmEventTest");
    let hash = sample_hash(&env);

    let before = env.events().all().len();
    client.update_disclosure(&issuer, &ns, &token, &uri, &hash);
    assert!(
        env.events().all().len() > before,
        "expected at least one new event after update_disclosure"
    );
}
