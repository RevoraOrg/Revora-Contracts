#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Map, Symbol, Vec};

// Event symbols
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_BL_ADD: Symbol = symbol_short!("bl_add");
const EVENT_BL_REM: Symbol = symbol_short!("bl_rem");
const EVENT_INIT: Symbol = symbol_short!("init");
const EVENT_PAUSED: Symbol = symbol_short!("paused");
const EVENT_UNPAUSED: Symbol = symbol_short!("unpaused");

// Storage keys and types
#[contracttype]
pub enum DataKey {
    Blacklist(Address),
    Admin,
    Safety,
    Paused,
}

#[contract]
pub struct RevoraRevenueShare;

#[contractimpl]
impl RevoraRevenueShare {
    /// Initialize admin and optional safety role. Can be called only once.
    pub fn initialize(env: Env, admin: Address, safety: Option<Address>) {
        if env.storage().persistent().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().persistent().set(&DataKey::Admin, &admin.clone());
        if let Some(s) = safety {
            env.storage().persistent().set(&DataKey::Safety, &s.clone());
            env.events().publish((EVENT_INIT, admin.clone()), (true,));
        } else {
            env.events().publish((EVENT_INIT, admin.clone()), (false,));
        }
        env.storage().persistent().set(&DataKey::Paused, &false);
    }

    /// Pause/unpause controlled by admin
    pub fn pause_admin(env: Env, caller: Address) {
        caller.require_auth();
        let admin: Address = env.storage().persistent().get(&DataKey::Admin).expect("admin not set");
        if caller != admin {
            panic!("not admin");
        }
        env.storage().persistent().set(&DataKey::Paused, &true);
        env.events().publish((EVENT_PAUSED, caller.clone()), ());
    }

    pub fn unpause_admin(env: Env, caller: Address) {
        caller.require_auth();
        let admin: Address = env.storage().persistent().get(&DataKey::Admin).expect("admin not set");
        if caller != admin {
            panic!("not admin");
        }
        env.storage().persistent().set(&DataKey::Paused, &false);
        env.events().publish((EVENT_UNPAUSED, caller.clone()), ());
    }

    /// Pause/unpause controlled by safety role
    pub fn pause_safety(env: Env, caller: Address) {
        caller.require_auth();
        let safety: Address = env.storage().persistent().get(&DataKey::Safety).expect("safety not set");
        if caller != safety {
            panic!("not safety");
        }
        env.storage().persistent().set(&DataKey::Paused, &true);
        env.events().publish((EVENT_PAUSED, caller.clone()), ());
    }

    pub fn unpause_safety(env: Env, caller: Address) {
        caller.require_auth();
        let safety: Address = env.storage().persistent().get(&DataKey::Safety).expect("safety not set");
        if caller != safety {
            panic!("not safety");
        }
        env.storage().persistent().set(&DataKey::Paused, &false);
        env.events().publish((EVENT_UNPAUSED, caller.clone()), ());
    }

    /// Query paused state
    pub fn is_paused(env: Env) -> bool {
        env.storage().persistent().get::<DataKey, bool>(&DataKey::Paused).unwrap_or(false)
    }

    // ── Entry points guarded by pause ─────────────────────────
    pub fn register_offering(env: Env, issuer: Address, token: Address, revenue_share_bps: u32) {
        require_not_paused(&env);
        issuer.require_auth();
        env.events().publish((symbol_short!("offer_reg"), issuer.clone()), (token, revenue_share_bps));
    }

    pub fn report_revenue(env: Env, issuer: Address, token: Address, amount: i128, period_id: u64) {
        require_not_paused(&env);
        issuer.require_auth();
        let blacklist = Self::get_blacklist(env.clone(), token.clone());
        env.events().publish((EVENT_REVENUE_REPORTED, issuer.clone(), token.clone()), (amount, period_id, blacklist));
    }

    pub fn blacklist_add(env: Env, caller: Address, token: Address, investor: Address) {
        require_not_paused(&env);
        caller.require_auth();
        let key = DataKey::Blacklist(token.clone());
        let mut map: Map<Address, bool> = env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));
        map.set(investor.clone(), true);
        env.storage().persistent().set(&key, &map);
        env.events().publish((EVENT_BL_ADD, token, caller), investor);
    }

    pub fn blacklist_remove(env: Env, caller: Address, token: Address, investor: Address) {
        require_not_paused(&env);
        caller.require_auth();
        let key = DataKey::Blacklist(token.clone());
        let mut map: Map<Address, bool> = env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));
        map.remove(investor.clone());
        env.storage().persistent().set(&key, &map);
        env.events().publish((EVENT_BL_REM, token, caller), investor);
    }

    pub fn is_blacklisted(env: Env, token: Address, investor: Address) -> bool {
        let key = DataKey::Blacklist(token);
        env.storage().persistent().get::<DataKey, Map<Address, bool>>(&key).map(|m| m.get(investor).unwrap_or(false)).unwrap_or(false)
    }

    pub fn get_blacklist(env: Env, token: Address) -> Vec<Address> {
        let key = DataKey::Blacklist(token);
        env.storage().persistent().get::<DataKey, Map<Address, bool>>(&key).map(|m| m.keys()).unwrap_or_else(|| Vec::new(&env))
    }
}

fn require_not_paused(env: &Env) {
    let paused: bool = env.storage().persistent().get::<DataKey, bool>(&DataKey::Paused).unwrap_or(false);
    if paused {
        panic!("contract is paused");
    }
}

mod test;
mod test;
>>>>>>> ef896b0 (feat(contracts): add per-offering investor blacklist (#13))
