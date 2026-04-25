#![no_std]
#![deny(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::todo, clippy::unimplemented)]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, xdr::ToXdr, Address,
    BytesN, Env, IntoVal, Map, String, Symbol, Vec,
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
    /// Overriding an existing revenue report.
    OutdatedSnapshot = 13,
    /// Payout asset mismatch.
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
    /// period_id not strictly greater than previous (violates ordering invariant).
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
    /// Multisig proposal has expired.
    ProposalExpired = 30,
    /// Cross-contract token transfer failed.
    TransferFailed = 30,
    /// Contract is already at the target version; no migration needed.
    AlreadyAtTargetVersion = 31,
    /// Target version is lower than the current deployed version.
    MigrationDowngradeNotAllowed = 32,
    /// Admin rotation failed: new admin cannot be the same as current.
    AdminRotationSameAddress = 33,
    /// Admin rotation failed: another rotation is already pending.
    AdminRotationPending = 34,
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
pub const EVENT_SCHEMA_VERSION_V2: u32 = 2;

// Versioned event symbols (v2). All core events emit with leading `version` field.
const EVENT_OFFER_REG_V2: Symbol = symbol_short!("ofr_reg2");
const EVENT_REV_INIT_V2: Symbol = symbol_short!("rv_init2");
const EVENT_REV_INIA_V2: Symbol = symbol_short!("rv_inia2");
const EVENT_REV_REP_V2: Symbol = symbol_short!("rv_rep2");
const EVENT_REV_REPA_V2: Symbol = symbol_short!("rv_repa2");
const EVENT_REV_DEPOSIT_V2: Symbol = symbol_short!("rev_dep2");
const EVENT_REV_DEP_SNAP_V2: Symbol = symbol_short!("rev_snp2");
const EVENT_CLAIM_V2: Symbol = symbol_short!("claim2");
const EVENT_SHARE_SET_V2: Symbol = symbol_short!("sh_set2");
const EVENT_FREEZE_V2: Symbol = symbol_short!("frz2");
const EVENT_CLAIM_DELAY_SET_V2: Symbol = symbol_short!("dly_set2");
const EVENT_CONCENTRATION_WARNING_V2: Symbol = symbol_short!("conc2");

const EVENT_PROPOSAL_CREATED_V2: Symbol = symbol_short!("prop_n2");
const EVENT_PROPOSAL_APPROVED_V2: Symbol = symbol_short!("prop_a2");
const EVENT_PROPOSAL_EXECUTED_V2: Symbol = symbol_short!("prop_e2");
const EVENT_PROPOSAL_APPROVED: Symbol = symbol_short!("prop_app");
const EVENT_PROPOSAL_EXECUTED: Symbol = symbol_short!("prop_exe");
const EVENT_DURATION_SET: Symbol = symbol_short!("dur_set");

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(test, derive(proptest::prelude::Arbitrary))]
pub enum ProposalAction {
    SetAdmin(Address),
    Freeze,
    SetThreshold(u32),
    AddOwner(Address),
    RemoveOwner(Address),
    SetProposalDuration(u64),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub id: u32,
    pub action: ProposalAction,
    pub proposer: Address,
    pub approvals: Vec<Address>,
    pub executed: bool,
    pub expiry: u64,
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
const EVENT_MULTISIG_INIT: Symbol = symbol_short!("ms_init");
const EVENT_META_REV_APPROVE: Symbol = symbol_short!("meta_rev");
/// Emitted when `repair_audit_summary` writes a corrected `AuditSummary` to storage.
const EVENT_AUDIT_REPAIRED: Symbol = symbol_short!("aud_rep");

/// Current schema for `EVENT_INDEXED_V2` topics.
const INDEXER_EVENT_SCHEMA_VERSION: u32 = 2;

const EVENT_CONC_LIMIT_SET: Symbol = symbol_short!("conc_lim");
const EVENT_ROUNDING_MODE_SET: Symbol = symbol_short!("rnd_mode");
const EVENT_ADMIN_SET: Symbol = symbol_short!("admin_set");
const EVENT_PLATFORM_FEE_SET: Symbol = symbol_short!("fee_set");
const BPS_DENOMINATOR: i128 = 10_000;
/// Stellar network canonical decimal precision (7 decimal places, i.e., stroops).
const STELLAR_CANONICAL_DECIMALS: u32 = 7;
/// Maximum accepted decimal precision (safety cap for normalization math).
const MAX_TOKEN_DECIMALS: u32 = 18;

// ── Missing legacy/v1 event symbols ──────────────────────────
/// v1 schema version tag (legacy; v2 is the current standard).
pub const EVENT_SCHEMA_VERSION: u32 = 1;
const EVENT_SHARE_SET: Symbol = symbol_short!("sh_set");
const EVENT_OFFER_REG_V1: Symbol = symbol_short!("ofr_reg1");
const EVENT_REV_INIT_V1: Symbol = symbol_short!("rv_init1");
const EVENT_CONCENTRATION_WARNING: Symbol = symbol_short!("conc_wrn");
const EVENT_CONCENTRATION_REPORTED: Symbol = symbol_short!("conc_rep");
const EVENT_SNAP_COMMIT: Symbol = symbol_short!("snap_cmt");
const EVENT_SNAP_SHARES_APPLIED: Symbol = symbol_short!("snap_shr");
const EVENT_FREEZE_OFFERING: Symbol = symbol_short!("frz_off");
const EVENT_UNFREEZE_OFFERING: Symbol = symbol_short!("ufrz_off");
const EVENT_PROPOSAL_CREATED: Symbol = symbol_short!("prop_new");
const EVENT_FREEZE: Symbol = symbol_short!("freeze");
/// Issuer transfer expiry: 7 days in seconds.
const ISSUER_TRANSFER_EXPIRY_SECS: u64 = 7 * 24 * 60 * 60;
const EVENT_CLAIM: Symbol = symbol_short!("claim");
const EVENT_CLAIM_DELAY_SET: Symbol = symbol_short!("dly_set");

/// Represents a revenue-share offering registered on-chain.
/// Offerings are immutable once registered.
// ── Data structures ──────────────────────────────────────────
/// Contract version identifier (#23). Bumped when storage or semantics change; used for migration and compatibility.
pub const CONTRACT_VERSION: u32 = 4;

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

/// Pending issuer transfer details including expiry tracking.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PendingTransfer {
    pub new_issuer: Address,
    pub timestamp: u64,
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

/// Immutable record of a committed snapshot for an offering.
///
/// A snapshot captures the canonical state of holder shares at a specific point in time,
/// identified by a monotonically increasing `snapshot_ref`. Once committed, the entry
/// is write-once: subsequent calls with the same `snapshot_ref` are rejected.
///
/// The `content_hash` field is a 32-byte SHA-256 (or equivalent) digest of the off-chain
/// holder-share dataset. It is provided by the issuer and stored verbatim; the contract
/// does not recompute it. Integrators MUST verify the hash off-chain before trusting
/// the snapshot data.
///
/// Security assumption: the issuer is trusted to supply a correct `content_hash`.
/// The contract enforces monotonicity and write-once semantics; it does NOT verify
/// that `content_hash` matches the on-chain holder entries written by `apply_snapshot_shares`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SnapshotEntry {
    /// Monotonically increasing snapshot identifier (must be > previous snapshot_ref).
    pub snapshot_ref: u64,
    /// Ledger timestamp at commit time (set by the contract, not the caller).
    pub committed_at: u64,
    /// Off-chain content hash of the holder-share dataset (32 bytes, caller-supplied).
    pub content_hash: BytesN<32>,
    /// Total number of holder entries recorded in this snapshot.
    pub holder_count: u32,
    /// Total basis points across all holders (informational; not enforced on-chain).
    pub total_bps: u32,
}

/// Primary storage keys for core contract state.
/// Split from the full key set to stay within the Soroban XDR union variant limit (≤50).
#[contracttype]
pub enum DataKey {
    /// Last deposited/reported period_id for offering (enforces strictly increasing ordering).
    LastPeriodId(OfferingId),
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
    /// Proposed new admin address (pending two-step rotation).
    PendingAdmin,

    /// Multisig admin threshold.
    MultisigThreshold,
    /// Multisig admin owners.
    MultisigOwners,
    /// Multisig proposal by ID.
    MultisigProposal(u32),
    /// Multisig proposal count.
    MultisigProposalCount,
    /// Multisig proposal duration in seconds.
    MultisigProposalDuration,

    /// Whether snapshot distribution is enabled for an offering.
    SnapshotConfig(OfferingId),
    /// Latest recorded snapshot reference for an offering.
    LastSnapshotRef(OfferingId),
    /// Committed snapshot entry keyed by (offering_id, snapshot_ref).
    SnapshotEntry(OfferingId, u64),
    /// Per-snapshot holder share at index N.
    SnapshotHolder(OfferingId, u64, u32),
    /// Total number of holders recorded in a snapshot.
    SnapshotHolderCount(OfferingId, u64),

    /// Pending issuer transfer for an offering.
    PendingIssuerTransfer(OfferingId),
    /// Current issuer lookup by offering token.
    OfferingIssuer(OfferingId),
    /// Testnet mode flag.
    TestnetMode,

    /// Safety role address for emergency pause (#7).
    Safety,
    /// Global pause flag.
    Paused,

    /// Configuration flag: when true, contract is event-only (no persistent business state).
    EventOnlyMode,

    /// Metadata reference for an offering.
    OfferingMetadata(OfferingId),
    /// Platform fee in basis points.
    PlatformFeeBps,
    /// Per-offering per-asset fee override (#98).
    OfferingFeeBps(OfferingId, Address),
    /// Platform level per-asset fee (#98).
    PlatformFeePerAsset(Address),

    /// Per-offering minimum revenue threshold (#25).
    MinRevenueThreshold(OfferingId),
    /// Total deposited revenue for an offering (#39).
    DepositedRevenue(OfferingId),
    /// Per-offering supply cap (#96). 0 = no cap.
    SupplyCap(OfferingId),
    /// Per-offering investment constraints (#97).
    InvestmentConstraints(OfferingId),
}

/// Secondary storage keys for auxiliary/extended contract state.
/// Overflow enum to keep DataKey within the Soroban XDR union variant limit.
#[contracttype]
pub enum DataKey2 {
    /// Global count of unique issuers (#39).
    IssuerCount,
    /// Issuer address at global index (#39).
    IssuerItem(u32),
    /// Whether an issuer is already registered in the global registry (#39).
    IssuerRegistered(Address),

    /// Per-issuer namespace tracking.
    NamespaceCount(Address),
    NamespaceItem(Address, u32),
    NamespaceRegistered(Address, Symbol),

    /// DataKey for testing storage boundaries without affecting business state.
    StressDataEntry(Address, u32),
    /// Tracks total amount of dummy data allocated per admin.
    StressDataCount(Address),
    /// Packed flags: (event_versioning_enabled: bool, event_only_mode: bool).
    ContractFlags,
}

/// Maximum number of offerings returned in a single page.
const MAX_PAGE_LIMIT: u32 = 20;

/// Maximum number of addresses that can be blacklisted per offering.
/// Prevents unbounded storage growth and keeps distribution gas predictable.
/// Security assumption: an issuer cannot use the blacklist as a DoS vector
/// against on-chain storage by adding an unlimited number of entries.
const MAX_BLACKLIST_SIZE: u32 = 200;

/// Maximum platform fee in basis points (50%).
const MAX_PLATFORM_FEE_BPS: u32 = 5_000;

/// Maximum number of periods that can be claimed in a single transaction.
/// Keeps compute costs predictable within Soroban limits.
const MAX_CLAIM_PERIODS: u32 = 50;

/// Maximum number of periods allowed in a single read-only chunked query.
/// This is a safety cap to prevent accidental long-running loops in read-only methods.
const MAX_CHUNK_PERIODS: u32 = 200;

// ── Negative Amount Validation Matrix (#163) ───────────────────

/// Categories of amount validation contexts in the contract.
/// Each category has specific rules for what constitutes a valid amount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmountValidationCategory {
    /// Revenue deposit: amount must be strictly positive (> 0).
    /// Reason: Depositing zero or negative tokens has no economic meaning.
    RevenueDeposit,
    /// Revenue report: amount can be zero but not negative (>= 0).
    /// Reason: Zero revenue is valid (no distribution triggered); negative is impossible.
    RevenueReport,
    /// Holder share allocation: amount can be zero but not negative (>= 0).
    /// Reason: Zero share means no allocation; negative share is invalid.
    HolderShare,
    /// Minimum revenue threshold: must be non-negative (>= 0).
    /// Reason: Threshold of zero means no minimum; negative threshold is nonsensical.
    MinRevenueThreshold,
    /// Supply cap configuration: must be non-negative (>= 0).
    /// Reason: Zero cap means unlimited; negative cap is invalid.
    SupplyCap,
    /// Investment constraints (min_stake): must be non-negative (>= 0).
    /// Reason: Minimum stake cannot be negative.
    InvestmentMinStake,
    /// Investment constraints (max_stake): must be non-negative (>= 0) and >= min_stake.
    /// Reason: Maximum stake must be valid range; zero means unlimited.
    InvestmentMaxStake,
    /// Snapshot reference: must be positive (> 0) and strictly increasing.
    /// Reason: Zero is invalid; must be strictly monotonic.
    SnapshotReference,
    /// Period ID: unsigned, but some contexts require > 0.
    /// Reason: Period 0 may be ambiguous in some business logic.
    PeriodId,
    /// Generic distribution simulation: any i128 is valid (can be negative for modeling).
    /// Reason: Simulation-only, no state mutation.
    Simulation,
}

/// Result of amount validation with detailed classification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmountValidationResult {
    /// The original amount that was validated.
    pub amount: i128,
    /// The category of validation applied.
    pub category: AmountValidationCategory,
    /// Whether the amount passed validation.
    pub is_valid: bool,
    /// Specific error code if validation failed.
    pub error_code: Option<u32>,
    /// Human-readable description of why validation passed/failed.
    pub reason: Symbol,
}

impl AmountValidationResult {
    fn new(
        amount: i128,
        category: AmountValidationCategory,
        is_valid: bool,
        error_code: Option<u32>,
        reason: Symbol,
    ) -> Self {
        Self { amount, category, is_valid, error_code, reason }
    }
}

/// Event symbol emitted when amount validation fails.
const EVENT_AMOUNT_VALIDATION_FAILED: Symbol = symbol_short!("amt_valid");

/// Centralized amount validation matrix for all contract operations.
///
/// This matrix defines deterministic validation rules for amounts across different
/// contract contexts, ensuring consistent handling of edge cases like zero and
/// negative values. The matrix is stateless and pure - it only validates,
/// it does not modify storage.
pub struct AmountValidationMatrix;

impl AmountValidationMatrix {
    /// Validate an amount against the specified category's rules.
    ///
    /// # Arguments
    /// * `amount` - The i128 amount to validate
    /// * `category` - The validation context/category
    ///
    /// # Returns
    /// * `Ok(())` if validation passes
    /// * `Err((RevoraError, Symbol))` with specific error and reason if validation fails
    ///
    /// # Security Properties
    /// - All negative amounts are rejected in deposit contexts
    /// - Zero is allowed where semantically meaningful (reports, shares)
    /// - Overflow-protected comparisons via saturating arithmetic where needed
    pub fn validate(
        amount: i128,
        category: AmountValidationCategory,
    ) -> Result<(), (RevoraError, Symbol)> {
        match category {
            AmountValidationCategory::RevenueDeposit => {
                if amount <= 0 {
                    return Err((RevoraError::InvalidAmount, symbol_short!("must_pos")));
                }
            }
            AmountValidationCategory::RevenueReport => {
                if amount < 0 {
                    return Err((RevoraError::InvalidAmount, symbol_short!("no_neg")));
                }
            }
            AmountValidationCategory::HolderShare => {
                if amount < 0 {
                    return Err((RevoraError::InvalidAmount, symbol_short!("no_neg")));
                }
            }
            AmountValidationCategory::MinRevenueThreshold => {
                if amount < 0 {
                    return Err((RevoraError::InvalidAmount, symbol_short!("no_neg")));
                }
            }
            AmountValidationCategory::SupplyCap => {
                if amount < 0 {
                    return Err((RevoraError::InvalidAmount, symbol_short!("no_neg")));
                }
            }
            AmountValidationCategory::InvestmentMinStake => {
                if amount < 0 {
                    return Err((RevoraError::InvalidAmount, symbol_short!("no_neg")));
                }
            }
            AmountValidationCategory::InvestmentMaxStake => {
                if amount < 0 {
                    return Err((RevoraError::InvalidAmount, symbol_short!("no_neg")));
                }
            }
            AmountValidationCategory::SnapshotReference => {
                if amount <= 0 {
                    return Err((RevoraError::InvalidAmount, symbol_short!("snap_pos")));
                }
            }
            AmountValidationCategory::PeriodId => {
                if amount < 0 {
                    return Err((RevoraError::InvalidPeriodId, symbol_short!("no_neg")));
                }
            }
            AmountValidationCategory::Simulation => {}
        }
        Ok(())
    }

    /// Validate that max_stake >= min_stake when both are provided.
    ///
    /// # Arguments
    /// * `min_stake` - The minimum stake value
    /// * `max_stake` - The maximum stake value
    ///
    /// # Returns
    /// * `Ok(())` if min <= max
    /// * `Err(RevoraError::InvalidAmount)` if min > max
    pub fn validate_stake_range(min_stake: i128, max_stake: i128) -> Result<(), RevoraError> {
        if max_stake > 0 && min_stake > max_stake {
            return Err(RevoraError::InvalidAmount);
        }
        Ok(())
    }

    /// Validate that snapshot reference is strictly increasing.
    ///
    /// # Arguments
    /// * `new_ref` - The new snapshot reference
    /// * `last_ref` - The last recorded snapshot reference
    ///
    /// # Returns
    /// * `Ok(())` if new_ref > last_ref
    /// * `Err(RevoraError::OutdatedSnapshot)` if new_ref <= last_ref
    pub fn validate_snapshot_monotonic(new_ref: i128, last_ref: i128) -> Result<(), RevoraError> {
        if new_ref <= last_ref {
            return Err(RevoraError::OutdatedSnapshot);
        }
        Ok(())
    }

    /// Get a detailed validation result for an amount.
    ///
    /// Unlike `validate()`, this always returns a result struct with full context.
    pub fn validate_detailed(
        amount: i128,
        category: AmountValidationCategory,
    ) -> AmountValidationResult {
        let (is_valid, error_code, reason) = match Self::validate(amount, category) {
            Ok(()) => (true, None, symbol_short!("valid")),
            Err((err, reason)) => (false, Some(err as u32), reason),
        };
        AmountValidationResult::new(amount, category, is_valid, error_code, reason)
    }

    /// Batch validate multiple amounts against the same category.
    ///
    /// Returns the first failing index, or None if all pass.
    pub fn validate_batch(amounts: &[i128], category: AmountValidationCategory) -> Option<usize> {
        for (i, &amount) in amounts.iter().enumerate() {
            if Self::validate(amount, category).is_err() {
                return Some(i);
            }
        }
        None
    }

    /// Get the default validation category for a given function name (for testing/debugging).
    ///
    /// This is a best-effort mapping; some functions have multiple amount parameters
    /// with different validation requirements.
    pub fn category_for_function(fn_name: &str) -> Option<AmountValidationCategory> {
        match fn_name {
            "deposit_revenue" => Some(AmountValidationCategory::RevenueDeposit),
            "report_revenue" => Some(AmountValidationCategory::RevenueReport),
            "set_holder_share" => Some(AmountValidationCategory::HolderShare),
            "set_min_revenue_threshold" => Some(AmountValidationCategory::MinRevenueThreshold),
            "set_investment_constraints" => Some(AmountValidationCategory::InvestmentMinStake),
            "simulate_distribution" => Some(AmountValidationCategory::Simulation),
            _ => None,
        }
    }
}

// ── Contract ─────────────────────────────────────────────────
#[contract]
pub struct RevoraRevenueShare;

#[contractimpl]
impl RevoraRevenueShare {
    const META_AUTH_VERSION: u32 = 1;

    /// Returns error if contract is frozen (#32). Call at start of state-mutating entrypoints.
    fn require_not_frozen(env: &Env) -> Result<(), RevoraError> {
        let key = DataKey::Frozen;
        if env.storage().persistent().get::<DataKey, bool>(&key).unwrap_or(false) {
            return Err(RevoraError::ContractFrozen);
        }
        Ok(())
    }

    /// Helper to emit deterministic v2 versioned events for core event versioning.
    /// Emits: topic -> (EVENT_SCHEMA_VERSION_V2, data...)
    /// All core events MUST use this for schema compliance and indexer compatibility.
    fn emit_v2_event<Topics, T>(env: &Env, topic_tuple: Topics, data: T)
    where
        Topics: IntoVal<Env, soroban_sdk::Val> + soroban_sdk::events::Topics,
        T: IntoVal<Env, soroban_sdk::Val> + soroban_sdk::TryIntoVal<Env, soroban_sdk::Val>,
    {
        env.events().publish(topic_tuple, (EVENT_SCHEMA_VERSION_V2, data));
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
        let public_key: BytesN<32> =
            env.storage().persistent().get(&pk_key).ok_or(RevoraError::SignerKeyNotRegistered)?;
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
        env.events().publish((EVENT_SHARE_SET, issuer, namespace, token), (holder, share_bps));
        Ok(())
    }

    /// Return the locked payment token for an offering.
    ///
    /// Backward compatibility: older offerings may not have an explicit `PaymentToken` entry yet.
    /// In that case, the offering's configured `payout_asset` is treated as the canonical lock.
    fn get_locked_payment_token_for_offering(
        env: &Env,
        offering_id: &OfferingId,
    ) -> Result<Address, RevoraError> {
        let pt_key = DataKey::PaymentToken(offering_id.clone());
        if let Some(payment_token) = env.storage().persistent().get::<DataKey, Address>(&pt_key) {
            return Ok(payment_token);
        }

        let offering = Self::get_offering(
            env.clone(),
            offering_id.issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
        )
        .ok_or(RevoraError::OfferingNotFound)?;
        Ok(offering.payout_asset)
    }

    /// Internal helper for revenue deposits.
    /// Validates amount using the Negative Amount Validation Matrix (#163).
    fn do_deposit_revenue(
        env: &Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        payment_token: Address,
        amount: i128,
        period_id: u64,
    ) -> Result<(), RevoraError> {
        // Negative Amount Validation Matrix: RevenueDeposit requires amount > 0 (#163)
        if let Err((err, reason)) =
            AmountValidationMatrix::validate(amount, AmountValidationCategory::RevenueDeposit)
        {
            env.events().publish(
                (EVENT_AMOUNT_VALIDATION_FAILED, issuer.clone(), namespace.clone(), token.clone()),
                (amount, err as u32, reason),
            );
            return Err(err);
        }

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Validate inputs (#35)
        Self::require_valid_period_id(period_id)?;
        Self::require_positive_amount(amount)?;

        // Verify offering exists
        if Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
            .is_none()
        {
            return Err(RevoraError::OfferingNotFound);
        }

        // Enforce period ordering invariant (double-check at deposit)
        Self::require_next_period_id(env, &offering_id, period_id)?;

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

        // Enforce the offering's locked payment token. For legacy offerings without an
        // explicit storage entry yet, `payout_asset` is the canonical lock and is persisted
        // only after a successful deposit using that token.
        let locked_payment_token = Self::get_locked_payment_token_for_offering(env, &offering_id)?;
        if locked_payment_token != payment_token {
            return Err(RevoraError::PaymentTokenMismatch);
        }
        let pt_key = DataKey::PaymentToken(offering_id.clone());
        if !env.storage().persistent().has(&pt_key) {
            env.storage().persistent().set(&pt_key, &locked_payment_token);
        }

        // Transfer tokens from issuer to contract
        let contract_addr = env.current_contract_address();
        if token::Client::new(env, &payment_token)
            .try_transfer(&issuer, &contract_addr, &amount)
            .is_err()
        {
            return Err(RevoraError::TransferFailed);
        }

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

        /// Versioned event v2: [version: u32, payment_token: Address, amount: i128, period_id: u64]
        Self::emit_v2_event(
            env,
            (EVENT_REV_DEPOSIT_V2, issuer.clone(), namespace.clone(), token.clone()),
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
        let (_, event_only): (bool, bool) =
            env.storage().persistent().get(&DataKey::ContractFlags).unwrap_or((false, false));
        event_only
    }

    /// Input validation (#35): require amount > 0 for transfers/deposits.
    #[allow(dead_code)]
    fn require_positive_amount(amount: i128) -> Result<(), RevoraError> {
        if amount <= 0 {
            return Err(RevoraError::InvalidAmount);
        }
        Ok(())
    }

    /// Require period_id is valid next in strictly increasing sequence for offering.
    /// Panics if offering not found.
    fn require_next_period_id(
        env: &Env,
        offering_id: &OfferingId,
        period_id: u64,
    ) -> Result<(), RevoraError> {
        if period_id == 0 {
            return Err(RevoraError::InvalidPeriodId);
        }
        let key = DataKey::LastPeriodId(offering_id.clone());
        let last: u64 = env.storage().persistent().get(&key).unwrap_or(0);
        if period_id <= last {
            return Err(RevoraError::InvalidPeriodId);
        }
        env.storage().persistent().set(&key, &period_id);
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
            return; // Already initialized, no-op
        }
        env.storage().persistent().set(&DataKey::Admin, &admin);
        env.events().publish((EVENT_ADMIN_SET,), admin.clone());
        if let Some(ref s) = safety {
            env.storage().persistent().set(&DataKey::Safety, &s);
        }
        env.storage().persistent().set(&DataKey::Paused, &false);
        let eo = event_only.unwrap_or(false);
        env.storage().persistent().set(&DataKey::ContractFlags, &(false, eo));
        env.events().publish((EVENT_INIT, admin.clone()), (safety, eo));
    }

    /// Pause the contract (Admin only).
    ///
    /// When paused, all state-mutating operations are disabled to protect the system.
    /// This operation is idempotent.
    ///
    /// ### Parameters
    /// - `caller`: The address of the admin (must match initialized admin).
    pub fn pause_admin(env: Env, caller: Address) -> Result<(), RevoraError> {
        caller.require_auth();
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;
        if caller != admin {
            return Err(RevoraError::NotAuthorized);
        }
        env.storage().persistent().set(&DataKey::Paused, &true);
        env.events().publish((EVENT_PAUSED, caller.clone()), ());
        Ok(())
    }

    /// Unpause the contract (Admin only).
    ///
    /// Re-enables state-mutating operations after a pause.
    /// This operation is idempotent.
    ///
    /// ### Parameters
    /// - `caller`: The address of the admin (must match initialized admin).
    pub fn unpause_admin(env: Env, caller: Address) -> Result<(), RevoraError> {
        caller.require_auth();
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;
        if caller != admin {
            return Err(RevoraError::NotAuthorized);
        }
        env.storage().persistent().set(&DataKey::Paused, &false);
        env.events().publish((EVENT_UNPAUSED, caller.clone()), ());
        Ok(())
    }

    /// Pause the contract (Safety role only).
    ///
    /// Allows the safety role to trigger an emergency pause.
    /// This operation is idempotent.
    ///
    /// ### Parameters
    /// - `caller`: The address of the safety role (must match initialized safety address).
    pub fn pause_safety(env: Env, caller: Address) -> Result<(), RevoraError> {
        caller.require_auth();
        let safety: Address =
            env.storage().persistent().get(&DataKey::Safety).ok_or(RevoraError::NotInitialized)?;
        if caller != safety {
            return Err(RevoraError::NotAuthorized);
        }
        env.storage().persistent().set(&DataKey::Paused, &true);
        env.events().publish((EVENT_PAUSED, caller.clone()), ());
        Ok(())
    }

    /// Unpause the contract (Safety role only).
    ///
    /// Allows the safety role to resume contract operations.
    /// This operation is idempotent.
    ///
    /// ### Parameters
    /// - `caller`: The address of the safety role (must match initialized safety address).
    pub fn unpause_safety(env: Env, caller: Address) -> Result<(), RevoraError> {
        caller.require_auth();
        let safety: Address =
            env.storage().persistent().get(&DataKey::Safety).ok_or(RevoraError::NotInitialized)?;
        if caller != safety {
            return Err(RevoraError::NotAuthorized);
        }
        env.storage().persistent().set(&DataKey::Paused, &false);
        env.events().publish((EVENT_UNPAUSED, caller.clone()), ());
        Ok(())
    }

    /// Query the paused state of the contract.
    pub fn is_paused(env: Env) -> bool {
        env.storage().persistent().get::<DataKey, bool>(&DataKey::Paused).unwrap_or(false)
    }

    /// Helper: return error if contract is paused. Used by state-mutating entrypoints.
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
    /// Validates supply_cap using the Negative Amount Validation Matrix (#163).
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

        // Negative Amount Validation Matrix: SupplyCap requires >= 0 (#163)
        if let Err((err, _)) =
            AmountValidationMatrix::validate(supply_cap, AmountValidationCategory::SupplyCap)
        {
            return Err(err);
        }

        // Skip bps validation in testnet mode
        let testnet_mode = Self::is_testnet_mode(env.clone());
        if !testnet_mode && revenue_share_bps > 10_000 {
            return Err(RevoraError::InvalidRevenueShareBps);
        }

        // Register namespace for issuer if not already present
        let ns_reg_key = DataKey2::NamespaceRegistered(issuer.clone(), namespace.clone());
        if !env.storage().persistent().has(&ns_reg_key) {
            let ns_count_key = DataKey2::NamespaceCount(issuer.clone());
            let count: u32 = env.storage().persistent().get(&ns_count_key).unwrap_or(0);
            env.storage()
                .persistent()
                .set(&DataKey2::NamespaceItem(issuer.clone(), count), &namespace);
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

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let issuer_lookup_key = DataKey::OfferingIssuer(offering_id.clone());
        env.storage().persistent().set(&issuer_lookup_key, &issuer);

        if supply_cap > 0 {
            let cap_key = DataKey::SupplyCap(offering_id.clone());
            env.storage().persistent().set(&cap_key, &supply_cap);
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

    /// Return the locked payment token for an offering.
    ///
    /// For offerings created before explicit payment-token lock storage existed, this falls back
    /// to the offering's configured `payout_asset`, which is treated as the canonical lock.
    pub fn get_payment_token(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<Address> {
        let offering_id = OfferingId { issuer, namespace, token };
        Self::get_locked_payment_token_for_offering(&env, &offering_id).ok()
    }

    /// Record a revenue report for an offering; updates audit summary and emits events.
    /// Validates amount using the Negative Amount Validation Matrix (#163).
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

        // Negative Amount Validation Matrix: RevenueReport requires amount >= 0 (#163)
        if let Err((err, reason)) =
            AmountValidationMatrix::validate(amount, AmountValidationCategory::RevenueReport)
        {
            env.events().publish(
                (EVENT_AMOUNT_VALIDATION_FAILED, issuer.clone(), namespace.clone(), token.clone()),
                (amount, err as u32, reason),
            );
            return Err(err);
        }

        let event_only = Self::is_event_only(&env);
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        Self::require_not_offering_frozen(&env, &offering_id)?;
        Self::require_report_window_open(&env, &offering_id)?;

        // Enforce period ordering invariant
        Self::require_next_period_id(&env, &offering_id, period_id)?;

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

            // Track the net audit delta for this call:
            //   (revenue_delta, count_delta)
            // Initial report  → (+amount, +1)
            // Override        → (new - old, 0)   — period already counted
            // Rejected        → (0, 0)            — no mutation
            let mut audit_revenue_delta: i128 = 0;
            let mut audit_count_delta: u64 = 0;

            match reports.get(period_id) {
                Some((existing_amount, _timestamp)) => {
                    if override_existing {
                        // Net delta = new amount minus the old amount.
                        audit_revenue_delta = amount.saturating_sub(existing_amount);
                        // count_delta stays 0: the period was already counted.
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
                    // Initial report for this period.
                    audit_revenue_delta = amount;
                    audit_count_delta = 1;

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

            // Apply the net audit delta computed above (exactly once, after the match).
            if audit_revenue_delta != 0 || audit_count_delta != 0 {
                let summary_key = DataKey::AuditSummary(offering_id.clone());
                let mut summary: AuditSummary = env
                    .storage()
                    .persistent()
                    .get(&summary_key)
                    .unwrap_or(AuditSummary { total_revenue: 0, report_count: 0 });
                summary.total_revenue = summary.total_revenue.saturating_add(audit_revenue_delta);
                summary.report_count = summary.report_count.saturating_add(audit_count_delta);
                env.storage().persistent().set(&summary_key, &summary);
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

        // Audit log summary (#34): maintain per-offering total revenue and report count
        // only for persisted reports. Event-only mode should not mutate summary state.
        if !event_only {
            let summary_key = DataKey::AuditSummary(offering_id.clone());
            let mut summary: AuditSummary = env
                .storage()
                .persistent()
                .get(&summary_key)
                .unwrap_or(AuditSummary { total_revenue: 0, report_count: 0 });
            summary.total_revenue = summary.total_revenue.saturating_add(amount);
            summary.report_count = summary.report_count.saturating_add(1);
            env.storage().persistent().set(&summary_key, &summary);
        }
        // Optionally emit versioned v1 events for forward-compatible consumers
        if Self::is_event_versioning_enabled(env.clone()) {
            // Versioned event v2: [version: u32, payout_asset: Address, amount: i128, period_id: u64, blacklist: Vec<Address>]
            Self::emit_v2_event(
                &env,
                (EVENT_REV_INIA_V2, issuer.clone(), namespace.clone(), token.clone()),
                (payout_asset.clone(), amount, period_id, blacklist.clone()),
            );
        }

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

        Ok(())
    }

    /// Repair the `AuditSummary` for an offering by recomputing it from the
    /// authoritative `RevenueReports` map and writing the corrected value.
    ///
    /// ### Auth
    /// Only the current issuer or the contract admin may call this. This prevents
    /// arbitrary callers from triggering unnecessary storage writes.
    ///
    /// ### Security notes
    /// - This function is idempotent: calling it when the summary is already correct
    ///   is safe and produces no observable side-effects beyond the storage write.
    /// - If `RevenueReports` is empty (no reports ever filed), the summary is reset
    ///   to `{total_revenue: 0, report_count: 0}`.
    /// - Overflow during recomputation is handled with saturation; the resulting
    ///   summary will have `total_revenue == i128::MAX` in that case.
    ///
    /// ### Returns
    /// The corrected `AuditSummary` that was written to storage.
    pub fn repair_audit_summary(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Result<AuditSummary, RevoraError> {
        Self::require_not_frozen(&env)?;
        caller.require_auth();

        // Auth: caller must be current issuer or admin.
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        let admin = Self::get_admin(env.clone()).ok_or(RevoraError::NotInitialized)?;
        if caller != current_issuer && caller != admin {
            return Err(RevoraError::NotAuthorized);
        }

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Recompute from the authoritative RevenueReports map.
        let reports_key = DataKey::RevenueReports(offering_id.clone());
        let reports: Map<u64, (i128, u64)> =
            env.storage().persistent().get(&reports_key).unwrap_or_else(|| Map::new(&env));

        let computed_report_count = reports.len() as u64;
        let mut computed_total: i128 = 0;

        let keys = reports.keys();
        for i in 0..keys.len() {
            let period_id = keys.get(i).unwrap();
            if let Some((amount, _)) = reports.get(period_id) {
                computed_total = computed_total.saturating_add(amount);
            }
        }

        let corrected =
            AuditSummary { total_revenue: computed_total, report_count: computed_report_count };

        let summary_key = DataKey::AuditSummary(offering_id);
        env.storage().persistent().set(&summary_key, &corrected);

        env.events().publish(
            (EVENT_AUDIT_REPAIRED, issuer, namespace, token),
            (corrected.total_revenue, corrected.report_count),
        );

        Ok(corrected)
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
    ///
    /// Returns `(sum, next_start)` where `next_start` is `Some(period)` if there are remaining
    /// periods to process and a subsequent call can continue from that period.
    ///
    /// ### Features & Security
    /// - **Determinism**: The query is read-only and uses capped iterations to prevent CPU/Gas exhaustion.
    /// - **Input Validation**: Automatically handles `from_period > to_period` by returning an empty result.
    /// - **Capping**: `max_periods` of 0 or > `MAX_CHUNK_PERIODS` will be capped to `MAX_CHUNK_PERIODS`.
    pub fn get_revenue_range_chunk(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        from_period: u64,
        to_period: u64,
        max_periods: u32,
    ) -> (i128, Option<u64>) {
        if from_period > to_period {
            return (0, None);
        }

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
            total = total.saturating_add(Self::get_revenue_by_period(
                env.clone(),
                issuer.clone(),
                namespace.clone(),
                token.clone(),
                p,
            ));
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
    /// - `caller`: The address authorized to manage the blacklist. Must be the current issuer of the offering.
    /// - `token`: The token representing the offering.
    /// - `investor`: The address to be blacklisted.
    ///
    /// ### Security Assumptions
    /// - `caller` must be the current issuer of the offering or the contract admin.
    /// - The blacklist is capped at `MAX_BLACKLIST_SIZE` entries per offering to prevent
    ///   unbounded storage growth and keep distribution gas predictable.
    /// - Idempotent adds (address already present) do not count against the size limit.
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
    /// - `Err(RevoraError::NotAuthorized)` if caller is not the current issuer.
    /// - `Err(RevoraError::BlacklistSizeLimitExceeded)` if the blacklist is at capacity.
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
        // Verify auth: caller must be issuer or admin
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        let admin = Self::get_admin(env.clone()).ok_or(RevoraError::NotInitialized)?;

        if caller != current_issuer && caller != admin {
            return Err(RevoraError::NotAuthorized);
        }

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if !Self::is_event_only(&env) {
            let key = DataKey::Blacklist(offering_id.clone());
            let mut map: Map<Address, bool> =
                env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));

            let was_present = map.get(investor.clone()).unwrap_or(false);
            if !was_present {
                // Guard: reject if the blacklist is already at capacity.
                if map.len() >= MAX_BLACKLIST_SIZE {
                    return Err(RevoraError::BlacklistSizeLimitExceeded);
                }
                map.set(investor.clone(), true);
                env.storage().persistent().set(&key, &map);

                // Maintain insertion order for deterministic get_blacklist (#38)
                let order_key = DataKey::BlacklistOrder(offering_id.clone());
                let mut order: Vec<Address> =
                    env.storage().persistent().get(&order_key).unwrap_or_else(|| Vec::new(&env));
                order.push_back(investor.clone());
                env.storage().persistent().set(&order_key, &order);
            }
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
    /// - `caller`: The address authorized to manage the blacklist. Must be the current issuer of the offering.
    /// - `token`: The token representing the offering.
    /// - `investor`: The address to be removed from the blacklist.
    ///
    /// ### Security Assumptions
    /// - `caller` must be the current issuer of the offering or the contract admin.
    /// - `namespace` isolation ensures that removing from one blacklist does not affect others.
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
    /// - `Err(RevoraError::NotAuthorized)` if caller is not the current issuer.
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
        Self::require_not_offering_frozen(&env, &offering_id)?;

        // Verify auth: caller must be issuer or admin.
        // Security assumption: only the current issuer or contract admin may remove
        // addresses from the blacklist. This mirrors the add-side guard and prevents
        // unauthorized actors from re-enabling blacklisted investors.
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

    /// Return the current number of blacklisted addresses for an offering.
    ///
    /// This is a cheap O(1) read of the underlying map length and can be used
    /// by off-chain tooling to monitor proximity to `MAX_BLACKLIST_SIZE` (200)
    /// before attempting an add.
    ///
    /// Returns 0 when no blacklist exists yet for the offering.
    pub fn get_blacklist_size(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::Blacklist(offering_id);
        env.storage()
            .persistent()
            .get::<DataKey, Map<Address, bool>>(&key)
            .map(|m| m.len())
            .unwrap_or(0)
    }

    // ── Whitelist management ──────────────────────────────────

    /// Set per-offering concentration limit. Caller must be the offering issuer.
    /// `max_bps`: max allowed single-holder share in basis points (0 = disable).
    /// Add `investor` to the per-offering whitelist for `token`.
    ///
    /// Idempotent — calling with an already-whitelisted address is safe.
    /// When a whitelist exists (non-empty), only whitelisted addresses
    /// are eligible for revenue distribution (subject to blacklist override).
    /// ### Security Assumptions
    /// - `caller` must be the current issuer of the offering.
    /// - `namespace` partitioning prevents whitelists from leaking across tenants.
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::OfferingNotFound)` if the offering is not registered.
    /// - `Err(RevoraError::NotAuthorized)` if the caller is not authorized.
    pub fn whitelist_add(
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

        // Verify offering exists and get current issuer for auth check
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        let admin = Self::get_admin(env.clone());
        let is_admin = admin.as_ref().map(|a| caller == *a).unwrap_or(false);
        if caller != current_issuer && !is_admin {
            return Err(RevoraError::NotAuthorized);
        }

        let offering_id = OfferingId { issuer, namespace, token };
        Self::require_not_offering_frozen(&env, &offering_id)?;

        if !Self::is_event_only(&env) {
            let key = DataKey::Whitelist(offering_id.clone());
            let mut map: Map<Address, bool> =
                env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));
            map.set(investor.clone(), true);
            env.storage().persistent().set(&key, &map);
        }

        env.events().publish(
            (
                EVENT_WL_ADD,
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
            ),
            (caller, investor),
        );
        Ok(())
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
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        caller.require_auth();

        // Verify offering exists and get current issuer for auth check
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        let admin = Self::get_admin(env.clone());
        let is_admin = admin.as_ref().map(|a| caller == *a).unwrap_or(false);
        if caller != current_issuer && !is_admin {
            return Err(RevoraError::NotAuthorized);
        }

        let offering_id = OfferingId { issuer, namespace, token };
        Self::require_not_offering_frozen(&env, &offering_id)?;
        let key = DataKey::Whitelist(offering_id.clone());
        let mut map: Map<Address, bool> =
            env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));

        if !Self::is_event_only(&env) {
            let key = DataKey::Whitelist(offering_id.clone());
            if let Some(mut map) =
                env.storage().persistent().get::<DataKey, Map<Address, bool>>(&key)
            {
                if map.remove(investor.clone()).is_some() {
                    env.storage().persistent().set(&key, &map);
                }
            }
        }

        env.events().publish(
            (
                EVENT_WL_REM,
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
            ),
            (caller, investor),
        );
        Ok(())
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

        Self::require_not_offering_frozen(&env, &offering_id)?;

        if !Self::is_event_only(&env) {
            issuer.require_auth();
            let key = DataKey::ConcentrationLimit(offering_id);
            env.storage().persistent().set(&key, &ConcentrationLimitConfig { max_bps, enforce });
            env.events()
                .publish((EVENT_CONC_LIMIT_SET, issuer, namespace, token), (max_bps, enforce));
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

        // Verify offering exists and get current issuer for auth check
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::NotAuthorized);
        }

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

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
        Self::require_not_offering_frozen(&env, &offering_id)?;
        issuer.require_auth();
        let key = DataKey::RoundingMode(offering_id);
        env.storage().persistent().set(&key, &mode);
        env.events().publish((EVENT_ROUNDING_MODE_SET, issuer, namespace, token), mode);
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
    /// Validates amounts using the Negative Amount Validation Matrix (#163).
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
        Self::require_not_offering_frozen(&env, &offering_id)?;
        issuer.require_auth();

        // Negative Amount Validation Matrix: InvestmentMinStake requires >= 0 (#163)
        if let Err((err, _)) = AmountValidationMatrix::validate(
            min_stake,
            AmountValidationCategory::InvestmentMinStake,
        ) {
            return Err(err);
        }

        // Negative Amount Validation Matrix: InvestmentMaxStake requires >= 0 (#163)
        if let Err((err, _)) = AmountValidationMatrix::validate(
            max_stake,
            AmountValidationCategory::InvestmentMaxStake,
        ) {
            return Err(err);
        }

        // Validate range: max_stake >= min_stake when max_stake > 0
        AmountValidationMatrix::validate_stake_range(min_stake, max_stake)?;

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
    /// Only the offering issuer may set this. Emits event when configured or changed.
    /// Pass 0 to disable the threshold.
    /// Validates amount using the Negative Amount Validation Matrix (#163).
    pub fn set_min_revenue_threshold(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        min_amount: i128,
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

        Self::require_not_offering_frozen(&env, &offering_id)?;
        issuer.require_auth();

        // Negative Amount Validation Matrix: MinRevenueThreshold requires >= 0 (#163)
        if let Err((err, _)) = AmountValidationMatrix::validate(
            min_amount,
            AmountValidationCategory::MinRevenueThreshold,
        ) {
            return Err(err);
        }

        let key = DataKey::MinRevenueThreshold(offering_id);
        let previous: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &min_amount);

        env.events().publish(
            (EVENT_MIN_REV_THRESHOLD_SET, issuer, namespace, token),
            (previous, min_amount),
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
    /// Security assumptions:
    /// - Callers should pass `revenue_share_bps` in [0, 10_000]. Values above 10_000 are rejected by returning 0.
    /// - Revenue flows in this contract are non-negative, but this helper is total over signed `amount` for testability.
    ///
    /// Guarantees:
    /// - Overflow-resistant arithmetic without panic.
    /// - Result is clamped to [min(0, amount), max(0, amount)] to avoid over-distribution.
    pub fn compute_share(
        _env: Env,
        amount: i128,
        revenue_share_bps: u32,
        mode: RoundingMode,
    ) -> i128 {
        if revenue_share_bps > 10_000 {
            return 0;
        }
        if amount == 0 || revenue_share_bps == 0 {
            return 0;
        }

        // Decompose `amount` to avoid `amount * bps` overflow:
        // amount = q * 10_000 + r, so (amount * bps) / 10_000 = q * bps + (r * bps) / 10_000.
        // `r` is bounded to (-10_000, 10_000), so `r * bps` is always safe in i128.
        let q = amount / 10_000;
        let r = amount % 10_000;
        let bps = revenue_share_bps as i128;
        let base = q.checked_mul(bps).unwrap_or_else(|| {
            if (q >= 0 && bps >= 0) || (q < 0 && bps < 0) {
                i128::MAX
            } else {
                i128::MIN
            }
        });

        let remainder_product = r * bps;
        let remainder_share = match mode {
            RoundingMode::Truncation => remainder_product / 10_000,
            RoundingMode::RoundHalfUp => {
                let half = 5_000_i128;
                if remainder_product >= 0 {
                    remainder_product.saturating_add(half) / 10_000
                } else {
                    remainder_product.saturating_sub(half) / 10_000
                }
            }
        };

        let share = base.checked_add(remainder_share).unwrap_or_else(|| {
            if (base >= 0 && remainder_share >= 0) || (base < 0 && remainder_share < 0) {
                if base >= 0 { i128::MAX } else { i128::MIN }
            } else {
                0
            }
        });

        // Clamp to [min(0, amount), max(0, amount)] to avoid overflow semantics affecting bounds
        let lo = core::cmp::min(0, amount);
        let hi = core::cmp::max(0, amount);
        core::cmp::min(core::cmp::max(share, lo), hi)
    }

    /// Normalize `amount` from the token's native decimal precision to Stellar's canonical 7-decimal
    /// (stroop) precision used internally by this contract.
    ///
    /// - If `from_decimals == 7`: returns `amount` unchanged.
    /// - If `from_decimals < 7`: scales **up** by `10^(7 - from_decimals)` (e.g., 6-decimal USDC → 7).
    /// - If `from_decimals > 7`: scales **down** by `10^(from_decimals - 7)` using integer truncation.
    ///
    /// Returns `0` if intermediate arithmetic overflows to prevent fund inflation bugs.
    fn normalize_amount(amount: i128, from_decimals: u32) -> i128 {
        if from_decimals == STELLAR_CANONICAL_DECIMALS {
            return amount;
        }
        if from_decimals < STELLAR_CANONICAL_DECIMALS {
            let exp = STELLAR_CANONICAL_DECIMALS - from_decimals;
            let factor: i128 = match 10_i128.checked_pow(exp) {
                Some(f) => f,
                None => return 0,
            };
            amount.checked_mul(factor).unwrap_or(0)
        } else {
            let exp = from_decimals - STELLAR_CANONICAL_DECIMALS;
            let factor: i128 = match 10_i128.checked_pow(exp) {
                Some(f) => f,
                None => return 0,
            };
            amount.checked_div(factor).unwrap_or(0)
        }
    }

    /// Set the decimal precision of the payout asset for an offering.
    ///
    /// Must be called by the offering `issuer`. Accepted range is `0..=18`.
    /// If not set, the contract defaults to `7` (Stellar canonical stroops).
    ///
    /// ### Security
    /// - Only the offering issuer may configure decimals.
    /// - Misconfigured decimals directly affect payout arithmetic; issuers must supply
    ///   the on-chain token's actual decimal value.
    ///
    /// ### Errors
    /// - `RevoraError::NotAuthorized` if caller is not the issuer.
    /// - `RevoraError::LimitReached` if `decimals > 18`.
    pub fn set_payment_token_decimals(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        decimals: u32,
    ) -> Result<(), RevoraError> {
        issuer.require_auth();
        if decimals > MAX_TOKEN_DECIMALS {
            return Err(RevoraError::LimitReached);
        }
        let offering_id = OfferingId { issuer: issuer.clone(), namespace: namespace.clone(), token: token.clone() };
        env.storage()
            .persistent()
            .set(&DataKey::PaymentTokenDecimals(offering_id), &decimals);
        env.events()
            .publish((EVENT_DECIMAL_SET, issuer, namespace, token), decimals);
        Ok(())
    }

    /// Get the configured decimal precision of the payout asset for an offering.
    /// Defaults to `7` (Stellar canonical stroops) if not explicitly set.
    pub fn get_payment_token_decimals(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get(&DataKey::PaymentTokenDecimals(offering_id))
            .unwrap_or(STELLAR_CANONICAL_DECIMALS)
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

        // Input validation (#35): reject zero/invalid period_id and non-positive amounts.
        Self::require_valid_period_id(period_id)?;
        Self::require_positive_amount(amount)?;

        // Verify offering exists and issuer is current
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        Self::require_not_offering_frozen(&env, &offering_id)?;

        Self::do_deposit_revenue(&env, issuer, namespace, token, payment_token, amount, period_id)
    }

    /// any previously recorded snapshot for this offering to prevent duplication.
    /// Validates amount and snapshot reference using the Negative Amount Validation Matrix (#163).
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

        // 0. Validate snapshot reference using Negative Amount Validation Matrix (#163)
        // SnapshotReference requires > 0 and strictly increasing
        if let Err((err, _)) = AmountValidationMatrix::validate(
            snapshot_reference as i128,
            AmountValidationCategory::SnapshotReference,
        ) {
            return Err(err);
        }

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
        Self::require_not_offering_frozen(&env, &offering_id)?;

        // 2. Validate snapshot reference is strictly monotonic using matrix helper
        let snap_key = DataKey::LastSnapshotRef(offering_id.clone());
        let last_snap: u64 = env.storage().persistent().get(&snap_key).unwrap_or(0);
        AmountValidationMatrix::validate_snapshot_monotonic(
            snapshot_reference as i128,
            last_snap as i128,
        )?;

        // 3. Delegate to core deposit logic (includes RevenueDeposit validation)
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
        /// Versioned event v2: [version: u32, payment_token: Address, amount: i128, period_id: u64, snapshot_reference: u64]
        Self::emit_v2_event(
            &env,
            (EVENT_REV_DEP_SNAP_V2, issuer.clone(), namespace.clone(), token.clone()),
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
        Self::require_not_offering_frozen(&env, &offering_id)?;
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

    // ── Deterministic Snapshot Expansion (#054) ──────────────────────────────
    //
    // Design:
    //   A "snapshot" is an immutable, write-once record that captures the
    //   canonical holder-share distribution at a specific point in time.
    //
    //   Workflow:
    //     1. Issuer calls `commit_snapshot` with a strictly-increasing `snapshot_ref`
    //        and a 32-byte `content_hash` of the off-chain holder dataset.
    //        The contract stores a `SnapshotEntry` and emits `snap_com`.
    //     2. Issuer calls `apply_snapshot_shares` (one or more times) to write
    //        holder shares for this snapshot into persistent storage.
    //        Each call appends a bounded batch of (holder, share_bps) pairs.
    //        Emits `snap_shr` per batch.
    //     3. Issuer calls `deposit_revenue_with_snapshot` (existing) to deposit
    //        revenue tied to this snapshot_ref.
    //
    //   Security assumptions:
    //   - `content_hash` is caller-supplied and stored verbatim. The contract
    //     does NOT verify it matches the on-chain holder entries. Off-chain
    //     consumers MUST recompute and compare the hash.
    //   - Snapshot refs are strictly monotonic per offering; replay is impossible.
    //   - `apply_snapshot_shares` is idempotent per (snapshot_ref, index): writing
    //     the same index twice overwrites with the same value (no double-credit).
    //   - Only the current offering issuer may commit or apply snapshots.
    //   - Frozen/paused contract blocks all snapshot writes.

    /// Maximum holders per `apply_snapshot_shares` batch.
    /// Keeps per-call compute bounded within Soroban limits.
    const MAX_SNAPSHOT_BATCH: u32 = 50;

    /// Commit a new snapshot entry for an offering.
    ///
    /// Records an immutable `SnapshotEntry` keyed by `(offering_id, snapshot_ref)`.
    /// `snapshot_ref` must be strictly greater than the last committed ref for this
    /// offering (monotonicity invariant). The `content_hash` is a 32-byte digest of
    /// the off-chain holder-share dataset; it is stored verbatim and not verified
    /// on-chain.
    ///
    /// ### Auth
    /// Requires `issuer.require_auth()`. Only the current offering issuer may commit.
    ///
    /// ### Errors
    /// - `OfferingNotFound`: offering does not exist or caller is not current issuer.
    /// - `SnapshotNotEnabled`: snapshot distribution is not enabled for this offering.
    /// - `OutdatedSnapshot`: `snapshot_ref` ≤ last committed ref (replay / stale).
    /// - `ContractFrozen` / paused: contract is not operational.
    ///
    /// ### Events
    /// Emits `snap_com` with `(issuer, namespace, token)` topics and
    /// `(snapshot_ref, content_hash, committed_at)` data.
    pub fn commit_snapshot(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        snapshot_ref: u64,
        content_hash: BytesN<32>,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

        // Verify offering exists and caller is current issuer.
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        // Snapshot distribution must be enabled for this offering.
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        if !env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::SnapshotConfig(offering_id.clone()))
            .unwrap_or(false)
        {
            return Err(RevoraError::SnapshotNotEnabled);
        }

        // Enforce strict monotonicity: snapshot_ref must exceed the last committed ref.
        let last_ref_key = DataKey::LastSnapshotRef(offering_id.clone());
        let last_ref: u64 = env.storage().persistent().get(&last_ref_key).unwrap_or(0);
        if snapshot_ref <= last_ref {
            return Err(RevoraError::OutdatedSnapshot);
        }

        let committed_at = env.ledger().timestamp();
        let entry = SnapshotEntry {
            snapshot_ref,
            committed_at,
            content_hash: content_hash.clone(),
            holder_count: 0,
            total_bps: 0,
        };

        // Write-once: store the entry and advance the last-ref pointer atomically.
        env.storage()
            .persistent()
            .set(&DataKey::SnapshotEntry(offering_id.clone(), snapshot_ref), &entry);
        env.storage().persistent().set(&last_ref_key, &snapshot_ref);

        env.events().publish(
            (EVENT_SNAP_COMMIT, issuer, namespace, token),
            (snapshot_ref, content_hash, committed_at),
        );
        Ok(())
    }

    /// Retrieve a committed snapshot entry.
    ///
    /// Returns `None` if no snapshot with `snapshot_ref` has been committed for this offering.
    pub fn get_snapshot_entry(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        snapshot_ref: u64,
    ) -> Option<SnapshotEntry> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey::SnapshotEntry(offering_id, snapshot_ref))
    }

    /// Apply a batch of holder shares for a committed snapshot.
    ///
    /// Writes `(holder, share_bps)` pairs into persistent storage indexed by
    /// `(offering_id, snapshot_ref, sequential_index)`. Batches are bounded by
    /// `MAX_SNAPSHOT_BATCH` (50) per call. Updates `HolderShare` for each holder.
    ///
    /// ### Auth
    /// Requires `issuer.require_auth()`. Only the current offering issuer may apply.
    ///
    /// ### Errors
    /// - `OfferingNotFound`, `SnapshotNotEnabled`, `OutdatedSnapshot`,
    ///   `LimitReached`, `InvalidShareBps`, `ContractFrozen`.
    pub fn apply_snapshot_shares(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        snapshot_ref: u64,
        start_index: u32,
        holders: Vec<(Address, u32)>,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if !env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::SnapshotConfig(offering_id.clone()))
            .unwrap_or(false)
        {
            return Err(RevoraError::SnapshotNotEnabled);
        }

        // Snapshot must have been committed first.
        let entry_key = DataKey::SnapshotEntry(offering_id.clone(), snapshot_ref);
        let mut entry: SnapshotEntry =
            env.storage().persistent().get(&entry_key).ok_or(RevoraError::OutdatedSnapshot)?;

        let batch_len = holders.len();
        if batch_len > Self::MAX_SNAPSHOT_BATCH {
            return Err(RevoraError::LimitReached);
        }

        // Validate all share_bps before writing anything (fail-fast).
        for i in 0..batch_len {
            let (_, share_bps) = holders.get(i).unwrap();
            if share_bps > 10_000 {
                return Err(RevoraError::InvalidShareBps);
            }
        }

        let mut added_bps: u32 = 0;
        for i in 0..batch_len {
            let (holder, share_bps) = holders.get(i).unwrap();
            let slot = start_index.saturating_add(i);

            // Write indexed slot for deterministic enumeration.
            env.storage().persistent().set(
                &DataKey::SnapshotHolder(offering_id.clone(), snapshot_ref, slot),
                &(holder.clone(), share_bps),
            );

            // Update live holder share so claim() works immediately.
            env.storage()
                .persistent()
                .set(&DataKey::HolderShare(offering_id.clone(), holder), &share_bps);

            added_bps = added_bps.saturating_add(share_bps);
        }

        // Update snapshot metadata.
        let new_holder_count = entry.holder_count.saturating_add(batch_len);
        let new_total_bps = entry.total_bps.saturating_add(added_bps);
        entry.holder_count = new_holder_count;
        entry.total_bps = new_total_bps;
        env.storage().persistent().set(&entry_key, &entry);

        env.events().publish(
            (EVENT_SNAP_SHARES_APPLIED, issuer, namespace, token),
            (snapshot_ref, start_index, batch_len, new_total_bps),
        );
        Ok(())
    }

    /// Return the total number of holder entries recorded for a snapshot.
    ///
    /// Returns 0 if the snapshot has not been committed or no shares have been applied.
    pub fn get_snapshot_holder_count(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        snapshot_ref: u64,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get::<DataKey, SnapshotEntry>(&DataKey::SnapshotEntry(offering_id, snapshot_ref))
            .map(|e| e.holder_count)
            .unwrap_or(0)
    }

    /// Read a single holder entry from a committed snapshot by its sequential index.
    ///
    /// Returns `None` if the slot has not been written.
    pub fn get_snapshot_holder_at(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        snapshot_ref: u64,
        index: u32,
    ) -> Option<(Address, u32)> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey::SnapshotHolder(offering_id, snapshot_ref, index))
    }
}

// ── Holder shares, claims, admin, governance, and utility methods ─────────────
// Plain impl block — excluded from the ABI spec to keep spec XDR within limit.
impl RevoraRevenueShare {
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

        Self::require_not_offering_frozen(&env, &offering_id)?;
        issuer.require_auth();
        Self::set_holder_share_internal(
            &env,
            offering_id.issuer,
            offering_id.namespace,
            offering_id.token,
            holder,
            share_bps,
        )
    }

    // ── Meta-authorization, claims, windows, and query methods ───────────────────

    /// Register an ed25519 public key for a signer address.
    /// The signer must authorize this binding.
    pub fn register_meta_signer_key(
        env: Env,
        signer: Address,
        public_key: BytesN<32>,
    ) -> Result<(), RevoraError> {
        signer.require_auth();
        env.storage().persistent().set(&MetaDataKey::SignerKey(signer.clone()), &public_key);
        env.events().publish((EVENT_META_SIGNER_SET, signer), public_key);
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
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
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
        env.storage().persistent().set(&MetaDataKey::Delegate(offering_id), &delegate);
        env.events().publish((EVENT_META_DELEGATE_SET, issuer, namespace, token), delegate);
        Ok(())
    }

    /// Get the configured offering-level delegate signer.
    pub fn get_meta_delegate(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<Address> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&MetaDataKey::Delegate(offering_id))
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
        Self::require_not_offering_frozen(&env, &offering_id)?;
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
            (EVENT_META_SHARE_SET, payload.issuer, payload.namespace, payload.token),
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
        Self::require_not_offering_frozen(&env, &offering_id)?;
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
            .set(&MetaDataKey::RevenueApproved(offering_id, payload.period_id), &true);
        Self::mark_meta_nonce_used(&env, &signer, nonce);
        env.events().publish(
            (EVENT_META_REV_APPROVE, payload.issuer, payload.namespace, payload.token),
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
    ///
    /// ### Idempotency and Safety Invariants
    ///
    /// This function provides the following hard guarantees:
    ///
    /// 1. **No double-pay**: `LastClaimedIdx` is written to storage only *after* the token
    ///    transfer succeeds. If the transfer panics (e.g. insufficient contract balance),
    ///    the index is not advanced and the holder may retry. Soroban's atomic transaction
    ///    model ensures partial state is never committed.
    ///
    /// 2. **Index advances only on processed periods**: The index is set to
    ///    `last_claimed_idx`, which reflects only periods that passed the delay check.
    ///    Periods blocked by `ClaimDelaySecs` are not counted; the function returns
    ///    `ClaimDelayNotElapsed` without writing any state.
    ///
    /// 3. **Zero-payout periods advance the index**: A period with `revenue = 0` (or
    ///    where `revenue * share_bps / 10_000 == 0` due to truncation) still advances
    ///    `LastClaimedIdx`. No transfer is issued for zero amounts. This prevents
    ///    permanently stuck indices on dust periods.
    ///
    /// 4. **Exhausted state returns `NoPendingClaims`**: Once `LastClaimedIdx >= PeriodCount`,
    ///    every subsequent call returns `Err(NoPendingClaims)` without touching storage.
    ///    Callers may safely retry without risk of side effects.
    ///
    /// 5. **Per-holder isolation**: Each holder's `LastClaimedIdx` is keyed by
    ///    `(offering_id, holder)`. One holder's claim progress never affects another's.
    ///
    /// 6. **Auth checked first**: `holder.require_auth()` is the first operation.
    ///    All subsequent checks (blacklist, share, period count) are read-only and
    ///    produce no state changes on failure.
    ///
    /// 7. **Blacklist check is pre-transfer**: A blacklisted holder is rejected before
    ///    any storage write or token transfer occurs.
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
            let period_id: u64 = env.storage().persistent().get(&entry_key).unwrap();
            let time_key = DataKey::PeriodDepositTime(offering_id.clone(), period_id);
            let deposit_time: u64 = env.storage().persistent().get(&time_key).unwrap_or(0);
            if delay_secs > 0 && now < deposit_time.saturating_add(delay_secs) {
                break;
            }
            let rev_key = DataKey::PeriodRevenue(offering_id.clone(), period_id);
            let revenue: i128 = env.storage().persistent().get(&rev_key).unwrap();
            let decimals = Self::get_payment_token_decimals(
                env.clone(),
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
            );
            let normalized = Self::normalize_amount(revenue, decimals);
            let payout = normalized * (share_bps as i128) / 10_000;
            total_payout += payout;
            claimed_periods.push_back(period_id);
            last_claimed_idx = i + 1;
        }

        if last_claimed_idx == start_idx {
            return Err(RevoraError::ClaimDelayNotElapsed);
        }

        // Transfer only if there is a positive payout
        if total_payout > 0 {
            let payment_token = Self::get_locked_payment_token_for_offering(&env, &offering_id)?;
            let contract_addr = env.current_contract_address();
            if token::Client::new(&env, &payment_token)
                .try_transfer(&contract_addr, &holder, &total_payout)
                .is_err()
            {
                return Err(RevoraError::TransferFailed);
            }
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
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        issuer.require_auth();
        let window = AccessWindow { start_timestamp, end_timestamp };
        Self::validate_window(&window)?;
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        env.storage().persistent().set(&WindowDataKey::Report(offering_id), &window);
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
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        issuer.require_auth();
        let window = AccessWindow { start_timestamp, end_timestamp };
        Self::validate_window(&window)?;
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        env.storage().persistent().set(&WindowDataKey::Claim(offering_id), &window);
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
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&WindowDataKey::Report(offering_id))
    }

    /// Read configured claiming window (if any) for an offering.
    pub fn get_claim_window(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<AccessWindow> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&WindowDataKey::Claim(offering_id))
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

    /// Shared claim-preview engine used by both full and chunked read-only views.
    ///
    /// Security assumptions:
    /// - Previews must never overstate what `claim` could legally pay at the current ledger state.
    /// - Callers may provide stale or adversarial cursors, so we clamp to the holder's current
    ///   `LastClaimedIdx` before iterating.
    /// - The first delayed period forms a hard stop because later periods are not claimable either.
    ///
    /// Returns `(total, next_cursor)` where `next_cursor` resumes from the first unprocessed index.
    fn compute_claimable_preview(
        env: &Env,
        offering_id: &OfferingId,
        holder: &Address,
        share_bps: u32,
        requested_start_idx: u32,
        count: Option<u32>,
    ) -> (i128, Option<u32>) {
        let count_key = DataKey::PeriodCount(offering_id.clone());
        let period_count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let idx_key = DataKey::LastClaimedIdx(offering_id.clone(), holder.clone());
        let holder_start_idx: u32 = env.storage().persistent().get(&idx_key).unwrap_or(0);
        let actual_start = core::cmp::max(requested_start_idx, holder_start_idx);

        if actual_start >= period_count {
            return (0, None);
        }

        let effective_cap = count.map(|requested| {
            if requested == 0 || requested > MAX_CHUNK_PERIODS {
                MAX_CHUNK_PERIODS
            } else {
                requested
            }
        });

        let delay_key = DataKey::ClaimDelaySecs(offering_id.clone());
        let delay_secs: u64 = env.storage().persistent().get(&delay_key).unwrap_or(0);
        let now = env.ledger().timestamp();

        let mut total: i128 = 0;
        let mut processed: u32 = 0;
        let mut idx = actual_start;

        while idx < period_count {
            if let Some(cap) = effective_cap {
                if processed >= cap {
                    return (total, Some(idx));
                }
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
            let decimals = Self::get_payment_token_decimals(
                env.clone(),
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
            );
            let normalized = Self::normalize_amount(revenue, decimals);
            total = total.saturating_add(Self::compute_share(
                env.clone(),
                normalized,
                share_bps,
                RoundingMode::Truncation,
            ));
            processed = processed.saturating_add(1);
            idx = idx.saturating_add(1);
        }

        (total, None)
    }

    /// Preview the total claimable amount for a holder without mutating state.
    ///
    /// This method respects the same blacklist, claim-window, and claim-delay gates that can block
    /// `claim`, then sums only periods currently eligible for payout.
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

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        if Self::is_blacklisted(env.clone(), issuer, namespace, token, holder.clone()) {
            return 0;
        }
        if Self::require_claim_window_open(&env, &offering_id).is_err() {
            return 0;
        }

        let (total, _) =
            Self::compute_claimable_preview(&env, &offering_id, &holder, share_bps, 0, None);
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

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        if Self::is_blacklisted(env.clone(), issuer, namespace, token, holder.clone()) {
            return (0, None);
        }
        if Self::require_claim_window_open(&env, &offering_id).is_err() {
            return (0, None);
        }

        Self::compute_claimable_preview(
            &env,
            &offering_id,
            &holder,
            share_bps,
            start_idx,
            Some(count),
        )
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

        Self::require_not_offering_frozen(&env, &offering_id)?;
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

    /// Test helper: set a holder's claim cursor without performing token transfers.
    #[cfg(test)]
    pub fn test_set_last_claimed_idx(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        last_claimed_idx: u32,
    ) {
        let offering_id = OfferingId { issuer, namespace, token };
        let idx_key = DataKey::LastClaimedIdx(offering_id, holder);
        env.storage().persistent().set(&idx_key, &last_claimed_idx);
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

    // ── Admin rotation safety flow (Issue #191) ───────────────

    pub fn propose_admin_rotation(env: Env, new_admin: Address) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;

        admin.require_auth();

        if new_admin == admin {
            return Err(RevoraError::AdminRotationSameAddress);
        }

        if env.storage().persistent().has(&DataKey::PendingAdmin) {
            return Err(RevoraError::AdminRotationPending);
        }

        env.storage().persistent().set(&DataKey::PendingAdmin, &new_admin);

        env.events().publish((symbol_short!("adm_prop"), admin), new_admin);

        Ok(())
    }

    pub fn accept_admin_rotation(env: Env, new_admin: Address) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        let pending: Address = env
            .storage()
            .persistent()
            .get(&DataKey::PendingAdmin)
            .ok_or(RevoraError::NoAdminRotationPending)?;

        if new_admin != pending {
            return Err(RevoraError::UnauthorizedRotationAccept);
        }

        new_admin.require_auth();

        let old_admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;

        env.storage().persistent().set(&DataKey::Admin, &new_admin);
        env.storage().persistent().remove(&DataKey::PendingAdmin);

        env.events().publish((symbol_short!("adm_acc"), old_admin), new_admin);

        Ok(())
    }

    pub fn cancel_admin_rotation(env: Env) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;

        admin.require_auth();

        let pending: Address = env
            .storage()
            .persistent()
            .get(&DataKey::PendingAdmin)
            .ok_or(RevoraError::NoAdminRotationPending)?;

        env.storage().persistent().remove(&DataKey::PendingAdmin);

        env.events().publish((symbol_short!("adm_canc"), admin), pending);

        Ok(())
    }

    pub fn get_pending_admin_rotation(env: Env) -> Option<Address> {
        env.storage().persistent().get(&DataKey::PendingAdmin)
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
        /// Versioned event v2: [version: u32, frozen: bool]
        Self::emit_v2_event(&env, (EVENT_FREEZE_V2,), true);
        Ok(())
    }

    /// Freeze a single offering while keeping other offerings operational.
    ///
    /// Authorization boundary:
    /// - Current issuer for the offering, or
    /// - Global admin
    ///
    /// Security posture:
    /// - This action is blocked when the whole contract is globally frozen (fail-closed).
    /// - Claims remain intentionally allowed for frozen offerings so users can exit.
    pub fn freeze_offering(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        caller.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        let admin = Self::get_admin(env.clone());
        let is_admin = admin.as_ref().map(|a| caller == *a).unwrap_or(false);
        if caller != current_issuer && !is_admin {
            return Err(RevoraError::NotAuthorized);
        }

        let key = DataKey::FrozenOffering(offering_id);
        env.storage().persistent().set(&key, &true);
        env.events().publish((EVENT_FREEZE_OFFERING, issuer, namespace, token), (caller, true));
        Ok(())
    }

    /// Unfreeze a single offering.
    ///
    /// Authorization mirrors `freeze_offering`: issuer or admin.
    pub fn unfreeze_offering(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        caller.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        let admin = Self::get_admin(env.clone());
        let is_admin = admin.as_ref().map(|a| caller == *a).unwrap_or(false);
        if caller != current_issuer && !is_admin {
            return Err(RevoraError::NotAuthorized);
        }

        let key = DataKey::FrozenOffering(offering_id);
        env.storage().persistent().set(&key, &false);
        env.events().publish((EVENT_UNFREEZE_OFFERING, issuer, namespace, token), (caller, false));
        Ok(())
    }

    /// Return true if an individual offering is frozen.
    pub fn is_offering_frozen(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> bool {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::FrozenOffering(offering_id))
            .unwrap_or(false)
    }

    /// Return true if the contract is frozen.
    pub fn is_frozen(env: Env) -> bool {
        env.storage().persistent().get::<DataKey, bool>(&DataKey::Frozen).unwrap_or(false)
    }

    // ── Multisig admin logic ───────────────────────────────────

    pub const MAX_MULTISIG_OWNERS: u32 = 20;

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
        proposal_duration: u64,
    ) -> Result<(), RevoraError> {
        caller.require_auth();

        // Must be the initialized admin
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;
        if caller != admin {
            return Err(RevoraError::NotAuthorized);
        }

        if env.storage().persistent().has(&DataKey::MultisigThreshold) {
            return Err(RevoraError::LimitReached); // Already initialized
        }
        if owners.is_empty() {
            return Err(RevoraError::LimitReached); // Must have at least one owner
        }
        if owners.len() > Self::MAX_MULTISIG_OWNERS {
            return Err(RevoraError::LimitReached);
        }
        if threshold == 0 || threshold > owners.len() {
            return Err(RevoraError::LimitReached); // Improper threshold
        }

        // Check for duplicate owners
        for i in 0..owners.len() {
            let owner_i = owners.get(i).unwrap();
            for j in (i + 1)..owners.len() {
                if owner_i == owners.get(j).unwrap() {
                    return Err(RevoraError::LimitReached);
                }
            }
        }

        env.storage().persistent().set(&DataKey::MultisigThreshold, &threshold);
        env.storage().persistent().set(&DataKey::MultisigOwners, &owners.clone());
        env.storage().persistent().set(&DataKey::MultisigProposalCount, &0_u32);
        env.events().publish((EVENT_MULTISIG_INIT, caller.clone()), (owners.len(), threshold));
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

        let duration: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::MultisigProposalDuration)
            .ok_or(RevoraError::NotInitialized)?;
        let now = env.ledger().timestamp();
        let expiry = now.checked_add(duration).ok_or(RevoraError::InvalidAmount)?;

        // Proposer's vote counts as the first approval automatically
        let mut initial_approvals = Vec::new(&env);
        initial_approvals.push_back(proposer.clone());

        let proposal = Proposal {
            id,
            action,
            proposer: proposer.clone(),
            approvals: initial_approvals,
            executed: false,
            expiry,
        };

        env.storage().persistent().set(&DataKey::MultisigProposal(id), &proposal);
        env.storage().persistent().set(&count_key, &(id + 1));

        env.events().publish((EVENT_PROPOSAL_CREATED, proposer.clone()), (id, expiry));
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

        if env.ledger().timestamp() >= proposal.expiry {
            return Err(RevoraError::ProposalExpired);
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

#![no_std]

        // Execute the action
        match proposal.action.clone() {
            ProposalAction::SetAdmin(new_admin) => {
                env.storage().persistent().set(&DataKey::Admin, &new_admin);
            }
            ProposalAction::Freeze => {
                Self::require_not_frozen(&env)?;
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
            ProposalAction::RemoveOwner(addr) => {
                let mut owners: Vec<Address> =
                    env.storage().persistent().get(&DataKey::MultisigOwners).unwrap();

                // Guard 1: existence check — addr must currently be an owner
                if !owners.contains(&addr) {
                    return Err(RevoraError::NotAuthorized);
                }

                // Guard 2: threshold invariant — removal must not drop owner count below threshold
                let threshold: u32 =
                    env.storage().persistent().get(&DataKey::MultisigThreshold).unwrap();
                if (owners.len() - 1) < threshold {
                    return Err(RevoraError::LimitReached);
                }

                // Remove addr from owners
                let mut new_owners = Vec::new(&env);
                for i in 0..owners.len() {
                    let owner = owners.get(i).unwrap();
                    if owner != addr {
                        new_owners.push_back(owner);
                    }
                }
                owners = new_owners;

                // Persist updated owners list
                env.storage().persistent().set(&DataKey::MultisigOwners, &owners);
            }
            ProposalAction::SetProposalDuration(new_duration) => {
                if new_duration == 0 {
                    return Err(RevoraError::InvalidAmount);
                }
                env.storage()
                    .persistent()
                    .set(&DataKey::MultisigProposalDuration, &new_duration);
                env.events().publish(
                    (EVENT_DURATION_SET, proposal.proposer.clone()),
                    new_duration,
                );
            }
        }

// ─── Storage key types ────────────────────────────────────────────────────────

/// Top-level storage keys stored in persistent contract storage.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The contract admin address.
    Admin,
    /// The token contract ID used for all deposits and claims.
    Token,
    /// Counter tracking the next period ID to be assigned.
    PeriodCounter,
    /// All registered period IDs (Vec<u32>).
    PeriodIds,
    /// Per-period metadata, keyed by period ID.
    Period(u32),
    /// Per-period beneficiary list, keyed by period ID.
    Beneficiaries(u32),
    /// Claim record: whether `address` has claimed from `period_id`.
    Claimed(u32, Address),
}

// ─── Domain types ─────────────────────────────────────────────────────────────

/// Metadata for a single revenue period.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Period {
    /// Unique monotonically-increasing identifier.
    pub id: u32,
    /// Ledger sequence number at which the period opens (inclusive).
    pub start_ledger: u32,
    /// Ledger sequence number at which the period closes (inclusive).
    pub end_ledger: u32,
    /// Total token amount deposited for distribution this period.
    pub revenue_amount: i128,
    /// How many tokens have been claimed so far.
    pub claimed_amount: i128,
}

// ─── Error codes ──────────────────────────────────────────────────────────────

/// Canonical error codes returned by contract functions.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    /// Caller is not the admin.
    Unauthorized = 1,
    /// Contract has already been initialised.
    AlreadyInitialized = 2,
    /// The referenced period does not exist.
    PeriodNotFound = 3,
    /// The period's end ledger has not been reached yet.
    PeriodNotEnded = 4,
    /// The caller is not registered as a beneficiary for this period.
    NotBeneficiary = 5,
    /// The caller has already claimed their share for this period.
    AlreadyClaimed = 6,
    /// A period with overlapping ledger range already exists.
    PeriodOverlap = 7,
    /// The supplied parameters are logically invalid (e.g. start > end, zero amount).
    InvalidInput = 8,
    /// The revenue deposit failed (e.g. insufficient token balance).
    DepositFailed = 9,
    /// Arithmetic overflow occurred.
    Overflow = 10,
    /// No beneficiaries are registered; nothing to distribute.
    NoBeneficiaries = 11,
}

// ─── Contract struct ──────────────────────────────────────────────────────────

#[contract]
pub struct RevenueDepositContract;

// ─── Implementation ───────────────────────────────────────────────────────────

#[contractimpl]
impl RevenueDepositContract {
    // ── Initialisation ────────────────────────────────────────────────────────

    /// Initialise the contract.
    ///
    /// # Arguments
    /// * `admin` – Address that will hold admin privileges.
    /// * `token` – Stellar token contract address used for deposits/claims.
    ///
    /// # Errors
    /// * [`ContractError::AlreadyInitialized`] – if called more than once.
    pub fn initialize(env: Env, admin: Address, token: Address) -> Result<(), ContractError> {
        if env.storage().persistent().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();

        env.storage().persistent().set(&DataKey::Admin, &admin);
        env.storage().persistent().set(&DataKey::Token, &token);
        env.storage().persistent().set(&DataKey::PeriodCounter, &0u32);
        env.storage()
            .persistent()
            .set(&DataKey::PeriodIds, &Vec::<u32>::new(&env));

        Ok(())
    }

    // ── Period management ─────────────────────────────────────────────────────

    /// Create a new revenue period and transfer `revenue_amount` tokens from the
    /// admin into the contract.
    ///
    /// # Arguments
    /// * `start_ledger` – First ledger of the period (inclusive, must be ≥ current ledger).
    /// * `end_ledger`   – Last ledger of the period (inclusive, must be > `start_ledger`).
    /// * `revenue_amount` – Positive token quantity to deposit for this period.
    ///
    /// # Errors
    /// * [`ContractError::Unauthorized`]   – caller is not admin.
    /// * [`ContractError::InvalidInput`]   – bad ledger range or zero/negative amount.
    /// * [`ContractError::PeriodOverlap`]  – range overlaps an existing period.
    pub fn create_period(
        env: Env,
        start_ledger: u32,
        end_ledger: u32,
        revenue_amount: i128,
    ) -> Result<u32, ContractError> {
        let admin: Address = env.storage().persistent().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        // ── Validate inputs ────────────────────────────────────────────────
        if revenue_amount <= 0 {
            return Err(ContractError::InvalidInput);
        }
        if start_ledger >= end_ledger {
            return Err(ContractError::InvalidInput);
        }

        // ── Overlap detection ──────────────────────────────────────────────
        Self::assert_no_overlap(&env, start_ledger, end_ledger)?;

        // ── Assign ID ─────────────────────────────────────────────────────
        let mut counter: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::PeriodCounter)
            .unwrap_or(0);
        let period_id = counter;
        counter = counter.checked_add(1).ok_or(ContractError::Overflow)?;
        env.storage()
            .persistent()
            .set(&DataKey::PeriodCounter, &counter);

        // ── Persist period ─────────────────────────────────────────────────
        let period = Period {
            id: period_id,
            start_ledger,
            end_ledger,
            revenue_amount,
            claimed_amount: 0,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Period(period_id), &period);
        env.storage()
            .persistent()
            .set(&DataKey::Beneficiaries(period_id), &Vec::<Address>::new(&env));

        let mut ids: Vec<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::PeriodIds)
            .unwrap_or_else(|| Vec::new(&env));
        ids.push_back(period_id);
        env.storage().persistent().set(&DataKey::PeriodIds, &ids);

        // ── Pull tokens from admin ─────────────────────────────────────────
        let token: Address = env.storage().persistent().get(&DataKey::Token).unwrap();
        let token_client = TokenClient::new(&env, &token);
        token_client.transfer(&admin, &env.current_contract_address(), &revenue_amount);

        Ok(period_id)
    }

    // ── Beneficiary management ────────────────────────────────────────────────

    /// Register `beneficiary` as eligible to claim from `period_id`.
    ///
    /// Idempotent — adding a beneficiary twice is a no-op (not an error).
    ///
    /// # Errors
    /// * [`ContractError::Unauthorized`]  – caller is not admin.
    /// * [`ContractError::PeriodNotFound`] – `period_id` does not exist.
    pub fn add_beneficiary(
        env: Env,
        period_id: u32,
        beneficiary: Address,
    ) -> Result<(), ContractError> {
        let admin: Address = env.storage().persistent().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        Self::assert_period_exists(&env, period_id)?;

        let mut beneficiaries: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Beneficiaries(period_id))
            .unwrap_or_else(|| Vec::new(&env));

        // Idempotency guard
        if !beneficiaries.contains(&beneficiary) {
            beneficiaries.push_back(beneficiary);
            env.storage()
                .persistent()
                .set(&DataKey::Beneficiaries(period_id), &beneficiaries);
        }

        Ok(())
    }

    /// Remove `beneficiary` from `period_id`.  If they have not yet claimed their
    /// share, that share reverts to the unclaimed pool (claimable by remaining
    /// beneficiaries or recoverable by admin via a future extension).
    ///
    /// # Errors
    /// * [`ContractError::Unauthorized`]   – caller is not admin.
    /// * [`ContractError::PeriodNotFound`] – `period_id` does not exist.
    /// * [`ContractError::NotBeneficiary`] – address not currently registered.
    pub fn remove_beneficiary(
        env: Env,
        period_id: u32,
        beneficiary: Address,
    ) -> Result<(), ContractError> {
        let admin: Address = env.storage().persistent().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        Self::assert_period_exists(&env, period_id)?;

        let mut beneficiaries: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Beneficiaries(period_id))
            .unwrap_or_else(|| Vec::new(&env));

        let pos = beneficiaries
            .iter()
            .position(|b| b == beneficiary)
            .ok_or(ContractError::NotBeneficiary)?;

        beneficiaries.remove(pos as u32);
        env.storage()
            .persistent()
            .set(&DataKey::Beneficiaries(period_id), &beneficiaries);

        Ok(())
    }

    // ── Claims ────────────────────────────────────────────────────────────────

    /// Claim a pro-rata share of `period_id`'s revenue.
    ///
    /// The share is `floor(revenue_amount / beneficiary_count)`.  Any remainder
    /// (due to integer division) stays in the contract as unclaimed dust.
    ///
    /// # Preconditions
    /// * Current ledger must be **strictly after** `end_ledger` of the period.
    /// * Caller must be a registered beneficiary.
    /// * Caller must not have claimed before.
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
    ) -> i128 {
        caller.require_auth();

        if total_supply == 0 {
            return 0i128;
        }

        let offering =
            match Self::get_offering(env.clone(), issuer.clone(), namespace, token.clone()) {
                Some(o) => o,
                None => return 0i128,
            };

        if Self::is_blacklisted(
            env.clone(),
            issuer.clone(),
            offering.namespace.clone(),
            token.clone(),
            holder.clone(),
        ) {
            return 0i128;
        }

        // ── Timing gate ────────────────────────────────────────────────────
        let current_ledger = env.ledger().sequence();
        if current_ledger <= period.end_ledger {
            return Err(ContractError::PeriodNotEnded);
        }

        // ── Beneficiary check ──────────────────────────────────────────────
        let beneficiaries: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Beneficiaries(period_id))
            .unwrap_or_else(|| Vec::new(&env));

        if beneficiaries.is_empty() {
            return Err(ContractError::NoBeneficiaries);
        }

        if !beneficiaries.contains(&claimant) {
            return Err(ContractError::NotBeneficiary);
        }

        // ── Double-claim guard ─────────────────────────────────────────────
        let claim_key = DataKey::Claimed(period_id, claimant.clone());
        if env.storage().persistent().has(&claim_key) {
            return Err(ContractError::AlreadyClaimed);
        }

        // ── Compute share ──────────────────────────────────────────────────
        let count = beneficiaries.len() as i128;
        let share = period
            .revenue_amount
            .checked_div(count)
            .ok_or(ContractError::Overflow)?;

        if share <= 0 {
            return Err(ContractError::InvalidInput);
        }

        // ── Update state (checks-effects-interactions) ─────────────────────
        env.storage().persistent().set(&claim_key, &true);
        period.claimed_amount = period
            .claimed_amount
            .checked_add(share)
            .ok_or(ContractError::Overflow)?;
        env.storage()
            .persistent()
            .set(&DataKey::Period(period_id), &period);

        // ── Transfer tokens ────────────────────────────────────────────────
        let token: Address = env.storage().persistent().get(&DataKey::Token).unwrap();
        let token_client = TokenClient::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &claimant, &share);

        Ok(share)
    }

    // ── Read-only helpers ─────────────────────────────────────────────────────

    /// Return metadata for a period.
    pub fn get_period(env: Env, period_id: u32) -> Result<Period, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Period(period_id))
            .ok_or(ContractError::PeriodNotFound)
    }

    /// Return all period IDs registered with this contract.
    pub fn get_period_ids(env: Env) -> Vec<u32> {
        env.storage()
            .persistent()
            .get(&DataKey::PeriodIds)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Return the beneficiary list for a period.
    pub fn get_beneficiaries(env: Env, period_id: u32) -> Result<Vec<Address>, ContractError> {
        Self::assert_period_exists(&env, period_id)?;
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::Beneficiaries(period_id))
            .unwrap_or_else(|| Vec::new(&env)))
    }

    /// Return whether `address` has claimed from `period_id`.
    pub fn has_claimed(env: Env, period_id: u32, address: Address) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Claimed(period_id, address))
    }

    /// Return the current admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage().persistent().get(&DataKey::Admin).unwrap()
    }

    /// Return the token contract address.
    pub fn get_token(env: Env) -> Address {
        env.storage().persistent().get(&DataKey::Token).unwrap()
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Assert that `period_id` is stored.
    fn assert_period_exists(env: &Env, period_id: u32) -> Result<(), ContractError> {
        if !env.storage().persistent().has(&DataKey::Period(period_id)) {
            return Err(ContractError::PeriodNotFound);
        }
        Ok(())
    }

    /// Assert that [start_ledger, end_ledger] does not overlap any existing period.
    fn assert_no_overlap(
        env: &Env,
        start_ledger: u32,
        end_ledger: u32,
    ) -> Result<(), ContractError> {
        let ids: Vec<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::PeriodIds)
            .unwrap_or_else(|| Vec::new(env));

        for id in ids.iter() {
            let existing: Period = env
                .storage()
                .persistent()
                .get(&DataKey::Period(id))
                .unwrap();
            // Overlap: NOT (new_end < existing_start OR new_start > existing_end)
            if !(end_ledger < existing.start_ledger || start_ledger > existing.end_ledger) {
                return Err(ContractError::PeriodOverlap);
            }
        }
        Ok(())
    }

    /// Build a summary map of unclaimed amounts per period (useful for admin dashboards).
    pub fn unclaimed_summary(env: Env) -> Map<u32, i128> {
        let ids: Vec<u32> = env
            .storage()
            .persistent()
            .get(&DataKey::PeriodIds)
            .unwrap_or_else(|| Vec::new(&env));

        let mut map: Map<u32, i128> = Map::new(&env);
        for id in ids.iter() {
            if let Some(period) = env
                .storage()
                .persistent()
                .get::<DataKey, Period>(&DataKey::Period(id))
            {
                let unclaimed = period.revenue_amount - period.claimed_amount;
                map.set(id, unclaimed);
            }
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

    /// Migrate the contract state to the current CONTRACT_VERSION.
    ///
    /// This function is intended to be called by the admin after a WASM upgrade.
    /// It executes versioned migration hooks sequentially from the last recorded version
    /// to the current `CONTRACT_VERSION`.
    ///
    /// Security properties:
    /// - Only the current admin can call this.
    /// - Respects the contract freeze state.
    /// - Prevents downgrades or re-running the same migration.
    /// - Updates `DataKey::DeployedVersion` only on success.
    pub fn migrate(env: Env) -> Result<u32, RevoraError> {
        Self::require_not_frozen(&env)?;

        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .ok_or(RevoraError::NotInitialized)?;

        admin.require_auth();

        let stored_version = env
            .storage()
            .persistent()
            .get(&DataKey::DeployedVersion)
            .unwrap_or(0u32);

        if stored_version == CONTRACT_VERSION {
            return Err(RevoraError::AlreadyAtTargetVersion);
        }

        if stored_version > CONTRACT_VERSION {
            return Err(RevoraError::MigrationDowngradeNotAllowed);
        }

        // Run migration hooks sequentially
        for version in (stored_version + 1)..=CONTRACT_VERSION {
            Self::run_migration_hook(&env, version)?;
        }

        env.storage().persistent().set(&DataKey::DeployedVersion, &CONTRACT_VERSION);

        env.events().publish(
            (symbol_short!("migrated"), admin),
            (stored_version, CONTRACT_VERSION),
        );

        Ok(CONTRACT_VERSION)
    }

    /// Internal helper to run migration logic for a specific version bump.
    fn run_migration_hook(env: &Env, version: u32) -> Result<(), RevoraError> {
        match version {
            1 => {
                // Initial version setup if needed (usually handled by initialize)
            }
            2 => {
                // Example v2 migration logic
            }
            3 => {
                // Example v3 migration logic
            }
            4 => {
                // Example v4 migration logic
            }
            _ => {
                // Future versions will be handled here
            }
        }
        Ok(())
    }

    /// Return the current deployed version of the contract state.
    pub fn get_deployed_version(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::DeployedVersion)
            .unwrap_or(0)
    }

    /// Return the current contract version (#23). Used for upgrade compatibility and migration.
    pub fn get_version(env: Env) -> u32 {
        let _ = env;
        CONTRACT_VERSION
    }

    /// Deterministic fixture payloads for indexer integration tests (#187).
    ///
    /// Returns canonical v2 indexed topics in a stable order so indexers can
    /// validate decoding, routing and storage schemas without replaying full
    /// contract flows.
    pub fn get_indexer_fixture_topics(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        period_id: u64,
    ) -> Vec<EventIndexTopicV2> {
        let mut fixtures = Vec::new(&env);
        fixtures.push_back(EventIndexTopicV2 {
            version: 2,
            event_type: EVENT_TYPE_OFFER,
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
            period_id: 0,
        });
        fixtures.push_back(EventIndexTopicV2 {
            version: 2,
            event_type: EVENT_TYPE_REV_INIT,
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
            period_id,
        });
        fixtures.push_back(EventIndexTopicV2 {
            version: 2,
            event_type: EVENT_TYPE_REV_OVR,
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
            period_id,
        });
        fixtures.push_back(EventIndexTopicV2 {
            version: 2,
            event_type: EVENT_TYPE_REV_REJ,
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
            period_id,
        });
        fixtures.push_back(EventIndexTopicV2 {
            version: 2,
            event_type: EVENT_TYPE_REV_REP,
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
            period_id,
        });
        fixtures.push_back(EventIndexTopicV2 {
            version: 2,
            event_type: EVENT_TYPE_CLAIM,
            issuer,
            namespace,
            token,
            period_id: 0,
        });
        fixtures
    }
}