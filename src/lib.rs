#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Map,
    Symbol, Vec,
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
    /// No offering found for the given (issuer, token) pair.
    OfferingNotFound = 3,
    /// Revenue already deposited for this period.
    PeriodAlreadyDeposited = 4,
    /// No unclaimed periods for this holder.
    NoPendingClaims = 5,
    /// Holder is blacklisted for this offering.
    HolderBlacklisted = 6,
    /// Holder share_bps exceeded 10000 (100%).
    InvalidShareBps = 7,
    /// Payment token does not match previously set token for this offering.
    PaymentTokenMismatch = 8,
}

// ── Event symbols ────────────────────────────────────────────
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_BL_ADD: Symbol = symbol_short!("bl_add");
const EVENT_BL_REM: Symbol = symbol_short!("bl_rem");
const EVENT_REV_DEPOSIT: Symbol = symbol_short!("rev_dep");
const EVENT_CLAIM: Symbol = symbol_short!("claim");
const EVENT_SHARE_SET: Symbol = symbol_short!("share_set");

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Offering {
    pub issuer: Address,
    pub token: Address,
    pub revenue_share_bps: u32,
}

/// Storage keys: offerings use OfferCount/OfferItem; blacklist uses Blacklist(token).
/// Multi-period claim keys use PeriodRevenue/PeriodEntry/PeriodCount for per-offering
/// period tracking, HolderShare for holder allocations, LastClaimedIdx for claim progress,
/// and PaymentToken for the token used to pay out revenue.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Blacklist(Address),
    OfferCount(Address),
    OfferItem(Address, u32),
    /// Revenue amount deposited for (offering_token, period_id).
    PeriodRevenue(Address, u64),
    /// Maps (offering_token, sequential_index) -> period_id for enumeration.
    PeriodEntry(Address, u32),
    /// Total number of deposited periods for an offering token.
    PeriodCount(Address),
    /// Holder's share in basis points for (offering_token, holder).
    HolderShare(Address, Address),
    /// Next period index to claim for (offering_token, holder).
    LastClaimedIdx(Address, Address),
    /// Payment token address for an offering token.
    PaymentToken(Address),
}

/// Maximum number of offerings returned in a single page.
const MAX_PAGE_LIMIT: u32 = 20;

/// Maximum number of periods that can be claimed in a single transaction.
/// Keeps compute costs predictable within Soroban limits.
const MAX_CLAIM_PERIODS: u32 = 50;

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

    /// Record a revenue report for an offering.
    pub fn report_revenue(env: Env, issuer: Address, token: Address, amount: i128, period_id: u64) {
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

    // ── Multi-period aggregated claims ───────────────────────────

    /// Deposit revenue for a specific period of an offering.
    ///
    /// Transfers `amount` of `payment_token` from `issuer` to the contract.
    /// The payment token is locked per offering on first deposit; subsequent
    /// deposits must use the same payment token.
    pub fn deposit_revenue(
        env: Env,
        issuer: Address,
        token: Address,
        payment_token: Address,
        amount: i128,
        period_id: u64,
    ) -> Result<(), RevoraError> {
        issuer.require_auth();

        // Verify offering exists
        if Self::get_offering(env.clone(), issuer.clone(), token.clone()).is_none() {
            return Err(RevoraError::OfferingNotFound);
        }

        // Check period not already deposited
        let rev_key = DataKey::PeriodRevenue(token.clone(), period_id);
        if env.storage().persistent().has(&rev_key) {
            return Err(RevoraError::PeriodAlreadyDeposited);
        }

        // Store or validate payment token for this offering
        let pt_key = DataKey::PaymentToken(token.clone());
        if let Some(existing_pt) =
            env.storage().persistent().get::<DataKey, Address>(&pt_key)
        {
            if existing_pt != payment_token {
                return Err(RevoraError::PaymentTokenMismatch);
            }
        } else {
            env.storage().persistent().set(&pt_key, &payment_token);
        }

        // Transfer tokens from issuer to contract
        let contract_addr = env.current_contract_address();
        token::Client::new(&env, &payment_token).transfer(
            &issuer,
            &contract_addr,
            &amount,
        );

        // Store period revenue
        env.storage().persistent().set(&rev_key, &amount);

        // Append to indexed period list
        let count_key = DataKey::PeriodCount(token.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        let entry_key = DataKey::PeriodEntry(token.clone(), count);
        env.storage().persistent().set(&entry_key, &period_id);
        env.storage().persistent().set(&count_key, &(count + 1));

        env.events().publish(
            (EVENT_REV_DEPOSIT, issuer, token),
            (payment_token, amount, period_id),
        );
        Ok(())
    }

    /// Set a holder's revenue share (in basis points) for an offering.
    ///
    /// Only the offering issuer may call this. `share_bps` must be <= 10000.
    pub fn set_holder_share(
        env: Env,
        issuer: Address,
        token: Address,
        holder: Address,
        share_bps: u32,
    ) -> Result<(), RevoraError> {
        issuer.require_auth();

        if Self::get_offering(env.clone(), issuer.clone(), token.clone()).is_none() {
            return Err(RevoraError::OfferingNotFound);
        }

        if share_bps > 10_000 {
            return Err(RevoraError::InvalidShareBps);
        }

        let key = DataKey::HolderShare(token.clone(), holder.clone());
        env.storage().persistent().set(&key, &share_bps);

        env.events().publish(
            (EVENT_SHARE_SET, issuer, token),
            (holder, share_bps),
        );
        Ok(())
    }

    /// Return a holder's share in basis points for an offering (0 if unset).
    pub fn get_holder_share(env: Env, token: Address, holder: Address) -> u32 {
        let key = DataKey::HolderShare(token, holder);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    /// Claim aggregated revenue across multiple unclaimed periods.
    ///
    /// `max_periods` controls how many periods to process in one call
    /// (0 = up to MAX_CLAIM_PERIODS). Returns the total payout amount.
    ///
    /// Aggregation semantics:
    /// - Periods are processed in deposit order (sequential index).
    /// - Each holder's payout per period = `period_revenue * share_bps / 10000`.
    /// - The holder's claim index advances regardless of zero-value periods.
    /// - Capped at MAX_CLAIM_PERIODS (50) per transaction for gas safety.
    pub fn claim(
        env: Env,
        holder: Address,
        token: Address,
        max_periods: u32,
    ) -> Result<i128, RevoraError> {
        holder.require_auth();

        if Self::is_blacklisted(env.clone(), token.clone(), holder.clone()) {
            return Err(RevoraError::HolderBlacklisted);
        }

        let share_bps = Self::get_holder_share(env.clone(), token.clone(), holder.clone());
        if share_bps == 0 {
            return Err(RevoraError::NoPendingClaims);
        }

        let count_key = DataKey::PeriodCount(token.clone());
        let period_count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let idx_key = DataKey::LastClaimedIdx(token.clone(), holder.clone());
        let start_idx: u32 = env.storage().persistent().get(&idx_key).unwrap_or(0);

        if start_idx >= period_count {
            return Err(RevoraError::NoPendingClaims);
        }

        let effective_max = if max_periods == 0 || max_periods > MAX_CLAIM_PERIODS {
            MAX_CLAIM_PERIODS
        } else {
            max_periods
        };
        let end_idx = core::cmp::min(start_idx + effective_max, period_count);

        let mut total_payout: i128 = 0;
        let mut claimed_periods = Vec::new(&env);

        for i in start_idx..end_idx {
            let entry_key = DataKey::PeriodEntry(token.clone(), i);
            let period_id: u64 = env.storage().persistent().get(&entry_key).unwrap();
            let rev_key = DataKey::PeriodRevenue(token.clone(), period_id);
            let revenue: i128 = env.storage().persistent().get(&rev_key).unwrap();

            let payout = revenue * (share_bps as i128) / 10_000;
            total_payout += payout;
            claimed_periods.push_back(period_id);
        }

        // Transfer only if there is a positive payout
        if total_payout > 0 {
            let pt_key = DataKey::PaymentToken(token.clone());
            let payment_token: Address = env.storage().persistent().get(&pt_key).unwrap();
            let contract_addr = env.current_contract_address();
            token::Client::new(&env, &payment_token).transfer(
                &contract_addr,
                &holder,
                &total_payout,
            );
        }

        // Advance claim index regardless of payout amount
        env.storage().persistent().set(&idx_key, &end_idx);

        env.events().publish(
            (EVENT_CLAIM, holder.clone(), token),
            (total_payout, claimed_periods),
        );

        Ok(total_payout)
    }

    /// Return unclaimed period IDs for a holder on an offering.
    pub fn get_pending_periods(env: Env, token: Address, holder: Address) -> Vec<u64> {
        let count_key = DataKey::PeriodCount(token.clone());
        let period_count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let idx_key = DataKey::LastClaimedIdx(token.clone(), holder);
        let start_idx: u32 = env.storage().persistent().get(&idx_key).unwrap_or(0);

        let mut periods = Vec::new(&env);
        for i in start_idx..period_count {
            let entry_key = DataKey::PeriodEntry(token.clone(), i);
            let period_id: u64 = env.storage().persistent().get(&entry_key).unwrap();
            periods.push_back(period_id);
        }
        periods
    }

    /// Preview the total claimable amount for a holder without claiming.
    pub fn get_claimable(env: Env, token: Address, holder: Address) -> i128 {
        let share_bps = Self::get_holder_share(env.clone(), token.clone(), holder.clone());
        if share_bps == 0 {
            return 0;
        }

        let count_key = DataKey::PeriodCount(token.clone());
        let period_count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let idx_key = DataKey::LastClaimedIdx(token.clone(), holder);
        let start_idx: u32 = env.storage().persistent().get(&idx_key).unwrap_or(0);

        let mut total: i128 = 0;
        for i in start_idx..period_count {
            let entry_key = DataKey::PeriodEntry(token.clone(), i);
            let period_id: u64 = env.storage().persistent().get(&entry_key).unwrap();
            let rev_key = DataKey::PeriodRevenue(token.clone(), period_id);
            let revenue: i128 = env.storage().persistent().get(&rev_key).unwrap();
            total += revenue * (share_bps as i128) / 10_000;
        }
        total
    }

    /// Return the total number of deposited periods for an offering token.
    pub fn get_period_count(env: Env, token: Address) -> u32 {
        let count_key = DataKey::PeriodCount(token);
        env.storage().persistent().get(&count_key).unwrap_or(0)
    }
}

mod test;
