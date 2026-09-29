//! # Adversarial and Boundary Test Coverage for `pause_safety` (#1066)
//!
//! Exposes and validates the behavior of `pause_safety` in `src/lib.rs`:
//! - Authorization gating: only the configured `safety` role may soft-pause.
//! - Environment boundaries: uninitialized contracts and contracts without a configured safety role reject the call.
//! - Caller boundaries: arbitrary attackers, admins, issuers, and holders are strictly rejected with `NotAuthorized`.
//! - State preservation: rejected calls leave the pause state completely intact and emit zero events.
//! - Observable side-effects: valid calls transition state to `SoftPaused`, emit `EVENT_PAUSED` and `EVENT_PAUSED2`.
//! - Operational invariants: `SoftPaused` allows `claim` while blocking deposits and configuration mutations.
//! - Idempotency & Lifecycle: repeated calls succeed idempotently, and round-trips with `unpause_safety` restore state.
//! - Multi-instance isolation: safety credentials from one contract cannot affect another instance.

#![cfg(test)]

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    token, Address, Env, IntoVal, Val, Vec,
};

/// Helper to spin up a contract client with admin and safety configured.
fn setup(env: &Env) -> (RevoraRevenueShareClient<'static>, Address, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let safety = Address::generate(env);
    client.initialize(&admin, &Some(safety.clone()), &None::<bool>);
    (client, admin, safety)
}

/// Helper to spin up a contract with an offering and token balance for functional checks.
fn setup_with_offering(
    env: &Env,
) -> (RevoraRevenueShareClient<'static>, Address, Address, Address, Address, Address, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let safety = Address::generate(env);
    client.initialize(&admin, &Some(safety.clone()), &None::<bool>);

    let issuer = Address::generate(env);
    let offering_token = Address::generate(env);
    let payment_admin = Address::generate(env);
    let payment_token = env.register_stellar_asset_contract_v2(payment_admin.clone());
    let holder = Address::generate(env);

    client.register_offering(
        &issuer,
        &Vec::new(env),
        &1u32,
        &symbol_short!("def"),
        &offering_token,
        &10_000,
        &payment_token.address(),
        &0,
        &symbol_short!(""),
        &0,
    );
    client.set_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder, &10_000, &1);

    token::StellarAssetClient::new(env, &payment_token.address()).mint(&issuer, &500_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token.address(),
        &100_000,
        &1,
    );

    (client, admin, safety, issuer, offering_token, payment_token.address(), holder)
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Environment Boundaries (Uninitialized & Safety Unconfigured)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn pause_safety_on_uninitialized_contract_returns_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let random_caller = Address::generate(&env);

    let events_before = env.events().all().len();
    let res = client.try_pause_safety(&random_caller);

    assert_eq!(res, Err(Ok(RevoraError::NotInitialized)));
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());
    assert_eq!(env.events().all().len(), events_before, "no events emitted on rejected call");
}

#[test]
fn pause_safety_when_safety_role_is_none_returns_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    // Initialize without safety role
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let caller = Address::generate(&env);
    let events_before = env.events().all().len();

    // Even admin cannot pause_safety if no safety role was registered
    let res_admin = client.try_pause_safety(&admin);
    assert_eq!(res_admin, Err(Ok(RevoraError::NotInitialized)));

    let res_caller = client.try_pause_safety(&caller);
    assert_eq!(res_caller, Err(Ok(RevoraError::NotInitialized)));

    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());
    assert_eq!(env.events().all().len(), events_before, "no events emitted on rejected call");
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Caller Authorization Boundaries (Admin, Attacker, Issuer, Holder)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn pause_safety_rejects_arbitrary_unauthorized_attacker() {
    let env = Env::default();
    let (client, _admin, _safety) = setup(&env);
    let attacker = Address::generate(&env);
    let events_before = env.events().all().len();

    let res = client.try_pause_safety(&attacker);

    assert_eq!(res, Err(Ok(RevoraError::NotAuthorized)));
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());
    assert_eq!(env.events().all().len(), events_before, "state and events must remain unchanged");
}

#[test]
fn pause_safety_rejects_admin_caller() {
    // Admin must use pause_admin, not pause_safety. Role separation is strict.
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);
    let events_before = env.events().all().len();

    let res = client.try_pause_safety(&admin);

    assert_eq!(res, Err(Ok(RevoraError::NotAuthorized)));
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());
    assert_eq!(env.events().all().len(), events_before, "state and events must remain unchanged");
}

#[test]
fn pause_safety_rejects_issuer_and_holder_callers() {
    let env = Env::default();
    let (client, _admin, _safety, issuer, _offering_token, _payment_token, holder) =
        setup_with_offering(&env);
    let events_before = env.events().all().len();

    let res_issuer = client.try_pause_safety(&issuer);
    assert_eq!(res_issuer, Err(Ok(RevoraError::NotAuthorized)));

    let res_holder = client.try_pause_safety(&holder);
    assert_eq!(res_holder, Err(Ok(RevoraError::NotAuthorized)));

    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());
    assert_eq!(env.events().all().len(), events_before, "state and events must remain unchanged");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Valid Calls & Observable State / Event Transitions
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn pause_safety_succeeds_for_safety_role_and_sets_soft_paused() {
    let env = Env::default();
    let (client, _admin, safety) = setup(&env);

    let events_before = env.events().all().len();
    let res = client.try_pause_safety(&safety);

    assert_eq!(res, Ok(Ok(())));
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
    assert!(client.is_paused());

    // Verify events: legacy EVENT_PAUSED and versioned EVENT_PAUSED2
    let events = env.events().all();
    assert_eq!(events.len(), events_before + 2, "must emit exactly two events");

    let paused_sym: Val = symbol_short!("paused").into_val(&env);
    let paused2_sym: Val = symbol_short!("paused2").into_val(&env);

    let has_paused = events[events_before..].iter().any(|e| e.1.contains(paused_sym));
    let has_paused2 = events[events_before..].iter().any(|e| e.1.contains(paused2_sym));

    assert!(has_paused, "legacy EVENT_PAUSED must be emitted");
    assert!(has_paused2, "versioned EVENT_PAUSED2 must be emitted");
}

#[test]
fn pause_safety_is_idempotent() {
    let env = Env::default();
    let (client, _admin, safety) = setup(&env);

    // First call transitions to SoftPaused
    client.pause_safety(&safety);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);

    // Second call succeeds idempotently without error
    let res = client.try_pause_safety(&safety);
    assert_eq!(res, Ok(Ok(())));
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
    assert!(client.is_paused());
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. State Invariants After Rejected Calls
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn pause_safety_rejected_call_preserves_hard_paused_state() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    // Admin hard-pauses the contract
    client.hard_pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::HardPaused);

    // An attacker tries pause_safety to tamper with the pause tier
    let attacker = Address::generate(&env);
    let events_before = env.events().all().len();
    let res = client.try_pause_safety(&attacker);

    assert_eq!(res, Err(Ok(RevoraError::NotAuthorized)));
    assert_eq!(
        client.get_pause_state(),
        PauseState::HardPaused,
        "HardPaused state must not be disturbed by unauthorized pause_safety"
    );
    assert_eq!(env.events().all().len(), events_before, "no events emitted on rejected attempt");
}

#[test]
fn pause_safety_rejected_call_preserves_soft_paused_state() {
    let env = Env::default();
    let (client, _admin, safety) = setup(&env);

    // Soft-pause validly
    client.pause_safety(&safety);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);

    // Unauthorized caller attempts pause_safety
    let attacker = Address::generate(&env);
    let events_before = env.events().all().len();
    let res = client.try_pause_safety(&attacker);

    assert_eq!(res, Err(Ok(RevoraError::NotAuthorized)));
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
    assert_eq!(env.events().all().len(), events_before);
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Functional Behavior: SoftPaused Allows Claim, Blocks Mutations
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn pause_safety_soft_pause_allows_claim_while_blocking_deposits() {
    let env = Env::default();
    let (client, _admin, safety, issuer, offering_token, payment_token, holder) =
        setup_with_offering(&env);

    // Safety invokes soft-pause
    client.pause_safety(&safety);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);

    // Claim must succeed under SoftPaused
    let claim_res = client.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);
    assert!(claim_res.is_ok(), "claim must succeed under SoftPaused");
    assert_eq!(claim_res.unwrap().unwrap(), 100_000);

    // Deposit must be blocked under SoftPaused
    token::StellarAssetClient::new(&env, &payment_token).mint(&issuer, &50_000);
    let deposit_res = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token,
        &50_000,
        &2,
    );
    assert_eq!(
        deposit_res,
        Err(Ok(RevoraError::ContractPaused)),
        "deposits must be rejected under SoftPaused"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Lifecycle Round-Trip (pause_safety -> unpause_safety -> pause_safety)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn pause_safety_round_trip_lifecycle() {
    let env = Env::default();
    let (client, _admin, safety) = setup(&env);

    // Initially NotPaused
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());

    // Step 1: pause_safety -> SoftPaused
    client.pause_safety(&safety);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
    assert!(client.is_paused());

    // Step 2: unpause_safety -> NotPaused
    client.unpause_safety(&safety);
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());

    // Step 3: pause_safety again -> SoftPaused
    client.pause_safety(&safety);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
    assert!(client.is_paused());
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. Direct Contract Method Invocation (Host Env Bypassing Client)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn pause_safety_direct_contract_invocation() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let safety = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    client.initialize(&admin, &Some(safety.clone()), &None::<bool>);

    // Direct invocation via contract method directly with safety address
    let direct_ok = env.as_contract(&contract_id, || {
        RevoraRevenueShare::pause_safety(env.clone(), safety.clone())
    });
    assert_eq!(direct_ok, Ok(()));
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);

    // Direct invocation with unauthorized caller
    let direct_err = env.as_contract(&contract_id, || {
        RevoraRevenueShare::pause_safety(env.clone(), unauthorized.clone())
    });
    assert_eq!(direct_err, Err(RevoraError::NotAuthorized));
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. Multi-Contract Tenant Isolation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn pause_safety_isolates_across_contract_instances() {
    let env = Env::default();
    env.mock_all_auths();

    // Instance 1
    let id_1 = env.register_contract(None, RevoraRevenueShare);
    let client_1 = RevoraRevenueShareClient::new(&env, &id_1);
    let admin_1 = Address::generate(&env);
    let safety_1 = Address::generate(&env);
    client_1.initialize(&admin_1, &Some(safety_1.clone()), &None::<bool>);

    // Instance 2
    let id_2 = env.register_contract(None, RevoraRevenueShare);
    let client_2 = RevoraRevenueShareClient::new(&env, &id_2);
    let admin_2 = Address::generate(&env);
    let safety_2 = Address::generate(&env);
    client_2.initialize(&admin_2, &Some(safety_2.clone()), &None::<bool>);

    // safety_1 attempts to pause instance 2
    let res = client_2.try_pause_safety(&safety_1);
    assert_eq!(res, Err(Ok(RevoraError::NotAuthorized)));
    assert_eq!(client_2.get_pause_state(), PauseState::NotPaused);

    // safety_2 pauses instance 2
    client_2.pause_safety(&safety_2);
    assert_eq!(client_2.get_pause_state(), PauseState::SoftPaused);

    // Instance 1 remains NotPaused
    assert_eq!(client_1.get_pause_state(), PauseState::NotPaused);
}
