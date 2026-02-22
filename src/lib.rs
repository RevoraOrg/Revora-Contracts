#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec};
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    Address, Env, Map, Symbol, Vec,
};

// ── Event symbols ────────────────────────────────────────────
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
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
    /// Per-offering reentrancy/in-progress flag used to prevent
    /// accidental double-execution or re-entrant calls that would
    /// mutate the same offering state within a single logical flow.
    ///
    /// Security assumptions:
    /// - The flag is set at the start of a mutating operation and
    ///   cleared at the end. If the enclosing transaction panics,
    ///   storage writes revert, so the flag will not be left set.
    /// - This is a simple defensive measure; it does not replace
    ///   careful protocol design or comprehensive audits.
    InProgress(Address),
}

// ── Contract ─────────────────────────────────────────────────
#[contract]
pub struct RevoraRevenueShare;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Offering {
    pub issuer: Address,
    pub token: Address,
    pub revenue_share_bps: u32,
}

/// Storage keys for offering persistence.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Total number of offerings registered by an issuer.
    OfferCount(Address),
    /// Individual offering stored at (issuer, index).
    OfferItem(Address, u32),
}

/// Maximum number of offerings returned in a single page.
const MAX_PAGE_LIMIT: u32 = 20;

const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");

#[contractimpl]
impl RevoraRevenueShare {
    // ── Existing entry-points ─────────────────────────────────

    /// Register a new revenue-share offering.
    pub fn register_offering(env: Env, issuer: Address, token: Address, revenue_share_bps: u32) {
        issuer.require_auth();

        // Persist the offering with an auto-incrementing index.
        let count_key = DataKey::OfferCount(issuer.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let offering = Offering {
            issuer: issuer.clone(),
            token: token.clone(),
            revenue_share_bps,
        };

        let item_key = DataKey::OfferItem(issuer.clone(), count);
        env.storage().persistent().set(&item_key, &offering);
        env.storage().persistent().set(&count_key, &(count + 1));

        env.events().publish(
            (symbol_short!("offer_reg"), issuer.clone()),
            (token, revenue_share_bps),
        );
    }

    /// Record a revenue report for an offering.
    ///
    /// The event payload now includes the current blacklist so off-chain
    /// distribution engines can filter recipients in the same atomic step.
    pub fn report_revenue(
        env: Env,
        issuer: Address,
        token: Address,
        amount: i128,
        period_id: u64,
    ) {
        issuer.require_auth();

        let blacklist = Self::get_blacklist(env.clone(), token.clone());

        env.events().publish(
            (EVENT_REVENUE_REPORTED, issuer.clone(), token.clone()),
            (amount, period_id, blacklist),
        );
    }
    /// Return the total number of offerings registered by `issuer`.
    pub fn get_offering_count(env: Env, issuer: Address) -> u32 {
        let count_key = DataKey::OfferCount(issuer);
        env.storage().persistent().get(&count_key).unwrap_or(0)
    }

    /// Return a page of offerings for `issuer`.
    ///
    /// # Arguments
    /// * `start` – Zero-based cursor indicating where to begin reading.
    /// * `limit` – Maximum items to return. Capped at `MAX_PAGE_LIMIT` (20).
    ///
    /// # Returns
    /// A tuple of `(offerings, next_cursor)` where `next_cursor` is `None`
    /// when there are no more items after this page.
    pub fn get_offerings_page(
        env: Env,
        issuer: Address,
        start: u32,
        limit: u32,
    ) -> (Vec<Offering>, Option<u32>) {
        let count: u32 = Self::get_offering_count(env.clone(), issuer.clone());

        // Clamp limit to MAX_PAGE_LIMIT; treat 0 as "use default max".
        let effective_limit = if limit == 0 || limit > MAX_PAGE_LIMIT {
            MAX_PAGE_LIMIT
        } else {
            limit
        };

        // If start is beyond the total count, return empty.
        if start >= count {
            return (Vec::new(&env), None);
        }

        let end = core::cmp::min(start + effective_limit, count);
        let mut results = Vec::new(&env);

        for i in start..end {
            let item_key = DataKey::OfferItem(issuer.clone(), i);
            let offering: Offering = env.storage().persistent().get(&item_key).unwrap();
            results.push_back(offering);
        }

        let next_cursor = if end < count { Some(end) } else { None };

        (results, next_cursor)
    }
}

    // ── Blacklist management ──────────────────────────────────

    /// Add `investor` to the per-offering blacklist for `token`.
    ///
    /// Idempotent — calling with an already-blacklisted address is safe.
    pub fn blacklist_add(env: Env, caller: Address, token: Address, investor: Address) {
        caller.require_auth();

        // Reentrancy / reuse guard: prevent the same offering's
        // mutable entrypoint being executed re-entrantly or while
        // another execution is in-progress for the same `token`.
        let guard_key = DataKey::InProgress(token.clone());
        let already = env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&guard_key)
            .unwrap_or(false);
        if already {
            panic!("reentrancy detected for token");
        }
        env.storage().persistent().set(&guard_key, &true);

        let key = DataKey::Blacklist(token.clone());
        let mut map: Map<Address, bool> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Map::new(&env));

        map.set(investor.clone(), true);
        env.storage().persistent().set(&key, &map);

        // clear the guard after mutation completes
        env.storage().persistent().set(&guard_key, &false);

        env.events().publish((EVENT_BL_ADD, token, caller), investor);
    }

    /// Remove `investor` from the per-offering blacklist for `token`.
    ///
    /// Idempotent — calling when the address is not listed is safe.
    pub fn blacklist_remove(env: Env, caller: Address, token: Address, investor: Address) {
        caller.require_auth();

        // Reentrancy / reuse guard (symmetric to `blacklist_add`).
        let guard_key = DataKey::InProgress(token.clone());
        let already = env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&guard_key)
            .unwrap_or(false);
        if already {
            panic!("reentrancy detected for token");
        }
        env.storage().persistent().set(&guard_key, &true);

        let key = DataKey::Blacklist(token.clone());
        let mut map: Map<Address, bool> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Map::new(&env));

        map.remove(investor.clone());
        env.storage().persistent().set(&key, &map);

        // clear the guard after mutation completes
        env.storage().persistent().set(&guard_key, &false);

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
}

mod test;