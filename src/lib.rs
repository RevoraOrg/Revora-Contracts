#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    Address, Env, Map, Symbol, Vec,
};

// ── Event symbols ────────────────────────────────────────────
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_BL_ADD: Symbol          = symbol_short!("bl_add");
const EVENT_BL_REM: Symbol          = symbol_short!("bl_rem");

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OfferingStatus {
    Active,
    Suspended,
    Closed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Offering {
    pub issuer: Address,
    pub token: Address,
    pub revenue_share_bps: u32,
    pub status: OfferingStatus,
}

// ── Storage key ──────────────────────────────────────────────
#[contracttype]
pub enum DataKey {
    Blacklist(Address),
    Offering(Address, Address), // (Issuer, Token)
    IssuerOfferings(Address),   // Issuer -> Vec<Token>
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

        if revenue_share_bps > 10_000 {
            panic!("Invalid BPS: exceeds 10000");
        }

        let offering_key = DataKey::Offering(issuer.clone(), token.clone());
        if env.storage().persistent().has(&offering_key) {
            panic!("Offering already exists");
        }

        let offering = Offering {
            issuer: issuer.clone(),
            token: token.clone(),
            revenue_share_bps,
            status: OfferingStatus::Active,
        };

        env.storage().persistent().set(&offering_key, &offering);

        let issuer_offerings_key = DataKey::IssuerOfferings(issuer.clone());
        let mut tokens: Vec<Address> = env
            .storage()
            .persistent()
            .get(&issuer_offerings_key)
            .unwrap_or_else(|| Vec::new(&env));
        
        tokens.push_back(token.clone());
        env.storage().persistent().set(&issuer_offerings_key, &tokens);

        env.events().publish(
            (symbol_short!("offer_reg"), issuer),
            (token, revenue_share_bps),
        );
    }

    /// Fetch a single offering by issuer and token.
    pub fn get_offering(env: Env, issuer: Address, token: Address) -> Option<Offering> {
        let key = DataKey::Offering(issuer, token);
        env.storage().persistent().get(&key)
    }

    /// List all offering tokens for an issuer.
    pub fn list_offerings(env: Env, issuer: Address) -> Vec<Address> {
        let key = DataKey::IssuerOfferings(issuer);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env))
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
}

mod test;