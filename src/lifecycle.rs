use soroban_sdk::{Address, Env};
use crate::types::{DataKey, OfferingStatus};
use crate::events::{EVENT_CLOSED, EVENT_PAUSED, EVENT_RESUMED};

// ── Internal helpers ──────────────────────────────────────────

pub fn get_status(env: &Env, token: &Address) -> OfferingStatus {
    env.storage()
        .persistent()
        .get::<DataKey, OfferingStatus>(&DataKey::Status(token.clone()))
        .unwrap_or(OfferingStatus::Active)
}

fn set_status(env: &Env, token: &Address, status: OfferingStatus) {
    env.storage()
        .persistent()
        .set(&DataKey::Status(token.clone()), &status);
}

/// Verify `caller` is the stored issuer for this offering.
pub fn assert_issuer(env: &Env, token: &Address, caller: &Address) {
    let stored: Address = env
        .storage()
        .persistent()
        .get(&DataKey::Issuer(token.clone()))
        .expect("offering not registered");
    assert!(stored == *caller, "caller is not the offering issuer");
}

// ── Public lifecycle functions ────────────────────────────────

pub fn pause_offering(env: &Env, issuer: &Address, token: &Address) {
    assert_issuer(env, token, issuer);

    match get_status(env, token) {
        OfferingStatus::Active => {}
        OfferingStatus::Paused => panic!("offering is already paused"),
        OfferingStatus::Closed => panic!("offering is closed and cannot be paused"),
    }

    set_status(env, token, OfferingStatus::Paused);
    env.events()
        .publish((EVENT_PAUSED, token.clone(), issuer.clone()), ());
}

pub fn resume_offering(env: &Env, issuer: &Address, token: &Address) {
    assert_issuer(env, token, issuer);

    match get_status(env, token) {
        OfferingStatus::Paused => {}
        OfferingStatus::Active => panic!("offering is already active"),
        OfferingStatus::Closed => panic!("offering is closed and cannot be resumed"),
    }

    set_status(env, token, OfferingStatus::Active);
    env.events()
        .publish((EVENT_RESUMED, token.clone(), issuer.clone()), ());
}

pub fn close_offering(env: &Env, issuer: &Address, token: &Address) {
    assert_issuer(env, token, issuer);

    match get_status(env, token) {
        OfferingStatus::Closed => panic!("offering is already closed"),
        OfferingStatus::Active | OfferingStatus::Paused => {}
    }

    set_status(env, token, OfferingStatus::Closed);
    env.events()
        .publish((EVENT_CLOSED, token.clone(), issuer.clone()), ());
}