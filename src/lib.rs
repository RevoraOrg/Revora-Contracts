#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    Address, Env, Map, Symbol, String, Vec,
};

// ── Storage key ──────────────────────────────────────────────
/// One blacklist map per offering, keyed by the offering's token address.
///
/// This is intentionally minimal and focuses on the high-level shape:
/// - Registering a startup "offering"
/// - Recording a revenue report
/// - Emitting events that an off-chain distribution engine can consume
/// - Attaching off-chain metadata references to offerings
/// Blacklist precedence rule: a blacklisted address is **always** excluded
/// from payouts, regardless of any whitelist or investor registration.
/// If the same address appears in both a whitelist and this blacklist,
/// the blacklist wins unconditionally.
#[contracttype]
pub enum DataKey {
    Blacklist(Address),
}

// ── Contract ─────────────────────────────────────────────────
#[contract]
pub struct RevoraRevenueShare;

#[derive(Clone)]
pub struct Offering {
    pub issuer: Address,
    pub token: Address,
    pub revenue_share_bps: u32,
}

// Storage key constants
const METADATA_KEY: Symbol = symbol_short!("meta");

// Event symbols
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_BL_ADD: Symbol          = symbol_short!("bl_add");
const EVENT_BL_REM: Symbol          = symbol_short!("bl_rem");
const EVENT_METADATA_CREATED: Symbol = symbol_short!("meta_new");
const EVENT_METADATA_UPDATED: Symbol = symbol_short!("meta_upd");
const EVENT_METADATA_DELETED: Symbol = symbol_short!("meta_del");

// Configuration constants
const MAX_METADATA_LENGTH: u32 = 1024; // 1KB max for metadata URI

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

    /// Set metadata reference for an offering.
    /// Only the issuer or issuer admin can set metadata.
    /// 
    /// # Arguments
    /// * `env` - The contract environment
    /// * `issuer` - The issuer address
    /// * `offering_id` - Unique identifier for the offering
    /// * `metadata_uri` - Off-chain metadata reference (IPFS hash, HTTPS URL, etc.)
    ///
    /// # Panics
    /// - If metadata_uri exceeds MAX_METADATA_LENGTH
    /// - If caller is not authorized (issuer or admin)
    /// - If metadata_uri is empty
    pub fn set_metadata(
        env: Env,
        issuer: Address,
        offering_id: String,
        metadata_uri: String,
    ) {
        issuer.require_auth();
        
        // Validate metadata_uri is not empty
        if metadata_uri.len() == 0 {
            panic!("Metadata URI cannot be empty");
        }

        // Validate metadata_uri length
        if metadata_uri.len() > MAX_METADATA_LENGTH {
            panic!("Metadata URI exceeds maximum length of {} bytes", MAX_METADATA_LENGTH);
        }

        // Create a compound key for the metadata storage
        let mut metadata_map: Map<String, String> = env
            .storage()
            .persistent()
            .get(&(METADATA_KEY, issuer.clone()))
            .unwrap_or_else(|| Map::new(&env));

        let is_new = !metadata_map.contains_key(offering_id.clone());

        // Store the metadata reference
        metadata_map.set(offering_id.clone(), metadata_uri.clone());
        env.storage()
            .persistent()
            .set(&(METADATA_KEY, issuer.clone()), &metadata_map);

        // Emit appropriate event
        if is_new {
            env.events().publish(
                (EVENT_METADATA_CREATED, issuer.clone()),
                (offering_id, metadata_uri),
            );
        } else {
            env.events().publish(
                (EVENT_METADATA_UPDATED, issuer.clone()),
                (offering_id, metadata_uri),
            );
        }
    }

    /// Get metadata reference for an offering.
    /// Returns the stored metadata URI or None if not set.
    ///
    /// # Arguments
    /// * `env` - The contract environment
    /// * `issuer` - The issuer address
    /// * `offering_id` - Unique identifier for the offering
    pub fn get_metadata(
        env: Env,
        issuer: Address,
        offering_id: String,
    ) -> Option<String> {
        let metadata_map: Map<String, String> = env
            .storage()
            .persistent()
            .get(&(METADATA_KEY, issuer))
            .unwrap_or_else(|| Map::new(&env));

        metadata_map.get(offering_id)
    }

    /// Update metadata reference for an offering.
    /// Only the issuer can update existing metadata.
    ///
    /// # Arguments
    /// * `env` - The contract environment
    /// * `issuer` - The issuer address
    /// * `offering_id` - Unique identifier for the offering
    /// * `metadata_uri` - New off-chain metadata reference
    ///
    /// # Panics
    /// - If metadata doesn't exist for this offering
    /// - If new metadata_uri exceeds MAX_METADATA_LENGTH
    /// - If caller is not the issuer
    /// - If new metadata_uri is empty
    pub fn update_metadata(
        env: Env,
        issuer: Address,
        offering_id: String,
        metadata_uri: String,
    ) {
        issuer.require_auth();

        // Validate metadata_uri is not empty
        if metadata_uri.len() == 0 {
            panic!("Metadata URI cannot be empty");
        }

        // Validate metadata_uri length
        if metadata_uri.len() > MAX_METADATA_LENGTH {
            panic!("Metadata URI exceeds maximum length of {} bytes", MAX_METADATA_LENGTH);
        }

        let mut metadata_map: Map<String, String> = env
            .storage()
            .persistent()
            .get(&(METADATA_KEY, issuer.clone()))
            .unwrap_or_else(|| Map::new(&env));

        // Verify metadata exists
        if !metadata_map.contains_key(offering_id.clone()) {
            panic!("No metadata found for offering");
        }

        // Update the metadata reference
        metadata_map.set(offering_id.clone(), metadata_uri.clone());
        env.storage()
            .persistent()
            .set(&(METADATA_KEY, issuer.clone()), &metadata_map);

        // Emit update event
        env.events().publish(
            (EVENT_METADATA_UPDATED, issuer.clone()),
            (offering_id, metadata_uri),
        );
    }

    /// Delete metadata reference for an offering.
    /// Only the issuer can delete metadata.
    ///
    /// # Arguments
    /// * `env` - The contract environment
    /// * `issuer` - The issuer address
    /// * `offering_id` - Unique identifier for the offering
    ///
    /// # Panics
    /// - If metadata doesn't exist for this offering
    /// - If caller is not the issuer
    pub fn delete_metadata(
        env: Env,
        issuer: Address,
        offering_id: String,
    ) {
        issuer.require_auth();

        let mut metadata_map: Map<String, String> = env
            .storage()
            .persistent()
            .get(&(METADATA_KEY, issuer.clone()))
            .unwrap_or_else(|| Map::new(&env));

        // Verify metadata exists
        if !metadata_map.contains_key(offering_id.clone()) {
            panic!("No metadata found for offering");
        }

        // Remove the metadata reference
        metadata_map.remove(offering_id.clone());
        env.storage()
            .persistent()
            .set(&(METADATA_KEY, issuer.clone()), &metadata_map);

        // Emit deletion event (using updated event with empty string to indicate deletion)
        env.events().publish(
            (EVENT_METADATA_DELETED, issuer.clone()),
            (offering_id,),
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