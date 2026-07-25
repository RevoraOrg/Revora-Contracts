//! Holder lockup period tests (#469).
//!
//! Covers:
//!  1.  set_holder_lockup stores the expiry timestamp
//!  2.  get_holder_lockup returns 0 when no lockup is set
//!  3.  transfer_with_attestation blocked before lockup_end (HolderLockupActive)
//!  4.  transfer_with_attestation allowed when now == lockup_end (inclusive boundary)
//!  5.  transfer_with_attestation allowed when now > lockup_end
//!  6.  request_redemption blocked before lockup_end
//!  7.  request_redemption allowed when now == lockup_end (inclusive boundary)
//!  8.  request_redemption allowed when now > lockup_end
//!  9.  claim (revenue) is NOT blocked during lockup
//! 10.  set_holder_lockup(0) clears the lockup (transfer now succeeds)
//! 11.  set_holder_lockup rejects unknown offering
//! 12.  set_holder_lockup requires issuer auth (should_panic)
//! 13.  transfer_with_attestation rejects unknown offering
//! 14.  request_redemption rejects unknown offering
//! 15.  transfer_with_attestation rejects when transfer_bps > from share
//! 16.  transfer_with_attestation rejects when new to_bps would exceed 10000
//! 17.  transfer_with_attestation updates from and to shares correctly
//! 18.  request_redemption zeroes out holder share
//! 19.  lkup_blk event emitted when transfer is blocked
//! 20.  lkup_blk event emitted when redemption is blocked
//! 21.  lockup is per-offering: separate offerings have independent lockups
//! 22.  frozen contract blocks set_holder_lockup
//! 23.  frozen contract blocks transfer_with_attestation
//! 24.  frozen contract blocks request_redemption
//! 25.  transfer_with_attestation requires issuer auth (should_panic)
//! 26.  request_redemption requires holder auth (should_panic)
//! 27.  transfer_with_attestation with transfer_bps=0 succeeds (no-op transfer)

#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    vec, Address, Env, IntoVal, Symbol,
};

// ── helpers ──────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);
    (env, id, client)
}

fn register(env: &Env, client: &RevoraRevenueShareClient) -> (Address, Symbol, Address) {
    let issuer = Address::generate(env);
    let namespace = Symbol::new(env, "ns");
    let token = Address::generate(env);
    client.register_offering(&issuer, &namespace, &token, &500, &token, &0);
    (issuer, namespace, token)
}

fn set_ts(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

// ── 1. set_holder_lockup stores the expiry timestamp ─────────────────────────

#[test]
fn lockup_set_stores_expiry() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let holder = Address::generate(&env);

    client.set_holder_lockup(&issuer, &ns, &token, &holder, &9_999_999);

    assert_eq!(client.get_holder_lockup(&issuer, &ns, &token, &holder), 9_999_999);
}

// ── 2. get_holder_lockup returns 0 when no lockup is set ─────────────────────

#[test]
fn lockup_get_returns_zero_when_unset() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let holder = Address::generate(&env);

    assert_eq!(client.get_holder_lockup(&issuer, &ns, &token, &holder), 0);
}

// ── 3. transfer blocked before lockup_end ────────────────────────────────────

#[test]
fn transfer_blocked_during_lockup() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &from, &5_000);
    set_ts(&env, 1_000);
    // lockup expires at 2_000; now=1_000 — still locked
    client.set_holder_lockup(&issuer, &ns, &token, &from, &2_000);

    let err = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &from, &to, &1_000)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RevoraError::HolderLockupActive);
}

// ── 4. transfer allowed at exact lockup_end (inclusive) ──────────────────────

#[test]
fn transfer_allowed_at_exact_lockup_end() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &from, &5_000);
    client.set_holder_lockup(&issuer, &ns, &token, &from, &2_000);

    // now == lockup_end → allowed (inclusive boundary)
    set_ts(&env, 2_000);
    client.transfer_with_attestation(&issuer, &ns, &token, &from, &to, &1_000);

    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &from), 4_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &to), 1_000);
}

// ── 5. transfer allowed after lockup_end ─────────────────────────────────────

#[test]
fn transfer_allowed_after_lockup_end() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &from, &3_000);
    client.set_holder_lockup(&issuer, &ns, &token, &from, &500);

    set_ts(&env, 10_000); // well past lockup
    client.transfer_with_attestation(&issuer, &ns, &token, &from, &to, &3_000);

    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &from), 0);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &to), 3_000);
}

// ── 6. request_redemption blocked before lockup_end ──────────────────────────

#[test]
fn redemption_blocked_during_lockup() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder, &2_000);
    set_ts(&env, 100);
    client.set_holder_lockup(&issuer, &ns, &token, &holder, &9_000);

    let err = client
        .try_request_redemption(&issuer, &ns, &token, &holder)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RevoraError::HolderLockupActive);
}

// ── 7. request_redemption allowed at exact lockup_end (inclusive) ─────────────

#[test]
fn redemption_allowed_at_exact_lockup_end() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder, &2_000);
    client.set_holder_lockup(&issuer, &ns, &token, &holder, &5_000);

    set_ts(&env, 5_000); // now == lockup_end → permitted
    client.request_redemption(&issuer, &ns, &token, &holder);

    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &holder), 0);
}

// ── 8. request_redemption allowed after lockup_end ───────────────────────────

#[test]
fn redemption_allowed_after_lockup_end() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder, &4_000);
    client.set_holder_lockup(&issuer, &ns, &token, &holder, &1_000);

    set_ts(&env, 99_999);
    client.request_redemption(&issuer, &ns, &token, &holder);

    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &holder), 0);
}

// ── 9. claim (revenue) is NOT blocked during lockup ──────────────────────────

#[test]
fn claim_not_blocked_during_lockup() {
    use soroban_sdk::testutils::MockAuth;
    use soroban_sdk::testutils::MockAuthInvoke;

    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let ns = Symbol::new(&env, "ns");

    // Use a real token contract for the payment token.
    let payment_token_id = env.register_stellar_asset_contract_v2(issuer.clone());
    let token_addr = payment_token_id.address();
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_addr);

    // Mint revenue into the contract.
    token_client.mint(&contract_id, &100_000);

    // Register offering using the payment token as payout asset.
    let offering_token = Address::generate(&env);
    client.register_offering(&issuer, &ns, &offering_token, &500, &token_addr, &0);

    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &ns, &offering_token, &holder, &5_000); // 50%

    // Deposit revenue for period 1.
    token_client.mint(&issuer, &10_000);
    client.deposit_revenue(&issuer, &ns, &offering_token, &token_addr, &10_000, &1);

    // Set a lockup far in the future.
    set_ts(&env, 1_000);
    client.set_holder_lockup(&issuer, &ns, &offering_token, &holder, &999_999_999);

    // Claim must succeed — revenue claim is not gated by lockup.
    let payout = client.claim(&holder, &issuer, &ns, &offering_token, &10);
    assert!(payout >= 0, "claim must not be blocked by lockup");
}

// ── 10. set_holder_lockup(0) clears the lockup ───────────────────────────────

#[test]
fn lockup_cleared_by_setting_zero() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &from, &5_000);
    set_ts(&env, 100);
    // Set a future lockup...
    client.set_holder_lockup(&issuer, &ns, &token, &from, &9_999_999);
    // ...then clear it.
    client.set_holder_lockup(&issuer, &ns, &token, &from, &0);

    assert_eq!(client.get_holder_lockup(&issuer, &ns, &token, &from), 0);

    // Transfer must now succeed.
    client.transfer_with_attestation(&issuer, &ns, &token, &from, &to, &1_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &from), 4_000);
}

// ── 11. set_holder_lockup rejects unknown offering ────────────────────────────

#[test]
fn lockup_set_unknown_offering_rejected() {
    let (env, _id, client) = setup();
    let issuer = Address::generate(&env);
    let ns = Symbol::new(&env, "ns");
    let ghost = Address::generate(&env);
    let holder = Address::generate(&env);

    let err = client
        .try_set_holder_lockup(&issuer, &ns, &ghost, &holder, &9_000)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RevoraError::OfferingNotFound);
}

// ── 12. set_holder_lockup requires issuer auth ────────────────────────────────

#[test]
#[should_panic]
fn lockup_set_requires_issuer_auth() {
    let env = Env::default(); // no mock_all_auths
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    // register with mocked auth then strip it
    env.mock_all_auths();
    let (issuer, ns, token) = {
        let issuer = Address::generate(&env);
        let ns = Symbol::new(&env, "ns");
        let token = Address::generate(&env);
        client.register_offering(&issuer, &ns, &token, &500, &token, &0);
        (issuer, ns, token)
    };
    env.set_auths(&[]);

    let holder = Address::generate(&env);
    client.set_holder_lockup(&issuer, &ns, &token, &holder, &9_000); // must panic
}

// ── 13. transfer_with_attestation rejects unknown offering ────────────────────

#[test]
fn transfer_unknown_offering_rejected() {
    let (env, _id, client) = setup();
    let issuer = Address::generate(&env);
    let ns = Symbol::new(&env, "ns");
    let ghost = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let err = client
        .try_transfer_with_attestation(&issuer, &ns, &ghost, &from, &to, &100)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RevoraError::OfferingNotFound);
}

// ── 14. request_redemption rejects unknown offering ───────────────────────────

#[test]
fn redemption_unknown_offering_rejected() {
    let (env, _id, client) = setup();
    let issuer = Address::generate(&env);
    let ns = Symbol::new(&env, "ns");
    let ghost = Address::generate(&env);
    let holder = Address::generate(&env);

    let err = client
        .try_request_redemption(&issuer, &ns, &ghost, &holder)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RevoraError::OfferingNotFound);
}

// ── 15. transfer_with_attestation rejects when transfer_bps > from share ──────

#[test]
fn transfer_rejects_insufficient_from_share() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &from, &1_000);

    let err = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &from, &to, &1_001)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RevoraError::InvalidShareBps);
}

// ── 16. transfer rejects when new to_bps would exceed 10000 ──────────────────

#[test]
fn transfer_rejects_to_share_overflow() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // from has 9_000, to already has 9_000; transferring 2_000 would put to at 11_000
    client.set_holder_share(&issuer, &ns, &token, &from, &9_000);
    client.set_holder_share(&issuer, &ns, &token, &to, &9_000);

    let err = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &from, &to, &2_000)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RevoraError::InvalidShareBps);
}

// ── 17. transfer updates shares correctly ─────────────────────────────────────

#[test]
fn transfer_updates_shares_correctly() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &from, &8_000);
    client.set_holder_share(&issuer, &ns, &token, &to, &1_000);

    client.transfer_with_attestation(&issuer, &ns, &token, &from, &to, &2_500);

    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &from), 5_500);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &to), 3_500);
}

// ── 18. request_redemption zeroes out holder share ────────────────────────────

#[test]
fn redemption_zeroes_share() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder, &7_500);
    client.request_redemption(&issuer, &ns, &token, &holder);

    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &holder), 0);
}

// ── 19. lkup_blk event emitted when transfer is blocked ──────────────────────

#[test]
fn transfer_block_emits_lockup_block_event() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &from, &5_000);
    set_ts(&env, 500);
    client.set_holder_lockup(&issuer, &ns, &token, &from, &10_000);

    let _ = client.try_transfer_with_attestation(&issuer, &ns, &token, &from, &to, &1_000);

    let events = env.events().all();
    let symbol_val: soroban_sdk::Val = Symbol::new(&env, "lkup_blk").into_val(&env);
    let has_block_event = events.iter().any(|(_, topics, _)| topics.contains(symbol_val));
    assert!(has_block_event, "lkup_blk event must be emitted on blocked transfer");
}

// ── 20. lkup_blk event emitted when redemption is blocked ────────────────────

#[test]
fn redemption_block_emits_lockup_block_event() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder, &3_000);
    set_ts(&env, 100);
    client.set_holder_lockup(&issuer, &ns, &token, &holder, &50_000);

    let _ = client.try_request_redemption(&issuer, &ns, &token, &holder);

    let events = env.events().all();
    let symbol_val: soroban_sdk::Val = Symbol::new(&env, "lkup_blk").into_val(&env);
    let has_block_event = events.iter().any(|(_, topics, _)| topics.contains(symbol_val));
    assert!(has_block_event, "lkup_blk event must be emitted on blocked redemption");
}

// ── 21. lockup is per-offering ────────────────────────────────────────────────

#[test]
fn lockup_is_per_offering_isolated() {
    let (env, _id, client) = setup();
    let issuer = Address::generate(&env);
    let ns = Symbol::new(&env, "ns");
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let holder = Address::generate(&env);
    let to = Address::generate(&env);

    client.register_offering(&issuer, &ns, &token_a, &500, &token_a, &0);
    client.register_offering(&issuer, &ns, &token_b, &500, &token_b, &0);

    // Lock holder on offering A only.
    client.set_holder_share(&issuer, &ns, &token_a, &holder, &5_000);
    client.set_holder_share(&issuer, &ns, &token_b, &holder, &5_000);

    set_ts(&env, 100);
    client.set_holder_lockup(&issuer, &ns, &token_a, &holder, &99_999);

    // Transfer on A must fail.
    let err = client
        .try_transfer_with_attestation(&issuer, &ns, &token_a, &holder, &to, &100)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RevoraError::HolderLockupActive);

    // Transfer on B must succeed (no lockup on B).
    client.transfer_with_attestation(&issuer, &ns, &token_b, &holder, &to, &100);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token_b, &holder), 4_900);
}

// ── 22. frozen contract blocks set_holder_lockup ──────────────────────────────

#[test]
fn lockup_set_blocked_when_frozen() {
    let (env, contract_id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let holder = Address::generate(&env);

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::Frozen, &true);
    });

    let err = client
        .try_set_holder_lockup(&issuer, &ns, &token, &holder, &9_000)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RevoraError::ContractFrozen);
}

// ── 23. frozen contract blocks transfer_with_attestation ─────────────────────

#[test]
fn transfer_blocked_when_frozen() {
    let (env, contract_id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &from, &5_000);

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::Frozen, &true);
    });

    let err = client
        .try_transfer_with_attestation(&issuer, &ns, &token, &from, &to, &100)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RevoraError::ContractFrozen);
}

// ── 24. frozen contract blocks request_redemption ────────────────────────────

#[test]
fn redemption_blocked_when_frozen() {
    let (env, contract_id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder, &3_000);

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::Frozen, &true);
    });

    let err = client
        .try_request_redemption(&issuer, &ns, &token, &holder)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, RevoraError::ContractFrozen);
}

// ── 25. transfer_with_attestation requires issuer auth ────────────────────────

#[test]
#[should_panic]
fn transfer_requires_issuer_auth() {
    let env = Env::default();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    env.mock_all_auths();
    let (issuer, ns, token) = {
        let issuer = Address::generate(&env);
        let ns = Symbol::new(&env, "ns");
        let token = Address::generate(&env);
        client.register_offering(&issuer, &ns, &token, &500, &token, &0);
        client.set_holder_share(&issuer, &ns, &token, &issuer, &5_000);
        (issuer, ns, token)
    };
    env.set_auths(&[]);

    let to = Address::generate(&env);
    client.transfer_with_attestation(&issuer, &ns, &token, &issuer, &to, &100);
}

// ── 26. request_redemption requires holder auth ───────────────────────────────

#[test]
#[should_panic]
fn redemption_requires_holder_auth() {
    let env = Env::default();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    env.mock_all_auths();
    let (issuer, ns, token) = {
        let issuer = Address::generate(&env);
        let ns = Symbol::new(&env, "ns");
        let token = Address::generate(&env);
        client.register_offering(&issuer, &ns, &token, &500, &token, &0);
        (issuer, ns, token)
    };
    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &ns, &token, &holder, &3_000);
    env.set_auths(&[]);

    client.request_redemption(&issuer, &ns, &token, &holder);
}

// ── 27. transfer with transfer_bps=0 is a valid no-op ────────────────────────

#[test]
fn transfer_zero_bps_is_noop() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &from, &5_000);

    // Zero-bps transfer should succeed without changing anything meaningful.
    client.transfer_with_attestation(&issuer, &ns, &token, &from, &to, &0);

    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &from), 5_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &to), 0);
}
