#![cfg(test)]

use crate::{Dispute, DisputeStatus, MAX_OPEN_DISPUTES_PER_HOLDER};
use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    BytesN, Env,
    symbol_short,
    testutils::{Address as _, Events as _},
    Address,
};

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Symbol, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("test");
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &ns, &token, &2500, &payout, &0, &symbol_short!(""), &0);
    (env, client, admin, issuer, ns, token)
}

fn make_holder(env: &Env, client: &RevoraRevenueShareClient<'static>, issuer: &Address, ns: &Symbol, token: &Address) -> Address {
    let holder = Address::generate(env);
    client.set_holder_share(issuer, ns, token, &holder, &500);
    holder
}

fn meta(env: &Env, val: u8) -> BytesN<32> {
    let mut arr = [0u8; 32];
    arr[0] = val;
    BytesN::from_array(env, &arr)
}

// ── Basic success ───────────────────────────────────────────────────────────

#[test]
fn open_dispute_succeeds() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 1);

    let dispute_id = client.try_open_dispute(&holder, &issuer, &ns, &token, &m)
        .unwrap();
    assert!(dispute_id.is_ok(), "open_dispute should succeed");
}

#[test]
fn open_dispute_returns_deterministic_id() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 42);

    let id1 = client.try_open_dispute(&holder, &issuer, &ns, &token, &m).unwrap().unwrap();
    let id2 = client.try_open_dispute(&holder, &issuer, &ns, &token, &m).unwrap().unwrap();
    assert_eq!(id1, id2, "same inputs must produce same dispute ID");
}

#[test]
fn open_dispute_different_meta_produces_different_id() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);

    let id1 = client.try_open_dispute(&holder, &issuer, &ns, &token, &meta(&env, 1)).unwrap().unwrap();
    let id2 = client.try_open_dispute(&holder, &issuer, &ns, &token, &meta(&env, 2)).unwrap().unwrap();
    assert_ne!(id1, id2, "different meta_hash must yield different ID");
}

// ── Event emission ──────────────────────────────────────────────────────────

#[test]
fn open_dispute_emits_event() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 7);

    let _ = client.try_open_dispute(&holder, &issuer, &ns, &token, &m).unwrap().unwrap();

    let events = env.events().all();
    let dispute_events: Vec<_> = events.iter().filter(|e| e.0.to_string().contains("dispute_open")).collect();
    assert!(!dispute_events.is_empty(), "expected dispute_open event");
}

// ── Error cases ─────────────────────────────────────────────────────────────

#[test]
fn open_dispute_zero_share_rejected() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let non_holder = Address::generate(&env);
    let m = meta(&env, 1);

    let res = client.try_open_dispute(&non_holder, &issuer, &ns, &token, &m);
    match res {
        Err(Ok(RevoraError::DisputeZeroShare)) => {}
        other => panic!("expected DisputeZeroShare, got: {:?}", other),
    }
}

#[test]
fn open_dispute_duplicate_rejected() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 1);

    client.try_open_dispute(&holder, &issuer, &ns, &token, &m).unwrap().unwrap();

    let res = client.try_open_dispute(&holder, &issuer, &ns, &token, &m);
    match res {
        Err(Ok(RevoraError::DisputeAlreadyOpen)) => {}
        other => panic!("expected DisputeAlreadyOpen, got: {:?}", other),
    }
}

#[test]
fn open_dispute_spam_cap_enforced() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);

    for i in 0..MAX_OPEN_DISPUTES_PER_HOLDER {
        let m = meta(&env, i as u8 + 1);
        let res = client.try_open_dispute(&holder, &issuer, &ns, &token, &m);
        assert!(res.is_ok() && res.unwrap().is_ok(),
            "dispute {} should succeed", i + 1);
    }

    // Cap reached — next one must fail
    let overflow = meta(&env, 99);
    let res = client.try_open_dispute(&holder, &issuer, &ns, &token, &overflow);
    match res {
        Err(Ok(RevoraError::MaxDisputesReached)) => {}
        other => panic!("expected MaxDisputesReached, got: {:?}", other),
    }
}

#[test]
fn open_dispute_frozen_rejected() {
    let (env, client, admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 1);

    client.freeze(&admin).unwrap();

    let res = client.try_open_dispute(&holder, &issuer, &ns, &token, &m);
    match res {
        Err(Ok(RevoraError::ContractFrozen)) => {}
        other => panic!("expected ContractFrozen, got: {:?}", other),
    }
}

#[test]
fn open_dispute_uninitialized_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let holder = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("test");
    let token = Address::generate(&env);
    let m = meta(&env, 1);

    let res = client.try_open_dispute(&holder, &issuer, &ns, &token, &m);
    match res {
        Err(Ok(RevoraError::NotInitialized)) => {}
        other => panic!("expected NotInitialized, got: {:?}", other),
    }
}

// ── get_dispute queries ─────────────────────────────────────────────────────

#[test]
fn get_dispute_returns_record() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 5);

    let dispute_id = client.try_open_dispute(&holder, &issuer, &ns, &token, &m).unwrap().unwrap();

    let record: Dispute = client.get_dispute(&dispute_id).unwrap();
    assert_eq!(record.id, dispute_id);
    assert_eq!(record.holder, holder);
    assert_eq!(record.meta_hash, m);
    assert_eq!(record.status, DisputeStatus::Open);
    assert!(record.opened_at > 0);
}

#[test]
fn get_dispute_nonexistent_returns_none() {
    let env = Env::default();
    let (client, _admin, _issuer, _ns, _token) = {
        let (_e, c, a, i, n, t) = setup();
        (c, a, i, n, t)
    };
    let id = meta(&env, 255);

    let res = client.get_dispute(&id);
    assert_eq!(res, None, "non-existent dispute must return None");
}

#[test]
fn get_dispute_field_integrity() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 9);

    let dispute_id = client.try_open_dispute(&holder, &issuer, &ns, &token, &m).unwrap().unwrap();
    let record: Dispute = client.get_dispute(&dispute_id).unwrap();

    assert_eq!(record.offering_id.issuer, issuer);
    assert_eq!(record.offering_id.namespace, ns);
    assert_eq!(record.offering_id.token, token);
    assert_eq!(record.holder, holder);
    assert_eq!(record.meta_hash, m);
    assert_eq!(record.status, DisputeStatus::Open);
    assert!(record.opened_at > 0);
}

// ── Per-holder isolation ────────────────────────────────────────────────────

#[test]
fn open_dispute_per_holder_isolation() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder_a = make_holder(&env, &client, &issuer, &ns, &token);
    let holder_b = make_holder(&env, &client, &issuer, &ns, &token);

    // Each holder can open MAX_OPEN_DISPUTES_PER_HOLDER disputes independently
    for i in 0..MAX_OPEN_DISPUTES_PER_HOLDER {
        let ma = meta(&env, (i * 2 + 1) as u8);
        let mb = meta(&env, (i * 2 + 2) as u8);
        assert!(client.try_open_dispute(&holder_a, &issuer, &ns, &token, &ma).unwrap().is_ok());
        assert!(client.try_open_dispute(&holder_b, &issuer, &ns, &token, &mb).unwrap().is_ok());
    }

    // Both should now be at cap
    let overflow = meta(&env, 99);
    let ra = client.try_open_dispute(&holder_a, &issuer, &ns, &token, &overflow);
    let rb = client.try_open_dispute(&holder_b, &issuer, &ns, &token, &overflow);
    match ra {
        Err(Ok(RevoraError::MaxDisputesReached)) => {}
        other => panic!("holder_a expected MaxDisputesReached, got: {:?}", other),
    }
    match rb {
        Err(Ok(RevoraError::MaxDisputesReached)) => {}
        other => panic!("holder_b expected MaxDisputesReached, got: {:?}", other),
    }
}
