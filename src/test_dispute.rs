#![cfg(test)]

use crate::{Dispute, DisputeSeverity, DisputeStatus, MAX_OPEN_DISPUTES_PER_HOLDER};
use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    BytesN, Env,
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Symbol, IntoVal,
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
    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &2500, &payout, &0i128, &symbol_short!(""), &0u32);
    (env, client, admin, issuer, ns, token)
}

fn make_holder(env: &Env, client: &RevoraRevenueShareClient<'static>, issuer: &Address, ns: &Symbol, token: &Address) -> Address {
    let holder = Address::generate(env);
    client.set_holder_share(issuer, ns, token, &holder, &500u32);
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

    let dispute_id = client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m);
    assert!(dispute_id.is_ok(), "open_dispute should succeed");
}

#[test]
fn open_dispute_returns_deterministic_id() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 42);

    let id1 = client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m).unwrap();
    let id2 = client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m).unwrap();
    assert_eq!(id1, id2, "same inputs must produce same dispute ID");
}

#[test]
fn open_dispute_different_meta_produces_different_id() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);

    let id1 = client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &meta(&env, 1)).unwrap();
    let id2 = client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &meta(&env, 2)).unwrap();
    assert_ne!(id1, id2, "different meta_hash must yield different ID");
}

// ── Event emission ──────────────────────────────────────────────────────────

#[test]
fn open_dispute_emits_event() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 7);

    let _ = client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m).unwrap();

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

    let res = client.open_dispute(&non_holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m);
    match res {
        Err(RevoraError::DisputeZeroShare) => {}
        other => panic!("expected DisputeZeroShare, got: {:?}", other),
    }
}

#[test]
fn open_dispute_duplicate_rejected() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 1);

    client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m).unwrap();

    let res = client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m);
    match res {
        Err(RevoraError::DisputeAlreadyOpen) => {}
        other => panic!("expected DisputeAlreadyOpen, got: {:?}", other),
    }
}

#[test]
fn open_dispute_spam_cap_enforced() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);

    for i in 0..MAX_OPEN_DISPUTES_PER_HOLDER {
        let m = meta(&env, i as u8 + 1);
        let res = client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m);
        assert!(res.is_ok(), "dispute {} should succeed", i + 1);
    }

    // Cap reached — next one must fail
    let overflow = meta(&env, 99);
    let res = client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &overflow);
    match res {
        Err(RevoraError::MaxDisputesReached) => {}
        other => panic!("expected MaxDisputesReached, got: {:?}", other),
    }
}

#[test]
fn open_dispute_frozen_rejected() {
    let (env, client, admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 1);

    client.freeze(&admin).unwrap();

    let res = client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m);
    match res {
        Err(RevoraError::ContractFrozen) => {}
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

    let res = client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m);
    match res {
        Err(RevoraError::NotInitialized) => {}
        other => panic!("expected NotInitialized, got: {:?}", other),
    }
}

// ── get_dispute queries ─────────────────────────────────────────────────────

#[test]
fn get_dispute_returns_record() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 5);

    let dispute_id = client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m).unwrap();

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

    let dispute_id = client.open_dispute(&holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m).unwrap();
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
        assert!(client.open_dispute(&holder_a, &issuer, &ns, &token, &DisputeSeverity::Standard, &ma).is_ok());
        assert!(client.open_dispute(&holder_b, &issuer, &ns, &token, &DisputeSeverity::Standard, &mb).is_ok());
    }

    // Both should now be at cap
    let overflow = meta(&env, 99);
    let ra = client.open_dispute(&holder_a, &issuer, &ns, &token, &DisputeSeverity::Standard, &overflow);
    let rb = client.open_dispute(&holder_b, &issuer, &ns, &token, &DisputeSeverity::Standard, &overflow);
    match ra {
        Err(RevoraError::MaxDisputesReached) => {}
        other => panic!("holder_a expected MaxDisputesReached, got: {:?}", other),
    }
    match rb {
        Err(RevoraError::MaxDisputesReached) => {}
        other => panic!("holder_b expected MaxDisputesReached, got: {:?}", other),
    }
}

// ── Dispute severity — Standard vs Critical ──────────────────────────────

#[test]
fn critical_dispute_blocks_claim() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 1);

    // Open a Critical dispute
    let dispute_id = client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Critical, &m,
    ).unwrap();

    // Claim must be blocked
    let res = client.try_claim(&holder, &issuer, &ns, &token, &0u32);
    assert_eq!(res, Err(Ok(RevoraError::DisputeFreezeActive)),
        "critical dispute must block claims");

    // Resolve the dispute — freeze lifts
    client.resolve_dispute(&issuer, &dispute_id, &DisputeStatus::Resolved);

    // Claim no longer frozen; falls through to NoPendingClaims (no periods deposited)
    let res = client.try_claim(&holder, &issuer, &ns, &token, &0u32);
    assert_eq!(res, Err(Ok(RevoraError::NoPendingClaims)),
        "after resolve, claim must proceed past freeze check");
}

#[test]
fn standard_dispute_does_not_block_claim() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 1);

    // Open a Standard dispute — no freeze effect
    client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m,
    ).unwrap();

    // Claim must NOT be blocked by dispute; proceeds to NoPendingClaims
    let res = client.try_claim(&holder, &issuer, &ns, &token, &0u32);
    assert_eq!(res, Err(Ok(RevoraError::NoPendingClaims)),
        "standard dispute must not block claims");
}

#[test]
fn critical_dispute_emits_freeze_on_event() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 1);

    client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Critical, &m,
    ).unwrap();

    let events = env.events().all();
    let freeze_events: Vec<_> = events.iter()
        .filter(|e| e.0.to_string().contains("dsp_frzon"))
        .collect();
    assert!(!freeze_events.is_empty(), "expected dispute_freeze_on event");
}

#[test]
fn resolve_dispute_emits_freeze_off_event() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 1);

    let dispute_id = client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Critical, &m,
    ).unwrap();

    client.resolve_dispute(&issuer, &dispute_id, &DisputeStatus::Resolved);

    let events = env.events().all();
    let off_events: Vec<_> = events.iter()
        .filter(|e| e.0.to_string().contains("dsp_frzoff"))
        .collect();
    assert!(!off_events.is_empty(), "expected dispute_freeze_off event");
}

#[test]
fn resolve_dispute_wrong_caller_rejected() {
    let (env, client, admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 1);
    let dispute_id = client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Critical, &m,
    ).unwrap();

    // Admin is not the issuer — must be rejected
    let res = client.try_resolve_dispute(&admin, &dispute_id, &DisputeStatus::Resolved);
    match res {
        Err(Ok(RevoraError::NotDisputeIssuer)) => {}
        other => panic!("expected NotDisputeIssuer, got: {:?}", other),
    }
}

#[test]
fn resolve_dispute_nonexistent_rejected() {
    let (env, client, _admin, issuer, _ns, _token) = setup();
    let id = meta(&env, 255);

    let res = client.try_resolve_dispute(&issuer, &id, &DisputeStatus::Resolved);
    match res {
        Err(Ok(RevoraError::DisputeNotFound)) => {}
        other => panic!("expected DisputeNotFound, got: {:?}", other),
    }
}

#[test]
fn resolve_dispute_already_resolved_rejected() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);
    let m = meta(&env, 1);
    let dispute_id = client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &m,
    ).unwrap();

    // First resolve succeeds
    client.resolve_dispute(&issuer, &dispute_id, &DisputeStatus::Resolved);

    // Second resolve must fail
    let res = client.try_resolve_dispute(&issuer, &dispute_id, &DisputeStatus::Rejected);
    match res {
        Err(Ok(RevoraError::DisputeAlreadyResolved)) => {}
        other => panic!("expected DisputeAlreadyResolved, got: {:?}", other),
    }
}

// ── Multiple overlapping critical disputes ──────────────────────────────────

#[test]
fn multiple_critical_disputes_keep_freeze_active_until_all_resolved() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);

    // Open two critical disputes
    let id1 = client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Critical, &meta(&env, 1),
    ).unwrap();
    let id2 = client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Critical, &meta(&env, 2),
    ).unwrap();

    // Freeze is active
    let res = client.try_claim(&holder, &issuer, &ns, &token, &0u32);
    assert_eq!(res, Err(Ok(RevoraError::DisputeFreezeActive)),
        "two critical disputes must block claims");

    // Resolve first dispute — freeze must still be active (second is still open)
    client.resolve_dispute(&issuer, &id1, &DisputeStatus::Resolved);
    let res = client.try_claim(&holder, &issuer, &ns, &token, &0u32);
    assert_eq!(res, Err(Ok(RevoraError::DisputeFreezeActive)),
        "freeze must remain after resolving only one of two critical disputes");

    // Resolve second dispute — freeze lifts
    client.resolve_dispute(&issuer, &id2, &DisputeStatus::Resolved);
    let res = client.try_claim(&holder, &issuer, &ns, &token, &0u32);
    assert_eq!(res, Err(Ok(RevoraError::NoPendingClaims)),
        "freeze must lift after last critical dispute is resolved");
}

#[test]
fn multiple_critical_disputes_only_emit_one_freeze_on_event() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);

    client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Critical, &meta(&env, 1),
    ).unwrap();
    client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Critical, &meta(&env, 2),
    ).unwrap();

    let events = env.events().all();
    let freeze_on: Vec<_> = events.iter()
        .filter(|e| {
            let (_, topics, _) = e;
            topics.len() >= 1 && {
                let t0: Symbol = topics.get(0).unwrap().into_val(&env);
                t0 == symbol_short!("dsp_frzon")
            }
        })
        .collect();
    assert_eq!(freeze_on.len(), 1,
        "only one dsp_frzon event must be emitted for multiple overlapping critical disputes");
}

#[test]
fn multiple_critical_disputes_only_emit_one_freeze_off_event() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);

    let id1 = client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Critical, &meta(&env, 1),
    ).unwrap();
    let id2 = client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Critical, &meta(&env, 2),
    ).unwrap();

    // Resolve first — no dsp_frzoff yet
    client.resolve_dispute(&issuer, &id1, &DisputeStatus::Resolved);
    let events_after_first = env.events().all();
    let freeze_off_after_first: Vec<_> = events_after_first.iter()
        .filter(|e| {
            let (_, topics, _) = e;
            topics.len() >= 1 && {
                let t0: Symbol = topics.get(0).unwrap().into_val(&env);
                t0 == symbol_short!("dsp_frzoff")
            }
        })
        .collect();
    assert_eq!(freeze_off_after_first.len(), 0,
        "no dsp_frzoff until last critical dispute is resolved");

    // Resolve second — dsp_frzoff emitted
    client.resolve_dispute(&issuer, &id2, &DisputeStatus::Resolved);
    let events_after_second = env.events().all();
    let freeze_off_after_second: Vec<_> = events_after_second.iter()
        .filter(|e| {
            let (_, topics, _) = e;
            topics.len() >= 1 && {
                let t0: Symbol = topics.get(0).unwrap().into_val(&env);
                t0 == symbol_short!("dsp_frzoff")
            }
        })
        .collect();
    assert_eq!(freeze_off_after_second.len(), 1,
        "exactly one dsp_frzoff event must be emitted after last critical dispute is resolved");
}

#[test]
fn resolve_dispute_with_rejected_also_clears_freeze() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);

    let dispute_id = client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Critical, &meta(&env, 1),
    ).unwrap();

    // Reject the dispute (not resolve) — freeze must also lift
    client.resolve_dispute(&issuer, &dispute_id, &DisputeStatus::Rejected);

    let res = client.try_claim(&holder, &issuer, &ns, &token, &0u32);
    assert_eq!(res, Err(Ok(RevoraError::NoPendingClaims)),
        "rejecting a critical dispute must also lift the freeze");
}

#[test]
fn critical_dispute_freeze_isolation_across_offerings() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("test");
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout = Address::generate(&env);
    let holder = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token_a, &2500, &payout, &0i128, &symbol_short!(""), &0u32);
    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token_b, &2500, &payout, &0i128, &symbol_short!(""), &0u32);
    client.set_holder_share(&issuer, &ns, &token_a, &holder, &500u32);
    client.set_holder_share(&issuer, &ns, &token_b, &holder, &500u32);

    // Open critical dispute on token_a only
    client.open_dispute(
        &holder, &issuer, &ns, &token_a, &DisputeSeverity::Critical, &meta(&env, 1),
    ).unwrap();

    // Claim on token_a must be blocked
    let res_a = client.try_claim(&holder, &issuer, &ns, &token_a, &0u32);
    assert_eq!(res_a, Err(Ok(RevoraError::DisputeFreezeActive)),
        "critical dispute on token_a must block its claims");

    // Claim on token_b must NOT be blocked
    let res_b = client.try_claim(&holder, &issuer, &ns, &token_b, &0u32);
    assert_eq!(res_b, Err(Ok(RevoraError::NoPendingClaims)),
        "critical dispute on token_a must NOT block claims on token_b");
}

#[test]
fn is_dispute_freeze_active_query() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);

    // No disputes yet — freeze must be false; claim proceeds to NoPendingClaims
    let res = client.try_claim(&holder, &issuer, &ns, &token, &0u32);
    assert_eq!(res, Err(Ok(RevoraError::NoPendingClaims)),
        "no disputes -> freeze inactive, claim proceeds normally");

    // Open a critical dispute — freeze becomes active
    let dispute_id = client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Critical, &meta(&env, 1),
    ).unwrap();
    let res = client.try_claim(&holder, &issuer, &ns, &token, &0u32);
    assert_eq!(res, Err(Ok(RevoraError::DisputeFreezeActive)),
        "critical dispute -> freeze active");

    // Standard dispute should not affect freeze
    client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &meta(&env, 2),
    ).unwrap();
    let res = client.try_claim(&holder, &issuer, &ns, &token, &0u32);
    assert_eq!(res, Err(Ok(RevoraError::DisputeFreezeActive)),
        "adding standard dispute must not change freeze state");

    // Resolve critical — freeze lifts
    client.resolve_dispute(&issuer, &dispute_id, &DisputeStatus::Resolved);
    let res = client.try_claim(&holder, &issuer, &ns, &token, &0u32);
    assert_eq!(res, Err(Ok(RevoraError::NoPendingClaims)),
        "after resolving critical dispute -> freeze inactive");
}

#[test]
fn standard_dispute_resolve_does_not_emit_freeze_events() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let holder = make_holder(&env, &client, &issuer, &ns, &token);

    let dispute_id = client.open_dispute(
        &holder, &issuer, &ns, &token, &DisputeSeverity::Standard, &meta(&env, 1),
    ).unwrap();

    client.resolve_dispute(&issuer, &dispute_id, &DisputeStatus::Resolved);

    let events = env.events().all();
    let freeze_events: Vec<_> = events.iter()
        .filter(|e| {
            let (_, topics, _) = e;
            if topics.len() < 1 { return false; }
            let t0: Symbol = topics.get(0).unwrap().into_val(&env);
            t0 == symbol_short!("dsp_frzon") || t0 == symbol_short!("dsp_frzoff")
        })
        .collect();
    assert_eq!(freeze_events.len(), 0,
        "standard dispute must not emit dsp_frzon or dsp_frzoff events");
}
