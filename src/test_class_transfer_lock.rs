//! # Tests for class-restricted transfer locking (#522)
//!
//! Cross-class transfers (e.g. Class A → Class B) are blocked by default.
//! Transfers between holders of the same class or involving unassigned holders
//! are allowed for backward compatibility.
//!
//! | Test scenario | Expected |
//! |--------------|-----------|
//! | Same class (A→A) | Succeeds |
//! | Same class (B→B) | Succeeds |
//! | Cross-class (A→B) | `ClassTransferBlocked` |
//! | Cross-class (B→A) | `ClassTransferBlocked` |
//! | Unassigned → Class A | Succeeds (backward compat) |
//! | Class A → Unassigned | Succeeds (backward compat) |
//! | Both unassigned | Succeeds (backward compat) |
//! | Cross-class blocked event | Emitted |
//! | Self-transfer bypass | `InvalidTransferParticipants` |
//! | Zero-value transfer bypass | `InvalidShareBps` (not class check) |
//! | Custom class A → Custom class B | `ClassTransferBlocked` |
//! | Multi-class holder → different class | `ClassTransferBlocked` |

#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, BytesN, Env, IntoVal, Val, Vec,
};

use crate::{
    ClassConfig, DataKey2, OfferingId, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient,
    ShareClass,
};

// ── Shared helpers ────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn attest(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0xabu8; 32])
}

/// Write share classes directly to storage (no dedicated setter API yet).
fn write_offering_classes(
    env: &Env,
    contract_id: &Address,
    issuer: &Address,
    token: &Address,
    classes: Vec<(ShareClass, ClassConfig)>,
) {
    let offering_id = OfferingId {
        issuer: issuer.clone(),
        namespace: symbol_short!("def"),
        token: token.clone(),
    };
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey2::OfferingClasses(offering_id), &classes);
    });
}

/// Write a per-class share balance directly to storage.
fn write_class_share(
    env: &Env,
    contract_id: &Address,
    issuer: &Address,
    token: &Address,
    holder: &Address,
    sc: &ShareClass,
    bps: u32,
) {
    let offering_id = OfferingId {
        issuer: issuer.clone(),
        namespace: symbol_short!("def"),
        token: token.clone(),
    };
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .set(
                &DataKey2::HolderShareClass(offering_id, holder.clone(), sc.clone()),
                &bps,
            );
    });
}

/// Register an offering with two share classes (A and B) and return
/// (client, contract_id, issuer, token).
fn setup_with_classes(
    env: &Env,
) -> (RevoraRevenueShareClient<'_>, Address, Address, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(env, &contract_id);
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let ns = symbol_short!("def");
    let payout = Address::generate(env);

    client.register_offering(
        &issuer,
        &Vec::new(env),
        &1u32,
        &ns,
        &token,
        &1_000u32,
        &payout,
        &0i128,
        &symbol_short!("TKN"),
        &0u32,
    );

    // Configure share classes: Class A (voting) and Class B (non-voting)
    let classes = Vec::from_array(
        env,
        [
            (
                ShareClass::A,
                ClassConfig {
                    bps: 10_000u32,
                    voting: true,
                },
            ),
            (
                ShareClass::B,
                ClassConfig {
                    bps: 10_000u32,
                    voting: false,
                },
            ),
        ],
    );
    write_offering_classes(env, &contract_id, &issuer, &token, classes);

    (client, contract_id, issuer, token)
}

/// Set a holder's total share (legacy, no class separation).
fn set_share(
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    token: &Address,
    holder: &Address,
    bps: u32,
) {
    client.set_holder_share(issuer, &symbol_short!("def"), token, holder, &bps);
}

// ── Same-class transfers succeed ──────────────────────────────────────────────

#[test]
fn same_class_a_to_a_succeeds() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &from, &ShareClass::A, 1_000);
    set_share(&client, &issuer, &token, &from, 1_000);

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &500u32, &attest(&env),
    );
    assert_eq!(result, Ok(Ok(())));
}

#[test]
fn same_class_b_to_b_succeeds() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &from, &ShareClass::B, 1_000);
    set_share(&client, &issuer, &token, &from, 1_000);

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &500u32, &attest(&env),
    );
    assert_eq!(result, Ok(Ok(())));
}

#[test]
fn same_class_a_to_a_partial_transfer() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &from, &ShareClass::A, 5_000);
    write_class_share(&env, &cid, &issuer, &token, &to, &ShareClass::A, 2_000);
    set_share(&client, &issuer, &token, &from, 5_000);
    set_share(&client, &issuer, &token, &to, 2_000);

    client.transfer_with_attestation(&issuer, &ns, &token, &from, &to, &1_500u32, &attest(&env));

    // Total shares preserved
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &from), 3_500);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &to), 3_500);
}

// ── Cross-class transfers blocked ─────────────────────────────────────────────

#[test]
fn cross_class_a_to_b_blocked() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &from, &ShareClass::A, 1_000);
    write_class_share(&env, &cid, &issuer, &token, &to, &ShareClass::B, 500);
    set_share(&client, &issuer, &token, &from, 1_000);
    set_share(&client, &issuer, &token, &to, 500);

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &300u32, &attest(&env),
    );
    assert_eq!(result, Err(Ok(RevoraError::ClassTransferBlocked)));

    // State must be unchanged
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &from), 1_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &to), 500);
}

#[test]
fn cross_class_b_to_a_blocked() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &from, &ShareClass::B, 1_000);
    write_class_share(&env, &cid, &issuer, &token, &to, &ShareClass::A, 500);
    set_share(&client, &issuer, &token, &from, 1_000);
    set_share(&client, &issuer, &token, &to, 500);

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &300u32, &attest(&env),
    );
    assert_eq!(result, Err(Ok(RevoraError::ClassTransferBlocked)));
}

/// Full transfer (all shares) across classes is also blocked.
#[test]
fn cross_class_full_transfer_blocked() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &from, &ShareClass::A, 5_000);
    write_class_share(&env, &cid, &issuer, &token, &to, &ShareClass::B, 0);
    set_share(&client, &issuer, &token, &from, 5_000);

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &5_000u32, &attest(&env),
    );
    assert_eq!(result, Err(Ok(RevoraError::ClassTransferBlocked)));

    // State unchanged
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &from), 5_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &to), 0);
}

// ── Backward compatibility (unassigned holders) ───────────────────────────────

#[test]
fn unassigned_to_class_a_succeeds() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    // from has shares but no class assigned (backward compat path)
    set_share(&client, &issuer, &token, &from, 1_000);
    write_class_share(&env, &cid, &issuer, &token, &to, &ShareClass::A, 0);

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &500u32, &attest(&env),
    );
    assert_eq!(result, Ok(Ok(())));
}

#[test]
fn class_a_to_unassigned_succeeds() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &from, &ShareClass::A, 1_000);
    set_share(&client, &issuer, &token, &from, 1_000);
    // to is unassigned (no class shares)

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &500u32, &attest(&env),
    );
    assert_eq!(result, Ok(Ok(())));
}

#[test]
fn both_unassigned_succeeds() {
    let env = Env::default();
    let (client, _cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    set_share(&client, &issuer, &token, &from, 1_000);
    // both holders are unassigned

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &500u32, &attest(&env),
    );
    assert_eq!(result, Ok(Ok(())));
}

// ── Transfer bypasses ─────────────────────────────────────────────────────────

/// Self-transfer across classes bypasses class check (self-transfer fires first).
#[test]
fn self_transfer_bypasses_class_check() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let holder = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &holder, &ShareClass::A, 1_000);
    set_share(&client, &issuer, &token, &holder, 1_000);

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &holder, &holder, &500u32, &attest(&env),
    );
    // Guard 3 (self-transfer) fires before Guard 11 (class check)
    assert_eq!(result, Err(Ok(RevoraError::InvalidTransferParticipants)));
}

/// Zero-value transfer bypasses class check (Guard 10 fires first).
#[test]
fn zero_value_transfer_bypasses_class_check() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &from, &ShareClass::A, 1_000);
    write_class_share(&env, &cid, &issuer, &token, &to, &ShareClass::B, 500);
    set_share(&client, &issuer, &token, &from, 1_000);
    set_share(&client, &issuer, &token, &to, 500);

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &0u32, &attest(&env),
    );
    // Guard 10 fires first — zero shares is already invalid
    assert_eq!(result, Err(Ok(RevoraError::InvalidShareBps)));
}

// ── Event emission ────────────────────────────────────────────────────────────

#[test]
fn class_xfer_block_event_emitted_on_cross_class() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &from, &ShareClass::A, 1_000);
    write_class_share(&env, &cid, &issuer, &token, &to, &ShareClass::B, 500);
    set_share(&client, &issuer, &token, &from, 1_000);
    set_share(&client, &issuer, &token, &to, 500);

    let before = env.events().all().len();

    let _ = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &300u32, &attest(&env),
    );

    let events = env.events().all();
    assert!(
        events.len() > before,
        "at least one event must be emitted on class block"
    );

    let cls_block_sym = symbol_short!("cls_block");
    let mut found = false;
    for i in before..events.len() {
        let (_, topics, data) = events.get(i).unwrap();
        let topics_vec: soroban_sdk::Vec<Val> = topics.clone().into_val(&env);
        let topic_sym: soroban_sdk::Symbol = topics_vec.get(0).unwrap().into_val(&env);
        if topic_sym == cls_block_sym {
            // Verify data: (from, to, from_class, to_class)
            let data_vec: soroban_sdk::Vec<Val> = data.clone().into_val(&env);
            let ev_from: Address = data_vec.get(0).unwrap().into_val(&env);
            let ev_to: Address = data_vec.get(1).unwrap().into_val(&env);
            assert_eq!(ev_from, from);
            assert_eq!(ev_to, to);
            found = true;
            break;
        }
    }
    assert!(found, "cls_block event must be emitted on class transfer block");
}

/// No `cls_block` event is emitted when a transfer succeeds (same class).
#[test]
fn no_class_xfer_block_event_on_same_class_transfer() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &from, &ShareClass::A, 1_000);
    set_share(&client, &issuer, &token, &from, 1_000);

    let cls_block_sym = symbol_short!("cls_block");
    let before = env.events().all().len();

    client.transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &500u32, &attest(&env),
    );

    let events = env.events().all();
    let blocked_events = (before..events.len())
        .filter(|&i| {
            let (_, topics, _) = events.get(i as u32).unwrap();
            let tv: soroban_sdk::Vec<Val> = topics.into_val(&env);
            let s: soroban_sdk::Symbol = tv.get(0).unwrap().into_val(&env);
            s == cls_block_sym
        })
        .count();
    assert_eq!(blocked_events, 0, "no cls_block event expected on successful transfer");
}

// ── Custom share classes ──────────────────────────────────────────────────────

#[test]
fn custom_class_cross_transfer_blocked() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    let custom_a = ShareClass::Custom(symbol_short!("Pref"));
    let custom_b = ShareClass::Custom(symbol_short!("Comm"));

    // Overwrite classes with custom ones
    let classes = Vec::from_array(
        &env,
        [
            (
                custom_a.clone(),
                ClassConfig {
                    bps: 5_000u32,
                    voting: true,
                },
            ),
            (
                custom_b.clone(),
                ClassConfig {
                    bps: 5_000u32,
                    voting: false,
                },
            ),
        ],
    );
    write_offering_classes(&env, &cid, &issuer, &token, classes);

    write_class_share(&env, &cid, &issuer, &token, &from, &custom_a, 1_000);
    write_class_share(&env, &cid, &issuer, &token, &to, &custom_b, 500);
    set_share(&client, &issuer, &token, &from, 1_000);
    set_share(&client, &issuer, &token, &to, 500);

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &300u32, &attest(&env),
    );
    assert_eq!(result, Err(Ok(RevoraError::ClassTransferBlocked)));
}

#[test]
fn custom_class_same_class_transfer_succeeds() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    let custom = ShareClass::Custom(symbol_short!("Pref"));

    let classes = Vec::from_array(
        &env,
        [(
            custom.clone(),
            ClassConfig {
                bps: 10_000u32,
                voting: true,
            },
        )],
    );
    write_offering_classes(&env, &cid, &issuer, &token, classes);

    write_class_share(&env, &cid, &issuer, &token, &from, &custom, 1_000);
    set_share(&client, &issuer, &token, &from, 1_000);

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &500u32, &attest(&env),
    );
    assert_eq!(result, Ok(Ok(())));
}

// ── Multi-class holder edge cases ─────────────────────────────────────────────

/// A holder with shares in both Class A and Class B. The primary (first non-zero)
/// class determines the class check.
#[test]
fn receiver_with_both_classes_receives_same_class() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &from, &ShareClass::A, 1_000);
    // to has Class A shares (primary, first in OfferingClasses order) and also Class B
    write_class_share(&env, &cid, &issuer, &token, &to, &ShareClass::A, 500);
    write_class_share(&env, &cid, &issuer, &token, &to, &ShareClass::B, 200);
    set_share(&client, &issuer, &token, &from, 1_000);
    set_share(&client, &issuer, &token, &to, 700);

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &300u32, &attest(&env),
    );
    // Both have primary Class A → allowed
    assert_eq!(result, Ok(Ok(())));
}

/// When receiver's primary (first non-zero) class is B, cross-class A→B is blocked.
#[test]
fn receiver_primary_class_b_blocks_a_transfer() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &from, &ShareClass::A, 1_000);
    // to has Class B as first non-zero (primary — Class A is first in order but zero balance)
    write_class_share(&env, &cid, &issuer, &token, &to, &ShareClass::A, 0);
    write_class_share(&env, &cid, &issuer, &token, &to, &ShareClass::B, 10_000);
    set_share(&client, &issuer, &token, &from, 1_000);
    set_share(&client, &issuer, &token, &to, 10_000);

    // Primary class is B (Class A has zero balance, skip to Class B)
    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &300u32, &attest(&env),
    );
    assert_eq!(result, Err(Ok(RevoraError::ClassTransferBlocked)));
}

// ── Offering without classes (full backward compat) ───────────────────────────

#[test]
fn no_classes_configured_still_allows_transfers() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");
    let payout = Address::generate(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns,
        &token,
        &1_000u32,
        &payout,
        &0i128,
        &symbol_short!("TKN"),
        &0u32,
    );
    // No classes configured — backward compat, no class checks

    set_share(&client, &issuer, &token, &from, 1_000);

    let result = client.try_transfer_with_attestation(
        &issuer, &ns, &token, &from, &to, &500u32, &attest(&env),
    );
    assert_eq!(result, Ok(Ok(())));
}

// ── get_primary_class behavior ────────────────────────────────────────────────

#[test]
fn get_primary_class_returns_correct_class() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let holder = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &holder, &ShareClass::A, 1_000);

    let primary = client.get_primary_class(&issuer, &ns, &token, &holder);
    assert_eq!(primary, Some(ShareClass::A));
}

#[test]
fn get_primary_class_returns_none_for_unassigned() {
    let env = Env::default();
    let (client, _cid, issuer, token) = setup_with_classes(&env);
    let holder = Address::generate(&env);
    let ns = symbol_short!("def");

    // Holder has no class shares
    let primary = client.get_primary_class(&issuer, &ns, &token, &holder);
    assert_eq!(primary, None);
}

#[test]
fn get_primary_class_returns_none_for_zero_balance() {
    let env = Env::default();
    let (client, cid, issuer, token) = setup_with_classes(&env);
    let holder = Address::generate(&env);
    let ns = symbol_short!("def");

    write_class_share(&env, &cid, &issuer, &token, &holder, &ShareClass::A, 0);

    let primary = client.get_primary_class(&issuer, &ns, &token, &holder);
    assert_eq!(primary, None);
}
