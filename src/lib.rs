#![no_std]

use soroban_sdk::{contract, contractimpl, Address, Env, Vec};

mod blacklist;
mod events;
mod lifecycle;
mod revenue;
mod types;

pub use types::{DataKey, OfferingStatus};

#[contract]
pub struct RevoraRevenueShare;

#[contractimpl]
impl RevoraRevenueShare {
    // ── Revenue ───────────────────────────────────────────────

    pub fn register_offering(env: Env, issuer: Address, token: Address, revenue_share_bps: u32) {
        issuer.require_auth();
        revenue::register_offering(&env, &issuer, &token, revenue_share_bps);
    }

    pub fn report_revenue(env: Env, issuer: Address, token: Address, amount: i128, period_id: u64) {
        issuer.require_auth();
        revenue::report_revenue(&env, &issuer, &token, amount, period_id);
    }

    // ── Lifecycle ─────────────────────────────────────────────

    pub fn pause_offering(env: Env, issuer: Address, token: Address) {
        issuer.require_auth();
        lifecycle::pause_offering(&env, &issuer, &token);
    }

    pub fn resume_offering(env: Env, issuer: Address, token: Address) {
        issuer.require_auth();
        lifecycle::resume_offering(&env, &issuer, &token);
    }

    pub fn close_offering(env: Env, issuer: Address, token: Address) {
        issuer.require_auth();
        lifecycle::close_offering(&env, &issuer, &token);
    }

    pub fn get_offering_status(env: Env, token: Address) -> OfferingStatus {
        lifecycle::get_status(&env, &token)
    }

    // ── Blacklist ─────────────────────────────────────────────

    pub fn blacklist_add(env: Env, caller: Address, token: Address, investor: Address) {
        caller.require_auth();
        blacklist::blacklist_add(&env, &caller, &token, &investor);
    }

    pub fn blacklist_remove(env: Env, caller: Address, token: Address, investor: Address) {
        caller.require_auth();
        blacklist::blacklist_remove(&env, &caller, &token, &investor);
    }

    pub fn is_blacklisted(env: Env, token: Address, investor: Address) -> bool {
        blacklist::is_blacklisted(&env, &token, &investor)
    }

    pub fn get_blacklist(env: Env, token: Address) -> Vec<Address> {
        blacklist::get_blacklist(&env, &token)
    }
}

#[cfg(test)]
mod test;