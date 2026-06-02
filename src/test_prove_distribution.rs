//! Tests for `prove_distribution_for_period`.
//!
//! Covers: normal case, empty holders, unknown period_id, share_bps==0, decimals != 7,
//! RoundHalfUp rounding, ordering affects digest, determinism, and holder cap.

#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient, RoundingMode};
use soroban_sdk::{symbol_short, testutils::Address as _, token, Address, Env, Vec};

fn make_client() -> (Env, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);
    (env, client)
}

fn create_payment_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    token::StellarAssetClient::new(env, token).mint(to, &amount);
}

// ── Normal case ───────────────────────────────────────────────────────────────

#[test]
fn prove_distribution_normal_case() {
    let (env, client) = make_client();

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000u32,
        &payment_token,
        &0i128,
    );

    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder_a, &3_000u32);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder_b, &2_000u32);

    mint(&env, &payment_token, &issuer, 10_000_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &10_000_000i128,
        &1u64,
    );

    let mut holders = Vec::new(&env);
    holders.push_back(holder_a.clone());
    holders.push_back(holder_b.clone());

    let (entries, digest) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1u64,
        &holders,
    );

    assert_eq!(entries.len(), 2);

    let ea = entries.get(0).unwrap();
    assert_eq!(ea.holder, holder_a);
    assert_eq!(ea.share_bps, 3_000u32);
    // 10_000_000 * 3000 / 10000 = 3_000_000
    assert_eq!(ea.normalized_payout, 3_000_000i128);

    let eb = entries.get(1).unwrap();
    assert_eq!(eb.holder, holder_b);
    assert_eq!(eb.share_bps, 2_000u32);
    // 10_000_000 * 2000 / 10000 = 2_000_000
    assert_eq!(eb.normalized_payout, 2_000_000i128);

    // Digest must be 32 bytes and non-zero
    assert_eq!(digest.len(), 32);
    assert_ne!(digest, soroban_sdk::BytesN::from_array(&env, &[0u8; 32]));
}

// ── Digest is deterministic ───────────────────────────────────────────────────

#[test]
fn prove_distribution_digest_is_deterministic() {
    let (env, client) = make_client();

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000u32,
        &payment_token,
        &0i128,
    );

    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder_a, &3_000u32);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder_b, &2_000u32);

    mint(&env, &payment_token, &issuer, 10_000_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &10_000_000i128,
        &1u64,
    );

    let mut holders = Vec::new(&env);
    holders.push_back(holder_a.clone());
    holders.push_back(holder_b.clone());

    let (_, digest1) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1u64,
        &holders,
    );
    let (_, digest2) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1u64,
        &holders,
    );

    assert_eq!(digest1, digest2);
}

// ── Ordering matters: swapped holders produce different digest ────────────────

#[test]
fn prove_distribution_ordering_affects_digest() {
    let (env, client) = make_client();

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000u32,
        &payment_token,
        &0i128,
    );

    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder_a, &3_000u32);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder_b, &2_000u32);

    mint(&env, &payment_token, &issuer, 10_000_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &10_000_000i128,
        &1u64,
    );

    let mut holders_ab = Vec::new(&env);
    holders_ab.push_back(holder_a.clone());
    holders_ab.push_back(holder_b.clone());

    let mut holders_ba = Vec::new(&env);
    holders_ba.push_back(holder_b.clone());
    holders_ba.push_back(holder_a.clone());

    let (_, digest_ab) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1u64,
        &holders_ab,
    );
    let (_, digest_ba) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1u64,
        &holders_ba,
    );

    assert_ne!(digest_ab, digest_ba);
}

// ── Empty holders ─────────────────────────────────────────────────────────────

#[test]
fn prove_distribution_empty_holders() {
    let (env, client) = make_client();

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000u32,
        &payment_token,
        &0i128,
    );

    mint(&env, &payment_token, &issuer, 10_000_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &10_000_000i128,
        &1u64,
    );

    let holders: Vec<Address> = Vec::new(&env);
    let (entries, digest) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1u64,
        &holders,
    );

    assert_eq!(entries.len(), 0);
    // Digest is still a valid 32-byte value (SHA-256 of the empty-entries payload)
    assert_eq!(digest.len(), 32);
}

// ── Unknown period_id ─────────────────────────────────────────────────────────

#[test]
fn prove_distribution_unknown_period_id_returns_zero_payouts() {
    let (env, client) = make_client();

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000u32,
        &payment_token,
        &0i128,
    );

    let holder_a = Address::generate(&env);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder_a, &3_000u32);

    mint(&env, &payment_token, &issuer, 10_000_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &10_000_000i128,
        &1u64,
    );

    let mut holders = Vec::new(&env);
    holders.push_back(holder_a.clone());

    // period 999 was never deposited
    let (entries, _digest) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &999u64,
        &holders,
    );

    assert_eq!(entries.len(), 1);
    let e = entries.get(0).unwrap();
    assert_eq!(e.share_bps, 3_000u32);
    assert_eq!(e.normalized_payout, 0i128);
}

// ── share_bps == 0 ────────────────────────────────────────────────────────────

#[test]
fn prove_distribution_zero_share_bps_yields_zero_payout() {
    let (env, client) = make_client();

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000u32,
        &payment_token,
        &0i128,
    );

    mint(&env, &payment_token, &issuer, 10_000_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &10_000_000i128,
        &1u64,
    );

    let no_share_holder = Address::generate(&env);
    // no set_holder_share call → defaults to 0

    let mut holders = Vec::new(&env);
    holders.push_back(no_share_holder.clone());

    let (entries, _digest) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1u64,
        &holders,
    );

    assert_eq!(entries.len(), 1);
    let e = entries.get(0).unwrap();
    assert_eq!(e.share_bps, 0u32);
    assert_eq!(e.normalized_payout, 0i128);
}

// ── Decimals != 7 (USDC = 6 decimals) ────────────────────────────────────────

#[test]
fn prove_distribution_usdc_6_decimals_normalizes_correctly() {
    let (env, client) = make_client();

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000u32,
        &payment_token,
        &0i128,
    );

    // Configure 6-decimal payment token (USDC-style)
    client.set_payment_token_decimals(&issuer, &symbol_short!("def"), &token, &6u32);

    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000u32);

    // Deposit 1_000_000 raw units (= 1.000000 USDC at 6 decimals)
    mint(&env, &payment_token, &issuer, 1_000_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &1_000_000i128,
        &1u64,
    );

    let mut holders = Vec::new(&env);
    holders.push_back(holder.clone());

    let (entries, _digest) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1u64,
        &holders,
    );

    assert_eq!(entries.len(), 1);
    let e = entries.get(0).unwrap();
    assert_eq!(e.share_bps, 5_000u32);
    // normalize_amount(1_000_000, 6) = 1_000_000 * 10 = 10_000_000
    // compute_share(10_000_000, 5000, Truncation) = 5_000_000
    assert_eq!(e.normalized_payout, 5_000_000i128);
}

// ── RoundHalfUp mode is respected ────────────────────────────────────────────

#[test]
fn prove_distribution_respects_round_half_up_mode() {
    let (env, client) = make_client();

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000u32,
        &payment_token,
        &0i128,
    );
    client.set_rounding_mode(&issuer, &symbol_short!("def"), &token, &RoundingMode::RoundHalfUp);

    let holder = Address::generate(&env);
    // amount=3, bps=5000 → 1.5 → truncation=1, half-up=2
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000u32);

    mint(&env, &payment_token, &issuer, 3);
    client.deposit_revenue(&issuer, &symbol_short!("def"), &token, &payment_token, &3i128, &1u64);

    let mut holders = Vec::new(&env);
    holders.push_back(holder.clone());

    let (entries, _) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1u64,
        &holders,
    );

    let e = entries.get(0).unwrap();
    // RoundHalfUp: 3 * 5000 / 10000 = 1.5 → rounds to 2
    assert_eq!(e.normalized_payout, 2i128);
}

// ── Holders cap at MAX_CHUNK_PERIODS (200) ────────────────────────────────────

#[test]
fn prove_distribution_caps_at_max_chunk_periods() {
    let (env, client) = make_client();

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000u32,
        &payment_token,
        &0i128,
    );

    mint(&env, &payment_token, &issuer, 1_000_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &1_000_000i128,
        &1u64,
    );

    // Build 201 holders (one over the cap)
    let mut holders = Vec::new(&env);
    for _ in 0..201 {
        holders.push_back(Address::generate(&env));
    }

    let (entries, _) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1u64,
        &holders,
    );

    // Should be capped at 200
    assert_eq!(entries.len(), 200);
}

// ── DistributionEntry fields are correct ─────────────────────────────────────

#[test]
fn prove_distribution_entry_fields_match() {
    let (env, client) = make_client();

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000u32,
        &payment_token,
        &0i128,
    );

    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &10_000u32);

    mint(&env, &payment_token, &issuer, 1_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &1_000i128,
        &1u64,
    );

    let mut holders = Vec::new(&env);
    holders.push_back(holder.clone());

    let (entries, _) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1u64,
        &holders,
    );

    let e = entries.get(0).unwrap();
    assert_eq!(e.holder, holder);
    assert_eq!(e.share_bps, 10_000u32);
    // 1000 * 10000 / 10000 = 1000
    assert_eq!(e.normalized_payout, 1_000i128);
}

// ── Different period_ids produce different digests ────────────────────────────

#[test]
fn prove_distribution_different_periods_produce_different_digests() {
    let (env, client) = make_client();

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000u32,
        &payment_token,
        &0i128,
    );

    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000u32);

    mint(&env, &payment_token, &issuer, 20_000_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &10_000_000i128,
        &1u64,
    );
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &10_000_000i128,
        &2u64,
    );

    let mut holders = Vec::new(&env);
    holders.push_back(holder.clone());

    let (_, digest1) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1u64,
        &holders,
    );
    let (_, digest2) = client.prove_distribution_for_period(
        &issuer,
        &symbol_short!("def"),
        &token,
        &2u64,
        &holders,
    );

    // period_id is included in the XDR payload, so different period_ids → different digests
    // even when revenue amounts are identical
    assert_ne!(digest1, digest2);
}
