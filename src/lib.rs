#![no_std]
use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Symbol};

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
const KEY_ADMIN: Symbol = symbol_short!("admin");
const KEY_SAFETY: Symbol = symbol_short!("safety");
const KEY_PAUSED: Symbol = symbol_short!("paused");

const EVENT_PAUSED: Symbol = symbol_short!("pa_on");
const EVENT_UNPAUSED: Symbol = symbol_short!("pa_off");

#[contractimpl]
impl RevoraRevenueShare {
    /// Register a new revenue-share offering.
    /// In a production contract this would handle access control, supply caps,
    /// and issuance hooks. Here we only emit an event.
    pub fn register_offering(env: Env, issuer: Address, token: Address, revenue_share_bps: u32) {
        // block when paused
        if is_paused(&env) {
            panic!("contract is paused")
        }

        issuer.require_auth();

        env.events().publish(
            (symbol_short!("offer_reg"), issuer.clone()),
            (token, revenue_share_bps),
        );
    }

    /// Record a revenue report for an offering.
    /// The actual payout calculation and distribution can be performed either
    /// fully on-chain or in a hybrid model where this event is the trigger.
    pub fn report_revenue(env: Env, issuer: Address, token: Address, amount: i128, period_id: u64) {
        // block when paused
        if is_paused(&env) {
            panic!("contract is paused")
        }

        issuer.require_auth();

        env.events().publish(
            (EVENT_REVENUE_REPORTED, issuer.clone(), token.clone()),
            (amount, period_id),
        );
    }
}

#[contractimpl]
impl RevoraRevenueShare {
    /// Initialize admin and optional safety role. Can be called only once.
    pub fn initialize(env: Env, admin: Address, safety: Option<Address>) {
        // only allow if not yet initialized
        if env.storage().persistent().has(&KEY_ADMIN) {
            panic!("already initialized")
        }

        // set admin
        env.storage().persistent().set(&KEY_ADMIN, &admin.clone());

        // set safety if provided
        if let Some(s) = safety {
            env.storage().persistent().set(&KEY_SAFETY, &s.clone());
            env.events()
                .publish((symbol_short!("init"), admin.clone()), (true,));
        } else {
            env.events()
                .publish((symbol_short!("init"), admin.clone()), (false,));
        }
    }

    /// Activate emergency pause as admin. Caller must provide the admin address and sign.
    pub fn pause_admin(env: Env, admin: Address) {
        // verify admin matches stored admin and is authorized
        let stored: Address = env
            .storage()
            .persistent()
            .get(&KEY_ADMIN)
            .expect("no admin");
        if stored != admin {
            panic!("admin mismatch");
        }
        admin.require_auth();
        env.storage().persistent().set(&KEY_PAUSED, &true);
        env.events().publish((EVENT_PAUSED,), ());
    }

    /// Deactivate emergency pause as admin.
    pub fn unpause_admin(env: Env, admin: Address) {
        let stored: Address = env
            .storage()
            .persistent()
            .get(&KEY_ADMIN)
            .expect("no admin");
        if stored != admin {
            panic!("admin mismatch");
        }
        admin.require_auth();
        env.storage().persistent().set(&KEY_PAUSED, &false);
        env.events().publish((EVENT_UNPAUSED,), ());
    }

    /// Activate emergency pause as safety role. Caller must provide the safety address and sign.
    pub fn pause_safety(env: Env, safety: Address) {
        let stored: Address = env
            .storage()
            .persistent()
            .get(&KEY_SAFETY)
            .expect("no safety role");
        if stored != safety {
            panic!("safety mismatch");
        }
        safety.require_auth();
        env.storage().persistent().set(&KEY_PAUSED, &true);
        env.events().publish((EVENT_PAUSED,), ());
    }

    /// Deactivate emergency pause as safety role.
    pub fn unpause_safety(env: Env, safety: Address) {
        let stored: Address = env
            .storage()
            .persistent()
            .get(&KEY_SAFETY)
            .expect("no safety role");
        if stored != safety {
            panic!("safety mismatch");
        }
        safety.require_auth();
        env.storage().persistent().set(&KEY_PAUSED, &false);
        env.events().publish((EVENT_UNPAUSED,), ());
    }

    /// Query whether the contract is paused.
    pub fn is_paused(env: Env) -> bool {
        is_paused(&env)
    }
}

fn is_paused(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get::<Symbol, bool>(&KEY_PAUSED)
        .unwrap_or_default()
}

mod test;
