use soroban_sdk::{symbol_short, Address, Env, Vec};
use crate::types::{DataKey, OfferingStatus};
use crate::events::EVENT_REVENUE_REPORTED;
use crate::blacklist::get_blacklist;

pub fn register_offering(
    env: &Env,
    issuer: &Address,
    token: &Address,
    revenue_share_bps: u32,
) {
    // Store issuer for future lifecycle auth checks
    env.storage()
        .persistent()
        .set(&DataKey::Issuer(token.clone()), issuer);

    // Default status: Active
    env.storage()
        .persistent()
        .set(&DataKey::Status(token.clone()), &OfferingStatus::Active);

    env.events().publish(
        (symbol_short!("offer_reg"), issuer.clone()),
        (token.clone(), revenue_share_bps),
    );
}

pub fn report_revenue(
    env: &Env,
    issuer: &Address,
    token: &Address,
    amount: i128,
    period_id: u64,
) {
    // Lifecycle gate
    let status: OfferingStatus = env
        .storage()
        .persistent()
        .get(&DataKey::Status(token.clone()))
        .unwrap_or(OfferingStatus::Active);

    match status {
        OfferingStatus::Active => {}
        OfferingStatus::Paused => panic!("offering is paused"),
        OfferingStatus::Closed => panic!("offering is closed"),
    }

    let blacklist: Vec<Address> = get_blacklist(env, token);

    env.events().publish(
        (EVENT_REVENUE_REPORTED, issuer.clone(), token.clone()),
        (amount, period_id, blacklist),
    );
}