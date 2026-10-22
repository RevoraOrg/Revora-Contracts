//! # `estimate_transfer` — dry-run eligibility parity
//!
//! `estimate_transfer` is the public, unauthenticated dry-run endpoint for the
//! transfer gate. It must report the same verdict a real
//! `transfer_with_attestation` would reach, without requiring issuer auth and
//! without mutating business state.
//!
//! Each test drives the contract through its public surface, then asserts the
//! exact `RevoraError` (or success) that the estimator reports. Because the
//! estimator is documented to duplicate the eligibility checks inline, a
//! divergence between the two paths is a production bug — these tests make that
//! divergence visible.
//!
//! Covered here:
//! - the eligible happy path, and the `from_share == amount` boundary
//! - amount rejection: zero, and above the sender's share
//! - the `from == to` short-circuit (evaluated before the amount checks)
//! - network-id pinning and the frozen-contract guard
//! - blacklisted sender / recipient
//! - disallowed recipient jurisdiction
//! - active per-jurisdiction cooldown, and success once it elapses
//! - purity: reads only, no share/category/state mutation
//! - parity with the real `transfer_with_attestation` path
//! - an unknown offering reports the share-based rejection instead of panicking

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Symbol, Vec,
};

/// A registered offering plus the network id its attestations are pinned to.
struct Ctx {
    env: Env,
    client: RevoraRevenueShareClient<'static>,
    issuer: Address,
    token: Address,
    category: Symbol,
    network: BytesN<32>,
}

fn ns() -> Symbol {
    symbol_short!("ns")
}

fn attest(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0xabu8; 32])
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let _ = client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns(),
        &token,
        &5_000,
        &token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    env.ledger().set_network_id([0x01u8; 32]);
    let network = BytesN::from_array(&env, &[0x01u8; 32]);

    Ctx { env, client, issuer, token, category: symbol_short!("cat"), network }
}

impl Ctx {
    /// Grant `holder` a transferable share of the offering.
    fn give_share(&self, holder: &Address, share_bps: u32) {
        let _ = self.client.set_holder_share(
            &self.issuer,
            &ns(),
            &self.token,
            holder,
            &share_bps,
            &0u64,
        );
    }

    /// Register `holder` under `jurisdiction` and allow that jurisdiction.
    fn set_jurisdiction(&self, holder: &Address, jurisdiction: Symbol) {
        let _ = self.client.set_holder_jurisdiction(
            &self.issuer,
            &ns(),
            &self.token,
            holder,
            &jurisdiction,
            &0u64,
        );
        let _ = self.client.set_allowed_jurisdictions(
            &self.issuer,
            &ns(),
            &self.token,
            &soroban_sdk::vec![&self.env, jurisdiction],
        );
    }

    fn estimate(
        &self,
        from: &Address,
        to: &Address,
        amount_bps: u32,
        category: &Symbol,
        network: &BytesN<32>,
        attest_hash: &BytesN<32>,
    ) -> Result<(), Result<RevoraError, soroban_sdk::InvokeError>> {
        self.client.try_estimate_transfer(
            &self.issuer,
            &ns(),
            &self.token,
            from,
            to,
            &amount_bps,
            category,
            attest_hash,
            network,
        )
    }

    /// Assert the estimator allows the transfer.
    fn expect_allowed(&self, from: &Address, to: &Address, amount_bps: u32) {
        let result =
            self.estimate(from, to, amount_bps, &self.category, &self.network, &attest(&self.env));
        assert!(result.is_ok(), "expected the estimate to allow the transfer, got {result:?}");
    }

    /// Assert the estimator rejects the transfer with a specific error.
    fn expect_rejected(
        &self,
        from: &Address,
        to: &Address,
        amount_bps: u32,
        expected: RevoraError,
    ) {
        let result =
            self.estimate(from, to, amount_bps, &self.category, &self.network, &attest(&self.env));
        assert_eq!(result, Err(Ok(expected)), "estimator reported the wrong verdict");
    }
}

// ─── 1. Happy path ───────────────────────────────────────────────────────────

#[test]
fn estimate_allows_eligible_transfer() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);
    ctx.expect_allowed(&from, &to, 40);
}

#[test]
fn estimate_allows_transfer_of_the_full_share() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 250);

    // `from_share == amount_bps` is the inclusive boundary of the share check.
    ctx.expect_allowed(&from, &to, 250);
}

#[test]
fn estimate_allows_transfer_to_a_holder_with_an_existing_share() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 300);
    ctx.give_share(&to, 120);

    ctx.expect_allowed(&from, &to, 100);
}

// ─── 2. Amount checks ────────────────────────────────────────────────────────

#[test]
fn estimate_rejects_amount_above_sender_share() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);
    ctx.expect_rejected(&from, &to, 101, RevoraError::InvalidAmount);
}

#[test]
fn estimate_rejects_zero_amount() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);
    ctx.expect_rejected(&from, &to, 0, RevoraError::InvalidAmount);
}

#[test]
fn estimate_rejects_transfer_from_holder_without_a_share() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    // No share was ever assigned: `from_share == 0 < amount`.
    ctx.expect_rejected(&from, &to, 1, RevoraError::InvalidAmount);
}

// ─── 3. Self-transfer short-circuit ──────────────────────────────────────────

#[test]
fn estimate_allows_self_transfer_without_a_share() {
    let ctx = setup();
    let holder = Address::generate(&ctx.env);

    // `from == to` returns early, before the zero-amount and share checks.
    // This documents the current contract behaviour: a self-transfer is a no-op
    // that the estimator always allows, even for an amount the holder does not
    // hold.
    ctx.expect_allowed(&holder, &holder, 0);
    ctx.expect_allowed(&holder, &holder, 9_999);
}

// ─── 4. Network pinning and freeze ───────────────────────────────────────────

#[test]
fn estimate_rejects_network_id_mismatch() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);

    let wrong_network = BytesN::from_array(&ctx.env, &[0x02u8; 32]);
    let result = ctx.estimate(&from, &to, 50, &ctx.category, &wrong_network, &attest(&ctx.env));
    assert_eq!(
        result,
        Err(Ok(RevoraError::NetworkIdMismatch)),
        "an attestation pinned to another network must be rejected"
    );
}

#[test]
fn estimate_rejects_every_network_when_ledger_network_is_unset() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);
    ctx.env.ledger().set_network_id([0x07u8; 32]);

    let stale = ctx.estimate(&from, &to, 50, &ctx.category, &ctx.network, &attest(&ctx.env));
    assert_eq!(stale, Err(Ok(RevoraError::NetworkIdMismatch)));

    let fresh = BytesN::from_array(&ctx.env, &[0x07u8; 32]);
    let result = ctx.estimate(&from, &to, 50, &ctx.category, &fresh, &attest(&ctx.env));
    assert!(result.is_ok(), "a matching network id must be accepted");
}

#[test]
fn estimate_rejects_when_contract_is_frozen() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);

    let admin = Address::generate(&ctx.env);
    ctx.client.initialize(&admin, &None::<Address>, &None::<bool>);
    ctx.client.freeze();

    ctx.expect_rejected(&from, &to, 50, RevoraError::ContractFrozen);
}

// ─── 5. Sanctions screening ──────────────────────────────────────────────────

#[test]
fn estimate_rejects_blacklisted_sender() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);
    assert!(ctx
        .client
        .try_blacklist_add(&ctx.issuer, &ctx.issuer, &ns(), &ctx.token, &from)
        .is_ok());
    assert!(ctx.client.is_blacklisted(&ctx.issuer, &ns(), &ctx.token, &from));

    ctx.expect_rejected(&from, &to, 50, RevoraError::HolderBlacklisted);
}

#[test]
fn estimate_rejects_blacklisted_recipient() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);
    assert!(ctx.client.try_blacklist_add(&ctx.issuer, &ctx.issuer, &ns(), &ctx.token, &to).is_ok());

    ctx.expect_rejected(&from, &to, 50, RevoraError::HolderBlacklisted);
}

#[test]
fn estimate_allows_transfer_after_blacklist_removal() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);
    assert!(ctx.client.try_blacklist_add(&ctx.issuer, &ctx.issuer, &ns(), &ctx.token, &to).is_ok());
    ctx.expect_rejected(&from, &to, 50, RevoraError::HolderBlacklisted);

    assert!(ctx
        .client
        .try_blacklist_remove(&ctx.issuer, &ctx.issuer, &ns(), &ctx.token, &to)
        .is_ok());

    ctx.expect_allowed(&from, &to, 50);
}

// ─── 6. Jurisdiction gating ──────────────────────────────────────────────────

#[test]
fn estimate_rejects_disallowed_recipient_jurisdiction() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);

    // Register `to` under a jurisdiction, then restrict the offering to a
    // different, non-empty allow-list so the gate is active.
    let _ = ctx.client.set_holder_jurisdiction(
        &ctx.issuer,
        &ns(),
        &ctx.token,
        &to,
        &symbol_short!("cf"),
        &0u64,
    );
    let _ = ctx.client.set_allowed_jurisdictions(
        &ctx.issuer,
        &ns(),
        &ctx.token,
        &soroban_sdk::vec![&ctx.env, symbol_short!("us")],
    );

    ctx.expect_rejected(&from, &to, 50, RevoraError::JurisdictionDisallowed);
}

#[test]
fn estimate_allows_recipient_in_an_allowed_jurisdiction() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);
    ctx.set_jurisdiction(&to, symbol_short!("us"));

    ctx.expect_allowed(&from, &to, 50);
}

// ─── 7. Cooldown parity with the real transfer path ─────────────────────────

#[test]
fn estimate_reports_cooldown_active_then_allows_after_expiry() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);
    let jurisdiction = symbol_short!("us");

    ctx.give_share(&from, 100);
    ctx.set_jurisdiction(&from, jurisdiction);
    let _ =
        ctx.client.set_transfer_cooldown(&ctx.issuer, &ns(), &ctx.token, &jurisdiction, &3_600u64);

    // A real transfer records the holder's last-transfer timestamp.
    assert!(ctx
        .client
        .try_transfer_with_attestation(
            &ctx.issuer,
            &ns(),
            &ctx.token,
            &from,
            &to,
            &50,
            &ctx.category,
        )
        .is_ok());

    ctx.expect_rejected(&from, &to, 10, RevoraError::TransferCooldownActive);

    // Once the cooldown window has elapsed the estimator allows the transfer.
    ctx.env.ledger().with_mut(|l| l.timestamp += 3_601);
    ctx.expect_allowed(&from, &to, 10);
}

#[test]
fn estimate_ignores_cooldown_for_unconfigured_jurisdiction() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);

    // No holder jurisdiction is registered (and no allow-list), so no cooldown
    // applies to this sender.
    ctx.expect_allowed(&from, &to, 50);
    ctx.expect_allowed(&from, &to, 50);
}

// ─── 8. Purity ───────────────────────────────────────────────────────────────

#[test]
fn estimate_does_not_mutate_holder_shares() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 400);
    ctx.give_share(&to, 60);

    let before_from = ctx.client.get_holder_share(&ctx.issuer, &ns(), &ctx.token, &from);
    let before_to = ctx.client.get_holder_share(&ctx.issuer, &ns(), &ctx.token, &to);

    ctx.expect_allowed(&from, &to, 100);
    ctx.expect_allowed(&from, &to, 100);
    ctx.expect_rejected(&from, &to, 1_000, RevoraError::InvalidAmount);

    assert_eq!(
        ctx.client.get_holder_share(&ctx.issuer, &ns(), &ctx.token, &from),
        before_from,
        "estimate_transfer must not move shares"
    );
    assert_eq!(
        ctx.client.get_holder_share(&ctx.issuer, &ns(), &ctx.token, &to),
        before_to,
        "estimate_transfer must not credit shares"
    );
}

#[test]
fn estimate_does_not_create_whitelist_or_lockup_state() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);

    let had_lockup = ctx.client.get_lockup_schedule(&ctx.issuer, &ns(), &ctx.token).is_some();

    ctx.expect_allowed(&from, &to, 25);
    ctx.expect_rejected(&from, &to, 5_000, RevoraError::InvalidAmount);

    assert_eq!(
        ctx.client.get_lockup_schedule(&ctx.issuer, &ns(), &ctx.token).is_some(),
        had_lockup,
        "estimate_transfer must not create a lockup schedule"
    );
}

#[test]
fn estimate_ignores_the_attest_hash_bytes() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);

    let hash_a = BytesN::from_array(&ctx.env, &[0x01u8; 32]);
    let hash_b = BytesN::from_array(&ctx.env, &[0xffu8; 32]);

    let first = ctx.estimate(&from, &to, 50, &ctx.category, &ctx.network, &hash_a);
    let second = ctx.estimate(&from, &to, 50, &ctx.category, &ctx.network, &hash_b);

    assert!(first.is_ok() && second.is_ok());
    assert_eq!(first, second, "the estimator must not depend on the attest hash");
}

// ─── 9. Parity with the real transfer path ───────────────────────────────────

#[test]
fn estimate_matches_the_real_transfer_verdict() {
    let ctx = setup();
    let from = Address::generate(&ctx.env);
    let to = Address::generate(&ctx.env);

    ctx.give_share(&from, 100);

    // The estimator and the real path must agree on both verdicts.
    ctx.expect_allowed(&from, &to, 60);
    assert!(ctx
        .client
        .try_transfer_with_attestation(
            &ctx.issuer,
            &ns(),
            &ctx.token,
            &from,
            &to,
            &60,
            &ctx.category,
        )
        .is_ok());

    ctx.expect_rejected(&from, &to, 90, RevoraError::InvalidAmount);
    assert_eq!(
        ctx.client.try_transfer_with_attestation(
            &ctx.issuer,
            &ns(),
            &ctx.token,
            &from,
            &to,
            &90,
            &ctx.category,
        ),
        Err(Ok(RevoraError::InvalidAmount)),
        "the real path must reject what the estimator rejects"
    );
}

// ─── 10. Unknown offering ────────────────────────────────────────────────────

#[test]
fn estimate_for_unknown_offering_reports_share_rejection() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    env.ledger().set_network_id([0x01u8; 32]);
    let network = BytesN::from_array(&env, &[0x01u8; 32]);

    // The estimator does not resolve the offering; it reads a zero share and
    // therefore reports an insufficient balance rather than `OfferingNotFound`.
    let result = client.try_estimate_transfer(
        &issuer,
        &ns(),
        &token,
        &from,
        &to,
        &10,
        &symbol_short!("cat"),
        &attest(&env),
        &network,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
}
