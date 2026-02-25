#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Map,
    String, Symbol, Vec,
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
    /// No offering found for the given (issuer, token) pair.
    OfferingNotFound = 4,
    /// Revenue already deposited for this period.
    PeriodAlreadyDeposited = 5,
    /// No unclaimed periods for this holder.
    NoPendingClaims = 6,
    /// Holder is blacklisted for this offering.
    HolderBlacklisted = 7,
    /// Holder share_bps exceeded 10000 (100%).
    InvalidShareBps = 8,
    /// Payment token does not match previously set token for this offering.
    PaymentTokenMismatch = 9,
    /// Contract is frozen; state-changing operations are disabled.
    ContractFrozen = 10,
    /// Revenue for this period is not yet claimable (delay not elapsed).
    ClaimDelayNotElapsed = 11,
    /// A transfer is already pending for this offering.
    IssuerTransferPending = 12,
    /// No transfer is pending for this offering.
    NoTransferPending = 13,
    /// Caller is not authorized to accept this transfer.
    UnauthorizedTransferAccept = 14,
    /// Payout asset does not match the configured payout asset for this offering.
    PayoutAssetMismatch = 15,
    /// Metadata string exceeds maximum allowed length.
    MetadataTooLarge = 16,
}

// ── Event symbols ────────────────────────────────────────────
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_REVENUE_REPORTED_ASSET: Symbol = symbol_short!("rev_repa");
const EVENT_REVENUE_REPORT_INITIAL: Symbol = symbol_short!("rev_init");
const EVENT_REVENUE_REPORT_INITIAL_ASSET: Symbol = symbol_short!("rev_inia");
const EVENT_REVENUE_REPORT_OVERRIDE: Symbol = symbol_short!("rev_ovrd");
const EVENT_REVENUE_REPORT_OVERRIDE_ASSET: Symbol = symbol_short!("rev_ovra");
const EVENT_REVENUE_REPORT_REJECTED: Symbol = symbol_short!("rev_rej");
const EVENT_REVENUE_REPORT_REJECTED_ASSET: Symbol = symbol_short!("rev_reja");
const EVENT_BL_ADD: Symbol = symbol_short!("bl_add");
const EVENT_BL_REM: Symbol = symbol_short!("bl_rem");
// Versioned event symbols (v1). We emit legacy events for compatibility
// and also emit explicit v1 events that include a leading `version` field.
const EVENT_OFFER_REG_V1: Symbol = symbol_short!("ofr_reg1");
const EVENT_REV_INIT_V1: Symbol = symbol_short!("rv_init1");
const EVENT_REV_INIA_V1: Symbol = symbol_short!("rv_inia1");
const EVENT_REV_REP_V1: Symbol = symbol_short!("rv_rep1");
const EVENT_REV_REPA_V1: Symbol = symbol_short!("rv_repa1");

const EVENT_SCHEMA_VERSION: u32 = 1;
const EVENT_CONCENTRATION_WARNING: Symbol = symbol_short!("conc_warn");
const EVENT_REV_DEPOSIT: Symbol = symbol_short!("rev_dep");
const EVENT_CLAIM: Symbol = symbol_short!("claim");
const EVENT_SHARE_SET: Symbol = symbol_short!("share_set");
const EVENT_FREEZE: Symbol = symbol_short!("freeze");
const EVENT_CLAIM_DELAY_SET: Symbol = symbol_short!("delay_set");
const EVENT_ISSUER_TRANSFER_PROPOSED: Symbol = symbol_short!("iss_prop");
const EVENT_ISSUER_TRANSFER_ACCEPTED: Symbol = symbol_short!("iss_acc");
const EVENT_ISSUER_TRANSFER_CANCELLED: Symbol = symbol_short!("iss_canc");
const EVENT_TESTNET_MODE: Symbol = symbol_short!("test_mode");
const EVENT_INIT: Symbol = symbol_short!("init");
const EVENT_PAUSED: Symbol = symbol_short!("paused");
const EVENT_UNPAUSED: Symbol = symbol_short!("unpaused");
const EVENT_DIST_CALC: Symbol = symbol_short!("dist_calc");
const EVENT_METADATA_SET: Symbol = symbol_short!("meta_set");
const EVENT_METADATA_UPDATED: Symbol = symbol_short!("meta_upd");

const BPS_DENOMINATOR: i128 = 10_000;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Offering {
    pub issuer: Address,
    pub token: Address,
    pub revenue_share_bps: u32,
    pub payout_asset: Address,
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

/// Result of simulate_distribution (#29): per-holder payout and total.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SimulateDistributionResult {
    /// Total amount that would be distributed.
    pub total_distributed: i128,
    /// Payout per holder (holder address, amount).
    pub payouts: Vec<(Address, i128)>,
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
/// Multi-period claim keys use PeriodRevenue/PeriodEntry/PeriodCount for per-offering
/// period tracking, HolderShare for holder allocations, LastClaimedIdx for claim progress,
/// and PaymentToken for the token used to pay out revenue.
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
    /// Per (issuer, token): revenue reports map (period_id -> (amount, timestamp)).
    RevenueReports(Address, Address),
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
    /// Per-offering claim delay in seconds (#27). 0 = immediate claim.
    ClaimDelaySecs(Address),
    /// Ledger timestamp when revenue was deposited for (offering_token, period_id).
    PeriodDepositTime(Address, u64),
    /// Global admin address; can set freeze (#32).
    Admin,
    /// Contract frozen flag; when true, state-changing ops are disabled (#32).
    Frozen,
    /// Pending issuer transfer for an offering token: token -> new_issuer.
    PendingIssuerTransfer(Address),
    /// Current issuer lookup by offering token: token -> issuer.
    OfferingIssuer(Address),
    /// Testnet mode flag; when true, enables fee-free/simplified behavior (#24).
    TestnetMode,
    /// Safety role address for emergency pause (#7).
    Safety,
    /// Global pause flag; when true, state-mutating ops are disabled (#7).
    Paused,
    /// Feature flag: emit versioned events when present (v1 schema).
    EventVersioningEnabled,
    /// Per (issuer, token): metadata reference (IPFS hash, HTTPS URI, etc.)
    OfferingMetadata(Address, Address),
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
    fn is_event_versioning_enabled(env: Env) -> bool {
        let key = DataKey::EventVersioningEnabled;
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&key)
            .unwrap_or(false)
    }

    /// Returns error if contract is frozen (#32). Call at start of state-mutating entrypoints.
    fn require_not_frozen(env: &Env) -> Result<(), RevoraError> {
        let key = DataKey::Frozen;
        if env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&key)
            .unwrap_or(false)
        {
            return Err(RevoraError::ContractFrozen);
        }
        Ok(())
    }

    /// Get the current issuer for an offering token (used for auth checks after transfers).
    fn get_current_issuer(env: &Env, token: &Address) -> Option<Address> {
        let key = DataKey::OfferingIssuer(token.clone());
        env.storage().persistent().get(&key)
    }

    /// Initialize admin and optional safety role for emergency pause (#7).
    /// Can only be called once; panics if already initialized.
    pub fn initialize(env: Env, admin: Address, safety: Option<Address>) {
        if env.storage().persistent().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage()
            .persistent()
            .set(&DataKey::Admin, &admin.clone());
        if let Some(s) = safety.clone() {
            env.storage().persistent().set(&DataKey::Safety, &s);
        }
        env.storage().persistent().set(&DataKey::Paused, &false);
        env.events().publish((EVENT_INIT, admin.clone()), (safety,));
    }

    /// Pause the contract (admin only). Idempotent.
    pub fn pause_admin(env: Env, caller: Address) {
        caller.require_auth();
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .expect("admin not set");
        if caller != admin {
            panic!("not admin");
        }
        env.storage().persistent().set(&DataKey::Paused, &true);
        env.events().publish((EVENT_PAUSED, caller.clone()), ());
    }

    /// Unpause the contract (admin only). Idempotent.
    pub fn unpause_admin(env: Env, caller: Address) {
        caller.require_auth();
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .expect("admin not set");
        if caller != admin {
            panic!("not admin");
        }
        env.storage().persistent().set(&DataKey::Paused, &false);
        env.events().publish((EVENT_UNPAUSED, caller.clone()), ());
    }

    /// Pause the contract (safety role only). Idempotent.
    pub fn pause_safety(env: Env, caller: Address) {
        caller.require_auth();
        let safety: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Safety)
            .expect("safety not set");
        if caller != safety {
            panic!("not safety");
        }
        env.storage().persistent().set(&DataKey::Paused, &true);
        env.events().publish((EVENT_PAUSED, caller.clone()), ());
    }

    /// Unpause the contract (safety role only). Idempotent.
    pub fn unpause_safety(env: Env, caller: Address) {
        caller.require_auth();
        let safety: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Safety)
            .expect("safety not set");
        if caller != safety {
            panic!("not safety");
        }
        env.storage().persistent().set(&DataKey::Paused, &false);
        env.events().publish((EVENT_UNPAUSED, caller.clone()), ());
    }

    /// Query the paused state of the contract.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::Paused)
            .unwrap_or(false)
    }

    /// Helper: panic if contract is paused. Used by state-mutating entrypoints.
    fn require_not_paused(env: &Env) {
        if env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::Paused)
            .unwrap_or(false)
        {
            panic!("contract is paused");
        }
    }

    /// Register a new revenue-share offering.
    /// Returns `Err(RevoraError::InvalidRevenueShareBps)` if revenue_share_bps > 10000.
    /// In testnet mode, bps validation is skipped to allow flexible testing.
    pub fn register_offering(
        env: Env,
        issuer: Address,
        token: Address,
        revenue_share_bps: u32,
        payout_asset: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env);
        issuer.require_auth();

        // Skip bps validation in testnet mode
        let testnet_mode = Self::is_testnet_mode(env.clone());
        if !testnet_mode && revenue_share_bps > 10_000 {
            return Err(RevoraError::InvalidRevenueShareBps);
        }

        let count_key = DataKey::OfferCount(issuer.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let offering = Offering {
            issuer: issuer.clone(),
            token: token.clone(),
            revenue_share_bps,
            payout_asset: payout_asset.clone(),
        };

        let item_key = DataKey::OfferItem(issuer.clone(), count);
        env.storage().persistent().set(&item_key, &offering);
        env.storage().persistent().set(&count_key, &(count + 1));

        // Maintain reverse lookup: token -> issuer
        let issuer_lookup_key = DataKey::OfferingIssuer(token.clone());
        env.storage().persistent().set(&issuer_lookup_key, &issuer);

        env.events().publish(
            (symbol_short!("offer_reg"), issuer.clone()),
            (token.clone(), revenue_share_bps, payout_asset.clone()),
        );
        // Optionally emit a versioned v1 event with explicit version field
        if Self::is_event_versioning_enabled(env.clone()) {
            env.events().publish(
                (EVENT_OFFER_REG_V1, issuer.clone()),
                (
                    EVENT_SCHEMA_VERSION,
                    token.clone(),
                    revenue_share_bps,
                    payout_asset.clone(),
                ),
            );
        }
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
    /// In testnet mode, concentration enforcement is skipped.
    /// `override_existing`: if true, allows overwriting a previously reported period.
    pub fn report_revenue(
        env: Env,
        issuer: Address,
        token: Address,
        payout_asset: Address,
        amount: i128,
        period_id: u64,
        override_existing: bool,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        // Verify offering exists and issuer is current
        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        Self::require_not_paused(&env);
        issuer.require_auth();

        let offering = Self::get_offering(env.clone(), issuer.clone(), token.clone())
            .ok_or(RevoraError::OfferingNotFound)?;
        if offering.payout_asset != payout_asset {
            return Err(RevoraError::PayoutAssetMismatch);
        }

        // Skip concentration enforcement in testnet mode
        let testnet_mode = Self::is_testnet_mode(env.clone());
        if !testnet_mode {
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
        }

        let blacklist = Self::get_blacklist(env.clone(), token.clone());

        let key = DataKey::RevenueReports(issuer.clone(), token.clone());
        let mut reports: Map<u64, (i128, u64)> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Map::new(&env));
        let current_timestamp = env.ledger().timestamp();

        match reports.get(period_id) {
            Some((existing_amount, _timestamp)) => {
                if override_existing {
                    reports.set(period_id, (amount, current_timestamp));
                    env.storage().persistent().set(&key, &reports);

                    env.events().publish(
                        (EVENT_REVENUE_REPORT_OVERRIDE, issuer.clone(), token.clone()),
                        (amount, period_id, existing_amount, blacklist.clone()),
                    );

                    env.events().publish(
                        (
                            EVENT_REVENUE_REPORT_OVERRIDE_ASSET,
                            issuer.clone(),
                            token.clone(),
                            payout_asset.clone(),
                        ),
                        (amount, period_id, existing_amount, blacklist.clone()),
                    );
                } else {
                    env.events().publish(
                        (EVENT_REVENUE_REPORT_REJECTED, issuer.clone(), token.clone()),
                        (amount, period_id, existing_amount, blacklist.clone()),
                    );

                    env.events().publish(
                        (
                            EVENT_REVENUE_REPORT_REJECTED_ASSET,
                            issuer.clone(),
                            token.clone(),
                            payout_asset.clone(),
                        ),
                        (amount, period_id, existing_amount, blacklist.clone()),
                    );
                }
            }
            None => {
                reports.set(period_id, (amount, current_timestamp));
                env.storage().persistent().set(&key, &reports);

                env.events().publish(
                    (EVENT_REVENUE_REPORT_INITIAL, issuer.clone(), token.clone()),
                    (amount, period_id, blacklist.clone()),
                );

                env.events().publish(
                    (
                        EVENT_REVENUE_REPORT_INITIAL_ASSET,
                        issuer.clone(),
                        token.clone(),
                        payout_asset.clone(),
                    ),
                    (amount, period_id, blacklist.clone()),
                );
            }
        }

        // Backward-compatible event (preserve `blacklist` for additional publishes)
        env.events().publish(
            (EVENT_REVENUE_REPORTED, issuer.clone(), token.clone()),
            (amount, period_id, blacklist.clone()),
        );

        env.events().publish(
            (
                EVENT_REVENUE_REPORTED_ASSET,
                issuer.clone(),
                token.clone(),
                payout_asset.clone(),
            ),
            (amount, period_id),
        );

        // Optionally emit versioned v1 events for forward-compatible consumers
        if Self::is_event_versioning_enabled(env.clone()) {
            env.events().publish(
                (EVENT_REV_INIT_V1, issuer.clone(), token.clone()),
                (EVENT_SCHEMA_VERSION, amount, period_id, blacklist.clone()),
            );

            env.events().publish(
                (
                    EVENT_REV_INIA_V1,
                    issuer.clone(),
                    token.clone(),
                    payout_asset.clone(),
                ),
                (EVENT_SCHEMA_VERSION, amount, period_id, blacklist.clone()),
            );

            env.events().publish(
                (EVENT_REV_REP_V1, issuer.clone(), token.clone()),
                (EVENT_SCHEMA_VERSION, amount, period_id, blacklist.clone()),
            );

            env.events().publish(
                (
                    EVENT_REV_REPA_V1,
                    issuer.clone(),
                    token.clone(),
                    payout_asset.clone(),
                ),
                (EVENT_SCHEMA_VERSION, amount, period_id),
            );
        }

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
    pub fn blacklist_add(
        env: Env,
        caller: Address,
        token: Address,
        investor: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env);
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
        Ok(())
    }

    /// Remove `investor` from the per-offering blacklist for `token`. Idempotent.
    pub fn blacklist_remove(
        env: Env,
        caller: Address,
        token: Address,
        investor: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env);
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
        Ok(())
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
        Self::require_not_frozen(&env)?;

        // Verify offering exists and issuer is current
        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::LimitReached)?;

        if current_issuer != issuer {
            return Err(RevoraError::LimitReached);
        }

        issuer.require_auth();
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
        Self::require_not_frozen(&env)?;

        // Verify offering exists and issuer is current
        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

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
        Self::require_not_frozen(&env)?;

        // Verify offering exists and issuer is current
        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::LimitReached)?;

        if current_issuer != issuer {
            return Err(RevoraError::LimitReached);
        }

        issuer.require_auth();
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
        Self::require_not_frozen(&env)?;

        // Verify offering exists and issuer is current
        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        // Verify offering exists
        let offering = Self::get_offering(env.clone(), issuer.clone(), token.clone())
            .ok_or(RevoraError::OfferingNotFound)?;
        if offering.payout_asset != payment_token {
            return Err(RevoraError::PayoutAssetMismatch);
        }

        issuer.require_auth();

        // Check period not already deposited
        let rev_key = DataKey::PeriodRevenue(token.clone(), period_id);
        if env.storage().persistent().has(&rev_key) {
            return Err(RevoraError::PeriodAlreadyDeposited);
        }

        // Store or validate payment token for this offering
        let pt_key = DataKey::PaymentToken(token.clone());
        if let Some(existing_pt) = env.storage().persistent().get::<DataKey, Address>(&pt_key) {
            if existing_pt != payment_token {
                return Err(RevoraError::PaymentTokenMismatch);
            }
        } else {
            env.storage().persistent().set(&pt_key, &payment_token);
        }

        // Transfer tokens from issuer to contract
        let contract_addr = env.current_contract_address();
        token::Client::new(&env, &payment_token).transfer(&issuer, &contract_addr, &amount);

        // Store period revenue
        env.storage().persistent().set(&rev_key, &amount);

        // Store deposit timestamp for time-delayed claims (#27)
        let deposit_time = env.ledger().timestamp();
        let time_key = DataKey::PeriodDepositTime(token.clone(), period_id);
        env.storage().persistent().set(&time_key, &deposit_time);

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
        Self::require_not_frozen(&env)?;

        // Verify offering exists and issuer is current
        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        issuer.require_auth();

        if share_bps > 10_000 {
            return Err(RevoraError::InvalidShareBps);
        }

        let key = DataKey::HolderShare(token.clone(), holder.clone());
        env.storage().persistent().set(&key, &share_bps);

        env.events()
            .publish((EVENT_SHARE_SET, issuer, token), (holder, share_bps));
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

        let delay_key = DataKey::ClaimDelaySecs(token.clone());
        let delay_secs: u64 = env.storage().persistent().get(&delay_key).unwrap_or(0);
        let now = env.ledger().timestamp();

        let mut total_payout: i128 = 0;
        let mut claimed_periods = Vec::new(&env);
        let mut last_claimed_idx = start_idx;

        for i in start_idx..end_idx {
            let entry_key = DataKey::PeriodEntry(token.clone(), i);
            let period_id: u64 = env.storage().persistent().get(&entry_key).unwrap();
            let time_key = DataKey::PeriodDepositTime(token.clone(), period_id);
            let deposit_time: u64 = env.storage().persistent().get(&time_key).unwrap_or(0);
            if delay_secs > 0 && now < deposit_time.saturating_add(delay_secs) {
                break;
            }
            let rev_key = DataKey::PeriodRevenue(token.clone(), period_id);
            let revenue: i128 = env.storage().persistent().get(&rev_key).unwrap();
            let payout = revenue * (share_bps as i128) / 10_000;
            total_payout += payout;
            claimed_periods.push_back(period_id);
            last_claimed_idx = i + 1;
        }

        if last_claimed_idx == start_idx {
            return Err(RevoraError::ClaimDelayNotElapsed);
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

        // Advance claim index only for periods actually claimed (respecting delay)
        env.storage().persistent().set(&idx_key, &last_claimed_idx);

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
    /// Respects per-offering claim delay (#27): only sums periods past the delay.
    pub fn get_claimable(env: Env, token: Address, holder: Address) -> i128 {
        let share_bps = Self::get_holder_share(env.clone(), token.clone(), holder.clone());
        if share_bps == 0 {
            return 0;
        }

        let count_key = DataKey::PeriodCount(token.clone());
        let period_count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let idx_key = DataKey::LastClaimedIdx(token.clone(), holder.clone());
        let start_idx: u32 = env.storage().persistent().get(&idx_key).unwrap_or(0);

        let delay_key = DataKey::ClaimDelaySecs(token.clone());
        let delay_secs: u64 = env.storage().persistent().get(&delay_key).unwrap_or(0);
        let now = env.ledger().timestamp();

        let mut total: i128 = 0;
        for i in start_idx..period_count {
            let entry_key = DataKey::PeriodEntry(token.clone(), i);
            let period_id: u64 = env.storage().persistent().get(&entry_key).unwrap();
            let time_key = DataKey::PeriodDepositTime(token.clone(), period_id);
            let deposit_time: u64 = env.storage().persistent().get(&time_key).unwrap_or(0);
            if delay_secs > 0 && now < deposit_time.saturating_add(delay_secs) {
                break;
            }
            let rev_key = DataKey::PeriodRevenue(token.clone(), period_id);
            let revenue: i128 = env.storage().persistent().get(&rev_key).unwrap();
            total += revenue * (share_bps as i128) / 10_000;
        }
        total
    }

    // ── Time-delayed claim configuration (#27) ──────────────────

    /// Set per-offering claim delay in seconds. Only issuer may set. 0 = immediate claim.
    pub fn set_claim_delay(
        env: Env,
        issuer: Address,
        token: Address,
        delay_secs: u64,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        // Verify offering exists and issuer is current
        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        issuer.require_auth();
        let key = DataKey::ClaimDelaySecs(token.clone());
        env.storage().persistent().set(&key, &delay_secs);
        env.events()
            .publish((EVENT_CLAIM_DELAY_SET, issuer, token), delay_secs);
        Ok(())
    }

    /// Get per-offering claim delay in seconds. 0 = immediate claim.
    pub fn get_claim_delay(env: Env, token: Address) -> u64 {
        let key = DataKey::ClaimDelaySecs(token);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    /// Return the total number of deposited periods for an offering token.
    pub fn get_period_count(env: Env, token: Address) -> u32 {
        let count_key = DataKey::PeriodCount(token);
        env.storage().persistent().get(&count_key).unwrap_or(0)
    }

    // ── On-chain distribution simulation (#29) ────────────────────

    /// Read-only: simulate distribution for sample inputs without mutating state.
    /// Returns expected payouts per holder and total. Uses offering's rounding mode.
    /// For integrators to preview outcomes before executing deposit/claim flows.
    pub fn simulate_distribution(
        env: Env,
        issuer: Address,
        token: Address,
        amount: i128,
        holder_shares: Vec<(Address, u32)>,
    ) -> SimulateDistributionResult {
        let mode = Self::get_rounding_mode(env.clone(), issuer, token.clone());
        let mut total: i128 = 0;
        let mut payouts = Vec::new(&env);
        for i in 0..holder_shares.len() {
            let (holder, share_bps) = holder_shares.get(i).unwrap();
            let payout = if share_bps > 10_000 {
                0_i128
            } else {
                Self::compute_share(env.clone(), amount, share_bps, mode)
            };
            total = total.saturating_add(payout);
            payouts.push_back((holder.clone(), payout));
        }
        SimulateDistributionResult {
            total_distributed: total,
            payouts,
        }
    }

    // ── Upgradeability guard and freeze (#32) ───────────────────

    /// Set the admin address. May only be called once; caller must authorize as the new admin.
    pub fn set_admin(env: Env, admin: Address) -> Result<(), RevoraError> {
        admin.require_auth();
        let key = DataKey::Admin;
        if env.storage().persistent().has(&key) {
            return Err(RevoraError::LimitReached);
        }
        env.storage().persistent().set(&key, &admin);
        Ok(())
    }

    /// Get the admin address, if set.
    pub fn get_admin(env: Env) -> Option<Address> {
        let key = DataKey::Admin;
        env.storage().persistent().get(&key)
    }

    /// Freeze the contract: no further state-changing operations allowed. Only admin may call.
    /// Emits event. Claim and read-only functions remain allowed.
    pub fn freeze(env: Env) -> Result<(), RevoraError> {
        let key = DataKey::Admin;
        let admin: Address = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(RevoraError::LimitReached)?;
        admin.require_auth();
        let frozen_key = DataKey::Frozen;
        env.storage().persistent().set(&frozen_key, &true);
        env.events().publish((EVENT_FREEZE, admin), true);
        Ok(())
    }

    /// Return true if the contract is frozen.
    pub fn is_frozen(env: Env) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::Frozen)
            .unwrap_or(false)
    }

    // ── Secure issuer transfer (two-step flow) ─────────────────

    /// Propose transferring issuer control of an offering to a new address.
    /// Only the current issuer may call this. Initiates a two-step transfer.
    pub fn propose_issuer_transfer(
        env: Env,
        token: Address,
        new_issuer: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        // Get current issuer and verify offering exists
        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;

        // Only current issuer can propose transfer
        current_issuer.require_auth();

        // Check if transfer already pending
        let pending_key = DataKey::PendingIssuerTransfer(token.clone());
        if env.storage().persistent().has(&pending_key) {
            return Err(RevoraError::IssuerTransferPending);
        }

        // Store pending transfer
        env.storage().persistent().set(&pending_key, &new_issuer);

        env.events().publish(
            (EVENT_ISSUER_TRANSFER_PROPOSED, token.clone()),
            (current_issuer, new_issuer),
        );

        Ok(())
    }

    /// Accept a pending issuer transfer. Only the proposed new issuer may call this.
    /// Completes the two-step transfer and grants full issuer control to the new address.
    pub fn accept_issuer_transfer(env: Env, token: Address) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        // Get pending transfer
        let pending_key = DataKey::PendingIssuerTransfer(token.clone());
        let new_issuer: Address = env
            .storage()
            .persistent()
            .get(&pending_key)
            .ok_or(RevoraError::NoTransferPending)?;

        // Only the proposed new issuer can accept
        new_issuer.require_auth();

        // Get current issuer
        let old_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;

        // Update the offering's issuer field in storage
        // We need to find and update the offering
        let offering = Self::get_offering(env.clone(), old_issuer.clone(), token.clone())
            .ok_or(RevoraError::OfferingNotFound)?;

        // Find the index of this offering
        let count = Self::get_offering_count(env.clone(), old_issuer.clone());
        let mut found_index: Option<u32> = None;
        for i in 0..count {
            let item_key = DataKey::OfferItem(old_issuer.clone(), i);
            let stored_offering: Offering = env.storage().persistent().get(&item_key).unwrap();
            if stored_offering.token == token {
                found_index = Some(i);
                break;
            }
        }

        let index = found_index.ok_or(RevoraError::OfferingNotFound)?;

        // Update the offering with new issuer
        let updated_offering = Offering {
            issuer: new_issuer.clone(),
            token: token.clone(),
            revenue_share_bps: offering.revenue_share_bps,
            payout_asset: offering.payout_asset,
        };

        // Remove from old issuer's storage
        let old_item_key = DataKey::OfferItem(old_issuer.clone(), index);
        env.storage().persistent().remove(&old_item_key);

        // If this wasn't the last offering, move the last offering to fill the gap
        let old_count = Self::get_offering_count(env.clone(), old_issuer.clone());
        if index < old_count - 1 {
            // Move the last offering to the removed index
            let last_key = DataKey::OfferItem(old_issuer.clone(), old_count - 1);
            let last_offering: Offering = env.storage().persistent().get(&last_key).unwrap();
            env.storage()
                .persistent()
                .set(&old_item_key, &last_offering);
            env.storage().persistent().remove(&last_key);
        }

        // Decrement old issuer's count
        let old_count_key = DataKey::OfferCount(old_issuer.clone());
        env.storage()
            .persistent()
            .set(&old_count_key, &(old_count - 1));

        // Add to new issuer's storage
        let new_count = Self::get_offering_count(env.clone(), new_issuer.clone());
        let new_item_key = DataKey::OfferItem(new_issuer.clone(), new_count);
        env.storage()
            .persistent()
            .set(&new_item_key, &updated_offering);

        // Increment new issuer's count
        let new_count_key = DataKey::OfferCount(new_issuer.clone());
        env.storage()
            .persistent()
            .set(&new_count_key, &(new_count + 1));

        // Update reverse lookup
        let issuer_lookup_key = DataKey::OfferingIssuer(token.clone());
        env.storage()
            .persistent()
            .set(&issuer_lookup_key, &new_issuer);

        // Clear pending transfer
        env.storage().persistent().remove(&pending_key);

        env.events().publish(
            (EVENT_ISSUER_TRANSFER_ACCEPTED, token),
            (old_issuer, new_issuer),
        );

        Ok(())
    }

    /// Cancel a pending issuer transfer. Only the current issuer may call this.
    pub fn cancel_issuer_transfer(env: Env, token: Address) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        // Get current issuer
        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;

        // Only current issuer can cancel
        current_issuer.require_auth();

        // Check if transfer is pending
        let pending_key = DataKey::PendingIssuerTransfer(token.clone());
        let proposed_new_issuer: Address = env
            .storage()
            .persistent()
            .get(&pending_key)
            .ok_or(RevoraError::NoTransferPending)?;

        // Clear pending transfer
        env.storage().persistent().remove(&pending_key);

        env.events().publish(
            (EVENT_ISSUER_TRANSFER_CANCELLED, token),
            (current_issuer, proposed_new_issuer),
        );

        Ok(())
    }

    /// Get the pending issuer transfer for an offering, if any.
    pub fn get_pending_issuer_transfer(env: Env, token: Address) -> Option<Address> {
        let pending_key = DataKey::PendingIssuerTransfer(token);
        env.storage().persistent().get(&pending_key)
    }

    // ── Revenue distribution calculation ───────────────────────────

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
    #[allow(clippy::too_many_arguments)]
    pub fn calculate_distribution(
        env: Env,
        caller: Address,
        issuer: Address,
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

        let offering = Self::get_offering(env.clone(), issuer.clone(), token.clone())
            .expect("offering not found");

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
    pub fn calculate_total_distributable(
        env: Env,
        issuer: Address,
        token: Address,
        total_revenue: i128,
    ) -> i128 {
        let offering =
            Self::get_offering(env, issuer, token).expect("offering not found for token");

        if total_revenue == 0 {
            return 0;
        }

        (total_revenue * offering.revenue_share_bps as i128)
            .checked_div(BPS_DENOMINATOR)
            .expect("division overflow")
    }

    // ── Per-offering metadata storage (#8) ─────────────────────

    /// Maximum allowed length for metadata strings (256 bytes).
    /// Supports IPFS CIDs (46 chars), URLs, and content hashes.
    const MAX_METADATA_LENGTH: usize = 256;

    /// Set or update metadata reference for an offering.
    ///
    /// Only callable by the current issuer of the offering.
    /// Metadata can be an IPFS hash (e.g., "Qm..."), HTTPS URI, or any reference string.
    /// Maximum length: 256 bytes.
    ///
    /// Emits `EVENT_METADATA_SET` on first set, `EVENT_METADATA_UPDATED` on subsequent updates.
    ///
    /// # Errors
    /// - `OfferingNotFound`: offering doesn't exist or caller is not the current issuer
    /// - `MetadataTooLarge`: metadata string exceeds MAX_METADATA_LENGTH
    /// - `ContractFrozen`: contract is frozen
    pub fn set_offering_metadata(
        env: Env,
        issuer: Address,
        token: Address,
        metadata: String,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env);

        // Verify offering exists and issuer is current
        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        issuer.require_auth();

        // Validate metadata length
        let metadata_bytes = metadata.len();
        if metadata_bytes > Self::MAX_METADATA_LENGTH as u32 {
            return Err(RevoraError::MetadataTooLarge);
        }

        let key = DataKey::OfferingMetadata(issuer.clone(), token.clone());
        let is_update = env.storage().persistent().has(&key);

        // Store metadata
        env.storage().persistent().set(&key, &metadata);

        // Emit appropriate event
        if is_update {
            env.events()
                .publish((EVENT_METADATA_UPDATED, issuer, token), metadata);
        } else {
            env.events()
                .publish((EVENT_METADATA_SET, issuer, token), metadata);
        }

        Ok(())
    }

    /// Retrieve metadata reference for an offering.
    ///
    /// Returns `None` if no metadata has been set for this offering.
    pub fn get_offering_metadata(env: Env, issuer: Address, token: Address) -> Option<String> {
        let key = DataKey::OfferingMetadata(issuer, token);
        env.storage().persistent().get(&key)
    }

    // ── Testnet mode configuration (#24) ───────────────────────

    /// Enable or disable testnet mode. Only admin may call.
    /// When enabled, certain validations are relaxed for testnet deployments.
    /// Emits event with new mode state.
    pub fn set_testnet_mode(env: Env, enabled: bool) -> Result<(), RevoraError> {
        let key = DataKey::Admin;
        let admin: Address = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(RevoraError::LimitReached)?;
        admin.require_auth();
        let mode_key = DataKey::TestnetMode;
        env.storage().persistent().set(&mode_key, &enabled);
        env.events().publish((EVENT_TESTNET_MODE, admin), enabled);
        Ok(())
    }

    /// Return true if testnet mode is enabled.
    pub fn is_testnet_mode(env: Env) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::TestnetMode)
            .unwrap_or(false)
    }
}

mod test;
