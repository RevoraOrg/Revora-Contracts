#![no_std]
#![deny(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::todo, clippy::unimplemented)]
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

    /// Snapshot distribution is not enabled for this offering.
    SnapshotNotEnabled = 12,
    /// Provided snapshot reference is outdated or duplicates a previous one.
    OutdatedSnapshot = 13,

    /// Payout asset does not match the configured payout asset for this offering.
    PayoutAssetMismatch = 12,

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
    /// Caller is not authorized to perform this action.
    NotAuthorized = 17,
    /// Contract is not initialized (admin not set).
    NotInitialized = 18,
    /// Amount is invalid (e.g. negative for deposit, or out of allowed range) (#35).
    InvalidAmount = 17,
    /// period_id is invalid (e.g. zero when required to be positive) (#35).
    InvalidPeriodId = 18,
}

// ── Event symbols ────────────────────────────────────────────
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_BL_ADD: Symbol          = symbol_short!("bl_add");
const EVENT_BL_REM: Symbol          = symbol_short!("bl_rem");
const EVENT_WL_ADD: Symbol          = symbol_short!("wl_add");
const EVENT_WL_REM: Symbol          = symbol_short!("wl_rem");

// ── Storage key ──────────────────────────────────────────────
/// One blacklist map per offering, keyed by the offering's token address.
///
/// Blacklist precedence rule: a blacklisted address is **always** excluded
/// from payouts, regardless of any whitelist or investor registration.
/// If the same address appears in both a whitelist and this blacklist,
/// the blacklist wins unconditionally.
///
/// Whitelist is optional per offering. When enabled (non-empty), only
/// whitelisted addresses are eligible for revenue distribution.
/// When disabled (empty), all non-blacklisted holders are eligible.
#[contracttype]
pub enum DataKey {
    Blacklist(Address),
    Whitelist(Address),
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
const EVENT_REV_DEP_SNAP: Symbol = symbol_short!("rev_snap");
const EVENT_CLAIM: Symbol = symbol_short!("claim");
const EVENT_SHARE_SET: Symbol = symbol_short!("share_set");
const EVENT_FREEZE: Symbol = symbol_short!("freeze");
const EVENT_CLAIM_DELAY_SET: Symbol = symbol_short!("delay_set");

const EVENT_PROPOSAL_CREATED: Symbol = symbol_short!("prop_new");
const EVENT_PROPOSAL_APPROVED: Symbol = symbol_short!("prop_app");
const EVENT_PROPOSAL_EXECUTED: Symbol = symbol_short!("prop_exe");

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalAction {
    SetAdmin(Address),
    Freeze,
    SetThreshold(u32),
    AddOwner(Address),
    RemoveOwner(Address),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub id: u32,
    pub action: ProposalAction,
    pub proposer: Address,
    pub approvals: Vec<Address>,
    pub executed: bool,
}

 
const EVENT_SNAP_CONFIG: Symbol = symbol_short!("snap_cfg");

const EVENT_INIT: Symbol = symbol_short!("init");
const EVENT_PAUSED: Symbol = symbol_short!("paused");
const EVENT_UNPAUSED: Symbol = symbol_short!("unpaused");
 

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
/// Emitted when per-offering minimum revenue threshold is set or changed (#25).
const EVENT_MIN_REV_THRESHOLD_SET: Symbol = symbol_short!("min_rev");
/// Emitted when reported revenue is below the offering's minimum threshold; no distribution triggered (#25).
const EVENT_REV_BELOW_THRESHOLD: Symbol = symbol_short!("rev_below");

const BPS_DENOMINATOR: i128 = 10_000;

/// Represents a revenue-share offering registered on-chain.
/// Offerings are immutable once registered.
// ── Data structures ──────────────────────────────────────────
/// Contract version identifier (#23). Bumped when storage or semantics change; used for migration and compatibility.
pub const CONTRACT_VERSION: u32 = 1;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Offering {
    /// The address authorized to manage this offering.
    pub issuer: Address,
    /// The token representing this offering.
    pub token: Address,
    /// Cumulative revenue share for all holders in basis points (0-10000).
    pub revenue_share_bps: u32,
    pub payout_asset: Address,
}

/// Per-offering concentration guardrail config (#26).
/// max_bps: max allowed single-holder share in basis points (0 = disabled).
/// enforce: if true, report_revenue fails when current concentration > max_bps.
/// Configuration for single-holder concentration guardrails.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ConcentrationLimitConfig {
    /// Maximum allowed share in basis points for a single holder (0 = disabled).
    pub max_bps: u32,
    /// If true, `report_revenue` will fail if current concentration exceeds `max_bps`.
    pub enforce: bool,
}

/// Per-offering audit log summary (#34).
/// Summarizes the audit trail for a specific offering.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AuditSummary {
    /// Cumulative revenue amount reported for this offering.
    pub total_revenue: i128,
    /// Total number of revenue reports submitted.
    pub report_count: u64,
}

/// Cross-offering aggregated metrics (#39).
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AggregatedMetrics {
    pub total_reported_revenue: i128,
    pub total_deposited_revenue: i128,
    pub total_report_count: u64,
    pub offering_count: u32,
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

/// Defines how fractional shares are handled during distribution calculations.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundingMode {
    /// Truncate toward zero: share = (amount * bps) / 10000.
    Truncation = 0,
    /// Standard rounding: share = round((amount * bps) / 10000), where >= 0.5 rounds up.
    RoundHalfUp = 1,
}

/// Storage keys: offerings use OfferCount/OfferItem; blacklist uses Blacklist(token).
/// Multi-period claim keys use PeriodRevenue/PeriodEntry/PeriodCount for per-offering
/// period tracking, HolderShare for holder allocations, LastClaimedIdx for claim progress,
/// and PaymentToken for the token used to pay out revenue.
/// `RevenueIndex` and `RevenueReports` track reported (un-deposited) revenue totals and details.
#[contracttype]
pub enum DataKey {
    Blacklist(Address),
    /// Per-token: blacklist addresses in insertion order for deterministic get_blacklist (#38).
    BlacklistOrder(Address),
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
    /// FLAT INDEX per (token, period_id): cumulative reported revenue amount.
    RevenueIndex(Address, u64),
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

    /// Multisig admin threshold.
    MultisigThreshold,
    /// Multisig admin owners.
    MultisigOwners,
    /// Multisig proposal by ID.
    MultisigProposal(u32),
    /// Multisig proposal count.
    MultisigProposalCount,

 
    /// Per (issuer, token): whether snapshot distribution is enabled.
    SnapshotConfig(Address, Address),
    /// Per (issuer, token): latest recorded snapshot reference.
    LastSnapshotRef(Address, Address),


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

    /// Configuration flag: when true, contract is event-only (no persistent business state).
    EventOnlyMode,

    /// Per (issuer, token): metadata reference (IPFS hash, HTTPS URI, etc.)
    OfferingMetadata(Address, Address),
    /// Platform fee in basis points (max 5000 = 50%) taken from reported revenue (#6).
    PlatformFeeBps,

    /// Per (issuer, token): minimum revenue per period below which no distribution is triggered (#25).
    MinRevenueThreshold(Address, Address),
    /// Global count of unique issuers (#39).
    IssuerCount,
    /// Issuer address at global index (#39).
    IssuerItem(u32),
    /// Whether an issuer is already registered in the global registry (#39).
    IssuerRegistered(Address),
    /// Total deposited revenue for an offering token (#39).
    DepositedRevenue(Address),
}

/// Maximum number of offerings returned in a single page.
const MAX_PAGE_LIMIT: u32 = 20;

/// Maximum platform fee in basis points (50%).
const MAX_PLATFORM_FEE_BPS: u32 = 5_000;

/// Maximum number of periods that can be claimed in a single transaction.
/// Keeps compute costs predictable within Soroban limits.
const MAX_CLAIM_PERIODS: u32 = 50;

// ── Contract ─────────────────────────────────────────────────
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
        if env.storage().persistent().get::<DataKey, bool>(&key).unwrap_or(false) {
            return Err(RevoraError::ContractFrozen);
        }
        Ok(())
    }

 
    /// Internal helper for revenue deposits.
    fn do_deposit_revenue(
        env: &Env,
        issuer: Address,
        token: Address,
        payment_token: Address,
        amount: i128,
        period_id: u64,
    ) -> Result<(), RevoraError> {
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
        if let Some(existing_pt) = env.storage().persistent().get::<DataKey, Address>(&pt_key) {
            if existing_pt != payment_token {
                return Err(RevoraError::PaymentTokenMismatch);
            }
        } else {
            env.storage().persistent().set(&pt_key, &payment_token);
        }

        // Transfer tokens from issuer to contract
        let contract_addr = env.current_contract_address();
        token::Client::new(env, &payment_token).transfer(&issuer, &contract_addr, &amount);

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
    /// Return true if the contract is in event-only mode.
    pub fn is_event_only(env: &Env) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::EventOnlyMode)
            .unwrap_or(false)
    /// Input validation (#35): require amount > 0 for transfers/deposits.
    fn require_positive_amount(amount: i128) -> Result<(), RevoraError> {
        if amount <= 0 {
            return Err(RevoraError::InvalidAmount);
        }
        Ok(())
    }

    /// Input validation (#35): require period_id > 0 where 0 would be ambiguous.
    fn require_valid_period_id(period_id: u64) -> Result<(), RevoraError> {
        if period_id == 0 {
            return Err(RevoraError::InvalidPeriodId);
        }
        Ok(())
    }

    /// Input validation (#35): require amount >= 0 for reporting (allow zero revenue report).
    fn require_non_negative_amount(amount: i128) -> Result<(), RevoraError> {
        if amount < 0 {
            return Err(RevoraError::InvalidAmount);
        }
        Ok(())
    }


    /// Initialize the contract with an admin and an optional safety role.
    ///
    /// This method follows the singleton pattern and can only be called once.
    ///
    /// ### Parameters
    /// - `admin`: The primary administrative address with authority to pause/unpause and manage offerings.
    /// - `safety`: Optional address allowed to trigger emergency pauses but not manage offerings.
    ///
    /// ### Panics
    /// Panics if the contract has already been initialized.

    /// Get the current issuer for an offering token (used for auth checks after transfers).
    fn get_current_issuer(env: &Env, token: &Address) -> Option<Address> {
        let key = DataKey::OfferingIssuer(token.clone());
        env.storage().persistent().get(&key)
    }

    /// Initialize admin and optional safety role for emergency pause (#7).
    /// Can only be called once; panics if already initialized
    pub fn initialize(env: Env, admin: Address, safety: Option<Address>) {
    /// `event_only` configures the contract to skip persistent business state (#72).
    /// Can only be called once; panics if already initialized.
    pub fn initialize(env: Env, admin: Address, safety: Option<Address>, event_only: Option<bool>) {
        if env.storage().persistent().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().persistent().set(&DataKey::Admin, &admin.clone());
        if let Some(s) = safety.clone() {
            env.storage().persistent().set(&DataKey::Safety, &s);
        }
        env.storage().persistent().set(&DataKey::Paused, &false);
        let eo = event_only.unwrap_or(false);
        env.storage().persistent().set(&DataKey::EventOnlyMode, &eo);
        env.events().publish((EVENT_INIT, admin.clone()), (safety, eo));
    }

    /// Pause the contract (Admin only).
    ///
    /// When paused, all state-mutating operations are disabled to protect the system.
    /// This operation is idempotent.
    ///
    /// ### Parameters
    /// - `caller`: The address of the admin (must match initialized admin).
    pub fn pause_admin(env: Env, caller: Address) {
        caller.require_auth();
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).expect("admin not set");
        if caller != admin {
            panic!("not admin");
        }
        env.storage().persistent().set(&DataKey::Paused, &true);
        env.events().publish((EVENT_PAUSED, caller.clone()), ());
    }

    /// Unpause the contract (Admin only).
    ///
    /// Re-enables state-mutating operations after a pause.
    /// This operation is idempotent.
    ///
    /// ### Parameters
    /// - `caller`: The address of the admin (must match initialized admin).
    pub fn unpause_admin(env: Env, caller: Address) {
        caller.require_auth();
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).expect("admin not set");
        if caller != admin {
            panic!("not admin");
        }
        env.storage().persistent().set(&DataKey::Paused, &false);
        env.events().publish((EVENT_UNPAUSED, caller.clone()), ());
    }

    /// Pause the contract (Safety role only).
    ///
    /// Allows the safety role to trigger an emergency pause.
    /// This operation is idempotent.
    ///
    /// ### Parameters
    /// - `caller`: The address of the safety role (must match initialized safety address).
    pub fn pause_safety(env: Env, caller: Address) {
        caller.require_auth();
        let safety: Address =
            env.storage().persistent().get(&DataKey::Safety).expect("safety not set");
        if caller != safety {
            panic!("not safety");
        }
        env.storage().persistent().set(&DataKey::Paused, &true);
        env.events().publish((EVENT_PAUSED, caller.clone()), ());
    }

    /// Unpause the contract (Safety role only).
    ///
    /// Allows the safety role to resume contract operations.
    /// This operation is idempotent.
    ///
    /// ### Parameters
    /// - `caller`: The address of the safety role (must match initialized safety address).
    pub fn unpause_safety(env: Env, caller: Address) {
        caller.require_auth();
        let safety: Address =
            env.storage().persistent().get(&DataKey::Safety).expect("safety not set");
        if caller != safety {
            panic!("not safety");
        }
        env.storage().persistent().set(&DataKey::Paused, &false);
        env.events().publish((EVENT_UNPAUSED, caller.clone()), ());
    }

    /// Query the paused state of the contract.
    pub fn is_paused(env: Env) -> bool {
        env.storage().persistent().get::<DataKey, bool>(&DataKey::Paused).unwrap_or(false)
    }

    /// Helper: panic if contract is paused. Used by state-mutating entrypoints.
    fn require_not_paused(env: &Env) {
        if env.storage().persistent().get::<DataKey, bool>(&DataKey::Paused).unwrap_or(false) {
            panic!("contract is paused");
        }
 
    }

    // ── Offering management ───────────────────────────────────

    /// Register a new revenue-share offering.

    ///
    /// Once registered, an offering's parameters are immutable.
    ///
    /// ### Parameters
    /// - `issuer`: The address with authority to manage this offering. Must provide authentication.
    /// - `token`: The token representing the offering.
    /// - `revenue_share_bps`: Total revenue share for all holders in basis points (0-10000).
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::InvalidRevenueShareBps)` if `revenue_share_bps` exceeds 10000.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.

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

        if !Self::is_event_only(&env) {
            let count_key = DataKey::OfferCount(issuer.clone());
            let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        let count_key = DataKey::OfferCount(issuer.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);


        let offering = Offering { issuer: issuer.clone(), token: token.clone(), revenue_share_bps };

        let offering = Offering {
            issuer: issuer.clone(),
            token: token.clone(),
            revenue_share_bps,
            payout_asset: payout_asset.clone(),
        };


        let item_key = DataKey::OfferItem(issuer.clone(), count);
        env.storage().persistent().set(&item_key, &offering);
        env.storage().persistent().set(&count_key, &(count + 1));
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
        }

        // Track issuer in global registry for cross-offering aggregation (#39)
        let registered_key = DataKey::IssuerRegistered(issuer.clone());
        if !env.storage().persistent().has(&registered_key) {
            let issuer_count_key = DataKey::IssuerCount;
            let issuer_count: u32 = env
                .storage()
                .persistent()
                .get(&issuer_count_key)
                .unwrap_or(0);
            let issuer_item_key = DataKey::IssuerItem(issuer_count);
            env.storage()
                .persistent()
                .set(&issuer_item_key, &issuer.clone());
            env.storage()
                .persistent()
                .set(&issuer_count_key, &(issuer_count + 1));
            env.storage().persistent().set(&registered_key, &true);
        }


        env.events().publish((symbol_short!("offer_reg"), issuer), (token, revenue_share_bps));

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

    /// Fetch a single offering by issuer and token.
    ///
    /// This method scans the issuer's registered offerings to find the one matching the given token.
    ///
    /// ### Parameters
    /// - `issuer`: The address that registered the offering.
    /// - `token`: The token address associated with the offering.
    ///
    /// ### Returns
    /// - `Some(Offering)` if found.
    /// - `None` otherwise.
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


    /// Record a revenue report for an offering and emit an audit event.
    ///
    /// This method is primarily for off-chain audit trails. It does not transfer funds.
    /// It emits an event containing the revenue amount, period ID, and a snapshot of the current blacklist.
    /// Updates the per-offering `AuditSummary`.
    ///
    /// ### Parameters
    /// - `issuer`: The offering issuer. Must provide authentication.
    /// - `token`: The token representing the offering.
    /// - `amount`: Total revenue amount to report.
    /// - `period_id`: Unique identifier for the revenue period (e.g., a timestamp or sequence).
    /// - `override_existing`: If true, allows updating reports for previously reported periods.
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::ConcentrationLimitExceeded)` if enforcement is enabled and concentration exceeds limit.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.

    /// Record a revenue report for an offering. Updates audit summary (#34).
    /// Fails with `ConcentrationLimitExceeded` (#26) if concentration enforcement is on and current concentration exceeds limit.
    /// In testnet mode, concentration enforcement is skipped.
    /// `override_existing`: if true, allows overwriting a previously reported period.

    ///
    /// The event payload includes the current blacklist so off-chain
    /// distribution engines can filter recipients in the same atomic step.
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
        Self::require_not_paused(&env);
        issuer.require_auth();

        let event_only = Self::is_event_only(&env);

        if !event_only {
            // Verify offering exists and issuer is current
            let current_issuer =
                Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;

            if current_issuer != issuer {
                return Err(RevoraError::OfferingNotFound);
            }

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
        }

        let blacklist = if event_only {
            Vec::new(&env)
        } else {
            Self::get_blacklist(env.clone(), token.clone())
        };

        if !event_only {
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
        Self::require_non_negative_amount(amount)?;

        let offering = Self::get_offering(env.clone(), issuer.clone(), token.clone())
            .ok_or(RevoraError::OfferingNotFound)?;
        if offering.payout_asset != payout_asset {
            return Err(RevoraError::PayoutAssetMismatch);
        }

        // Per-offering minimum revenue threshold (#25): skip distribution when below threshold
        let min_threshold =
            Self::get_min_revenue_threshold(env.clone(), issuer.clone(), token.clone());
        if min_threshold > 0 && amount < min_threshold {
            env.events().publish(
                (EVENT_REV_BELOW_THRESHOLD, issuer, token),
                (amount, period_id, min_threshold),
            );
            return Ok(());
        }

        // Skip concentration enforcement in testnet mode
        let testnet_mode = Self::is_testnet_mode(env.clone());
        if !testnet_mode {
            // Holder concentration guardrail (#26): reject if enforce and over limit
            let limit_key = DataKey::ConcentrationLimit(issuer.clone(), token.clone());
            if let Some(config) =
                env.storage().persistent().get::<DataKey, ConcentrationLimitConfig>(&limit_key)
            {
                if config.enforce && config.max_bps > 0 {
                    let curr_key = DataKey::CurrentConcentration(issuer.clone(), token.clone());
                    let current_bps: u32 = env.storage().persistent().get(&curr_key).unwrap_or(0);
                    if current_bps > config.max_bps {
                        return Err(RevoraError::ConcentrationLimitExceeded);
                    }
                }
            }
        }

        let blacklist = Self::get_blacklist(env.clone(), token.clone());
        let current_timestamp = env.ledger().timestamp();

        let key = DataKey::RevenueReports(issuer.clone(), token.clone());
        let mut reports: Map<u64, (i128, u64)> =
            env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));
        let current_timestamp = env.ledger().timestamp();
        let report_key = DataKey::RevenueReports(issuer.clone(), token.clone());
        let mut reports: Map<u64, (i128, u64)> = env
            .storage()
            .persistent()
            .get(&report_key)
            .unwrap_or_else(|| Map::new(&env));

        let idx_key = DataKey::RevenueIndex(token.clone(), period_id);
        let mut cumulative_revenue: i128 = env.storage().persistent().get(&idx_key).unwrap_or(0);

        match reports.get(period_id) {
            Some((existing_amount, _timestamp)) => {
                if override_existing {
                    // Update index: remove old, add new
                    cumulative_revenue = cumulative_revenue
                        .checked_sub(existing_amount)
                        .unwrap_or(cumulative_revenue)
                        .checked_add(amount)
                        .unwrap_or(amount);
                    env.storage()
                        .persistent()
                        .set(&idx_key, &cumulative_revenue);

                    reports.set(period_id, (amount, current_timestamp));
                    env.storage().persistent().set(&report_key, &reports);

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
        } else {
            // Event-only mode: always treat as initial report (or simply publish the event)
            env.events().publish(
                (EVENT_REVENUE_REPORT_INITIAL, issuer.clone(), token.clone()),
                (amount, period_id, blacklist.clone()),
            );
            None => {
                // Initial report for this period
                cumulative_revenue = cumulative_revenue.checked_add(amount).unwrap_or(amount);
                env.storage()
                    .persistent()
                    .set(&idx_key, &cumulative_revenue);

                reports.set(period_id, (amount, current_timestamp));
                env.storage().persistent().set(&report_key, &reports);

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

        // Audit log summary (#34): maintain per-offering total revenue and report count
        let summary_key = DataKey::AuditSummary(issuer.clone(), token.clone());
        let mut summary: AuditSummary = env
            .storage()
            .persistent()
            .get(&summary_key)
            .unwrap_or(AuditSummary { total_revenue: 0, report_count: 0 });
        summary.total_revenue = summary.total_revenue.saturating_add(amount);
        summary.report_count = summary.report_count.saturating_add(1);
        env.storage().persistent().set(&summary_key, &summary);
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

        if !event_only {
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
        }

        Ok(())
    }

    pub fn get_revenue_by_period(env: Env, token: Address, period_id: u64) -> i128 {
        let key = DataKey::RevenueIndex(token, period_id);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    pub fn get_revenue_range(env: Env, token: Address, from_period: u64, to_period: u64) -> i128 {
        let mut total: i128 = 0;
        for period in from_period..=to_period {
            total += Self::get_revenue_by_period(env.clone(), token.clone(), period);
        }
        total
    }
    /// Return the total number of offerings registered by `issuer`.
    pub fn get_offering_count(env: Env, issuer: Address) -> u32 {
        let count_key = DataKey::OfferCount(issuer);
        env.storage().persistent().get(&count_key).unwrap_or(0)
    }

    /// Return a page of offerings for `issuer`. Limit capped at MAX_PAGE_LIMIT (20).
    /// Ordering: by registration index (creation order), deterministic (#38).
    pub fn get_offerings_page(
        env: Env,
        issuer: Address,
        start: u32,
        limit: u32,
    ) -> (Vec<Offering>, Option<u32>) {
        let count = Self::get_offering_count(env.clone(), issuer.clone());

        let effective_limit =
            if limit == 0 || limit > MAX_PAGE_LIMIT { MAX_PAGE_LIMIT } else { limit };

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

    /// Add an investor to the per-offering blacklist.
    ///
    /// Blacklisted addresses are prohibited from claiming revenue for the specified token.
    /// This operation is idempotent.
    ///
    /// ### Parameters
    /// - `caller`: The address authorized to manage the blacklist. Must provide authentication.
    /// - `token`: The token representing the offering.
    /// - `investor`: The address to be blacklisted.
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
    pub fn blacklist_add(
        env: Env,
        caller: Address,
        token: Address,
        investor: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env);
        caller.require_auth();

        if !Self::is_event_only(&env) {
            let key = DataKey::Blacklist(token.clone());
            let mut map: Map<Address, bool> = env
                .storage()
                .persistent()
                .get(&key)
                .unwrap_or_else(|| Map::new(&env));

            map.set(investor.clone(), true);
            env.storage().persistent().set(&key, &map);
        }
        // Verify auth: caller must be issuer or admin
        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;
        let admin = Self::get_admin(env.clone()).ok_or(RevoraError::NotInitialized)?;

        if caller != current_issuer && caller != admin {
            return Err(RevoraError::NotAuthorized);
        }

        let key = DataKey::Blacklist(token.clone());
        let mut map: Map<Address, bool> =
            env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));

        let was_present = map.get(investor.clone()).unwrap_or(false);
        map.set(investor.clone(), true);
        env.storage().persistent().set(&key, &map);

        env.events().publish((EVENT_BL_ADD, token, caller), investor);
        // Maintain insertion order for deterministic get_blacklist (#38)
        if !was_present {
            let order_key = DataKey::BlacklistOrder(token.clone());
            let mut order: Vec<Address> = env
                .storage()
                .persistent()
                .get(&order_key)
                .unwrap_or_else(|| Vec::new(&env));
            order.push_back(investor.clone());
            env.storage().persistent().set(&order_key, &order);
        }

        env.events()
            .publish((EVENT_BL_ADD, token, caller), investor);
        Ok(())
    }

    /// Remove an investor from the per-offering blacklist.
    ///
    /// Re-enables the address to claim revenue for the specified token.
    /// This operation is idempotent.
    ///
    /// ### Parameters
    /// - `caller`: The address authorized to manage the blacklist. Must provide authentication.
    /// - `token`: The token representing the offering.
    /// - `investor`: The address to be removed from the blacklist.
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
    pub fn blacklist_remove(
        env: Env,
        caller: Address,
        token: Address,
        investor: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env);
        caller.require_auth();

        if !Self::is_event_only(&env) {
            let key = DataKey::Blacklist(token.clone());
            let mut map: Map<Address, bool> = env
                .storage()
                .persistent()
                .get(&key)
                .unwrap_or_else(|| Map::new(&env));
        // Verify auth: caller must be issuer or admin
        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;
        let admin = Self::get_admin(env.clone()).ok_or(RevoraError::NotInitialized)?;

        if caller != current_issuer && caller != admin {
            return Err(RevoraError::NotAuthorized);
        }

        let key = DataKey::Blacklist(token.clone());
        let mut map: Map<Address, bool> =
            env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));

            map.remove(investor.clone());
            env.storage().persistent().set(&key, &map);
        }

        // Rebuild order vec so get_blacklist stays deterministic (#38)
        let order_key = DataKey::BlacklistOrder(token.clone());
        let old_order: Vec<Address> = env
            .storage()
            .persistent()
            .get(&order_key)
            .unwrap_or_else(|| Vec::new(&env));
        let mut new_order = Vec::new(&env);
        for i in 0..old_order.len() {
            let addr = old_order.get(i).unwrap();
            if map.get(addr.clone()).unwrap_or(false) {
                new_order.push_back(addr);
            }
        }
        env.storage().persistent().set(&order_key, &new_order);

        env.events().publish((EVENT_BL_REM, token, caller), investor);
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
    /// Ordering: by insertion order, deterministic and stable across calls (#38).
    pub fn get_blacklist(env: Env, token: Address) -> Vec<Address> {
        let order_key = DataKey::BlacklistOrder(token);
        env.storage()
            .persistent()
            .get::<DataKey, Vec<Address>>(&order_key)
            .unwrap_or_else(|| Vec::new(&env))
    }

    // ── Whitelist management ──────────────────────────────────

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
        issuer.require_auth();
        if Self::get_offering(env.clone(), issuer.clone(), token.clone()).is_none() {
            return Err(RevoraError::LimitReached); // reuse: "offering not found" semantics
        }
        let key = DataKey::ConcentrationLimit(issuer, token);
        env.storage().persistent().set(&key, &ConcentrationLimitConfig { max_bps, enforce });
        Ok(())
    /// Add `investor` to the per-offering whitelist for `token`.
    ///
    /// Idempotent — calling with an already-whitelisted address is safe.
    /// When a whitelist exists (non-empty), only whitelisted addresses
    /// are eligible for revenue distribution (subject to blacklist override).
    pub fn whitelist_add(env: Env, caller: Address, token: Address, investor: Address) {
        caller.require_auth();

        let key = DataKey::Whitelist(token.clone());
        let mut map: Map<Address, bool> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Map::new(&env));

        map.set(investor.clone(), true);
        env.storage().persistent().set(&key, &map);

        env.events().publish((EVENT_WL_ADD, token, caller), investor);
    }

    /// Remove `investor` from the per-offering whitelist for `token`.
    ///
    /// Idempotent — calling when the address is not listed is safe.
    pub fn whitelist_remove(env: Env, caller: Address, token: Address, investor: Address) {
        caller.require_auth();

        let key = DataKey::Whitelist(token.clone());
        let mut map: Map<Address, bool> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Map::new(&env));

        map.remove(investor.clone());
        env.storage().persistent().set(&key, &map);

        env.events().publish((EVENT_WL_REM, token, caller), investor);
    }

    /// Returns `true` if `investor` is whitelisted for `token`'s offering.
    ///
    /// Note: If the whitelist is empty (disabled), this returns `false`.
    /// Use `is_whitelist_enabled` to check if whitelist enforcement is active.
    pub fn is_whitelisted(env: Env, token: Address, investor: Address) -> bool {
        let key = DataKey::Whitelist(token);
        env.storage()
            .persistent()
            .get::<DataKey, Map<Address, bool>>(&key)
            .map(|m| m.get(investor).unwrap_or(false))
            .unwrap_or(false)
    }

    /// Return all whitelisted addresses for `token`'s offering.
    pub fn get_whitelist(env: Env, token: Address) -> Vec<Address> {
        let key = DataKey::Whitelist(token);
        env.storage()
            .persistent()
            .get::<DataKey, Map<Address, bool>>(&key)
            .map(|m| m.keys())
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns `true` if whitelist enforcement is enabled for `token`'s offering.
    ///
    /// Whitelist is considered enabled when it contains at least one address.
    /// When disabled (empty), all non-blacklisted holders are eligible.
    pub fn is_whitelist_enabled(env: Env, token: Address) -> bool {
        Self::get_whitelist(env, token).len() > 0
    // ── Holder concentration guardrail (#26) ───────────────────

    /// Set the concentration limit for an offering.
    ///
    /// Configures the maximum share a single holder can own and whether it is enforced.
    ///
    /// ### Parameters
    /// - `issuer`: The offering issuer. Must provide authentication.
    /// - `token`: The token representing the offering.
    /// - `max_bps`: The maximum allowed single-holder share in basis points (0-10000, 0 = disabled).
    /// - `enforce`: If true, `report_revenue` will fail if current concentration exceeds `max_bps`.
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::LimitReached)` if the offering is not found.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
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

        if !Self::is_event_only(&env) {
            issuer.require_auth();
            let key = DataKey::ConcentrationLimit(issuer, token);
            env.storage()
                .persistent()
                .set(&key, &ConcentrationLimitConfig { max_bps, enforce });
        }
        Ok(())
    }

    /// Report the current top-holder concentration for an offering.
    ///
    /// Stores the provided concentration value. If it exceeds the configured limit,
    /// a `conc_warn` event is emitted. The stored value is used for enforcement in `report_revenue`.
    ///
    /// ### Parameters
    /// - `issuer`: The offering issuer. Must provide authentication.
    /// - `token`: The token representing the offering.
    /// - `concentration_bps`: The current top-holder share in basis points.
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
    pub fn report_concentration(
        env: Env,
        issuer: Address,
        token: Address,
        concentration_bps: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        issuer.require_auth();
        let curr_key = DataKey::CurrentConcentration(issuer.clone(), token.clone());
        env.storage().persistent().set(&curr_key, &concentration_bps);

        // Verify offering exists and issuer is current
        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        if !Self::is_event_only(&env) {
            issuer.require_auth();
            let curr_key = DataKey::CurrentConcentration(issuer.clone(), token.clone());
            env.storage()
                .persistent()
                .set(&curr_key, &concentration_bps);
        }

        let limit_key = DataKey::ConcentrationLimit(issuer.clone(), token.clone());
        if let Some(config) =
            env.storage().persistent().get::<DataKey, ConcentrationLimitConfig>(&limit_key)
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

    /// Set the rounding mode for an offering's share calculations.
    ///
    /// The rounding mode determines how fractional payouts are handled.
    ///
    /// ### Parameters
    /// - `issuer`: The offering issuer. Must provide authentication.
    /// - `token`: The token representing the offering.
    /// - `mode`: The rounding mode to use (`RoundingMode::Truncation` or `RoundingMode::RoundHalfUp`).
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::LimitReached)` if the offering is not found.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
    pub fn set_rounding_mode(
        env: Env,
        issuer: Address,
        token: Address,
        mode: RoundingMode,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        if !Self::is_event_only(&env) {
            // Verify offering exists and issuer is current
            let current_issuer =
                Self::get_current_issuer(&env, &token).ok_or(RevoraError::LimitReached)?;

            if current_issuer != issuer {
                return Err(RevoraError::LimitReached);
            }

            issuer.require_auth();
            let key = DataKey::RoundingMode(issuer, token);
            env.storage().persistent().set(&key, &mode);
        }
        Ok(())
    }

    /// Get rounding mode for an offering. Defaults to Truncation if not set.
    pub fn get_rounding_mode(env: Env, issuer: Address, token: Address) -> RoundingMode {
        let key = DataKey::RoundingMode(issuer, token);
        env.storage().persistent().get(&key).unwrap_or(RoundingMode::Truncation)
    }

    // ── Per-offering minimum revenue threshold (#25) ─────────────────────

    /// Set minimum revenue per period below which no distribution is triggered.
    /// Only the offering issuer may set this. Emits event when configured or changed.
    /// Pass 0 to disable the threshold.
    pub fn set_min_revenue_threshold(
        env: Env,
        issuer: Address,
        token: Address,
        min_amount: i128,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        let current_issuer =
            Self::get_current_issuer(&env, &token).ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        issuer.require_auth();

        Self::require_non_negative_amount(min_amount)?;

        let key = DataKey::MinRevenueThreshold(issuer.clone(), token.clone());
        let previous: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &min_amount);

        env.events().publish(
            (EVENT_MIN_REV_THRESHOLD_SET, issuer, token),
            (previous, min_amount),
        );
        Ok(())
    }

    /// Get minimum revenue threshold for an offering. 0 means no threshold.
    pub fn get_min_revenue_threshold(env: Env, issuer: Address, token: Address) -> i128 {
        let key = DataKey::MinRevenueThreshold(issuer, token);
        env.storage().persistent().get(&key).unwrap_or(0)
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
                let adjusted =
                    if raw >= 0 { raw.saturating_add(half) } else { raw.saturating_sub(half) };
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
    /// The payment token is locked per offering on the first deposit; subsequent
    /// deposits must use the same payment token.
    ///
    /// ### Parameters
    /// - `issuer`: The offering issuer. Must provide authentication.
    /// - `token`: The token representing the offering.
    /// - `payment_token`: The token used to pay out revenue (e.g., XLM or USDC).
    /// - `amount`: Total revenue amount to deposit.
    /// - `period_id`: Unique identifier for the revenue period.
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::OfferingNotFound)` if the offering is not found.
    /// - `Err(RevoraError::PeriodAlreadyDeposited)` if revenue has already been deposited for this `period_id`.
    /// - `Err(RevoraError::PaymentTokenMismatch)` if `payment_token` differs from previously locked token.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
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


        Self::do_deposit_revenue(&env, issuer, token, payment_token, amount, period_id)
    }

        // Verify offering exists
        let offering = Self::get_offering(env.clone(), issuer.clone(), token.clone())
            .ok_or(RevoraError::OfferingNotFound)?;
        if offering.payout_asset != payment_token {
            return Err(RevoraError::PayoutAssetMismatch);
        }


    /// Deposit revenue for an offering using a specific snapshot reference.
    ///
    /// Requires that snapshot distribution is enabled for the offering.
    /// The `snapshot_reference` (e.g., ledger sequence) must be strictly greater than
    /// any previously recorded snapshot for this offering to prevent duplication.
    pub fn deposit_revenue_with_snapshot(
        env: Env,
        issuer: Address,
        token: Address,
        payment_token: Address,
        amount: i128,
        period_id: u64,
        snapshot_reference: u64,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        issuer.require_auth();

        // 1. Verify snapshots are enabled
        if !Self::get_snapshot_config(env.clone(), issuer.clone(), token.clone()) {
            return Err(RevoraError::SnapshotNotEnabled);
        issuer.require_auth();

        Self::require_positive_amount(amount)?;
        Self::require_valid_period_id(period_id)?;

        // Check period not already deposited
        let rev_key = DataKey::PeriodRevenue(token.clone(), period_id);
        if env.storage().persistent().has(&rev_key) {
            return Err(RevoraError::PeriodAlreadyDeposited);
        }

        // 2. Validate snapshot reference (must be strictly monotonic)
        let snap_key = DataKey::LastSnapshotRef(issuer.clone(), token.clone());
        let last_snap: u64 = env.storage().persistent().get(&snap_key).unwrap_or(0);
        if snapshot_reference <= last_snap {
            return Err(RevoraError::OutdatedSnapshot);
        }

        // 3. Delegate to core deposit logic
        Self::do_deposit_revenue(
            &env,
            issuer.clone(),
            token.clone(),
            payment_token.clone(),
            amount,
            period_id,
        )?;

        // 4. Update last snapshot and emit specialized event
        env.storage()
            .persistent()
            .set(&snap_key, &snapshot_reference);
        env.events().publish(
            (EVENT_REV_DEP_SNAP, issuer, token),
            (payment_token, amount, period_id, snapshot_reference),
        );

        Ok(())
    }

    /// Enable or disable snapshot-based distribution for an offering.
    pub fn set_snapshot_config(
        env: Env,
        issuer: Address,
        token: Address,
        enabled: bool,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        issuer.require_auth();
        // Track total deposited revenue per offering (#39)
        let deposited_key = DataKey::DepositedRevenue(token.clone());
        let total_deposited: i128 = env.storage().persistent().get(&deposited_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&deposited_key, &total_deposited.saturating_add(amount));

        // Store deposit timestamp for time-delayed claims (#27)
        let deposit_time = env.ledger().timestamp();
        let time_key = DataKey::PeriodDepositTime(token.clone(), period_id);
        env.storage().persistent().set(&time_key, &deposit_time);

        if Self::get_offering(env.clone(), issuer.clone(), token.clone()).is_none() {
            return Err(RevoraError::OfferingNotFound);
        }

        env.events()
            .publish((EVENT_REV_DEPOSIT, issuer, token), (payment_token, amount, period_id));
        let key = DataKey::SnapshotConfig(issuer.clone(), token.clone());
        env.storage().persistent().set(&key, &enabled);

        env.events()
            .publish((EVENT_SNAP_CONFIG, issuer, token), enabled);
        Ok(())
    }

    /// Check if snapshot-based distribution is enabled for an offering.
    pub fn get_snapshot_config(env: Env, issuer: Address, token: Address) -> bool {
        let key = DataKey::SnapshotConfig(issuer, token);
        env.storage().persistent().get(&key).unwrap_or(false)
    }

    /// Get the latest recorded snapshot reference for an offering.
    pub fn get_last_snapshot_ref(env: Env, issuer: Address, token: Address) -> u64 {
        let key = DataKey::LastSnapshotRef(issuer, token);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    /// Set a holder's revenue share (in basis points) for an offering.
    ///
    /// The share determines the percentage of a period's revenue the holder can claim.
    ///
    /// ### Parameters
    /// - `issuer`: The offering issuer. Must provide authentication.
    /// - `token`: The token representing the offering.
    /// - `holder`: The address of the token holder.
    /// - `share_bps`: The holder's share in basis points (0-10000).
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::OfferingNotFound)` if the offering is not found.
    /// - `Err(RevoraError::InvalidShareBps)` if `share_bps` exceeds 10000.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
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

        env.events().publish((EVENT_SHARE_SET, issuer, token), (holder, share_bps));
        Ok(())
    }

    /// Return a holder's share in basis points for an offering (0 if unset).
    pub fn get_holder_share(env: Env, token: Address, holder: Address) -> u32 {
        let key = DataKey::HolderShare(token, holder);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    /// Claim aggregated revenue across multiple unclaimed periods.
    ///
    /// Payouts are calculated based on the holder's share at the time of claim.
    /// Capped at `MAX_CLAIM_PERIODS` (50) per transaction for gas safety.
    ///
    /// ### Parameters
    /// - `holder`: The address of the token holder. Must provide authentication.
    /// - `token`: The token representing the offering.
    /// - `max_periods`: Maximum number of periods to process (0 = `MAX_CLAIM_PERIODS`).
    ///
    /// ### Returns
    /// - `Ok(i128)` The total payout amount on success.
    /// - `Err(RevoraError::HolderBlacklisted)` if the holder is blacklisted.
    /// - `Err(RevoraError::NoPendingClaims)` if no share is set or all periods are claimed.
    /// - `Err(RevoraError::ClaimDelayNotElapsed)` if the next period is still within the claim delay window.
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

        env.events().publish((EVENT_CLAIM, holder.clone(), token), (total_payout, claimed_periods));

        Ok(total_payout)
    }

    /// Return unclaimed period IDs for a holder on an offering.
    /// Ordering: by deposit index (creation order), deterministic (#38).
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

    /// Preview the total claimable amount for a holder without mutating state.
    ///
    /// This method respects the per-offering claim delay and only sums periods that have passed the delay.
    ///
    /// ### Parameters
    /// - `token`: The token representing the offering.
    /// - `holder`: The address of the token holder.
    ///
    /// ### Returns
    /// The total amount (i128) currently claimable by the holder.
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

    /// Set the claim delay for an offering in seconds.
    ///
    /// The delay starts from the time of deposit and must elapse before a period can be claimed.
    ///
    /// ### Parameters
    /// - `issuer`: The offering issuer. Must provide authentication.
    /// - `token`: The token representing the offering.
    /// - `delay_secs`: Delay in seconds (0 = immediate claim).
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::OfferingNotFound)` if the offering is not found.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
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
        env.events().publish((EVENT_CLAIM_DELAY_SET, issuer, token), delay_secs);
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
        SimulateDistributionResult { total_distributed: total, payouts }
    }

    // ── Upgradeability guard and freeze (#32) ───────────────────

    /// Set the admin address. May only be called once; caller must authorize as the new admin.
    /// If multisig is initialized, this function is disabled in favor of execute_action(SetAdmin).
    pub fn set_admin(env: Env, admin: Address) -> Result<(), RevoraError> {
        if env.storage().persistent().has(&DataKey::MultisigThreshold) {
            return Err(RevoraError::LimitReached);
        }
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
    /// If multisig is initialized, this function is disabled in favor of execute_action(Freeze).
    pub fn freeze(env: Env) -> Result<(), RevoraError> {
        if env.storage().persistent().has(&DataKey::MultisigThreshold) {
            return Err(RevoraError::LimitReached);
        }
        let key = DataKey::Admin;
        let admin: Address =
            env.storage().persistent().get(&key).ok_or(RevoraError::LimitReached)?;
        admin.require_auth();
        let frozen_key = DataKey::Frozen;
        env.storage().persistent().set(&frozen_key, &true);
        env.events().publish((EVENT_FREEZE, admin), true);
        Ok(())
    }

    /// Return true if the contract is frozen.
    pub fn is_frozen(env: Env) -> bool {
        env.storage().persistent().get::<DataKey, bool>(&DataKey::Frozen).unwrap_or(false)
    }

    // ── Multisig admin logic ───────────────────────────────────

    /// Initialize the multisig admin system. May only be called once.
    /// Only the caller (deployer/admin) needs to authorize; owners are registered
    /// without requiring their individual signatures at init time.
    ///
    /// # Soroban Limitation Note
    /// Soroban does not support requiring multiple signers in a single transaction
    /// invocation. Each owner must separately call `approve_action` to sign proposals.
    pub fn init_multisig(
        env: Env,
        caller: Address,
        owners: Vec<Address>,
        threshold: u32,
    ) -> Result<(), RevoraError> {
        caller.require_auth();
        if env.storage().persistent().has(&DataKey::MultisigThreshold) {
            return Err(RevoraError::LimitReached); // Already initialized
        }
        if owners.is_empty() {
            return Err(RevoraError::LimitReached); // Must have at least one owner
        }
        if threshold == 0 || threshold > owners.len() {
            return Err(RevoraError::LimitReached); // Improper threshold
        }
        env.storage().persistent().set(&DataKey::MultisigThreshold, &threshold);
        env.storage().persistent().set(&DataKey::MultisigOwners, &owners);
        env.storage().persistent().set(&DataKey::MultisigProposalCount, &0_u32);
        Ok(())
    }

    /// Propose a sensitive administrative action.
    /// The proposer's address is automatically counted as the first approval.
    pub fn propose_action(
        env: Env,
        proposer: Address,
        action: ProposalAction,
    ) -> Result<u32, RevoraError> {
        proposer.require_auth();
        Self::require_multisig_owner(&env, &proposer)?;

        let count_key = DataKey::MultisigProposalCount;
        let id: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        // Proposer's vote counts as the first approval automatically
        let mut initial_approvals = Vec::new(&env);
        initial_approvals.push_back(proposer.clone());

        let proposal = Proposal {
            id,
            action,
            proposer: proposer.clone(),
            approvals: initial_approvals,
            executed: false,
        };

        env.storage().persistent().set(&DataKey::MultisigProposal(id), &proposal);
        env.storage().persistent().set(&count_key, &(id + 1));

        env.events().publish((EVENT_PROPOSAL_CREATED, proposer.clone()), id);
        env.events().publish((EVENT_PROPOSAL_APPROVED, proposer), id);
        Ok(id)
    }

    /// Approve an existing multisig proposal.
    pub fn approve_action(
        env: Env,
        approver: Address,
        proposal_id: u32,
    ) -> Result<(), RevoraError> {
        approver.require_auth();
        Self::require_multisig_owner(&env, &approver)?;

        let key = DataKey::MultisigProposal(proposal_id);
        let mut proposal: Proposal =
            env.storage().persistent().get(&key).ok_or(RevoraError::OfferingNotFound)?;

        if proposal.executed {
            return Err(RevoraError::LimitReached);
        }

        // Check for duplicate approvals
        for i in 0..proposal.approvals.len() {
            if proposal.approvals.get(i).unwrap() == approver {
                return Ok(()); // Already approved
            }
        }

        proposal.approvals.push_back(approver.clone());
        env.storage().persistent().set(&key, &proposal);

        env.events().publish((EVENT_PROPOSAL_APPROVED, approver), proposal_id);
        Ok(())
    }

    /// Execute a proposal if it has met the required threshold.
    pub fn execute_action(env: Env, proposal_id: u32) -> Result<(), RevoraError> {
        let key = DataKey::MultisigProposal(proposal_id);
        let mut proposal: Proposal =
            env.storage().persistent().get(&key).ok_or(RevoraError::OfferingNotFound)?;

        if proposal.executed {
            return Err(RevoraError::LimitReached);
        }

        let threshold: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::MultisigThreshold)
            .ok_or(RevoraError::LimitReached)?;

        if proposal.approvals.len() < threshold {
            return Err(RevoraError::LimitReached); // Threshold not met
        }

        // Execute the action
        match proposal.action.clone() {
            ProposalAction::SetAdmin(new_admin) => {
                env.storage().persistent().set(&DataKey::Admin, &new_admin);
            }
            ProposalAction::Freeze => {
                env.storage().persistent().set(&DataKey::Frozen, &true);
                env.events().publish((EVENT_FREEZE, proposal.proposer.clone()), true);
            }
            ProposalAction::SetThreshold(new_threshold) => {
                let owners: Vec<Address> =
                    env.storage().persistent().get(&DataKey::MultisigOwners).unwrap();
                if new_threshold == 0 || new_threshold > owners.len() {
                    return Err(RevoraError::InvalidShareBps);
                }
                env.storage().persistent().set(&DataKey::MultisigThreshold, &new_threshold);
            }
            ProposalAction::AddOwner(new_owner) => {
                let mut owners: Vec<Address> =
                    env.storage().persistent().get(&DataKey::MultisigOwners).unwrap();
                owners.push_back(new_owner);
                env.storage().persistent().set(&DataKey::MultisigOwners, &owners);
            }
            ProposalAction::RemoveOwner(old_owner) => {
                let owners: Vec<Address> =
                    env.storage().persistent().get(&DataKey::MultisigOwners).unwrap();
                let mut new_owners = Vec::new(&env);
                for i in 0..owners.len() {
                    let owner = owners.get(i).unwrap();
                    if owner != old_owner {
                        new_owners.push_back(owner);
                    }
                }
                let threshold: u32 =
                    env.storage().persistent().get(&DataKey::MultisigThreshold).unwrap();
                if new_owners.len() < threshold || new_owners.is_empty() {
                    return Err(RevoraError::LimitReached); // Would break threshold
                }
                env.storage().persistent().set(&DataKey::MultisigOwners, &new_owners);
            }
        }

        proposal.executed = true;
        env.storage().persistent().set(&key, &proposal);

        env.events().publish((EVENT_PROPOSAL_EXECUTED, proposal_id), true);
        Ok(())
    }

    /// Get a proposal by ID. Returns None if not found.
    pub fn get_proposal(env: Env, proposal_id: u32) -> Option<Proposal> {
        env.storage().persistent().get(&DataKey::MultisigProposal(proposal_id))
    }

    /// Get the current multisig owners list.
    pub fn get_multisig_owners(env: Env) -> Vec<Address> {
        env.storage().persistent().get(&DataKey::MultisigOwners).unwrap_or_else(|| Vec::new(&env))
    }

    /// Get the current multisig threshold.
    pub fn get_multisig_threshold(env: Env) -> Option<u32> {
        env.storage().persistent().get(&DataKey::MultisigThreshold)
    }

    fn require_multisig_owner(env: &Env, caller: &Address) -> Result<(), RevoraError> {
        let owners: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::MultisigOwners)
            .ok_or(RevoraError::LimitReached)?;
        for i in 0..owners.len() {
            if owners.get(i).unwrap() == *caller {
                return Ok(());
            }
        }
        Err(RevoraError::LimitReached)
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
    // This entrypoint shape is part of the public contract interface and mirrors
    // off-chain inputs directly, so we allow this specific arity.
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
        let admin: Address =
            env.storage().persistent().get(&key).ok_or(RevoraError::LimitReached)?;
        admin.require_auth();
        if !Self::is_event_only(&env) {
            let mode_key = DataKey::TestnetMode;
            env.storage().persistent().set(&mode_key, &enabled);
        }
        env.events().publish((EVENT_TESTNET_MODE, admin), enabled);
        Ok(())
    }

    /// Return true if testnet mode is enabled.
    pub fn is_testnet_mode(env: Env) -> bool {
        env.storage().persistent().get::<DataKey, bool>(&DataKey::TestnetMode).unwrap_or(false)
    }

    // ── Cross-offering aggregation queries (#39) ──────────────────

    /// Maximum number of issuers to iterate for platform-wide aggregation.
    const MAX_AGGREGATION_ISSUERS: u32 = 50;

    /// Aggregate metrics across all offerings for a single issuer.
    /// Iterates the issuer's offerings and sums audit summary and deposited revenue data.
    pub fn get_issuer_aggregation(env: Env, issuer: Address) -> AggregatedMetrics {
        let count = Self::get_offering_count(env.clone(), issuer.clone());
        let mut total_reported: i128 = 0;
        let mut total_deposited: i128 = 0;
        let mut total_reports: u64 = 0;

        for i in 0..count {
            let item_key = DataKey::OfferItem(issuer.clone(), i);
            let offering: Offering = env.storage().persistent().get(&item_key).unwrap();

            // Sum audit summary (reported revenue)
            let summary_key = DataKey::AuditSummary(issuer.clone(), offering.token.clone());
            if let Some(summary) = env
                .storage()
                .persistent()
                .get::<DataKey, AuditSummary>(&summary_key)
            {
                total_reported = total_reported.saturating_add(summary.total_revenue);
                total_reports = total_reports.saturating_add(summary.report_count);
            }

            // Sum deposited revenue
            let deposited_key = DataKey::DepositedRevenue(offering.token.clone());
            let deposited: i128 = env.storage().persistent().get(&deposited_key).unwrap_or(0);
            total_deposited = total_deposited.saturating_add(deposited);
        }

        AggregatedMetrics {
            total_reported_revenue: total_reported,
            total_deposited_revenue: total_deposited,
            total_report_count: total_reports,
            offering_count: count,
        }
    }

    /// Aggregate metrics across all issuers (platform-wide).
    /// Iterates the global issuer registry, capped at MAX_AGGREGATION_ISSUERS for gas safety.
    pub fn get_platform_aggregation(env: Env) -> AggregatedMetrics {
        let issuer_count_key = DataKey::IssuerCount;
        let issuer_count: u32 = env
            .storage()
            .persistent()
            .get(&issuer_count_key)
            .unwrap_or(0);

        let cap = core::cmp::min(issuer_count, Self::MAX_AGGREGATION_ISSUERS);

        let mut total_reported: i128 = 0;
        let mut total_deposited: i128 = 0;
        let mut total_reports: u64 = 0;
        let mut total_offerings: u32 = 0;

        for i in 0..cap {
            let issuer_item_key = DataKey::IssuerItem(i);
            let issuer: Address = env.storage().persistent().get(&issuer_item_key).unwrap();

            let metrics = Self::get_issuer_aggregation(env.clone(), issuer);
            total_reported = total_reported.saturating_add(metrics.total_reported_revenue);
            total_deposited = total_deposited.saturating_add(metrics.total_deposited_revenue);
            total_reports = total_reports.saturating_add(metrics.total_report_count);
            total_offerings = total_offerings.saturating_add(metrics.offering_count);
        }

        AggregatedMetrics {
            total_reported_revenue: total_reported,
            total_deposited_revenue: total_deposited,
            total_report_count: total_reports,
            offering_count: total_offerings,
        }
    }

    /// Return all registered issuer addresses (up to MAX_AGGREGATION_ISSUERS).
    pub fn get_all_issuers(env: Env) -> Vec<Address> {
        let issuer_count_key = DataKey::IssuerCount;
        let issuer_count: u32 = env
            .storage()
            .persistent()
            .get(&issuer_count_key)
            .unwrap_or(0);

        let cap = core::cmp::min(issuer_count, Self::MAX_AGGREGATION_ISSUERS);
        let mut issuers = Vec::new(&env);

        for i in 0..cap {
            let issuer_item_key = DataKey::IssuerItem(i);
            let issuer: Address = env.storage().persistent().get(&issuer_item_key).unwrap();
            issuers.push_back(issuer);
        }
        issuers
    }

    /// Return the total deposited revenue for a specific offering token.
    pub fn get_total_deposited_revenue(env: Env, token: Address) -> i128 {
        let key = DataKey::DepositedRevenue(token);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    // ── Platform fee configuration (#6) ────────────────────────

    /// Set the platform fee in basis points.  Admin-only.
    /// Maximum value is 5 000 bps (50 %).  Pass 0 to disable.
    pub fn set_platform_fee(env: Env, fee_bps: u32) -> Result<(), RevoraError> {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .ok_or(RevoraError::LimitReached)?;
        admin.require_auth();

        if fee_bps > MAX_PLATFORM_FEE_BPS {
            return Err(RevoraError::LimitReached);
        }

        env.storage()
            .persistent()
            .set(&DataKey::PlatformFeeBps, &fee_bps);
        Ok(())
    }

    /// Return the current platform fee in basis points (default 0).
    pub fn get_platform_fee(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::PlatformFeeBps)
            .unwrap_or(0)
    }

    /// Calculate the platform fee for a given amount.
    pub fn calculate_platform_fee(env: Env, amount: i128) -> i128 {
        let fee_bps = Self::get_platform_fee(env) as i128;
        (amount * fee_bps).checked_div(BPS_DENOMINATOR).unwrap_or(0)
    }

    /// Return the current contract version (#23). Used for upgrade compatibility and migration.
    pub fn get_version(env: Env) -> u32 {
        let _ = env;
        CONTRACT_VERSION
    }
}

mod test;
mod test_auth;
mod test_cross_contract;
