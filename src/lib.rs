#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Map, Symbol,
    Vec,
};

/// Centralized contract error codes. Auth failures are signaled by host panic (require_auth).
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u32)]
pub enum RevoraError {
    /// revenue_share_bps exceeded 10000 (100%).
    InvalidRevenueShareBps = 1,
    /// Reserved for future use (e.g. offering limit per issuer).
    LimitReached = 2,
    /// Holder concentration exceeds configured limit and enforcement is enabled.
    ConcentrationLimitExceeded = 3,
}

// ── Event symbols ────────────────────────────────────────────
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_BL_ADD: Symbol = symbol_short!("bl_add");
const EVENT_BL_REM: Symbol = symbol_short!("bl_rem");
const EVENT_CONCENTRATION_WARNING: Symbol = symbol_short!("conc_warn");

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Offering {
    pub issuer: Address,
    pub token: Address,
    pub revenue_share_bps: u32,
}

/// Per-offering concentration guardrail config (#26).
/// max_bps: max allowed single-holder share in basis points (0 = disabled).
/// enforce: if true, report_revenue fails when current concentration > max_bps.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ConcentrationLimitConfig {
    pub max_bps: u32,
    pub enforce: bool,
}

/// Per-offering audit log summary (#34).
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AuditSummary {
    pub total_revenue: i128,
    pub report_count: u64,
}

/// Rounding mode for distribution share calculations (#44).
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundingMode {
    /// Truncate toward zero: share = (amount * bps) / 10000
    Truncation = 0,
    /// Round half up: share = (amount * bps * 2 + 10000) / 20000
    RoundHalfUp = 1,
}

/// Storage keys: offerings use OfferCount/OfferItem; blacklist uses Blacklist(token).
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Blacklist(Address),
    OfferCount(Address),
    OfferItem(Address, u32),
    /// Per (issuer, token): concentration limit config.
    ConcentrationLimit(Address, Address),
    /// Per (issuer, token): last reported concentration in bps.
    CurrentConcentration(Address, Address),
    /// Per (issuer, token): audit summary.
    AuditSummary(Address, Address),
    /// Per (issuer, token): rounding mode for share math.
    RoundingMode(Address, Address),
}

/// Maximum number of offerings returned in a single page.
const MAX_PAGE_LIMIT: u32 = 20;

#[contract]
pub struct RevoraRevenueShare;

#[contractimpl]
impl RevoraRevenueShare {
    /// Register a new revenue-share offering.
    /// Returns `Err(RevoraError::InvalidRevenueShareBps)` if revenue_share_bps > 10000.
    pub fn register_offering(
        env: Env,
        issuer: Address,
        token: Address,
        revenue_share_bps: u32,
    ) -> Result<(), RevoraError> {
        issuer.require_auth();

        if revenue_share_bps > 10_000 {
            return Err(RevoraError::InvalidRevenueShareBps);
        }

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
            (symbol_short!("offer_reg"), issuer),
            (token, revenue_share_bps),
        );
        Ok(())
    }

    /// Fetch a single offering by issuer and token (scans issuer's offerings).
    pub fn get_offering(env: Env, issuer: Address, token: Address) -> Option<Offering> {
        let count = Self::get_offering_count(env.clone(), issuer.clone());
        for i in 0..count {
            let item_key = DataKey::OfferItem(issuer.clone(), i);
            let offering: Offering = env.storage().persistent().get(&item_key).unwrap();
            if offering.token == token {
                return Some(offering);
            }
        }
        None
    }

    /// List all offering tokens for an issuer.
    pub fn list_offerings(env: Env, issuer: Address) -> Vec<Address> {
        let (page, _) = Self::get_offerings_page(env.clone(), issuer.clone(), 0, MAX_PAGE_LIMIT);
        let mut tokens = Vec::new(&env);
        for i in 0..page.len() {
            tokens.push_back(page.get(i).unwrap().token);
        }
        tokens
    }

    /// Record a revenue report for an offering. Updates audit summary (#34).
    /// Fails with `ConcentrationLimitExceeded` (#26) if concentration enforcement is on and current concentration exceeds limit.
    pub fn report_revenue(
        env: Env,
        issuer: Address,
        token: Address,
        amount: i128,
        period_id: u64,
    ) -> Result<(), RevoraError> {
        issuer.require_auth();

        // Holder concentration guardrail (#26): reject if enforce and over limit
        let limit_key = DataKey::ConcentrationLimit(issuer.clone(), token.clone());
        if let Some(config) = env
            .storage()
            .persistent()
            .get::<DataKey, ConcentrationLimitConfig>(&limit_key)
        {
            if config.enforce && config.max_bps > 0 {
                let curr_key = DataKey::CurrentConcentration(issuer.clone(), token.clone());
                let current: u32 = env.storage().persistent().get(&curr_key).unwrap_or(0);
                if current > config.max_bps {
                    return Err(RevoraError::ConcentrationLimitExceeded);
                }
            }
        }

        let blacklist = Self::get_blacklist(env.clone(), token.clone());

        env.events().publish(
            (EVENT_REVENUE_REPORTED, issuer.clone(), token.clone()),
            (amount, period_id, blacklist),
        );

        // Audit log summary (#34): maintain per-offering total revenue and report count
        let summary_key = DataKey::AuditSummary(issuer.clone(), token.clone());
        let mut summary: AuditSummary =
            env.storage()
                .persistent()
                .get(&summary_key)
                .unwrap_or(AuditSummary {
                    total_revenue: 0,
                    report_count: 0,
                });
        summary.total_revenue = summary.total_revenue.saturating_add(amount);
        summary.report_count = summary.report_count.saturating_add(1);
        env.storage().persistent().set(&summary_key, &summary);

        Ok(())
    }

    /// Return the total number of offerings registered by `issuer`.
    pub fn get_offering_count(env: Env, issuer: Address) -> u32 {
        let count_key = DataKey::OfferCount(issuer);
        env.storage().persistent().get(&count_key).unwrap_or(0)
    }

    /// Return a page of offerings for `issuer`. Limit capped at MAX_PAGE_LIMIT (20).
    pub fn get_offerings_page(
        env: Env,
        issuer: Address,
        start: u32,
        limit: u32,
    ) -> (Vec<Offering>, Option<u32>) {
        let count = Self::get_offering_count(env.clone(), issuer.clone());

        let effective_limit = if limit == 0 || limit > MAX_PAGE_LIMIT {
            MAX_PAGE_LIMIT
        } else {
            limit
        };

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

    /// Add `investor` to the per-offering blacklist for `token`. Idempotent.
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

    /// Remove `investor` from the per-offering blacklist for `token`. Idempotent.
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

    // ── Holder concentration guardrail (#26) ───────────────────

    /// Set per-offering concentration limit. Caller must be the offering issuer.
    /// `max_bps`: max allowed single-holder share in basis points (0 = disable).
    /// `enforce`: if true, report_revenue will fail when reported concentration exceeds max_bps.
    pub fn set_concentration_limit(
        env: Env,
        issuer: Address,
        token: Address,
        max_bps: u32,
        enforce: bool,
    ) -> Result<(), RevoraError> {
        issuer.require_auth();
        if Self::get_offering(env.clone(), issuer.clone(), token.clone()).is_none() {
            return Err(RevoraError::LimitReached); // reuse: "offering not found" semantics
        }
        let key = DataKey::ConcentrationLimit(issuer, token);
        env.storage()
            .persistent()
            .set(&key, &ConcentrationLimitConfig { max_bps, enforce });
        Ok(())
    }

    /// Report current top-holder concentration in bps. Emits warning event if over configured limit.
    pub fn report_concentration(
        env: Env,
        issuer: Address,
        token: Address,
        concentration_bps: u32,
    ) -> Result<(), RevoraError> {
        issuer.require_auth();
        let curr_key = DataKey::CurrentConcentration(issuer.clone(), token.clone());
        env.storage()
            .persistent()
            .set(&curr_key, &concentration_bps);

        let limit_key = DataKey::ConcentrationLimit(issuer.clone(), token.clone());
        if let Some(config) = env
            .storage()
            .persistent()
            .get::<DataKey, ConcentrationLimitConfig>(&limit_key)
        {
            if config.max_bps > 0 && concentration_bps > config.max_bps {
                env.events().publish(
                    (EVENT_CONCENTRATION_WARNING, issuer, token),
                    (concentration_bps, config.max_bps),
                );
            }
        }
        Ok(())
    }

    /// Get concentration limit config for an offering.
    pub fn get_concentration_limit(
        env: Env,
        issuer: Address,
        token: Address,
    ) -> Option<ConcentrationLimitConfig> {
        let key = DataKey::ConcentrationLimit(issuer, token);
        env.storage().persistent().get(&key)
    }

    /// Get last reported concentration in bps for an offering.
    pub fn get_current_concentration(env: Env, issuer: Address, token: Address) -> Option<u32> {
        let key = DataKey::CurrentConcentration(issuer, token);
        env.storage().persistent().get(&key)
    }

    // ── Audit log summary (#34) ────────────────────────────────

    /// Get per-offering audit summary (total revenue and report count).
    pub fn get_audit_summary(env: Env, issuer: Address, token: Address) -> Option<AuditSummary> {
        let key = DataKey::AuditSummary(issuer, token);
        env.storage().persistent().get(&key)
    }

    // ── Configurable rounding (#44) ───────────────────────────

    /// Set rounding mode for an offering's share calculations. Caller must be issuer.
    pub fn set_rounding_mode(
        env: Env,
        issuer: Address,
        token: Address,
        mode: RoundingMode,
    ) -> Result<(), RevoraError> {
        issuer.require_auth();
        if Self::get_offering(env.clone(), issuer.clone(), token.clone()).is_none() {
            return Err(RevoraError::LimitReached);
        }
        let key = DataKey::RoundingMode(issuer, token);
        env.storage().persistent().set(&key, &mode);
        Ok(())
    }

    /// Get rounding mode for an offering. Defaults to Truncation if not set.
    pub fn get_rounding_mode(env: Env, issuer: Address, token: Address) -> RoundingMode {
        let key = DataKey::RoundingMode(issuer, token);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or(RoundingMode::Truncation)
    }

    /// Compute share of `amount` at `revenue_share_bps` using the given rounding mode.
    /// Guarantees: result between 0 and amount (inclusive); no loss of funds when summing shares if caller uses same mode.
    pub fn compute_share(
        _env: Env,
        amount: i128,
        revenue_share_bps: u32,
        mode: RoundingMode,
    ) -> i128 {
        if revenue_share_bps > 10_000 {
            return 0;
        }
        let bps = revenue_share_bps as i128;
        let raw = amount.checked_mul(bps).unwrap_or(0);
        let share = match mode {
            RoundingMode::Truncation => raw.checked_div(10_000).unwrap_or(0),
            RoundingMode::RoundHalfUp => {
                let half = 5_000_i128;
                let adjusted = if raw >= 0 {
                    raw.saturating_add(half)
                } else {
                    raw.saturating_sub(half)
                };
                adjusted.checked_div(10_000).unwrap_or(0)
            }
        };
        // Clamp to [min(0, amount), max(0, amount)] to avoid overflow semantics affecting bounds
        let lo = core::cmp::min(0, amount);
        let hi = core::cmp::max(0, amount);
        core::cmp::min(core::cmp::max(share, lo), hi)
    }
}

mod test;
