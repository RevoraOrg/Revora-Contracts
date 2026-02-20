#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, Map, Symbol, Vec,
};

// ── Constants ────────────────────────────────────────────────
const BPS_DENOMINATOR: i128 = 10_000;
const MAX_BPS: u32 = 10_000;

// ── Event symbols ────────────────────────────────────────────
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_BL_ADD: Symbol = symbol_short!("bl_add");
const EVENT_BL_REM: Symbol = symbol_short!("bl_rem");
const EVENT_DIST_CALC: Symbol = symbol_short!("dist_calc");

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
    Offering(Address),
}

// ── Offering data ─────────────────────────────────────────────
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Offering {
    pub issuer: Address,
    pub revenue_share_bps: u32,
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

        if revenue_share_bps > MAX_BPS {
            panic!("revenue_share_bps cannot exceed 10000 (100%)");
        }

        let offering = Offering {
            issuer: issuer.clone(),
            revenue_share_bps,
        };

        let key = DataKey::Offering(token.clone());
        env.storage().persistent().set(&key, &offering);

        env.events().publish(
            (symbol_short!("offer_reg"), issuer.clone()),
            (token, revenue_share_bps),
        );
    }

    /// Get offering details for a token.
    pub fn get_offering(env: Env, token: Address) -> Option<Offering> {
        let key = DataKey::Offering(token);
        env.storage().persistent().get(&key)
    }

    /// Record a revenue report for an offering.
    ///
    /// The event payload now includes the current blacklist so off-chain
    /// distribution engines can filter recipients in the same atomic step.
    pub fn report_revenue(env: Env, issuer: Address, token: Address, amount: i128, period_id: u64) {
        issuer.require_auth();

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

        env.events()
            .publish((EVENT_BL_ADD, token, caller), investor);
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

        env.events()
            .publish((EVENT_BL_REM, token, caller), investor);
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

    // ── Revenue distribution calculation ───────────────────────

    /// Calculate the distribution amount for a token holder.
    ///
    /// This function computes the payout amount for a single holder using
    /// fixed-point arithmetic with basis points (BPS) precision.
    ///
    /// Formula:
    ///   distributable_revenue = total_revenue * revenue_share_bps / BPS_DENOMINATOR
    ///   holder_payout = holder_balance * distributable_revenue / total_supply
    ///
    /// Rounding: Uses integer division which rounds down (floor).
    /// This is conservative and ensures the contract never over-distributes.
    ///
    /// # Arguments
    /// * `env` - Soroban environment
    /// * `caller` - Address calling the function (for auth)
    /// * `token` - The offering token address
    /// * `total_revenue` - Total revenue amount to be distributed
    /// * `total_supply` - Total supply of the token
    /// * `holder_balance` - Balance of the holder requesting distribution
    /// * `holder` - Address of the holder (for event emission)
    ///
    /// # Returns
    /// * `i128` - The calculated payout amount for the holder
    ///
    /// # Panics
    /// * If offering not found for token
    /// * If total_supply is zero
    /// * If holder is blacklisted
    pub fn calculate_distribution(
        env: Env,
        caller: Address,
        token: Address,
        total_revenue: i128,
        total_supply: i128,
        holder_balance: i128,
        holder: Address,
    ) -> i128 {
        caller.require_auth();

        if total_supply == 0 {
            panic!("total_supply cannot be zero");
        }

        let offering =
            Self::get_offering(env.clone(), token.clone()).expect("offering not found for token");

        if Self::is_blacklisted(env.clone(), token.clone(), holder.clone()) {
            panic!("holder is blacklisted and cannot receive distribution");
        }

        if total_revenue == 0 || holder_balance == 0 {
            let payout = 0i128;
            env.events().publish(
                (EVENT_DIST_CALC, token.clone(), holder.clone()),
                (
                    total_revenue,
                    total_supply,
                    holder_balance,
                    offering.revenue_share_bps,
                    payout,
                ),
            );
            return payout;
        }

        let distributable_revenue = (total_revenue * offering.revenue_share_bps as i128)
            .checked_div(BPS_DENOMINATOR)
            .expect("division overflow");

        let payout = (holder_balance * distributable_revenue)
            .checked_div(total_supply)
            .expect("division overflow");

        env.events().publish(
            (EVENT_DIST_CALC, token, holder),
            (
                total_revenue,
                total_supply,
                holder_balance,
                offering.revenue_share_bps,
                payout,
            ),
        );

        payout
    }

    /// Calculate the total distributable revenue for an offering.
    ///
    /// This is a helper function for off-chain verification.
    pub fn calculate_total_distributable(env: Env, token: Address, total_revenue: i128) -> i128 {
        let offering = Self::get_offering(env, token).expect("offering not found for token");

        if total_revenue == 0 {
            return 0;
        }

        (total_revenue * offering.revenue_share_bps as i128)
            .checked_div(BPS_DENOMINATOR)
            .expect("division overflow")
    }
}

mod test;
