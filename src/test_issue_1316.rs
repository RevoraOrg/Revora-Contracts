#![cfg(test)]
//! # Issue #1316 — Signed FX quotes are bound to contract/currency pair and range-checked.
//!
//! `report_revenue_with_attestation` verifies an ed25519-signed `SignedFxQuote`
//! whose payload binds the contract (network), offering, revenue symbol and
//! payout symbol. Quotes replayed across pairs/contracts, zero/negative rates,
//! future timestamps, and overflowing conversions are rejected.

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, SignedFxQuote};
use ed25519_dalek::Signer as _;
use soroban_sdk::{testutils::Address as _, xdr::ToXdr, Address, Bytes, BytesN, Env, Symbol, Vec};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'static> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn setup(
    env: &Env,
) -> (RevoraRevenueShareClient<'static>, Address, Symbol, Address, Address, Address) {
    env.mock_all_auths();
    env.ledger().with_mut(|ledger| ledger.timestamp = 1_000);

    let client = make_client(env);
    let issuer = Address::generate(env);
    let namespace = Symbol::new(env, "def");
    let token = Address::generate(env);
    let payout_asset = Address::generate(env);
    let reported_asset = Address::generate(env);

    client.register_offering(
        &issuer,
        &Vec::new(env),
        &1u32,
        &namespace,
        &token,
        &5_000u32,
        &payout_asset,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    (client, issuer, namespace, token, payout_asset, reported_asset)
}

fn fixed_signing_key() -> ed25519_dalek::SigningKey {
    // Fixed seed for deterministic, reproducible tests.
    ed25519_dalek::SigningKey::from_bytes(&[
        0x5a, 0xcc, 0x72, 0x53, 0x29, 0x5d, 0xfc, 0x35, 0x6c, 0x04, 0x62, 0x97, 0x92, 0x5a, 0x36,
        0x9f, 0x3d, 0x27, 0x62, 0xd0, 0x0a, 0xfd, 0xf2, 0x58, 0x3e, 0xcb, 0xe9, 0x21, 0x80, 0xb0,
        0x7c, 0x37,
    ])
}

/// Register an oracle whose public key corresponds to `sk`, and configure the
/// offering's FX oracle to point at it.
fn configure_oracle(
    env: &Env,
    client: &RevoraRevenueShareClient<'static>,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    sk: &ed25519_dalek::SigningKey,
    revenue_symbol: Symbol,
    payout_symbol: Symbol,
    max_age_secs: u64,
) -> Address {
    let oracle = Address::generate(env);
    let verifying_key = sk.verifying_key();
    let pk_bytes: [u8; 32] = verifying_key.to_bytes();
    let pk = BytesN::from_array(env, &pk_bytes);
    client.register_oracle_pubkey(&oracle, &pk);

    client.set_fx_oracle(
        issuer,
        namespace,
        token,
        &oracle,
        &revenue_symbol,
        &payout_symbol,
        &max_age_secs,
    );
    oracle
}

/// Sign a `SignedFxQuote` over its raw XDR bytes and return `(quote_bytes, signature)`.
fn sign_quote(
    env: &Env,
    sk: &ed25519_dalek::SigningKey,
    quote: &SignedFxQuote,
) -> (Bytes, BytesN<64>) {
    let quote_bytes: Vec<u8> = quote.to_xdr(env);
    let sig: [u8; 64] = sk.sign(quote_bytes.as_slice()).unwrap().to_bytes();
    (Bytes::from_slice(env, &quote_bytes), BytesN::from_array(env, &sig))
}

fn report_with_quote(
    client: &RevoraRevenueShareClient<'static>,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    reported_asset: &Address,
    amount: i128,
    period_id: u64,
    quote_bytes: &Bytes,
    signature: &BytesN<64>,
) -> Result<(), RevoraError> {
    client.try_report_revenue_with_attestation(
        issuer,
        namespace,
        token,
        reported_asset,
        &amount,
        &period_id,
        &false,
        quote_bytes,
        signature,
    )
}

// ── Section A: happy path — correctly bound quote converts as before ───

#[test]
fn signed_quote_happy_path_converts_cross_currency() {
    let env = Env::default();
    let (client, issuer, namespace, token, payout_asset, reported_asset) = setup(&env);
    let sk = fixed_signing_key();
    let _oracle = configure_oracle(
        &env,
        &client,
        &issuer,
        namespace,
        token,
        &sk,
        Symbol::new(&env, "EUR"),
        Symbol::new(&env, "USDC"),
        60,
    );

    let quote = SignedFxQuote {
        contract: env.current_contract_address(),
        revenue_symbol: Symbol::new(&env, "EUR"),
        payout_symbol: Symbol::new(&env, "USDC"),
        rate: 12_000,
        quoted_at: 1_000,
    };
    let (quote_bytes, signature) = sign_quote(&env, &sk, &quote);

    let result = report_with_quote(
        &client,
        &issuer,
        namespace,
        token,
        reported_asset,
        1_000,
        1,
        &quote_bytes,
        &signature,
    );
    assert!(result.is_ok(), "bound quote must be accepted, got {result:?}");
    assert_eq!(
        client.get_revenue_by_period(&issuer, namespace, token, &1),
        1_200,
        "1000 * 12000 / 10000 = 1200"
    );
}

// ── Section B: quote bound to wrong pair → rejected ───

#[test]
fn signed_quote_replayed_on_wrong_pair_rejected() {
    let env = Env::default();
    let (client, issuer, namespace, token, payout_asset, reported_asset) = setup(&env);
    let sk = fixed_signing_key();
    let _oracle = configure_oracle(
        &env,
        &client,
        &issuer,
        namespace,
        token,
        &sk,
        Symbol::new(&env, "EUR"),
        Symbol::new(&env, "USDC"),
        60,
    );

    // Quote signed for EUR/GBP but offering configured for EUR/USDC.
    let quote = SignedFxQuote {
        contract: env.current_contract_address(),
        revenue_symbol: Symbol::new(&env, "EUR"),
        payout_symbol: Symbol::new(&env, "GBP"),
        rate: 12_000,
        quoted_at: 1_000,
    };
    let (quote_bytes, signature) = sign_quote(&env, &sk, &quote);

    let result = report_with_quote(
        &client,
        &issuer,
        namespace,
        token,
        reported_asset,
        1_000,
        1,
        &quote_bytes,
        &signature,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::OracleQuoteMismatch)),
        "quote bound to a different currency pair must be rejected"
    );
}

// ── Section C: quote bound to different contract → rejected ───

#[test]
fn signed_quote_for_different_contract_rejected() {
    let env = Env::default();
    let (client, issuer, namespace, token, payout_asset, reported_asset) = setup(&env);
    let sk = fixed_signing_key();
    let _oracle = configure_oracle(
        &env,
        &client,
        &issuer,
        namespace,
        token,
        &sk,
        Symbol::new(&env, "EUR"),
        Symbol::new(&env, "USDC"),
        60,
    );

    let other_contract = Address::generate(&env);
    let quote = SignedFxQuote {
        contract: other_contract,
        revenue_symbol: Symbol::new(&env, "EUR"),
        payout_symbol: Symbol::new(&env, "USDC"),
        rate: 12_000,
        quoted_at: 1_000,
    };
    let (quote_bytes, signature) = sign_quote(&env, &sk, &quote);

    let result = report_with_quote(
        &client,
        &issuer,
        namespace,
        token,
        reported_asset,
        1_000,
        1,
        &quote_bytes,
        &signature,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::OracleQuoteMismatch)),
        "quote bound to a different contract must be rejected"
    );
}

// ── Section D: rate validation ───

#[test]
fn signed_quote_zero_rate_rejected() {
    let env = Env::default();
    let (client, issuer, namespace, token, payout_asset, reported_asset) = setup(&env);
    let sk = fixed_signing_key();
    let _oracle = configure_oracle(
        &env,
        &client,
        &issuer,
        namespace,
        token,
        &sk,
        Symbol::new(&env, "EUR"),
        Symbol::new(&env, "USDC"),
        60,
    );

    let quote = SignedFxQuote {
        contract: env.current_contract_address(),
        revenue_symbol: Symbol::new(&env, "EUR"),
        payout_symbol: Symbol::new(&env, "USDC"),
        rate: 0,
        quoted_at: 1_000,
    };
    let (quote_bytes, signature) = sign_quote(&env, &sk, &quote);

    let result = report_with_quote(
        &client,
        &issuer,
        namespace,
        token,
        reported_asset,
        1_000,
        1,
        &quote_bytes,
        &signature,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)), "zero rate must be rejected");
}

#[test]
fn signed_quote_negative_rate_rejected() {
    let env = Env::default();
    let (client, issuer, namespace, token, payout_asset, reported_asset) = setup(&env);
    let sk = fixed_signing_key();
    let _oracle = configure_oracle(
        &env,
        &client,
        &issuer,
        namespace,
        token,
        &sk,
        Symbol::new(&env, "EUR"),
        Symbol::new(&env, "USDC"),
        60,
    );

    let quote = SignedFxQuote {
        contract: env.current_contract_address(),
        revenue_symbol: Symbol::new(&env, "EUR"),
        payout_symbol: Symbol::new(&env, "USDC"),
        rate: -1_000,
        quoted_at: 1_000,
    };
    let (quote_bytes, signature) = sign_quote(&env, &sk, &quote);

    let result = report_with_quote(
        &client,
        &issuer,
        namespace,
        token,
        reported_asset,
        1_000,
        1,
        &quote_bytes,
        &signature,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)), "negative rate must be rejected");
}

// ── Section E: future timestamp ───

#[test]
fn signed_quote_future_timestamp_rejected() {
    let env = Env::default();
    let (client, issuer, namespace, token, payout_asset, reported_asset) = setup(&env);
    let sk = fixed_signing_key();
    let _oracle = configure_oracle(
        &env,
        &client,
        &issuer,
        namespace,
        token,
        &sk,
        Symbol::new(&env, "EUR"),
        Symbol::new(&env, "USDC"),
        60,
    );

    let quote = SignedFxQuote {
        contract: env.current_contract_address(),
        revenue_symbol: Symbol::new(&env, "EUR"),
        payout_symbol: Symbol::new(&env, "USDC"),
        rate: 12_000,
        quoted_at: 2_000, // future relative to ledger time 1_000
    };
    let (quote_bytes, signature) = sign_quote(&env, &sk, &quote);

    let result = report_with_quote(
        &client,
        &issuer,
        namespace,
        token,
        reported_asset,
        1_000,
        1,
        &quote_bytes,
        &signature,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::OracleQuoteFuture)),
        "future quoted_at must be rejected"
    );
}

// ── Section F: zero max age → signed path not reusable forever ───

#[test]
fn signed_quote_zero_max_age_rejected() {
    let env = Env::default();
    let (client, issuer, namespace, token, payout_asset, reported_asset) = setup(&env);
    let sk = fixed_signing_key();
    let _oracle = configure_oracle(
        &env,
        &client,
        &issuer,
        namespace,
        token,
        &sk,
        Symbol::new(&env, "EUR"),
        Symbol::new(&env, "USDC"),
        0, // signed path requires non-zero max age
    );

    let quote = SignedFxQuote {
        contract: env.current_contract_address(),
        revenue_symbol: Symbol::new(&env, "EUR"),
        payout_symbol: Symbol::new(&env, "USDC"),
        rate: 12_000,
        quoted_at: 1_000,
    };
    let (quote_bytes, signature) = sign_quote(&env, &sk, &quote);

    let result = report_with_quote(
        &client,
        &issuer,
        namespace,
        token,
        reported_asset,
        1_000,
        1,
        &quote_bytes,
        &signature,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::OracleQuoteStale)),
        "signed quote with max_oracle_age_secs == 0 must be rejected"
    );
}

// ── Section G: overflow ───

#[test]
fn signed_quote_overflowing_conversion_rejected() {
    let env = Env::default();
    let (client, issuer, namespace, token, payout_asset, reported_asset) = setup(&env);
    let sk = fixed_signing_key();
    let _oracle = configure_oracle(
        &env,
        &client,
        &issuer,
        namespace,
        token,
        &sk,
        Symbol::new(&env, "EUR"),
        Symbol::new(&env, "USDC"),
        60,
    );

    let quote = SignedFxQuote {
        contract: env.current_contract_address(),
        revenue_symbol: Symbol::new(&env, "EUR"),
        payout_symbol: Symbol::new(&env, "USDC"),
        rate: 12_000,
        quoted_at: 1_000,
    };
    let (quote_bytes, signature) = sign_quote(&env, &sk, &quote);

    // i128::MAX * 12_000 overflows i128.
    let result = report_with_quote(
        &client,
        &issuer,
        namespace,
        token,
        reported_asset,
        i128::MAX,
        1,
        &quote_bytes,
        &signature,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::InvalidAmount)),
        "overflowing amount * rate must be rejected"
    );
}
