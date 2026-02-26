#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

/// Basic skeleton for a revenue-share contract.
///
/// This is intentionally minimal and focuses on the high-level shape:
/// - Registering a startup "offering"
/// - Recording a revenue report
/// - Emitting events that an off-chain distribution engine can consume

#[contract]
pub struct RevoraRevenueShare;

#[derive(Clone)]
pub struct Offering {
    pub issuer: Address,
    pub token: Address,
    pub revenue_share_bps: u32,
}

const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_PERIOD_CLOSED: Symbol = symbol_short!("per_clo");

#[derive(Clone)]
#[contracttype]
enum DataKey {
    LatestPeriod(Address, Address),
    Closed(Address, Address, u64),
}

#[contractimpl]
impl RevoraRevenueShare {
    /// Register a new revenue-share offering.
    /// Access control is simplified; the issuer authorizes creation.
    pub fn register_offering(env: Env, issuer: Address, token: Address, revenue_share_bps: u32) {
        issuer.require_auth();

        env.events().publish(
            (symbol_short!("offer_reg"), issuer.clone()),
            (token, revenue_share_bps),
        );
    }

    /// Record a revenue report for an offering.
    /// Semantics:
    /// - Monotonic non-decreasing `period_id` per (issuer, token) is enforced.
    /// - Duplicate submissions for the same open period are allowed.
    /// - Submissions for a period explicitly closed via `close_period` are rejected.
    /// - Time windows are configured off-chain; this method only enforces ordering and closure.
    pub fn report_revenue(env: Env, issuer: Address, token: Address, amount: i128, period_id: u64) {
        issuer.require_auth();

        // Reject if this period has been explicitly closed.
        let is_closed = env.storage().instance().has(&DataKey::Closed(
            issuer.clone(),
            token.clone(),
            period_id,
        ));
        if is_closed {
            panic!("period closed");
        }

        // Enforce monotonic ordering by tracking the latest accepted period.
        let latest = env
            .storage()
            .instance()
            .get::<_, u64>(&DataKey::LatestPeriod(issuer.clone(), token.clone()));
        if let Some(prev) = latest {
            if period_id < prev {
                panic!("out-of-order period");
            }
            if period_id > prev {
                env.storage().instance().set(
                    &DataKey::LatestPeriod(issuer.clone(), token.clone()),
                    &period_id,
                );
            }
        } else {
            env.storage().instance().set(
                &DataKey::LatestPeriod(issuer.clone(), token.clone()),
                &period_id,
            );
        }

        env.events().publish(
            (EVENT_REVENUE_REPORTED, issuer.clone(), token.clone()),
            (amount, period_id),
        );
    }

    /// Close a specific period for an offering so it no longer accepts reports.
    /// Off-chain systems call this at the end of a configured reporting window.
    /// Idempotent: repeated calls for an already-closed period have no effect.
    pub fn close_period(env: Env, issuer: Address, token: Address, period_id: u64) {
        issuer.require_auth();

        let key = DataKey::Closed(issuer.clone(), token.clone(), period_id);
        if env.storage().instance().has(&key) {
            return;
        }
        env.storage().instance().set(&key, &true);
        env.events()
            .publish((EVENT_PERIOD_CLOSED, issuer, token), (period_id,));
    }

    /// Returns the latest accepted period for an offering, or 0 if none.
    /// Useful for integrators to track progress and enforce client-side ordering.
    pub fn latest_period(env: Env, issuer: Address, token: Address) -> u64 {
        env.storage()
            .instance()
            .get::<_, u64>(&DataKey::LatestPeriod(issuer, token))
            .unwrap_or(0)
    }

    /// Returns true if the given period has been explicitly closed.
    /// Integrators should consult this before attempting to report within the same process.
    pub fn is_period_closed(env: Env, issuer: Address, token: Address, period_id: u64) -> bool {
        env.storage()
            .instance()
            .has(&DataKey::Closed(issuer, token, period_id))
    }
}

mod test;
