#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    Address, Env, Map, Symbol, Vec,
};

// ── Event symbols ────────────────────────────────────────────
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_BL_ADD: Symbol          = symbol_short!("bl_add");
const EVENT_BL_REM: Symbol          = symbol_short!("bl_rem");
const EVENT_INIT: Symbol            = symbol_short!("init");
const EVENT_FEE_SET: Symbol         = symbol_short!("fee_set");
const EVENT_OWN_XFER: Symbol        = symbol_short!("own_xfer");

const MAX_FEE_BPS: u32 = 5_000;

// ── Storage keys ─────────────────────────────────────────────
/// One blacklist map per offering, keyed by the offering's token address.
///
/// Blacklist precedence rule: a blacklisted address is **always** excluded
/// from payouts, regardless of any whitelist or investor registration.
/// If the same address appears in both a whitelist and this blacklist,
/// the blacklist wins unconditionally.
#[contracttype]
pub enum DataKey {
    Blacklist(Address),
    PlatformOwner,
    PlatformFeeBps,
}

// ── Contract ─────────────────────────────────────────────────
#[contract]
pub struct RevoraRevenueShare;

#[contractimpl]
impl RevoraRevenueShare {
    // ── Platform administration ────────────────────────────────

    /// Initialize the contract with a platform owner address.
    /// Can only be called once.
    pub fn initialize(env: Env, owner: Address) {
        if env.storage().persistent().has(&DataKey::PlatformOwner) {
            panic!("already initialized");
        }
        owner.require_auth();
        env.storage().persistent().set(&DataKey::PlatformOwner, &owner);
        env.storage().persistent().set(&DataKey::PlatformFeeBps, &0u32);
        env.events().publish((EVENT_INIT,), owner);
    }

    /// Set the platform fee in basis points (max 5000 = 50%).
    /// Only the platform owner may call this.
    pub fn set_platform_fee(env: Env, fee_bps: u32) {
        let owner = Self::get_platform_owner(env.clone());
        owner.require_auth();
        if fee_bps > MAX_FEE_BPS {
            panic!("fee exceeds maximum of 5000 bps");
        }
        env.storage().persistent().set(&DataKey::PlatformFeeBps, &fee_bps);
        env.events().publish((EVENT_FEE_SET,), fee_bps);
    }

    /// Return the current platform fee in basis points.
    pub fn get_platform_fee(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::PlatformFeeBps)
            .unwrap_or(0)
    }

    /// Return the platform owner address.
    pub fn get_platform_owner(env: Env) -> Address {
        env.storage()
            .persistent()
            .get(&DataKey::PlatformOwner)
            .expect("not initialized")
    }

    /// Transfer platform ownership to a new address.
    /// Requires authorization from both current and new owner.
    pub fn transfer_ownership(env: Env, new_owner: Address) {
        let current = Self::get_platform_owner(env.clone());
        current.require_auth();
        new_owner.require_auth();
        env.storage().persistent().set(&DataKey::PlatformOwner, &new_owner);
        env.events().publish((EVENT_OWN_XFER,), new_owner);
    }

    /// Calculate the platform fee amount for a given revenue amount.
    pub fn calculate_platform_fee(env: Env, amount: i128) -> i128 {
        let fee_bps = Self::get_platform_fee(env) as i128;
        (amount * fee_bps) / 10_000
    }

    // ── Offering entry-points ─────────────────────────────────

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
    /// The event payload includes the current blacklist and the platform fee
    /// breakdown so off-chain distribution engines can apply both filters
    /// in the same atomic step.
    pub fn report_revenue(
        env: Env,
        issuer: Address,
        token: Address,
        amount: i128,
        period_id: u64,
    ) {
        issuer.require_auth();

        let blacklist = Self::get_blacklist(env.clone(), token.clone());
        let platform_fee = Self::calculate_platform_fee(env.clone(), amount);
        let net_amount = amount - platform_fee;

        env.events().publish(
            (EVENT_REVENUE_REPORTED, issuer.clone(), token.clone()),
            (amount, period_id, blacklist, platform_fee, net_amount),
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