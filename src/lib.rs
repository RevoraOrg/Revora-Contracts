#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    Address, Env, Map, Symbol, Vec,
};

// ── Event symbols ────────────────────────────────────────────
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_REVENUE_REPORT_INITIAL: Symbol = symbol_short!("rev_ini");
const EVENT_REVENUE_REPORT_OVERRIDE: Symbol = symbol_short!("rev_ovr");
const EVENT_REVENUE_REPORT_REJECTED: Symbol = symbol_short!("rev_rej");
const EVENT_BL_ADD: Symbol          = symbol_short!("bl_add");
const EVENT_BL_REM: Symbol          = symbol_short!("bl_rem");

// ── Storage key ──────────────────────────────────────────────
/// One blacklist map per offering, keyed by the offering's token address.
///
/// Blacklist precedence rule: a blacklisted address is **always** excluded
/// from payouts, regardless of any whitelist or investor registration.
/// If the same address appears in both a whitelist and this blacklist,
/// the blacklist wins unconditionally.
#[contracttype]
pub enum DataKey {
    Blacklist(Address),
    /// Tracks reported periods per (issuer, token) pair
    /// Maps (issuer, token) -> Map<period_id, (amount, timestamp)>
    RevenueReport(Address, Address),
}

// ── Contract ─────────────────────────────────────────────────
#[contract]
pub struct RevoraRevenueShare;

#[contractimpl]
impl RevoraRevenueShare {
    // ── Existing entry-points ─────────────────────────────────

    /// Register a new revenue-share offering.
    pub fn register_offering(env: Env, issuer: Address, token: Address, revenue_share_bps: u32) {
        issuer.require_auth();
        env.events().publish(
            (symbol_short!("offer_reg"), issuer.clone()),
            (token, revenue_share_bps),
        );
    }

    /// Record a revenue report for an offering.
    ///
    /// Idempotent - prevents duplicate reports for the same issuer, token, and period_id.
    /// 
    /// # Arguments
    /// * `issuer` - The address reporting the revenue
    /// * `token` - The token address for the offering
    /// * `amount` - The revenue amount to report
    /// * `period_id` - Unique identifier for the reporting period
    /// * `override_existing` - If true, allows overwriting an existing report
    pub fn report_revenue(
        env: Env,
        issuer: Address,
        token: Address,
        amount: i128,
        period_id: u64,
        override_existing: bool,
    ) {
        issuer.require_auth();

        let key = DataKey::RevenueReport(issuer.clone(), token.clone());
        let mut reports: Map<u64, (i128, u64)> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Map::new(&env));

        let current_timestamp = env.ledger().timestamp();
        
        match reports.get(period_id) {
            Some((existing_amount, _timestamp)) => {
                if override_existing {
                    // Allow override with explicit intent
                    reports.set(period_id, (amount, current_timestamp));
                    env.storage().persistent().set(&key, &reports);
                    
                    let blacklist = Self::get_blacklist(env.clone(), token.clone());
                    env.events().publish(
                        (EVENT_REVENUE_REPORT_OVERRIDE, issuer.clone(), token.clone()),
                        (amount, period_id, existing_amount, blacklist),
                    );
                } else {
                    // Reject duplicate report
                    let blacklist = Self::get_blacklist(env.clone(), token.clone());
                    env.events().publish(
                        (EVENT_REVENUE_REPORT_REJECTED, issuer.clone(), token.clone()),
                        (amount, period_id, existing_amount, blacklist),
                    );
                    // Note: In production, you might want to revert the transaction here
                    // For now, we emit a rejection event and continue
                }
            }
            None => {
                // First time reporting this period
                reports.set(period_id, (amount, current_timestamp));
                env.storage().persistent().set(&key, &reports);
                
                let blacklist = Self::get_blacklist(env.clone(), token.clone());
                env.events().publish(
                    (EVENT_REVENUE_REPORT_INITIAL, issuer.clone(), token.clone()),
                    (amount, period_id, blacklist),
                );
            }
        }

        // Maintain backward compatibility with existing event
        let blacklist = Self::get_blacklist(env.clone(), token.clone());
        env.events().publish(
            (EVENT_REVENUE_REPORTED, issuer.clone(), token.clone()),
            (amount, period_id, blacklist),
        );
    }

    // ── Blacklist management ──────────────────────────────────

    /// Add `investor` to the per-offering blacklist for `token`.
    ///
    /// Idempotent — calling with an already-blacklisted address is safe.
    pub fn blacklist_add(env: Env, caller: Address, token: Address, investor: Address) {
        caller.require_auth();

        let key = DataKey::Blacklist(token.clone());
        let mut map: Map<Address, bool> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Map::new(&env));

        map.set(investor.clone(), true);
        env.storage().persistent().set(&key, &map);

        env.events().publish((EVENT_BL_ADD, token, caller), investor);
    }

    /// Remove `investor` from the per-offering blacklist for `token`.
    ///
    /// Idempotent — calling when the address is not listed is safe.
    pub fn blacklist_remove(env: Env, caller: Address, token: Address, investor: Address) {
        caller.require_auth();

        let key = DataKey::Blacklist(token.clone());
        let mut map: Map<Address, bool> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Map::new(&env));

        map.remove(investor.clone());
        env.storage().persistent().set(&key, &map);

        env.events().publish((EVENT_BL_REM, token, caller), investor);
    }

    /// Returns `true` if `investor` is blacklisted for `token`'s offering.
    pub fn is_blacklisted(env: Env, token: Address, investor: Address) -> bool {
        let key = DataKey::Blacklist(token);
        env.storage()
            .persistent()
            .get::<DataKey, Map<Address, bool>>(&key)
            .map(|m| m.get(investor).unwrap_or(false))
            .unwrap_or(false)
    }

    /// Return all blacklisted addresses for `token`'s offering.
    pub fn get_blacklist(env: Env, token: Address) -> Vec<Address> {
        let key = DataKey::Blacklist(token);
        env.storage()
            .persistent()
            .get::<DataKey, Map<Address, bool>>(&key)
            .map(|m| m.keys())
            .unwrap_or_else(|| Vec::new(&env))
    }

    // ── Revenue report management ───────────────────────────────

    /// Check if a revenue report exists for the given issuer, token, and period_id.
    pub fn has_revenue_report(env: Env, issuer: Address, token: Address, period_id: u64) -> bool {
        let key = DataKey::RevenueReport(issuer, token);
        env.storage()
            .persistent()
            .get::<DataKey, Map<u64, (i128, u64)>>(&key)
            .map(|reports| reports.contains_key(period_id))
            .unwrap_or(false)
    }

    /// Get the revenue report for a specific period.
    /// Returns (amount, timestamp) if found, None otherwise.
    pub fn get_revenue_report(env: Env, issuer: Address, token: Address, period_id: u64) -> Option<(i128, u64)> {
        let key = DataKey::RevenueReport(issuer, token);
        env.storage()
            .persistent()
            .get::<DataKey, Map<u64, (i128, u64)>>(&key)
            .and_then(|reports| reports.get(period_id))
    }

    /// Get all revenue reports for an issuer's token offering.
    /// Returns a vector of (period_id, amount, timestamp).
    pub fn get_revenue_report_history(env: Env, issuer: Address, token: Address) -> Vec<(u64, i128, u64)> {
        let key = DataKey::RevenueReport(issuer, token);
        env.storage()
            .persistent()
            .get::<DataKey, Map<u64, (i128, u64)>>(&key)
            .map(|reports| {
                let mut result = Vec::new(&env);
                for (period_id, (amount, timestamp)) in reports {
                    result.push_back((period_id, amount, timestamp));
                }
                result
            })
            .unwrap_or_else(|| Vec::new(&env))
    }
}

mod test;