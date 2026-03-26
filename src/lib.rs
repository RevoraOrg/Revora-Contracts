#![no_std]
#![deny(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::todo, clippy::unimplemented)]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, xdr::ToXdr, Address,
    BytesN, Env, Map, String, Symbol, Vec, I256,
};

// Issue #109 — Revenue report correction workflow with audit trail.
// Placeholder branch for upstream PR scaffolding; full implementation in follow-up.

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
    PayoutAssetMismatch = 14,
    /// A transfer is already pending for this offering.
    IssuerTransferPending = 15,
    /// No transfer is pending for this offering.
    NoTransferPending = 16,
    /// Caller is not authorized to accept this transfer.
    UnauthorizedTransferAccept = 17,
    /// Metadata string exceeds maximum allowed length.
    MetadataTooLarge = 18,
    /// Caller is not authorized to perform this action.
    NotAuthorized = 19,
    /// Contract is not initialized (admin not set).
    NotInitialized = 20,
    /// Amount is invalid (e.g. negative for deposit, or out of allowed range) (#35).
    InvalidAmount = 21,
    /// period_id is invalid (e.g. zero when required to be positive) (#35).
    InvalidPeriodId = 22,
    /// Deposit would exceed the offering's supply cap (#96).
    SupplyCapExceeded = 23,
    /// Metadata format is invalid for configured scheme rules.
    MetadataInvalidFormat = 24,
    /// Current ledger timestamp is outside configured reporting window.
    ReportingWindowClosed = 25,
    /// Current ledger timestamp is outside configured claiming window.
    ClaimWindowClosed = 26,
    /// Off-chain signature has expired.
    SignatureExpired = 27,
    /// Signature nonce has already been used.
    SignatureReplay = 28,
    /// Off-chain signer key has not been registered.
    SignerKeyNotRegistered = 29,
    /// Contract is paused; state-mutating operations are temporarily disabled.
    ContractPaused = 30,
}

// ── Event symbols ────────────────────────────────────────────
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_BL_ADD: Symbol = symbol_short!("bl_add");
const EVENT_BL_REM: Symbol = symbol_short!("bl_rem");
const EVENT_WL_ADD: Symbol = symbol_short!("wl_add");
const EVENT_WL_REM: Symbol = symbol_short!("wl_rem");

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
const EVENT_REVENUE_REPORTED_ASSET: Symbol = symbol_short!("rev_repa");
const EVENT_REVENUE_REPORT_INITIAL: Symbol = symbol_short!("rev_init");
const EVENT_REVENUE_REPORT_INITIAL_ASSET: Symbol = symbol_short!("rev_inia");
const EVENT_REVENUE_REPORT_OVERRIDE: Symbol = symbol_short!("rev_ovrd");
const EVENT_REVENUE_REPORT_OVERRIDE_ASSET: Symbol = symbol_short!("rev_ovra");
const EVENT_REVENUE_REPORT_REJECTED: Symbol = symbol_short!("rev_rej");
const EVENT_REVENUE_REPORT_REJECTED_ASSET: Symbol = symbol_short!("rev_reja");
// Versioned event symbols (v1). We emit legacy events for compatibility
// and also emit explicit v1 events that include a leading `version` field.
const EVENT_OFFER_REG_V1: Symbol = symbol_short!("ofr_reg1");
const EVENT_REV_INIT_V1: Symbol = symbol_short!("rv_init1");
const EVENT_REV_INIA_V1: Symbol = symbol_short!("rv_inia1");
const EVENT_REV_REP_V1: Symbol = symbol_short!("rv_rep1");
const EVENT_REV_REPA_V1: Symbol = symbol_short!("rv_repa1");

const EVENT_SCHEMA_VERSION: u32 = 1;
const EVENT_CONCENTRATION_WARNING: Symbol = symbol_short!("conc_warn");
const EVENT_CONCENTRATION_REPORTED: Symbol = symbol_short!("conc_rep");
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

const EVENT_DIST_CALC: Symbol = symbol_short!("dist_calc");
const EVENT_METADATA_SET: Symbol = symbol_short!("meta_set");
const EVENT_METADATA_UPDATED: Symbol = symbol_short!("meta_upd");
/// Emitted when per-offering minimum revenue threshold is set or changed (#25).
const EVENT_MIN_REV_THRESHOLD_SET: Symbol = symbol_short!("min_rev");
/// Emitted when reported revenue is below the offering's minimum threshold; no distribution triggered (#25).
#[allow(dead_code)]
const EVENT_REV_BELOW_THRESHOLD: Symbol = symbol_short!("rev_below");
/// Emitted when an offering's supply cap is reached (#96).
const EVENT_SUPPLY_CAP_REACHED: Symbol = symbol_short!("cap_reach");
/// Emitted when per-offering investment constraints are set or updated (#97).
const EVENT_INV_CONSTRAINTS: Symbol = symbol_short!("inv_cfg");
/// Emitted when per-offering or platform per-asset fee is set (#98).
const EVENT_FEE_CONFIG: Symbol = symbol_short!("fee_cfg");
const EVENT_INDEXED_V2: Symbol = symbol_short!("ev_idx2");
const EVENT_TYPE_OFFER: Symbol = symbol_short!("offer");
const EVENT_TYPE_REV_INIT: Symbol = symbol_short!("rv_init");
const EVENT_TYPE_REV_OVR: Symbol = symbol_short!("rv_ovr");
const EVENT_TYPE_REV_REJ: Symbol = symbol_short!("rv_rej");
const EVENT_TYPE_REV_REP: Symbol = symbol_short!("rv_rep");
const EVENT_TYPE_CLAIM: Symbol = symbol_short!("claim");
const EVENT_REPORT_WINDOW_SET: Symbol = symbol_short!("rep_win");
const EVENT_CLAIM_WINDOW_SET: Symbol = symbol_short!("clm_win");
const EVENT_META_SIGNER_SET: Symbol = symbol_short!("meta_key");
const EVENT_META_DELEGATE_SET: Symbol = symbol_short!("meta_del");
const EVENT_META_SHARE_SET: Symbol = symbol_short!("meta_shr");
const EVENT_META_REV_APPROVE: Symbol = symbol_short!("meta_rev");

const BPS_DENOMINATOR: i128 = 10_000;

/// Represents a revenue-share offering registered on-chain.
/// Offerings are immutable once registered.
// ── Data structures ──────────────────────────────────────────
/// Contract version identifier (#23). Bumped when storage or semantics change; used for migration and compatibility.
pub const CONTRACT_VERSION: u32 = 2;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TenantId {
    pub issuer: Address,
    pub namespace: Symbol,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct OfferingId {
    pub issuer: Address,
    pub namespace: Symbol,
    pub token: Address,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Offering {
    /// The address authorized to manage this offering.
    pub issuer: Address,
    /// The namespace this offering belongs to.
    pub namespace: Symbol,
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

/// Per-offering investment constraints (#97). Min/max stake per investor; off-chain enforced.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InvestmentConstraintsConfig {
    pub min_stake: i128,
    pub max_stake: i128,
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

/// Versioned structured topic payload for indexers.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct EventIndexTopicV2 {
    pub version: u32,
    pub event_type: Symbol,
    pub issuer: Address,
    pub namespace: Symbol,
    pub token: Address,
    /// 0 when the event is not period-scoped.
    pub period_id: u64,
}

/// Versioned domain-separated payload for off-chain authorized actions.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MetaAuthorization {
    pub version: u32,
    pub contract: Address,
    pub signer: Address,
    pub nonce: u64,
    pub expiry: u64,
    pub action: MetaAction,
}

/// Off-chain authorized action variants.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum MetaAction {
    SetHolderShare(MetaSetHolderSharePayload),
    ApproveRevenueReport(MetaRevenueApprovalPayload),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MetaSetHolderSharePayload {
    pub issuer: Address,
    pub namespace: Symbol,
    pub token: Address,
    pub holder: Address,
    pub share_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MetaRevenueApprovalPayload {
    pub issuer: Address,
    pub namespace: Symbol,
    pub token: Address,
    pub payout_asset: Address,
    pub amount: i128,
    pub period_id: u64,
    pub override_existing: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AccessWindow {
    pub start_timestamp: u64,
    pub end_timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum WindowDataKey {
    Report(OfferingId),
    Claim(OfferingId),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum MetaDataKey {
    /// Off-chain signer public key (ed25519) bound to signer address.
    SignerKey(Address),
    /// Offering-scoped delegate signer allowed for meta-actions.
    Delegate(OfferingId),
    /// Replay protection key: signer + nonce consumed marker.
    NonceUsed(Address, u64),
    /// Approved revenue report marker keyed by offering and period.
    RevenueApproved(OfferingId, u64),
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
    Blacklist(OfferingId),
    /// Per-offering whitelist; when non-empty, only these addresses are eligible for distribution.
    Whitelist(OfferingId),
    /// Per-offering: blacklist addresses in insertion order for deterministic get_blacklist (#38).
    BlacklistOrder(OfferingId),
    OfferCount(TenantId),
    OfferItem(TenantId, u32),
    /// Per-offering concentration limit config.
    ConcentrationLimit(OfferingId),
    /// Per-offering: last reported concentration in bps.
    CurrentConcentration(OfferingId),
    /// Per-offering: audit summary.
    AuditSummary(OfferingId),
    /// Per-offering: rounding mode for share math.
    RoundingMode(OfferingId),
    /// Per-offering: revenue reports map (period_id -> (amount, timestamp)).
    RevenueReports(OfferingId),
    /// Per-offering per period: cumulative reported revenue amount.
    RevenueIndex(OfferingId, u64),
    /// Revenue amount deposited for (offering_id, period_id).
    PeriodRevenue(OfferingId, u64),
    /// Maps (offering_id, sequential_index) -> period_id for enumeration.
    PeriodEntry(OfferingId, u32),
    /// Total number of deposited periods for an offering.
    PeriodCount(OfferingId),
    /// Holder's share in basis points for (offering_id, holder).
    HolderShare(OfferingId, Address),
    /// Next period index to claim for (offering_id, holder).
    LastClaimedIdx(OfferingId, Address),
    /// Payment token address for an offering.
    PaymentToken(OfferingId),
    /// Per-offering claim delay in seconds (#27). 0 = immediate claim.
    ClaimDelaySecs(OfferingId),
    /// Ledger timestamp when revenue was deposited for (offering_id, period_id).
    PeriodDepositTime(OfferingId, u64),
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

    /// Whether snapshot distribution is enabled for an offering.
    SnapshotConfig(OfferingId),
    /// Latest recorded snapshot reference for an offering.
    LastSnapshotRef(OfferingId),

    /// Pending issuer transfer for an offering: OfferingId -> new_issuer.
    PendingIssuerTransfer(OfferingId),
    /// Current issuer lookup by offering token: OfferingId -> issuer.
    OfferingIssuer(OfferingId),
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

    /// Metadata reference (IPFS hash, HTTPS URI, etc.) for an offering.
    OfferingMetadata(OfferingId),
    /// Platform fee in basis points (max 5000 = 50%) taken from reported revenue (#6).
    PlatformFeeBps,
    /// Per-offering per-asset fee override (#98).
    OfferingFeeBps(OfferingId, Address),
    /// Platform level per-asset fee (#98).
    PlatformFeePerAsset(Address),

    /// Per-offering minimum revenue threshold below which no distribution is triggered (#25).
    MinRevenueThreshold(OfferingId),
    /// Global count of unique issuers (#39).
    IssuerCount,
    /// Issuer address at global index (#39).
    IssuerItem(u32),
    /// Whether an issuer is already registered in the global registry (#39).
    IssuerRegistered(Address),
    /// Total deposited revenue for an offering (#39).
    DepositedRevenue(OfferingId),
    /// Per-offering supply cap (#96). 0 = no cap.
    SupplyCap(OfferingId),
    /// Per-offering investment constraints: min and max stake per investor (#97).
    InvestmentConstraints(OfferingId),

    /// Per-issuer namespace tracking
    NamespaceCount(Address),
    NamespaceItem(Address, u32),
    NamespaceRegistered(Address, Symbol),
}

/// Maximum number of offerings returned in a single page.
const MAX_PAGE_LIMIT: u32 = 20;

/// Maximum platform fee in basis points (50%).
const MAX_PLATFORM_FEE_BPS: u32 = 5_000;

/// Maximum number of periods that can be claimed in a single transaction.
/// Keeps compute costs predictable within Soroban limits.
const MAX_CLAIM_PERIODS: u32 = 50;

/// Maximum number of periods allowed in a single read-only chunked query.
/// This is a safety cap to prevent accidental long-running loops in read-only methods.
const MAX_CHUNK_PERIODS: u32 = 200;

// ── Contract ─────────────────────────────────────────────────
#[contract]
pub struct RevoraRevenueShare;

#[contractimpl]
impl RevoraRevenueShare {
    const META_AUTH_VERSION: u32 = 1;

    fn is_event_versioning_enabled(env: Env) -> bool {
        let key = DataKey::EventVersioningEnabled;
        env.storage().persistent().get::<DataKey, bool>(&key).unwrap_or(false)
    }

    /// Returns error if contract is frozen (#32). Call at start of state-mutating entrypoints.
    fn require_not_frozen(env: &Env) -> Result<(), RevoraError> {
        let key = DataKey::Frozen;
        if env.storage().persistent().get::<DataKey, bool>(&key).unwrap_or(false) {
            return Err(RevoraError::ContractFrozen);
        }
        Ok(())
    }

    fn validate_window(window: &AccessWindow) -> Result<(), RevoraError> {
        if window.start_timestamp > window.end_timestamp {
            return Err(RevoraError::LimitReached);
        }
        Ok(())
    }

    fn require_valid_meta_nonce_and_expiry(
        env: &Env,
        signer: &Address,
        nonce: u64,
        expiry: u64,
    ) -> Result<(), RevoraError> {
        if env.ledger().timestamp() > expiry {
            return Err(RevoraError::SignatureExpired);
        }
        let nonce_key = MetaDataKey::NonceUsed(signer.clone(), nonce);
        if env.storage().persistent().has(&nonce_key) {
            return Err(RevoraError::SignatureReplay);
        }
        Ok(())
    }

    fn is_window_open(env: &Env, window: &AccessWindow) -> bool {
        let now = env.ledger().timestamp();
        now >= window.start_timestamp && now <= window.end_timestamp
    }

    fn require_report_window_open(env: &Env, offering_id: &OfferingId) -> Result<(), RevoraError> {
        let key = WindowDataKey::Report(offering_id.clone());
        if let Some(window) = env.storage().persistent().get::<WindowDataKey, AccessWindow>(&key) {
            if !Self::is_window_open(env, &window) {
                return Err(RevoraError::ReportingWindowClosed);
            }
        }
        Ok(())
    }

    fn require_claim_window_open(env: &Env, offering_id: &OfferingId) -> Result<(), RevoraError> {
        let key = WindowDataKey::Claim(offering_id.clone());
        if let Some(window) = env.storage().persistent().get::<WindowDataKey, AccessWindow>(&key) {
            if !Self::is_window_open(env, &window) {
                return Err(RevoraError::ClaimWindowClosed);
            }
        }
        Ok(())
    }

    fn mark_meta_nonce_used(env: &Env, signer: &Address, nonce: u64) {
        let nonce_key = MetaDataKey::NonceUsed(signer.clone(), nonce);
        env.storage().persistent().set(&nonce_key, &true);
    }

    fn verify_meta_signature(
        env: &Env,
        signer: &Address,
        nonce: u64,
        expiry: u64,
        action: MetaAction,
        signature: &BytesN<64>,
    ) -> Result<(), RevoraError> {
        Self::require_valid_meta_nonce_and_expiry(env, signer, nonce, expiry)?;
        let pk_key = MetaDataKey::SignerKey(signer.clone());
        let public_key: BytesN<32> = env
            .storage()
            .persistent()
            .get(&pk_key)
            .ok_or(RevoraError::SignerKeyNotRegistered)?;
        let payload = MetaAuthorization {
            version: Self::META_AUTH_VERSION,
            contract: env.current_contract_address(),
            signer: signer.clone(),
            nonce,
            expiry,
            action,
        };
        let payload_bytes = payload.to_xdr(env);
        env.crypto().ed25519_verify(&public_key, &payload_bytes, signature);
        Ok(())
    }

    fn set_holder_share_internal(
        env: &Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        share_bps: u32,
    ) -> Result<(), RevoraError> {
        if share_bps > 10_000 {
            return Err(RevoraError::InvalidShareBps);
        }
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::HolderShare(offering_id, holder.clone()), &share_bps);
        env.events()
            .publish((EVENT_SHARE_SET, issuer, namespace, token), (holder, share_bps));
        Ok(())
    }

 
    /// Internal helper for revenue deposits.
    fn do_deposit_revenue(
        env: &Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        payment_token: Address,
        amount: i128,
        period_id: u64,
    ) -> Result<(), RevoraError> {
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Verify offering exists
        if Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
            .is_none()
        {
            return Err(RevoraError::OfferingNotFound);
        }

        // Check minimum revenue threshold (#25)
        let threshold_key = DataKey::MinRevenueThreshold(offering_id.clone());
        let threshold: i128 = env.storage().persistent().get(&threshold_key).unwrap_or(0);
        if amount < threshold {
            return Err(RevoraError::InvalidAmount);
        }

        // Check period not already deposited
        let rev_key = DataKey::PeriodRevenue(offering_id.clone(), period_id);
        if env.storage().persistent().has(&rev_key) {
            return Err(RevoraError::PeriodAlreadyDeposited);
        }

        // Supply cap check (#96): reject if deposit would exceed cap
        let cap_key = DataKey::SupplyCap(offering_id.clone());
        let cap: i128 = env.storage().persistent().get(&cap_key).unwrap_or(0);
        if cap > 0 {
            let deposited_key = DataKey::DepositedRevenue(offering_id.clone());
            let deposited: i128 = env.storage().persistent().get(&deposited_key).unwrap_or(0);
            let new_total = deposited.saturating_add(amount);
            if new_total > cap {
                return Err(RevoraError::SupplyCapExceeded);
            }
        }

        // Store or validate payment token for this offering
        let pt_key = DataKey::PaymentToken(offering_id.clone());
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
        let time_key = DataKey::PeriodDepositTime(offering_id.clone(), period_id);
        env.storage().persistent().set(&time_key, &deposit_time);

        // Append to indexed period list
        let count_key = DataKey::PeriodCount(offering_id.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        let entry_key = DataKey::PeriodEntry(offering_id.clone(), count);
        env.storage().persistent().set(&entry_key, &period_id);
        env.storage().persistent().set(&count_key, &(count + 1));

        // Update cumulative deposited revenue and emit cap-reached event if applicable (#96)
        let deposited_key = DataKey::DepositedRevenue(offering_id.clone());
        let deposited: i128 = env.storage().persistent().get(&deposited_key).unwrap_or(0);
        let new_deposited = deposited.saturating_add(amount);
        env.storage().persistent().set(&deposited_key, &new_deposited);

        let cap_val: i128 = env.storage().persistent().get(&cap_key).unwrap_or(0);
        if cap_val > 0 && new_deposited >= cap_val {
            env.events().publish(
                (EVENT_SUPPLY_CAP_REACHED, issuer.clone(), namespace.clone(), token.clone()),
                (new_deposited, cap_val),
            );
        }

        env.events().publish(
            (EVENT_REV_DEPOSIT, issuer, namespace, token),
            (payment_token, amount, period_id),
        );
        Ok(())
    }

    /// Return the supply cap for an offering (0 = no cap). (#96)
    pub fn get_supply_cap(env: Env, issuer: Address, namespace: Symbol, token: Address) -> i128 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey::SupplyCap(offering_id)).unwrap_or(0)
    }

    /// Return true if the contract is in event-only mode.
    pub fn is_event_only(env: &Env) -> bool {
        env.storage().persistent().get::<DataKey, bool>(&DataKey::EventOnlyMode).unwrap_or(false)
    }

    /// Input validation (#35): require amount > 0 for transfers/deposits.
    #[allow(dead_code)]
    fn require_positive_amount(amount: i128) -> Result<(), RevoraError> {
        if amount <= 0 {
            return Err(RevoraError::InvalidAmount);
        }
        Ok(())
    }

    /// Input validation (#35): require period_id > 0 where 0 would be ambiguous.
    #[allow(dead_code)]
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
    fn get_current_issuer(
        env: &Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<Address> {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::OfferingIssuer(offering_id);
        env.storage().persistent().get(&key)
    }

    /// Initialize admin and optional safety role for emergency pause (#7).
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

    /// Helper: result Err if contract is paused. Used by state-mutating entrypoints.
    fn require_not_paused(env: &Env) -> Result<(), RevoraError> {
        if env.storage().persistent().get::<DataKey, bool>(&DataKey::Paused).unwrap_or(false) {
            return Err(RevoraError::ContractPaused);
        }
        Ok(())
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
    ///
    /// Returns `Err(RevoraError::InvalidRevenueShareBps)` if revenue_share_bps > 10000.
    /// In testnet mode, bps validation is skipped to allow flexible testing.
    ///
    /// Register a new offering. `supply_cap`: max cumulative deposited revenue for this offering; 0 = no cap (#96).
    pub fn register_offering(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        revenue_share_bps: u32,
        payout_asset: Address,
        supply_cap: i128,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

        // Skip bps validation in testnet mode
        let testnet_mode = Self::is_testnet_mode(env.clone());
        if !testnet_mode && revenue_share_bps > 10_000 {
            return Err(RevoraError::InvalidRevenueShareBps);
        }

        if !Self::is_event_only(&env) {
            // Track issuer in global registry for cross-offering aggregation (#39)
            let registered_key = DataKey::IssuerRegistered(issuer.clone());
            if !env.storage().persistent().has(&registered_key) {
                let issuer_count_key = DataKey::IssuerCount;
                let issuer_count: u32 =
                    env.storage().persistent().get(&issuer_count_key).unwrap_or(0);
                let issuer_item_key = DataKey::IssuerItem(issuer_count);
                env.storage().persistent().set(&issuer_item_key, &issuer.clone());
                env.storage().persistent().set(&issuer_count_key, &(issuer_count + 1));
                env.storage().persistent().set(&registered_key, &true);
            }

            // Register namespace for issuer if not already present
            let ns_reg_key = DataKey::NamespaceRegistered(issuer.clone(), namespace.clone());
            if !env.storage().persistent().has(&ns_reg_key) {
                let ns_count_key = DataKey::NamespaceCount(issuer.clone());
                let count: u32 = env.storage().persistent().get(&ns_count_key).unwrap_or(0);
                env.storage()
                    .persistent()
                    .set(&DataKey::NamespaceItem(issuer.clone(), count), &namespace);
                env.storage().persistent().set(&ns_count_key, &(count + 1));
                env.storage().persistent().set(&ns_reg_key, &true);
            }

            let tenant_id = TenantId { issuer: issuer.clone(), namespace: namespace.clone() };
            let count_key = DataKey::OfferCount(tenant_id.clone());
            let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

            let offering = Offering {
                issuer: issuer.clone(),
                namespace: namespace.clone(),
                token: token.clone(),
                revenue_share_bps,
                payout_asset: payout_asset.clone(),
            };

            let item_key = DataKey::OfferItem(tenant_id.clone(), count);
            env.storage().persistent().set(&item_key, &offering);
            env.storage().persistent().set(&count_key, &(count + 1));

            // Maintain reverse lookup: id -> current_issuer
            let offering_id = OfferingId {
                issuer: issuer.clone(),
                namespace: namespace.clone(),
                token: token.clone(),
            };
            let issuer_lookup_key = DataKey::OfferingIssuer(offering_id.clone());
            env.storage().persistent().set(&issuer_lookup_key, &issuer);

            if supply_cap > 0 {
                let cap_key = DataKey::SupplyCap(offering_id);
                env.storage().persistent().set(&cap_key, &supply_cap);
            }
        }

        env.events().publish(
            (symbol_short!("offer_reg"), issuer.clone(), namespace.clone()),
            (token.clone(), revenue_share_bps, payout_asset.clone()),
        );
        env.events().publish(
            (
                EVENT_INDEXED_V2,
                EventIndexTopicV2 {
                    version: 2,
                    event_type: EVENT_TYPE_OFFER,
                    issuer: issuer.clone(),
                    namespace: namespace.clone(),
                    token: token.clone(),
                    period_id: 0,
                },
            ),
            (revenue_share_bps, payout_asset.clone()),
        );

        // Optionally emit a versioned v1 event with explicit version field
        if Self::is_event_versioning_enabled(env.clone()) {
            env.events().publish(
                (EVENT_OFFER_REG_V1, issuer.clone(), namespace.clone()),
                (EVENT_SCHEMA_VERSION, token.clone(), revenue_share_bps, payout_asset.clone()),
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
    /// Fetch a single offering by issuer, namespace, and token.
    ///
    /// This method scans the registered offerings in the namespace to find the one matching the given token.
    ///
    /// ### Parameters
    /// - `issuer`: The address that registered the offering.
    /// - `namespace`: The namespace of the offering.
    /// - `token`: The token address associated with the offering.
    ///
    /// ### Returns
    /// - `Some(Offering)` if found.
    /// - `None` otherwise.
    pub fn get_offering(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<Offering> {
        let count = Self::get_offering_count(env.clone(), issuer.clone(), namespace.clone());
        let tenant_id = TenantId { issuer, namespace };
        for i in 0..count {
            let item_key = DataKey::OfferItem(tenant_id.clone(), i);
            let offering: Offering = env.storage().persistent().get(&item_key).unwrap();
            if offering.token == token {
                return Some(offering);
            }
        }
        None
    }

    /// List all offering tokens for an issuer in a namespace.
    pub fn list_offerings(env: Env, issuer: Address, namespace: Symbol) -> Vec<Address> {
        let (page, _) =
            Self::get_offerings_page(env.clone(), issuer.clone(), namespace, 0, MAX_PAGE_LIMIT);
        let mut tokens = Vec::new(&env);
        for i in 0..page.len() {
            tokens.push_back(page.get(i).unwrap().token);
        }
        tokens
    }

    /// Record a revenue report for an offering; updates audit summary and emits events.
    #[allow(clippy::too_many_arguments)]
    pub fn report_revenue(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        payout_asset: Address,
        amount: i128,
        period_id: u64,
        override_existing: bool,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

        let event_only = Self::is_event_only(&env);
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        Self::require_report_window_open(&env, &offering_id)?;

        if !event_only {
            // Verify offering exists and issuer is current
            let current_issuer =
                Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                    .ok_or(RevoraError::OfferingNotFound)?;

            if current_issuer != issuer {
                return Err(RevoraError::OfferingNotFound);
            }

            let offering =
                Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                    .ok_or(RevoraError::OfferingNotFound)?;
            if offering.payout_asset != payout_asset {
                return Err(RevoraError::PayoutAssetMismatch);
            }

            // Check minimum revenue threshold (#25)
            let threshold_key = DataKey::MinRevenueThreshold(offering_id.clone());
            let threshold: i128 = env.storage().persistent().get(&threshold_key).unwrap_or(0);
            if amount < threshold {
                env.events().publish(
                    (EVENT_REV_BELOW_THRESHOLD, issuer, namespace, token),
                    (amount, threshold, period_id),
                );
                return Ok(());
            }

            // Skip concentration enforcement in testnet mode
            let testnet_mode = Self::is_testnet_mode(env.clone());
            if !testnet_mode {
                // Holder concentration guardrail (#26): reject if enforce and over limit
                let limit_key = DataKey::ConcentrationLimit(offering_id.clone());
                if let Some(config) =
                    env.storage().persistent().get::<DataKey, ConcentrationLimitConfig>(&limit_key)
                {
                    if config.enforce && config.max_bps > 0 {
                        let curr_key = DataKey::CurrentConcentration(offering_id.clone());
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
            Self::get_blacklist(env.clone(), issuer.clone(), namespace.clone(), token.clone())
        };

        if !event_only {
            let key = DataKey::RevenueReports(offering_id.clone());
            let mut reports: Map<u64, (i128, u64)> =
                env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));
            let current_timestamp = env.ledger().timestamp();
            let idx_key = DataKey::RevenueIndex(offering_id.clone(), period_id);
            let mut cumulative_revenue: i128 =
                env.storage().persistent().get(&idx_key).unwrap_or(0);

            match reports.get(period_id) {
                Some((existing_amount, _timestamp)) => {
                    if override_existing {
                        reports.set(period_id, (amount, current_timestamp));
                        env.storage().persistent().set(&key, &reports);

                        env.events().publish(
                            (
                                EVENT_REVENUE_REPORT_OVERRIDE,
                                issuer.clone(),
                                namespace.clone(),
                                token.clone(),
                            ),
                            (amount, period_id, existing_amount, blacklist.clone()),
                        );
                        env.events().publish(
                            (
                                EVENT_INDEXED_V2,
                                EventIndexTopicV2 {
                                    version: 2,
                                    event_type: EVENT_TYPE_REV_OVR,
                                    issuer: issuer.clone(),
                                    namespace: namespace.clone(),
                                    token: token.clone(),
                                    period_id,
                                },
                            ),
                            (amount, existing_amount, payout_asset.clone()),
                        );

                        env.events().publish(
                            (
                                EVENT_REVENUE_REPORT_OVERRIDE_ASSET,
                                issuer.clone(),
                                namespace.clone(),
                                token.clone(),
                            ),
                            (
                                payout_asset.clone(),
                                amount,
                                period_id,
                                existing_amount,
                                blacklist.clone(),
                            ),
                        );
                    } else {
                        env.events().publish(
                            (
                                EVENT_REVENUE_REPORT_REJECTED,
                                issuer.clone(),
                                namespace.clone(),
                                token.clone(),
                            ),
                            (amount, period_id, existing_amount, blacklist.clone()),
                        );
                        env.events().publish(
                            (
                                EVENT_INDEXED_V2,
                                EventIndexTopicV2 {
                                    version: 2,
                                    event_type: EVENT_TYPE_REV_REJ,
                                    issuer: issuer.clone(),
                                    namespace: namespace.clone(),
                                    token: token.clone(),
                                    period_id,
                                },
                            ),
                            (amount, existing_amount, payout_asset.clone()),
                        );

                        env.events().publish(
                            (
                                EVENT_REVENUE_REPORT_REJECTED_ASSET,
                                issuer.clone(),
                                namespace.clone(),
                                token.clone(),
                            ),
                            (
                                payout_asset.clone(),
                                amount,
                                period_id,
                                existing_amount,
                                blacklist.clone(),
                            ),
                        );
                    }
                }
                None => {
                    // Initial report for this period
                    cumulative_revenue = cumulative_revenue.checked_add(amount).unwrap_or(amount);
                    env.storage().persistent().set(&idx_key, &cumulative_revenue);

                    reports.set(period_id, (amount, current_timestamp));
                    env.storage().persistent().set(&key, &reports);

                    env.events().publish(
                        (
                            EVENT_REVENUE_REPORT_INITIAL,
                            issuer.clone(),
                            namespace.clone(),
                            token.clone(),
                        ),
                        (amount, period_id, blacklist.clone()),
                    );
                    env.events().publish(
                        (
                            EVENT_INDEXED_V2,
                            EventIndexTopicV2 {
                                version: 2,
                                event_type: EVENT_TYPE_REV_INIT,
                                issuer: issuer.clone(),
                                namespace: namespace.clone(),
                                token: token.clone(),
                                period_id,
                            },
                        ),
                        (amount, payout_asset.clone()),
                    );

                    env.events().publish(
                        (
                            EVENT_REVENUE_REPORT_INITIAL_ASSET,
                            issuer.clone(),
                            namespace.clone(),
                            token.clone(),
                        ),
                        (payout_asset.clone(), amount, period_id, blacklist.clone()),
                    );
                }
            }
        } else {
            // Event-only mode: always treat as initial report (or simply publish the event)
            env.events().publish(
                (EVENT_REVENUE_REPORT_INITIAL, issuer.clone(), namespace.clone(), token.clone()),
                (amount, period_id, blacklist.clone()),
            );
        }
        env.events().publish(
            (EVENT_REVENUE_REPORTED, issuer.clone(), namespace.clone(), token.clone()),
            (amount, period_id, blacklist.clone()),
        );
        env.events().publish(
            (
                EVENT_INDEXED_V2,
                EventIndexTopicV2 {
                    version: 2,
                    event_type: EVENT_TYPE_REV_REP,
                    issuer: issuer.clone(),
                    namespace: namespace.clone(),
                    token: token.clone(),
                    period_id,
                },
            ),
            (amount, payout_asset.clone(), override_existing),
        );

        env.events().publish(
            (EVENT_REVENUE_REPORTED_ASSET, issuer.clone(), namespace.clone(), token.clone()),
            (payout_asset.clone(), amount, period_id),
        );
        // Optionally emit versioned v1 events for forward-compatible consumers
        if Self::is_event_versioning_enabled(env.clone()) {
            env.events().publish(
                (EVENT_REV_INIT_V1, issuer.clone(), namespace.clone(), token.clone()),
                (EVENT_SCHEMA_VERSION, amount, period_id, blacklist.clone()),
            );

            env.events().publish(
                (EVENT_REV_INIA_V1, issuer.clone(), namespace.clone(), token.clone()),
                (EVENT_SCHEMA_VERSION, payout_asset.clone(), amount, period_id, blacklist.clone()),
            );

            env.events().publish(
                (EVENT_REV_REP_V1, issuer.clone(), namespace.clone(), token.clone()),
                (EVENT_SCHEMA_VERSION, amount, period_id, blacklist.clone()),
            );

            env.events().publish(
                (EVENT_REV_REPA_V1, issuer.clone(), namespace.clone(), token.clone()),
                (EVENT_SCHEMA_VERSION, payout_asset.clone(), amount, period_id),
            );
        }

        if !event_only {
            // Audit log summary (#34): maintain per-offering total revenue and report count
            let summary_key = DataKey::AuditSummary(offering_id);
            let mut summary: AuditSummary = env
                .storage()
                .persistent()
                .get(&summary_key)
                .unwrap_or(AuditSummary { total_revenue: 0, report_count: 0 });
            summary.total_revenue = summary.total_revenue.saturating_add(amount);
            summary.report_count = summary.report_count.saturating_add(1);
            env.storage().persistent().set(&summary_key, &summary);
        }

        Ok(())
    }

    pub fn get_revenue_by_period(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        period_id: u64,
    ) -> i128 {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::RevenueIndex(offering_id, period_id);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    pub fn get_revenue_range(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        from_period: u64,
        to_period: u64,
    ) -> i128 {
        let mut total: i128 = 0;
        for period in from_period..=to_period {
            total += Self::get_revenue_by_period(
                env.clone(),
                issuer.clone(),
                namespace.clone(),
                token.clone(),
                period,
            );
        }
        total
    }

    /// Read-only: sum revenue for a numeric period range but bounded by `max_periods` per call.
    /// Returns `(sum, next_start)` where `next_start` is `Some(period)` if there are remaining
    /// periods to process and a subsequent call can continue from that period. `max_periods` of 0
    /// or > `MAX_CHUNK_PERIODS` will be capped to `MAX_CHUNK_PERIODS` to enforce limits.
    pub fn get_revenue_range_chunk(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        from_period: u64,
        to_period: u64,
        max_periods: u32,
    ) -> (i128, Option<u64>) {
        let mut total: i128 = 0;
        let mut processed: u32 = 0;
        let cap = if max_periods == 0 || max_periods > MAX_CHUNK_PERIODS {
            MAX_CHUNK_PERIODS
        } else {
            max_periods
        };

        let mut p = from_period;
        while p <= to_period {
            if processed >= cap {
                return (total, Some(p));
            }
            total += Self::get_revenue_by_period(
                env.clone(),
                issuer.clone(),
                namespace.clone(),
                token.clone(),
                p,
            );
            processed = processed.saturating_add(1);
            p = p.saturating_add(1);
        }
        (total, None)
    }
    /// Return the total number of offerings registered by `issuer` in `namespace`.
    pub fn get_offering_count(env: Env, issuer: Address, namespace: Symbol) -> u32 {
        let tenant_id = TenantId { issuer, namespace };
        let count_key = DataKey::OfferCount(tenant_id);
        env.storage().persistent().get(&count_key).unwrap_or(0)
    }

    /// Return a page of offerings for `issuer`. Limit capped at MAX_PAGE_LIMIT (20).
    /// Ordering: by registration index (creation order), deterministic (#38).
    /// Return a page of offerings for `issuer` in `namespace`. Limit capped at MAX_PAGE_LIMIT (20).
    /// Ordering: by registration index (creation order), deterministic (#38).
    pub fn get_offerings_page(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        start: u32,
        limit: u32,
    ) -> (Vec<Offering>, Option<u32>) {
        let count = Self::get_offering_count(env.clone(), issuer.clone(), namespace.clone());
        let tenant_id = TenantId { issuer, namespace };

        let effective_limit =
            if limit == 0 || limit > MAX_PAGE_LIMIT { MAX_PAGE_LIMIT } else { limit };

        if start >= count {
            return (Vec::new(&env), None);
        }

        let end = core::cmp::min(start + effective_limit, count);
        let mut results = Vec::new(&env);

        for i in start..end {
            let item_key = DataKey::OfferItem(tenant_id.clone(), i);
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
        issuer: Address,
        namespace: Symbol,
        token: Address,
        investor: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        caller.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if !Self::is_event_only(&env) {
            let key = DataKey::Blacklist(offering_id.clone());
            let mut map: Map<Address, bool> =
                env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));

            map.set(investor.clone(), true);
            env.storage().persistent().set(&key, &map);
        }
        // Verify auth: caller must be issuer or admin
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        let admin = Self::get_admin(env.clone()).ok_or(RevoraError::NotInitialized)?;

        if caller != current_issuer && caller != admin {
            return Err(RevoraError::NotAuthorized);
        }

        let key = DataKey::Blacklist(offering_id.clone());
        let mut map: Map<Address, bool> =
            env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));

        let was_present = map.get(investor.clone()).unwrap_or(false);
        map.set(investor.clone(), true);
        env.storage().persistent().set(&key, &map);

        // Maintain insertion order for deterministic get_blacklist (#38)
        if !was_present {
            let order_key = DataKey::BlacklistOrder(offering_id.clone());
            let mut order: Vec<Address> =
                env.storage().persistent().get(&order_key).unwrap_or_else(|| Vec::new(&env));
            order.push_back(investor.clone());
            env.storage().persistent().set(&order_key, &order);
        }

        env.events().publish((EVENT_BL_ADD, issuer, namespace, token), (caller, investor));
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
        issuer: Address,
        namespace: Symbol,
        token: Address,
        investor: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        caller.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        let admin = Self::get_admin(env.clone()).ok_or(RevoraError::NotInitialized)?;
        if caller != current_issuer && caller != admin {
            return Err(RevoraError::NotAuthorized);
        }

        if !Self::is_event_only(&env) {
            let key = DataKey::Blacklist(offering_id.clone());
            let mut map: Map<Address, bool> =
                env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));
            map.remove(investor.clone());
            env.storage().persistent().set(&key, &map);

            // Rebuild order vec so get_blacklist stays deterministic (#38)
            let order_key = DataKey::BlacklistOrder(offering_id.clone());
            let old_order: Vec<Address> =
                env.storage().persistent().get(&order_key).unwrap_or_else(|| Vec::new(&env));
            let mut new_order = Vec::new(&env);
            for i in 0..old_order.len() {
                let addr = old_order.get(i).unwrap();
                if map.get(addr.clone()).unwrap_or(false) {
                    new_order.push_back(addr);
                }
            }
            env.storage().persistent().set(&order_key, &new_order);
        }

        env.events().publish((EVENT_BL_REM, issuer, namespace, token), (caller, investor));
        Ok(())
    }

    /// Returns `true` if `investor` is blacklisted for an offering.
    pub fn is_blacklisted(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        investor: Address,
    ) -> bool {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::Blacklist(offering_id);
        env.storage()
            .persistent()
            .get::<DataKey, Map<Address, bool>>(&key)
            .map(|m| m.get(investor).unwrap_or(false))
            .unwrap_or(false)
    }

    /// Return all blacklisted addresses for an offering.
    /// Ordering: by insertion order, deterministic and stable across calls (#38).
    pub fn get_blacklist(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Vec<Address> {
        let offering_id = OfferingId { issuer, namespace, token };
        let order_key = DataKey::BlacklistOrder(offering_id);
        env.storage()
            .persistent()
            .get::<DataKey, Vec<Address>>(&order_key)
            .unwrap_or_else(|| Vec::new(&env))
    }

    // ── Whitelist management ──────────────────────────────────

    /// Set per-offering concentration limit. Caller must be the offering issuer.
    /// `max_bps`: max allowed single-holder share in basis points (0 = disable).
    /// Add `investor` to the per-offering whitelist for `token`.
    ///
    /// Idempotent — calling with an already-whitelisted address is safe.
    /// When a whitelist exists (non-empty), only whitelisted addresses
    /// are eligible for revenue distribution (subject to blacklist override).
    /// Add `investor` to the per-offering whitelist.
    pub fn whitelist_add(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        investor: Address,
    ) {
        caller.require_auth();
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .expect("offering not found");
        if caller != current_issuer {
            panic!("not authorized");
        }

        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::Whitelist(offering_id.clone());
        let mut map: Map<Address, bool> =
            env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));

        map.set(investor.clone(), true);
        env.storage().persistent().set(&key, &map);

        env.events().publish(
            (
                EVENT_WL_ADD,
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
            ),
            (caller, investor),
        );
    }

    /// Remove `investor` from the per-offering whitelist for `token`.
    ///
    /// Idempotent — calling when the address is not listed is safe.
    /// Remove `investor` from the per-offering whitelist.
    pub fn whitelist_remove(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        investor: Address,
    ) {
        caller.require_auth();
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .expect("offering not found");
        if caller != current_issuer {
            panic!("not authorized");
        }

        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::Whitelist(offering_id.clone());
        let mut map: Map<Address, bool> =
            env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));

        map.remove(investor.clone());
        env.storage().persistent().set(&key, &map);

        env.events().publish(
            (
                EVENT_WL_REM,
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
            ),
            (caller, investor),
        );
    }

    /// Returns `true` if `investor` is whitelisted for `token`'s offering.
    ///
    /// Note: If the whitelist is empty (disabled), this returns `false`.
    /// Use `is_whitelist_enabled` to check if whitelist enforcement is active.
    pub fn is_whitelisted(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        investor: Address,
    ) -> bool {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::Whitelist(offering_id);
        env.storage()
            .persistent()
            .get::<DataKey, Map<Address, bool>>(&key)
            .map(|m| m.get(investor).unwrap_or(false))
            .unwrap_or(false)
    }

    /// Return all whitelisted addresses for an offering.
    pub fn get_whitelist(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Vec<Address> {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::Whitelist(offering_id);
        env.storage()
            .persistent()
            .get::<DataKey, Map<Address, bool>>(&key)
            .map(|m| m.keys())
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns `true` if whitelist enforcement is enabled for an offering.
    pub fn is_whitelist_enabled(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> bool {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::Whitelist(offering_id);
        let map: Map<Address, bool> =
            env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));
        !map.is_empty()
    }

    // ── Holder concentration guardrail (#26) ───────────────────

    /// Set the concentration limit for an offering.
    ///
    /// Configures the maximum share a single holder can own and whether it is enforced.
    ///
    /// ### Parameters
    /// - `issuer`: The offering issuer. Must provide authentication.
    /// - `namespace`: The namespace the offering belongs to.
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
        namespace: Symbol,
        token: Address,
        max_bps: u32,
        enforce: bool,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        if env.storage().persistent().get::<DataKey, bool>(&DataKey::Paused).unwrap_or(false) {
            return Err(RevoraError::ContractPaused);
        }

        if max_bps > 10_000 {
            return Err(RevoraError::InvalidShareBps);
        }

        // Verify offering exists and issuer is current
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::LimitReached)?;

        if current_issuer != issuer {
            return Err(RevoraError::LimitReached);
        }

        if !Self::is_event_only(&env) {
            issuer.require_auth();
            let key = DataKey::ConcentrationLimit(offering_id);
            env.storage().persistent().set(&key, &ConcentrationLimitConfig { max_bps, enforce });
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
        namespace: Symbol,
        token: Address,
        concentration_bps: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        if env.storage().persistent().get::<DataKey, bool>(&DataKey::Paused).unwrap_or(false) {
            return Err(RevoraError::ContractPaused);
        }
        issuer.require_auth();

        if concentration_bps > 10_000 {
            return Err(RevoraError::InvalidShareBps);
        }
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Verify offering exists and issuer is current
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        if !Self::is_event_only(&env) {
            let curr_key = DataKey::CurrentConcentration(offering_id.clone());
            env.storage().persistent().set(&curr_key, &concentration_bps);
        }

        let limit_key = DataKey::ConcentrationLimit(offering_id);
        if let Some(config) =
            env.storage().persistent().get::<DataKey, ConcentrationLimitConfig>(&limit_key)
        {
            if config.max_bps > 0 && concentration_bps > config.max_bps {
                env.events().publish(
                    (EVENT_CONCENTRATION_WARNING, issuer.clone(), namespace.clone(), token.clone()),
                    (concentration_bps, config.max_bps),
                );
            }
        }
        
        if !Self::is_event_only(&env) {
            env.events().publish(
                (EVENT_CONCENTRATION_REPORTED, issuer, namespace, token),
                concentration_bps,
            );
        }
        Ok(())
    }

    /// Get concentration limit config for an offering.
    pub fn get_concentration_limit(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<ConcentrationLimitConfig> {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::ConcentrationLimit(offering_id);
        env.storage().persistent().get(&key)
    }

    /// Get last reported concentration in bps for an offering.
    pub fn get_current_concentration(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<u32> {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::CurrentConcentration(offering_id);
        env.storage().persistent().get(&key)
    }

    // ── Audit log summary (#34) ────────────────────────────────

    /// Get per-offering audit summary (total revenue and report count).
    pub fn get_audit_summary(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<AuditSummary> {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::AuditSummary(offering_id);
        env.storage().persistent().get(&key)
    }

    /// Set rounding mode for an offering. Default is truncation.
    pub fn set_rounding_mode(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        mode: RoundingMode,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        issuer.require_auth();
        let key = DataKey::RoundingMode(offering_id);
        env.storage().persistent().set(&key, &mode);
        Ok(())
    }

    /// Get rounding mode for an offering.
    pub fn get_rounding_mode(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> RoundingMode {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::RoundingMode(offering_id);
        env.storage().persistent().get(&key).unwrap_or(RoundingMode::Truncation)
    }

    // ── Per-offering investment constraints (#97) ─────────────

    /// Set min and max stake per investor for an offering. Issuer/admin only. Constraints are read by off-chain systems for enforcement.
    pub fn set_investment_constraints(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        min_stake: i128,
        max_stake: i128,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        issuer.require_auth();
        if min_stake < 0 || max_stake < 0 {
            return Err(RevoraError::InvalidAmount);
        }
        if max_stake > 0 && min_stake > max_stake {
            return Err(RevoraError::InvalidAmount);
        }
        let key = DataKey::InvestmentConstraints(offering_id);
        let previous = env.storage().persistent().get::<DataKey, InvestmentConstraintsConfig>(&key);
        env.storage().persistent().set(&key, &InvestmentConstraintsConfig { min_stake, max_stake });
        env.events().publish(
            (EVENT_INV_CONSTRAINTS, issuer, namespace, token),
            (min_stake, max_stake, previous.is_some()),
        );
        Ok(())
    }

    /// Get per-offering investment constraints. Returns None if not set.
    pub fn get_investment_constraints(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<InvestmentConstraintsConfig> {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::InvestmentConstraints(offering_id);
        env.storage().persistent().get(&key)
    }

    // ── Per-offering minimum revenue threshold (#25) ─────────────────────

    /// Set minimum revenue per period below which no distribution is triggered.
    /// Authorized by: Offering issuer OR platform admin.
    /// Emits `min_rev` event when configured or changed.
    /// Pass 0 to disable the threshold.
    ///
    /// ### Parameters
    /// - `caller`: The authorized address (issuer or admin).
    /// - `issuer`: The offering issuer address.
    /// - `namespace`: The offering namespace.
    /// - `token`: The offering token address.
    /// - `min_amount`: The threshold amount (>= 0).
    pub fn set_min_revenue_threshold(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        min_amount: i128,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        caller.require_auth();

        // Authorization: caller must be issuer or admin
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        let admin = Self::get_admin(env.clone()).ok_or(RevoraError::NotInitialized)?;

        if caller != current_issuer && caller != admin {
            return Err(RevoraError::NotAuthorized);
        }

        Self::require_non_negative_amount(min_amount)?;

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if !Self::is_event_only(&env) {
            let key = DataKey::MinRevenueThreshold(offering_id);
            env.storage().persistent().set(&key, &min_amount);
        }

        env.events().publish(
            (EVENT_MIN_REV_THRESHOLD_SET, issuer, namespace, token),
            (caller, min_amount),
        );
        Ok(())
    }

    /// Get minimum revenue threshold for an offering. 0 means no threshold.
    pub fn get_min_revenue_threshold(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> i128 {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::MinRevenueThreshold(offering_id);
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
        namespace: Symbol,
        token: Address,
        payment_token: Address,
        amount: i128,
        period_id: u64,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        // Verify offering exists and issuer is current
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        Self::do_deposit_revenue(&env, issuer, namespace, token, payment_token, amount, period_id)
    }

    /// any previously recorded snapshot for this offering to prevent duplication.
    #[allow(clippy::too_many_arguments)]
    pub fn deposit_revenue_with_snapshot(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        payment_token: Address,
        amount: i128,
        period_id: u64,
        snapshot_reference: u64,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        issuer.require_auth();

        // 1. Verify snapshots are enabled
        if !Self::get_snapshot_config(env.clone(), issuer.clone(), namespace.clone(), token.clone())
        {
            return Err(RevoraError::SnapshotNotEnabled);
        }

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // 2. Validate snapshot reference (must be strictly monotonic)
        let snap_key = DataKey::LastSnapshotRef(offering_id.clone());
        let last_snap: u64 = env.storage().persistent().get(&snap_key).unwrap_or(0);
        if snapshot_reference <= last_snap {
            return Err(RevoraError::OutdatedSnapshot);
        }

        // 3. Delegate to core deposit logic
        Self::do_deposit_revenue(
            &env,
            issuer.clone(),
            namespace.clone(),
            token.clone(),
            payment_token.clone(),
            amount,
            period_id,
        )?;

        // 4. Update last snapshot and emit specialized event
        env.storage().persistent().set(&snap_key, &snapshot_reference);
        env.events().publish(
            (EVENT_REV_DEP_SNAP, issuer, namespace, token),
            (payment_token, amount, period_id, snapshot_reference),
        );

        Ok(())
    }

    /// Enable or disable snapshot-based distribution for an offering.
    pub fn set_snapshot_config(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        enabled: bool,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        issuer.require_auth();
        if Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
            .is_none()
        {
            return Err(RevoraError::OfferingNotFound);
        }
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::SnapshotConfig(offering_id.clone());
        env.storage().persistent().set(&key, &enabled);
        env.events().publish(
            (EVENT_SNAP_CONFIG, offering_id.issuer, offering_id.namespace, offering_id.token),
            enabled,
        );
        Ok(())
    }

    /// Check if snapshot-based distribution is enabled for an offering.
    pub fn get_snapshot_config(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> bool {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::SnapshotConfig(offering_id);
        env.storage().persistent().get(&key).unwrap_or(false)
    }

    /// Get the latest recorded snapshot reference for an offering.
    pub fn get_last_snapshot_ref(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> u64 {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::LastSnapshotRef(offering_id);
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
    /// Set a holder's revenue share (in basis points) for an offering.
    pub fn set_holder_share(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        share_bps: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        // Verify offering exists and issuer is current
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        issuer.require_auth();
        Self::set_holder_share_internal(&env, offering_id.issuer, offering_id.namespace, offering_id.token, holder, share_bps)
    }

    /// Register an ed25519 public key for a signer address.
    /// The signer must authorize this binding.
    pub fn register_meta_signer_key(
        env: Env,
        signer: Address,
        public_key: BytesN<32>,
    ) -> Result<(), RevoraError> {
        signer.require_auth();
        env.storage()
            .persistent()
            .set(&MetaDataKey::SignerKey(signer.clone()), &public_key);
        env.events()
            .publish((EVENT_META_SIGNER_SET, signer), public_key);
        Ok(())
    }

    /// Set or update an offering-level delegate signer for off-chain authorizations.
    /// Only the current issuer may set this value.
    pub fn set_meta_delegate(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        delegate: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        let current_issuer = Self::get_current_issuer(
            &env,
            issuer.clone(),
            namespace.clone(),
            token.clone(),
        )
        .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        issuer.require_auth();
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        env.storage()
            .persistent()
            .set(&MetaDataKey::Delegate(offering_id), &delegate);
        env.events()
            .publish((EVENT_META_DELEGATE_SET, issuer, namespace, token), delegate);
        Ok(())
    }

    /// Get the configured offering-level delegate signer.
    pub fn get_meta_delegate(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<Address> {
        let offering_id = OfferingId {
            issuer,
            namespace,
            token,
        };
        env.storage()
            .persistent()
            .get(&MetaDataKey::Delegate(offering_id))
    }

    /// Meta-transaction variant of `set_holder_share`.
    /// A registered delegate signer authorizes this action via off-chain ed25519 signature.
    #[allow(clippy::too_many_arguments)]
    pub fn meta_set_holder_share(
        env: Env,
        signer: Address,
        payload: MetaSetHolderSharePayload,
        nonce: u64,
        expiry: u64,
        signature: BytesN<64>,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        let current_issuer = Self::get_current_issuer(
            &env,
            payload.issuer.clone(),
            payload.namespace.clone(),
            payload.token.clone(),
        )
        .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != payload.issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        let offering_id = OfferingId {
            issuer: payload.issuer.clone(),
            namespace: payload.namespace.clone(),
            token: payload.token.clone(),
        };
        let configured_delegate: Address = env
            .storage()
            .persistent()
            .get(&MetaDataKey::Delegate(offering_id))
            .ok_or(RevoraError::NotAuthorized)?;
        if configured_delegate != signer {
            return Err(RevoraError::NotAuthorized);
        }
        let action = MetaAction::SetHolderShare(payload.clone());
        Self::verify_meta_signature(&env, &signer, nonce, expiry, action, &signature)?;
        Self::set_holder_share_internal(
            &env,
            payload.issuer.clone(),
            payload.namespace.clone(),
            payload.token.clone(),
            payload.holder.clone(),
            payload.share_bps,
        )?;
        Self::mark_meta_nonce_used(&env, &signer, nonce);
        env.events().publish(
            (
                EVENT_META_SHARE_SET,
                payload.issuer,
                payload.namespace,
                payload.token,
            ),
            (signer, payload.holder, payload.share_bps, nonce, expiry),
        );
        Ok(())
    }

    /// Meta-transaction authorization for a revenue report payload.
    /// This does not mutate revenue data directly; it records a signed approval.
    #[allow(clippy::too_many_arguments)]
    pub fn meta_approve_revenue_report(
        env: Env,
        signer: Address,
        payload: MetaRevenueApprovalPayload,
        nonce: u64,
        expiry: u64,
        signature: BytesN<64>,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        let current_issuer = Self::get_current_issuer(
            &env,
            payload.issuer.clone(),
            payload.namespace.clone(),
            payload.token.clone(),
        )
        .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != payload.issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        let offering_id = OfferingId {
            issuer: payload.issuer.clone(),
            namespace: payload.namespace.clone(),
            token: payload.token.clone(),
        };
        let configured_delegate: Address = env
            .storage()
            .persistent()
            .get(&MetaDataKey::Delegate(offering_id.clone()))
            .ok_or(RevoraError::NotAuthorized)?;
        if configured_delegate != signer {
            return Err(RevoraError::NotAuthorized);
        }
        let action = MetaAction::ApproveRevenueReport(payload.clone());
        Self::verify_meta_signature(&env, &signer, nonce, expiry, action, &signature)?;
        env.storage()
            .persistent()
            .set(
                &MetaDataKey::RevenueApproved(offering_id, payload.period_id),
                &true,
            );
        Self::mark_meta_nonce_used(&env, &signer, nonce);
        env.events().publish(
            (
                EVENT_META_REV_APPROVE,
                payload.issuer,
                payload.namespace,
                payload.token,
            ),
            (
                signer,
                payload.payout_asset,
                payload.amount,
                payload.period_id,
                payload.override_existing,
                nonce,
                expiry,
            ),
        );
        Ok(())
    }

    /// Return a holder's share in basis points for an offering (0 if unset).
    pub fn get_holder_share(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::HolderShare(offering_id, holder);
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
        issuer: Address,
        namespace: Symbol,
        token: Address,
        max_periods: u32,
    ) -> Result<i128, RevoraError> {
        holder.require_auth();

        if Self::is_blacklisted(
            env.clone(),
            issuer.clone(),
            namespace.clone(),
            token.clone(),
            holder.clone(),
        ) {
            return Err(RevoraError::HolderBlacklisted);
        }

        let share_bps = Self::get_holder_share(
            env.clone(),
            issuer.clone(),
            namespace.clone(),
            token.clone(),
            holder.clone(),
        );
        if share_bps == 0 {
            return Err(RevoraError::NoPendingClaims);
        }

        let offering_id = OfferingId { issuer, namespace, token };
        Self::require_claim_window_open(&env, &offering_id)?;

        let count_key = DataKey::PeriodCount(offering_id.clone());
        let period_count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let idx_key = DataKey::LastClaimedIdx(offering_id.clone(), holder.clone());
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

        let delay_key = DataKey::ClaimDelaySecs(offering_id.clone());
        let delay_secs: u64 = env.storage().persistent().get(&delay_key).unwrap_or(0);
        let now = env.ledger().timestamp();

        let mut total_payout: i128 = 0;
        let mut claimed_periods = Vec::new(&env);
        let mut last_claimed_idx = start_idx;

        for i in start_idx..end_idx {
            let entry_key = DataKey::PeriodEntry(offering_id.clone(), i);
            let period_id: u64 = env.storage().persistent().get(&entry_key).ok_or(RevoraError::NoPendingClaims)?;
            let time_key = DataKey::PeriodDepositTime(offering_id.clone(), period_id);
            let deposit_time: u64 = env.storage().persistent().get(&time_key).unwrap_or(0);
            if delay_secs > 0 && now < deposit_time.saturating_add(delay_secs) {
                break;
            }
            let rev_key = DataKey::PeriodRevenue(offering_id.clone(), period_id);
            let revenue: i128 = env.storage().persistent().get(&rev_key).ok_or(RevoraError::NoPendingClaims)?;
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
            let pt_key = DataKey::PaymentToken(offering_id.clone());
            let payment_token: Address = env.storage().persistent().get(&pt_key).ok_or(RevoraError::OfferingNotFound)?;
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
            (
                EVENT_CLAIM,
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
            ),
            (holder, total_payout, claimed_periods),
        );
        env.events().publish(
            (
                EVENT_INDEXED_V2,
                EventIndexTopicV2 {
                    version: 2,
                    event_type: EVENT_TYPE_CLAIM,
                    issuer: offering_id.issuer,
                    namespace: offering_id.namespace,
                    token: offering_id.token,
                    period_id: 0,
                },
            ),
            (total_payout,),
        );

        Ok(total_payout)
    }

    /// Configure the reporting access window for an offering.
    /// If unset, reporting remains always permitted.
    pub fn set_report_window(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        start_timestamp: u64,
        end_timestamp: u64,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        let current_issuer = Self::get_current_issuer(
            &env,
            issuer.clone(),
            namespace.clone(),
            token.clone(),
        )
        .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        issuer.require_auth();
        let window = AccessWindow {
            start_timestamp,
            end_timestamp,
        };
        Self::validate_window(&window)?;
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        env.storage()
            .persistent()
            .set(&WindowDataKey::Report(offering_id), &window);
        env.events().publish(
            (EVENT_REPORT_WINDOW_SET, issuer, namespace, token),
            (start_timestamp, end_timestamp),
        );
        Ok(())
    }

    /// Configure the claiming access window for an offering.
    /// If unset, claiming remains always permitted.
    pub fn set_claim_window(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        start_timestamp: u64,
        end_timestamp: u64,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        let current_issuer = Self::get_current_issuer(
            &env,
            issuer.clone(),
            namespace.clone(),
            token.clone(),
        )
        .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        issuer.require_auth();
        let window = AccessWindow {
            start_timestamp,
            end_timestamp,
        };
        Self::validate_window(&window)?;
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        env.storage()
            .persistent()
            .set(&WindowDataKey::Claim(offering_id), &window);
        env.events().publish(
            (EVENT_CLAIM_WINDOW_SET, issuer, namespace, token),
            (start_timestamp, end_timestamp),
        );
        Ok(())
    }

    /// Read configured reporting window (if any) for an offering.
    pub fn get_report_window(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<AccessWindow> {
        let offering_id = OfferingId {
            issuer,
            namespace,
            token,
        };
        env.storage()
            .persistent()
            .get(&WindowDataKey::Report(offering_id))
    }

    /// Read configured claiming window (if any) for an offering.
    pub fn get_claim_window(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<AccessWindow> {
        let offering_id = OfferingId {
            issuer,
            namespace,
            token,
        };
        env.storage()
            .persistent()
            .get(&WindowDataKey::Claim(offering_id))
    }

    /// Return unclaimed period IDs for a holder on an offering.
    /// Ordering: by deposit index (creation order), deterministic (#38).
    pub fn get_pending_periods(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
    ) -> Vec<u64> {
        let offering_id = OfferingId { issuer, namespace, token };
        let count_key = DataKey::PeriodCount(offering_id.clone());
        let period_count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let idx_key = DataKey::LastClaimedIdx(offering_id.clone(), holder);
        let start_idx: u32 = env.storage().persistent().get(&idx_key).unwrap_or(0);

        let mut periods = Vec::new(&env);
        for i in start_idx..period_count {
            let entry_key = DataKey::PeriodEntry(offering_id.clone(), i);
            let period_id: u64 = env.storage().persistent().get(&entry_key).unwrap_or(0);
            if period_id == 0 {
                continue;
            }
            periods.push_back(period_id);
        }
        periods
    }

    /// Read-only: return a page of pending period IDs for a holder, bounded by `limit`.
    /// Returns `(periods_page, next_cursor)` where `next_cursor` is `Some(next_index)` when more
    /// periods remain, otherwise `None`. `limit` of 0 or greater than `MAX_PAGE_LIMIT` will be
    /// capped to `MAX_PAGE_LIMIT` to keep calls predictable.
    pub fn get_pending_periods_page(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        start: u32,
        limit: u32,
    ) -> (Vec<u64>, Option<u32>) {
        let offering_id = OfferingId { issuer, namespace, token };
        let count_key = DataKey::PeriodCount(offering_id.clone());
        let period_count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let idx_key = DataKey::LastClaimedIdx(offering_id.clone(), holder);
        let holder_start_idx: u32 = env.storage().persistent().get(&idx_key).unwrap_or(0);

        let actual_start = core::cmp::max(start, holder_start_idx);

        if actual_start >= period_count {
            return (Vec::new(&env), None);
        }

        let effective_limit =
            if limit == 0 || limit > MAX_PAGE_LIMIT { MAX_PAGE_LIMIT } else { limit };
        let end = core::cmp::min(actual_start + effective_limit, period_count);

        let mut results = Vec::new(&env);
        for i in actual_start..end {
            let entry_key = DataKey::PeriodEntry(offering_id.clone(), i);
            let period_id: u64 = env.storage().persistent().get(&entry_key).unwrap_or(0);
            if period_id == 0 {
                continue;
            }
            results.push_back(period_id);
        }

        let next_cursor = if end < period_count { Some(end) } else { None };
        (results, next_cursor)
    }

    /// Preview the total claimable amount for a holder without mutating state.
    ///
    /// This method respects the per-offering claim delay and only sums periods that have passed the delay.
    pub fn get_claimable(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
    ) -> i128 {
        let share_bps = Self::get_holder_share(
            env.clone(),
            issuer.clone(),
            namespace.clone(),
            token.clone(),
            holder.clone(),
        );
        if share_bps == 0 {
            return 0;
        }

        let offering_id = OfferingId { issuer, namespace, token };

        let count_key = DataKey::PeriodCount(offering_id.clone());
        let period_count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let idx_key = DataKey::LastClaimedIdx(offering_id.clone(), holder.clone());
        let start_idx: u32 = env.storage().persistent().get(&idx_key).unwrap_or(0);

        let delay_key = DataKey::ClaimDelaySecs(offering_id.clone());
        let delay_secs: u64 = env.storage().persistent().get(&delay_key).unwrap_or(0);
        let now = env.ledger().timestamp();

        let mut total: i128 = 0;
        for i in start_idx..period_count {
            let entry_key = DataKey::PeriodEntry(offering_id.clone(), i);
            let period_id: u64 = env.storage().persistent().get(&entry_key).unwrap_or(0);
            if period_id == 0 {
                continue;
            }
            let time_key = DataKey::PeriodDepositTime(offering_id.clone(), period_id);
            let deposit_time: u64 = env.storage().persistent().get(&time_key).unwrap_or(0);
            if delay_secs > 0 && now < deposit_time.saturating_add(delay_secs) {
                break;
            }
            let rev_key = DataKey::PeriodRevenue(offering_id.clone(), period_id);
            let revenue: i128 = env.storage().persistent().get(&rev_key).unwrap_or(0);
            total += revenue * (share_bps as i128) / 10_000;
        }
        total
    }

    /// Read-only: compute claimable amount for a holder over a bounded index window.
    /// Returns `(total, next_cursor)` where `next_cursor` is `Some(next_index)` if more
    /// eligible periods exist after the processed window. `count` of 0 or > `MAX_CHUNK_PERIODS`
    /// will be capped to `MAX_CHUNK_PERIODS` to enforce limits.
    pub fn get_claimable_chunk(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        start_idx: u32,
        count: u32,
    ) -> (i128, Option<u32>) {
        let share_bps = Self::get_holder_share(
            env.clone(),
            issuer.clone(),
            namespace.clone(),
            token.clone(),
            holder.clone(),
        );
        if share_bps == 0 {
            return (0, None);
        }

        let offering_id = OfferingId { issuer, namespace, token };

        let count_key = DataKey::PeriodCount(offering_id.clone());
        let period_count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        if start_idx >= period_count {
            return (0, None);
        }

        let cap = if count == 0 || count > MAX_CHUNK_PERIODS { MAX_CHUNK_PERIODS } else { count };

        let delay_key = DataKey::ClaimDelaySecs(offering_id.clone());
        let delay_secs: u64 = env.storage().persistent().get(&delay_key).unwrap_or(0);
        let now = env.ledger().timestamp();

        let mut total: i128 = 0;
        let mut processed: u32 = 0;
        let mut idx = start_idx;

        while idx < period_count {
            if processed >= cap {
                return (total, Some(idx));
            }
            let entry_key = DataKey::PeriodEntry(offering_id.clone(), idx);
            let period_id: u64 = env.storage().persistent().get(&entry_key).unwrap_or(0);
            if period_id == 0 {
                idx = idx.saturating_add(1);
                continue;
            }
            let time_key = DataKey::PeriodDepositTime(offering_id.clone(), period_id);
            let deposit_time: u64 = env.storage().persistent().get(&time_key).unwrap_or(0);
            if delay_secs > 0 && now < deposit_time.saturating_add(delay_secs) {
                return (total, Some(idx));
            }
            let rev_key = DataKey::PeriodRevenue(offering_id.clone(), period_id);
            let revenue: i128 = env.storage().persistent().get(&rev_key).unwrap_or(0);
            total += revenue * (share_bps as i128) / 10_000;
            processed = processed.saturating_add(1);
            idx = idx.saturating_add(1);
        }

        (total, None)
    }

    // ── Time-delayed claim configuration (#27) ──────────────────

    /// Set the claim delay for an offering in seconds.
    pub fn set_claim_delay(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        delay_secs: u64,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        // Verify offering exists and issuer is current
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        issuer.require_auth();
        let key = DataKey::ClaimDelaySecs(offering_id);
        env.storage().persistent().set(&key, &delay_secs);
        env.events().publish((EVENT_CLAIM_DELAY_SET, issuer, namespace, token), delay_secs);
        Ok(())
    }

    /// Get per-offering claim delay in seconds. 0 = immediate claim.
    pub fn get_claim_delay(env: Env, issuer: Address, namespace: Symbol, token: Address) -> u64 {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::ClaimDelaySecs(offering_id);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    /// Return the total number of deposited periods for an offering.
    pub fn get_period_count(env: Env, issuer: Address, namespace: Symbol, token: Address) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        let count_key = DataKey::PeriodCount(offering_id);
        env.storage().persistent().get(&count_key).unwrap_or(0)
    }

    /// Test helper: insert a period entry and revenue without transferring tokens.
    /// Only compiled in test builds to avoid affecting production contract.
    #[cfg(test)]
    pub fn test_insert_period(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        period_id: u64,
        amount: i128,
    ) {
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        // Append to indexed period list
        let count_key = DataKey::PeriodCount(offering_id.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        let entry_key = DataKey::PeriodEntry(offering_id.clone(), count);
        env.storage().persistent().set(&entry_key, &period_id);
        env.storage().persistent().set(&count_key, &(count + 1));

        // Store period revenue and deposit time
        let rev_key = DataKey::PeriodRevenue(offering_id.clone(), period_id);
        env.storage().persistent().set(&rev_key, &amount);
        let time_key = DataKey::PeriodDepositTime(offering_id.clone(), period_id);
        let deposit_time = env.ledger().timestamp();
        env.storage().persistent().set(&time_key, &deposit_time);

        // Update cumulative deposited revenue
        let deposited_key = DataKey::DepositedRevenue(offering_id.clone());
        let deposited: i128 = env.storage().persistent().get(&deposited_key).unwrap_or(0);
        let new_deposited = deposited.saturating_add(amount);
        env.storage().persistent().set(&deposited_key, &new_deposited);
    }

    // ── On-chain distribution simulation (#29) ────────────────────

    /// Read-only: simulate distribution for sample inputs without mutating state.
    /// Returns expected payouts per holder and total. Uses offering's rounding mode.
    /// For integrators to preview outcomes before executing deposit/claim flows.
    pub fn simulate_distribution(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        amount: i128,
        holder_shares: Vec<(Address, u32)>,
    ) -> SimulateDistributionResult {
        let mode = Self::get_rounding_mode(env.clone(), issuer, namespace, token.clone());
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
    pub fn propose_issuer_transfer(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        new_issuer: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        // Get current issuer and verify offering exists
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        // Only current issuer can propose transfer
        current_issuer.require_auth();

        // Check if transfer already pending
        let pending_key = DataKey::PendingIssuerTransfer(offering_id.clone());
        if env.storage().persistent().has(&pending_key) {
            return Err(RevoraError::IssuerTransferPending);
        }

        // Store pending transfer
        env.storage().persistent().set(&pending_key, &new_issuer);

        env.events().publish(
            (EVENT_ISSUER_TRANSFER_PROPOSED, issuer, namespace, token),
            (current_issuer, new_issuer),
        );

        Ok(())
    }

    /// Accept a pending issuer transfer. Only the proposed new issuer may call this.
    pub fn accept_issuer_transfer(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Get pending transfer
        let pending_key = DataKey::PendingIssuerTransfer(offering_id.clone());
        let new_issuer: Address =
            env.storage().persistent().get(&pending_key).ok_or(RevoraError::NoTransferPending)?;

        // Only the proposed new issuer can accept
        new_issuer.require_auth();

        // Get current issuer
        let old_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        // Update the offering's issuer field in storage
        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        let old_tenant = TenantId { issuer: old_issuer.clone(), namespace: namespace.clone() };
        let new_tenant = TenantId { issuer: new_issuer.clone(), namespace: namespace.clone() };

        // Find the index of this offering in old tenant's list
        let count = Self::get_offering_count(env.clone(), old_issuer.clone(), namespace.clone());
        let mut found_index: Option<u32> = None;
        for i in 0..count {
            let item_key = DataKey::OfferItem(old_tenant.clone(), i);
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
            namespace: namespace.clone(),
            token: token.clone(),
            revenue_share_bps: offering.revenue_share_bps,
            payout_asset: offering.payout_asset,
        };

        // Remove from old issuer's storage
        let old_item_key = DataKey::OfferItem(old_tenant.clone(), index);
        env.storage().persistent().remove(&old_item_key);

        // If this wasn't the last offering, move the last offering to fill the gap
        if index < count - 1 {
            // Move the last offering to the removed index
            let last_key = DataKey::OfferItem(old_tenant.clone(), count - 1);
            let last_offering: Offering = env.storage().persistent().get(&last_key).unwrap();
            env.storage().persistent().set(&old_item_key, &last_offering);
            env.storage().persistent().remove(&last_key);
        }

        // Decrement old issuer's count
        let old_count_key = DataKey::OfferCount(old_tenant.clone());
        env.storage().persistent().set(&old_count_key, &(count - 1));

        // Add to new issuer's storage
        let new_count =
            Self::get_offering_count(env.clone(), new_issuer.clone(), namespace.clone());
        let new_item_key = DataKey::OfferItem(new_tenant.clone(), new_count);
        env.storage().persistent().set(&new_item_key, &updated_offering);

        // Increment new issuer's count
        let new_count_key = DataKey::OfferCount(new_tenant.clone());
        env.storage().persistent().set(&new_count_key, &(new_count + 1));

        // Update reverse lookup and supply cap keys (they use OfferingId which has issuer)
        // Wait, does OfferingId change? YES, because issuer is part of OfferingId!
        // This is tricky. If we change the issuer, the data keys for this offering CHANGE!
        // THIS IS A MAJOR PROBLEM. The data (blacklist, revenue, etc.) is tied to (issuer, namespace, token).
        // If we transfer the issuer, do we move all the data?
        // Or do we say OfferingId is (original_issuer, namespace, token)? No, that's not good.

        // Actually, if we transfer issuer, the OfferingId for the new issuer will be different.
        // We SHOULD probably move all namespaced data or just update the OfferingIssuer mapping.

        // Let's look at DataKey again. OfferingIssuer(OfferingId).
        // If we want to keep the data, maybe OfferingId should NOT include the issuer?
        // But the requirement said: "Partition on-chain data based on an issuer identifier (e.g., an address) and a namespace ID (e.g., a symbol)."

        // If issuer A transfers to issuer B, and both are in the SAME namespace,
        // they might want to keep the same token's data.

        // If we use OfferingId { issuer, namespace, token } as key, transferring issuer is basically DELETING the old offering and CREATING a new one.

        // Wait, I should probably use a stable internal ID if I want to support issuer transfers.
        // But the current implementation uses (issuer, token) as key in many places.

        // If I change (issuer, token) to OfferingId { issuer, namespace, token }, then issuer transfer becomes very expensive (must move all keys).

        // LET'S ASSUME FOR NOW THAT ISSUER TRANSFER UPDATES THE REVERSE LOOKUP and we just deal with the fact that old data is under the old OfferingId.
        // Actually, that's not good.

        // THE BEST WAY is for the OfferingId to be (namespace, token) ONLY, IF (namespace, token) is unique.
        // Is (namespace, token) unique across the whole contract?
        // The requirement says: "Offerings: Partition by namespace."
        // An issuer can have multiple namespaces.
        // Usually, a token address is unique on-chain.
        // If multiple issuers try to register the SAME token in DIFFERENT namespaces, is that allowed?
        // Requirement 1.2: "Enable partitioning of data... Allowing multiple issuers to manage their offerings independently."

        // If Issuer A and Issuer B both register Token T, they should be isolated.
        // So (Issuer, Namespace, Token) IS the unique identifier.

        // If Issuer A transfers Token T to Issuer B, it's effectively a new (Issuer, Namespace, Token) tuple.

        // For now, I'll follow the logical conclusion: issuer transfer in a multi-tenant system with issuer-based partitioning is basically migrating the data or creating a new partition.

        // But wait, the original code had `OfferingIssuer(token)`.
        // I changed it to `OfferingIssuer(OfferingId)`.

        // I'll update the OfferingIssuer lookup for the NEW OfferingId but the old data remains under the old OfferingId unless I migrate it.
        // Migrating data is too expensive in Soroban.

        // Maybe I should RECONSIDER OfferingId.
        // If OfferingId was (namespace, token), then issuer transfer would just update the `OfferingIssuer` lookup.
        // But can different issuers use the same (namespace, token)?
        // Probably not if namespaces are shared. But if namespaces are PRIVATE to issuers?
        // "Multiple issuers to manage their offerings independently."

        // If Namespace "STOCKS" is used by Issuer A and Issuer B, they should be isolated.
        // So OfferingId MUST include issuer.

        // Okay, I'll stick with OfferingId including issuer. Issuer transfer will be a "new" offering from the storage perspective.

        let new_offering_id = OfferingId {
            issuer: new_issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let issuer_lookup_key = DataKey::OfferingIssuer(new_offering_id);
        env.storage().persistent().set(&issuer_lookup_key, &new_issuer);

        // Clear pending transfer
        env.storage().persistent().remove(&pending_key);

        env.events().publish(
            (EVENT_ISSUER_TRANSFER_ACCEPTED, issuer, namespace, token),
            (old_issuer, new_issuer),
        );

        Ok(())
    }

    /// Cancel a pending issuer transfer. Only the current issuer may call this.
    pub fn cancel_issuer_transfer(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        // Get current issuer
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        // Only current issuer can cancel
        current_issuer.require_auth();

        let offering_id = OfferingId { issuer, namespace, token };

        // Check if transfer is pending
        let pending_key = DataKey::PendingIssuerTransfer(offering_id.clone());
        let proposed_new_issuer: Address =
            env.storage().persistent().get(&pending_key).ok_or(RevoraError::NoTransferPending)?;

        // Clear pending transfer
        env.storage().persistent().remove(&pending_key);

        env.events().publish(
            (
                EVENT_ISSUER_TRANSFER_CANCELLED,
                offering_id.issuer,
                offering_id.namespace,
                offering_id.token,
            ),
            (current_issuer, proposed_new_issuer),
        );

        Ok(())
    }

    /// Get the pending issuer transfer for an offering, if any.
    pub fn get_pending_issuer_transfer(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<Address> {
        let offering_id = OfferingId { issuer, namespace, token };
        let pending_key = DataKey::PendingIssuerTransfer(offering_id);
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
        namespace: Symbol,
        token: Address,
        total_revenue: i128,
        total_supply: i128,
        holder_balance: i128,
        holder: Address,
    ) -> Result<i128, RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        caller.require_auth();

        Self::require_non_negative_amount(total_revenue)?;
        Self::require_non_negative_amount(total_supply)?;
        Self::require_non_negative_amount(holder_balance)?;

        if total_supply == 0 {
            return Err(RevoraError::InvalidAmount);
        }

        let offering = Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
            .ok_or(RevoraError::OfferingNotFound)?;

        // Verify auth: caller must be current issuer, admin, or the holder themselves
        let admin = Self::get_admin(env.clone()).ok_or(RevoraError::NotInitialized)?;
        let current_issuer = offering.issuer.clone();

        if caller != current_issuer && caller != admin && caller != holder {
            return Err(RevoraError::NotAuthorized);
        }

        if Self::is_blacklisted(
            env.clone(),
            issuer.clone(),
            offering.namespace.clone(),
            token.clone(),
            holder.clone(),
        ) {
            return Err(RevoraError::HolderBlacklisted);
        }

        if total_revenue == 0 || holder_balance == 0 {
            let payout = 0i128;
            env.events().publish(
                (EVENT_DIST_CALC, issuer, offering.namespace, token),
                (
                    holder.clone(),
                    total_revenue,
                    total_supply,
                    holder_balance,
                    offering.revenue_share_bps,
                    payout,
                ),
            );
            return Ok(payout);
        }

        // Precise payout calculation using I256 intermediate:
        // payout = (holder_balance * total_revenue * revenue_share_bps) / (total_supply * BPS_DENOMINATOR)
        
        let h_bal = I256::from_i128(&env, holder_balance);
        let t_rev = I256::from_i128(&env, total_revenue);
        let bps = I256::from_i128(&env, offering.revenue_share_bps as i128);
        let t_sup = I256::from_i128(&env, total_supply);
        let denom_bps = I256::from_i128(&env, BPS_DENOMINATOR as i128);

        // (h_bal * t_rev * bps) / (t_sup * denom_bps)
        let numerator = h_bal.mul(&t_rev).mul(&bps);
        let denominator = t_sup.mul(&denom_bps);
        let payout_256 = numerator.div(&denominator);

        let payout = payout_256.to_i128().ok_or(RevoraError::InvalidAmount)?;

        env.events().publish(
            (EVENT_DIST_CALC, issuer, offering.namespace, token),
            (
                holder,
                total_revenue,
                total_supply,
                holder_balance,
                offering.revenue_share_bps,
                payout,
            ),
        );

        Ok(payout)
    }

    /// Calculate the total distributable revenue for an offering.
    ///
    /// This is a helper function for off-chain verification.
    pub fn calculate_total_distributable(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        total_revenue: i128,
    ) -> Result<i128, RevoraError> {
        let offering = Self::get_offering(env, issuer, namespace, token)
            .ok_or(RevoraError::OfferingNotFound)?;

        if total_revenue == 0 {
            return Ok(0);
        }

        let total = total_revenue
            .checked_mul(offering.revenue_share_bps as i128)
            .and_then(|v| v.checked_div(BPS_DENOMINATOR))
            .ok_or(RevoraError::InvalidAmount)?;
        
        Ok(total)
    }

    // ── Per-offering metadata storage (#8) ─────────────────────

    /// Maximum allowed length for metadata strings (256 bytes).
    /// Supports IPFS CIDs (46 chars), URLs, and content hashes.
    const MAX_METADATA_LENGTH: usize = 256;
    const META_SCHEME_IPFS: &'static [u8] = b"ipfs://";
    const META_SCHEME_HTTPS: &'static [u8] = b"https://";
    const META_SCHEME_AR: &'static [u8] = b"ar://";
    const META_SCHEME_SHA256: &'static [u8] = b"sha256:";

    fn has_prefix(bytes: &[u8], prefix: &[u8]) -> bool {
        if bytes.len() < prefix.len() {
            return false;
        }
        for i in 0..prefix.len() {
            if bytes[i] != prefix[i] {
                return false;
            }
        }
        true
    }

    fn validate_metadata_reference(metadata: &String) -> Result<(), RevoraError> {
        if metadata.len() == 0 {
            return Ok(());
        }
        if metadata.len() > Self::MAX_METADATA_LENGTH as u32 {
            return Err(RevoraError::MetadataTooLarge);
        }
        let mut bytes = [0u8; Self::MAX_METADATA_LENGTH];
        let len = metadata.len() as usize;
        metadata.copy_into_slice(&mut bytes[0..len]);
        let slice = &bytes[0..len];
        if Self::has_prefix(slice, Self::META_SCHEME_IPFS)
            || Self::has_prefix(slice, Self::META_SCHEME_HTTPS)
            || Self::has_prefix(slice, Self::META_SCHEME_AR)
            || Self::has_prefix(slice, Self::META_SCHEME_SHA256)
        {
            return Ok(());
        }
        Err(RevoraError::MetadataInvalidFormat)
    }

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
        namespace: Symbol,
        token: Address,
        metadata: String,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;

        // Verify offering exists and issuer is current
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        issuer.require_auth();

        // Validate metadata length and allowed scheme prefixes.
        Self::validate_metadata_reference(&metadata)?;

        let key = DataKey::OfferingMetadata(offering_id);
        let is_update = env.storage().persistent().has(&key);

        // Store metadata
        env.storage().persistent().set(&key, &metadata);

        // Emit appropriate event
        if is_update {
            env.events().publish((EVENT_METADATA_UPDATED, issuer, namespace, token), metadata);
        } else {
            env.events().publish((EVENT_METADATA_SET, issuer, namespace, token), metadata);
        }

        Ok(())
    }

    /// Retrieve metadata reference for an offering.
    pub fn get_offering_metadata(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<String> {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::OfferingMetadata(offering_id);
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
        let mut total_reported: i128 = 0;
        let mut total_deposited: i128 = 0;
        let mut total_reports: u64 = 0;
        let mut total_offerings: u32 = 0;

        let ns_count_key = DataKey::NamespaceCount(issuer.clone());
        let ns_count: u32 = env.storage().persistent().get(&ns_count_key).unwrap_or(0);

        for ns_idx in 0..ns_count {
            let ns_key = DataKey::NamespaceItem(issuer.clone(), ns_idx);
            let namespace: Symbol = env.storage().persistent().get(&ns_key).unwrap();

            let tenant_id = TenantId { issuer: issuer.clone(), namespace: namespace.clone() };
            let count = Self::get_offering_count(env.clone(), issuer.clone(), namespace.clone());
            total_offerings = total_offerings.saturating_add(count);

            for i in 0..count {
                let item_key = DataKey::OfferItem(tenant_id.clone(), i);
                let offering: Offering = env.storage().persistent().get(&item_key).unwrap();
                let offering_id = OfferingId {
                    issuer: issuer.clone(),
                    namespace: namespace.clone(),
                    token: offering.token.clone(),
                };

                // Sum audit summary (reported revenue)
                let summary_key = DataKey::AuditSummary(offering_id.clone());
                if let Some(summary) =
                    env.storage().persistent().get::<DataKey, AuditSummary>(&summary_key)
                {
                    total_reported = total_reported.saturating_add(summary.total_revenue);
                    total_reports = total_reports.saturating_add(summary.report_count);
                }

                // Sum deposited revenue
                let deposited_key = DataKey::DepositedRevenue(offering_id);
                let deposited: i128 = env.storage().persistent().get(&deposited_key).unwrap_or(0);
                total_deposited = total_deposited.saturating_add(deposited);
            }
        }

        AggregatedMetrics {
            total_reported_revenue: total_reported,
            total_deposited_revenue: total_deposited,
            total_report_count: total_reports,
            offering_count: total_offerings,
        }
    }

    /// Aggregate metrics across all issuers (platform-wide).
    /// Iterates the global issuer registry, capped at MAX_AGGREGATION_ISSUERS for gas safety.
    pub fn get_platform_aggregation(env: Env) -> AggregatedMetrics {
        let issuer_count_key = DataKey::IssuerCount;
        let issuer_count: u32 = env.storage().persistent().get(&issuer_count_key).unwrap_or(0);

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
        let issuer_count: u32 = env.storage().persistent().get(&issuer_count_key).unwrap_or(0);

        let cap = core::cmp::min(issuer_count, Self::MAX_AGGREGATION_ISSUERS);
        let mut issuers = Vec::new(&env);

        for i in 0..cap {
            let issuer_item_key = DataKey::IssuerItem(i);
            let issuer: Address = env.storage().persistent().get(&issuer_item_key).unwrap();
            issuers.push_back(issuer);
        }
        issuers
    }

    /// Return the total deposited revenue for a specific offering.
    pub fn get_total_deposited_revenue(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> i128 {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::DepositedRevenue(offering_id);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    // ── Platform fee configuration (#6) ────────────────────────

    /// Set the platform fee in basis points.  Admin-only.
    /// Maximum value is 5 000 bps (50 %).  Pass 0 to disable.
    pub fn set_platform_fee(env: Env, fee_bps: u32) -> Result<(), RevoraError> {
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::LimitReached)?;
        admin.require_auth();

        if fee_bps > MAX_PLATFORM_FEE_BPS {
            return Err(RevoraError::LimitReached);
        }

        env.storage().persistent().set(&DataKey::PlatformFeeBps, &fee_bps);
        Ok(())
    }

    /// Return the current platform fee in basis points (default 0).
    pub fn get_platform_fee(env: Env) -> u32 {
        env.storage().persistent().get(&DataKey::PlatformFeeBps).unwrap_or(0)
    }

    /// Calculate the platform fee for a given amount.
    pub fn calculate_platform_fee(env: Env, amount: i128) -> i128 {
        let fee_bps = Self::get_platform_fee(env) as i128;
        (amount * fee_bps).checked_div(BPS_DENOMINATOR).unwrap_or(0)
    }

    // ── Multi-currency fee config (#98) ───────────────────────

    /// Set per-offering per-asset fee in bps. Issuer only. Max 5000 (50%).
    pub fn set_offering_fee_bps(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        asset: Address,
        fee_bps: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        issuer.require_auth();
        if fee_bps > MAX_PLATFORM_FEE_BPS {
            return Err(RevoraError::LimitReached);
        }
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let key = DataKey::OfferingFeeBps(offering_id, asset.clone());
        env.storage().persistent().set(&key, &fee_bps);
        env.events().publish((EVENT_FEE_CONFIG, issuer, namespace, token), (asset, fee_bps, true));
        Ok(())
    }

    /// Set platform-level per-asset fee in bps. Admin only. Overrides global platform fee for this asset.
    pub fn set_platform_fee_per_asset(
        env: Env,
        admin: Address,
        asset: Address,
        fee_bps: u32,
    ) -> Result<(), RevoraError> {
        admin.require_auth();
        let stored_admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::LimitReached)?;
        if admin != stored_admin {
            return Err(RevoraError::NotAuthorized);
        }
        if fee_bps > MAX_PLATFORM_FEE_BPS {
            return Err(RevoraError::LimitReached);
        }
        env.storage().persistent().set(&DataKey::PlatformFeePerAsset(asset.clone()), &fee_bps);
        env.events().publish((EVENT_FEE_CONFIG, admin, asset), (fee_bps, false));
        Ok(())
    }

    /// Effective fee bps for (offering, asset). Precedence: offering fee > platform per-asset > global platform fee.
    pub fn get_effective_fee_bps(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        asset: Address,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        let offering_key = DataKey::OfferingFeeBps(offering_id, asset.clone());
        if let Some(bps) = env.storage().persistent().get::<DataKey, u32>(&offering_key) {
            return bps;
        }
        let platform_asset_key = DataKey::PlatformFeePerAsset(asset);
        if let Some(bps) = env.storage().persistent().get::<DataKey, u32>(&platform_asset_key) {
            return bps;
        }
        env.storage().persistent().get(&DataKey::PlatformFeeBps).unwrap_or(0)
    }

    /// Calculate fee for (offering, asset, amount) using effective fee bps.
    pub fn calculate_fee_for_asset(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        asset: Address,
        amount: i128,
    ) -> i128 {
        let fee_bps = Self::get_effective_fee_bps(env, issuer, namespace, token, asset) as i128;
        (amount * fee_bps).checked_div(BPS_DENOMINATOR).unwrap_or(0)
    }

    /// Return the current contract version (#23). Used for upgrade compatibility and migration.
    pub fn get_version(env: Env) -> u32 {
        let _ = env;
        CONTRACT_VERSION
    }
}

pub mod vesting;

#[cfg(test)]
mod vesting_test;

#[cfg(test)]
mod test_utils;
#[cfg(test)]
mod chunking_tests;
#[cfg(test)]
mod test;
#[cfg(test)]
mod test_auth;
#[cfg(test)]
mod test_cross_contract;
#[cfg(test)]
mod test_namespaces;
