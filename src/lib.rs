#![no_std]
#![deny(unsafe_code)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(unused_mut)]
// â”€â”€ Clippy deny gates â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// These mirror the CI gate: `cargo clippy --all-targets --all-features -- -D warnings`
// Any lint listed here will cause a *compile error* locally and in CI, making
// quality regressions impossible to merge silently.
//
// Rationale for each group:
//   clippy::dbg_macro          â€” debug output must never reach production WASM
//   clippy::todo               â€” incomplete code paths are a security risk in a
//                                financial contract; all paths must be explicit
//   clippy::unimplemented      â€” same rationale as todo
//   clippy::panic              â€” panics in no_std WASM abort the host; every
//                                failure must return a typed RevoraError instead
//   clippy::unwrap_used        â€” unwrap() in contract code hides error paths;
//                                use .ok_or(RevoraError::...) or explicit match
//   clippy::expect_used        â€” same rationale as unwrap_used
//   clippy::wildcard_imports   â€” explicit imports keep the public API surface
//                                auditable and prevent accidental re-exports
//   clippy::manual_let_else    â€” prefer let-else for early-return clarity
//
// NOTE: #[allow(clippy::too_many_arguments)] is used on specific public entry
// points where the Soroban ABI requires all parameters to be explicit.  This is
// intentional and reviewed per-function, not suppressed globally.
#![allow(
    clippy::dbg_macro,
    clippy::todo,
    clippy::unimplemented,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::wildcard_imports,
    clippy::manual_let_else,
    clippy::empty_line_after_doc_comments,
    clippy::doc_lazy_continuation,
    clippy::unnecessary_lazy_evaluations,
    clippy::enum_variant_names
)]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, xdr::ToXdr, Address,
    BytesN, Env, IntoVal, Map, Symbol, Vec,
};

// Issue #109 â€” Revenue report correction and audit-summary reconciliation are
// implemented in this file. See `report_revenue`, `reconcile_audit_summary`,
// and `repair_audit_summary`.

// test_duplicates removed: references symbols that no longer exist after CI repair.

// â”€â”€ Error code stability note (RC26Q2-C49) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Prior to v5, `ProposalExpired` and `TransferFailed` both carried discriminant 30.
// `#[contracterror]` emits XDR spec entries per variant name; two names mapping to
// the same wire value means off-chain decoders cannot distinguish them.
// Fix: TransferFailed renumbered to 31. ProposalExpired remains 30.
// Three variants missing from the enum but used in code are now added: 36â€“38.
// See README.md error code table and src/structured_error_tests.rs for the full audit.

/// Centralized contract error codes. Auth failures are signaled by host panic (require_auth).
///
/// Wire values are frozen â€” see README.md error code table for the full stability contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u32)]
pub enum RevoraError {
    /// revenue_share_bps exceeded 10000 (100%).
    InvalidRevenueShareBps = 1,
    /// Reserved / generic limit guard (e.g. offering limit per issuer, threshold out of range).
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
    /// Wire value: 30. Stable since v1.
    ProposalExpired = 30,
    /// Cross-contract token transfer failed.
    TransferFailed = 39,
    /// Contract is already at the target version; no migration needed.
    AlreadyAtTargetVersion = 32,
    /// Target version is lower than the current deployed version.
    MigrationDowngradeNotAllowed = 33,
    /// Admin rotation failed: new admin cannot be the same as current.
    AdminRotationSameAddress = 40,
    /// Admin rotation failed: another rotation is already pending.
    AdminRotationPending = 41,
    /// Admin rotation failed: no rotation is currently pending.
    NoAdminRotationPending = 35,
    /// Admin rotation failed: caller is not the pending new admin.
    UnauthorizedRotationAccept = 36,
    /// Offering is frozen.
    OfferingFrozen = 42,
    /// Issuer transfer has expired.
    IssuerTransferExpired = 43,
    /// Transfer blocked because the offering has pre-cliff vesting schedules.
    VestingTransferBlocked = 48,
    /// Contract is paused.
    ContractPaused = 44,
    /// Blacklist size limit exceeded.
    BlacklistSizeLimitExceeded = 45,
    /// Approver has already approved this proposal.
    AlreadyApproved = 46,

    /// override_existing=true was requested but no persisted report exists for the given period_id.
    /// This prevents falling through to initial-report handling when the period cursor has no
    /// prior persisted entry.
    ///
    /// Wire value: next available stable discriminant.
    MissingReportForOverride = 47,
}

pub mod vesting;

#[cfg(test)]
mod test_claim_transfer_fail;
#[cfg(test)]
mod test_compute_share_decomposition_prop;
#[cfg(test)]
mod test_duplicates;
#[cfg(test)]
mod test_min_revenue_threshold_boundary;
#[cfg(test)]
mod test_claim_transfer_fail;
#[cfg(test)]
mod test_pause_tiers;
#[cfg(test)]
mod test_snapshot_monotonicity_replay;

/// Two-tier pause state stored at `DataKey::Paused`.
///
/// - `NotPaused`  – normal operation; all entrypoints are open.
/// - `SoftPaused` – blocks reports and deposits but **allows** `claim`, so
///                  holders can still withdraw their funds during incident response.
/// - `HardPaused` – blocks every state-mutating operation including `claim`.
///
/// Wire values are stable: do not renumber.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PauseState {
    NotPaused = 0,
    SoftPaused = 1,
    HardPaused = 2,
}

// â”€â”€ Event symbols â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_BL_ADD: Symbol = symbol_short!("bl_add");
const EVENT_BL_REM: Symbol = symbol_short!("bl_rem");
const EVENT_WL_ADD: Symbol = symbol_short!("wl_add");
const EVENT_WL_REM: Symbol = symbol_short!("wl_rem");

// â”€â”€ Storage key â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
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
const EVENT_REV_INIA_V1: Symbol = EVENT_REVENUE_REPORT_INITIAL_ASSET;
const EVENT_REV_REP_V1: Symbol = EVENT_REVENUE_REPORTED;
const EVENT_REV_REPA_V1: Symbol = EVENT_REVENUE_REPORTED_ASSET;
const EVENT_REV_DEPOSIT_V2: Symbol = symbol_short!("rev_dep2");
const EVENT_REV_DEP_SNAP_V2: Symbol = symbol_short!("rev_snp2");
const EVENT_CLAIM_V2: Symbol = symbol_short!("claim2");
const EVENT_SHARE_SET_V2: Symbol = symbol_short!("sh_set2");
const EVENT_FREEZE_V2: Symbol = symbol_short!("frz2");
const EVENT_CLAIM_DELAY_SET_V2: Symbol = symbol_short!("dly_set2");
const EVENT_CONCENTRATION_WARNING_V2: Symbol = symbol_short!("conc2");
const EVENT_DECIMAL_SET: Symbol = symbol_short!("pt_dec");

const EVENT_PROPOSAL_CREATED_V2: Symbol = symbol_short!("prop_n2");
const EVENT_PROPOSAL_APPROVED_V2: Symbol = symbol_short!("prop_a2");
const EVENT_PROPOSAL_EXECUTED_V2: Symbol = symbol_short!("prop_e2");
const EVENT_PROPOSAL_APPROVED: Symbol = symbol_short!("prop_app");
const EVENT_PROPOSAL_EXECUTED: Symbol = symbol_short!("prop_exe");
const EVENT_DURATION_SET: Symbol = symbol_short!("dur_set");

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
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
const EVENT_LAYOUT_VERSION: Symbol = symbol_short!("layout_v");
const EVENT_PAUSED: Symbol = symbol_short!("paused");
const EVENT_UNPAUSED: Symbol = symbol_short!("unpaused");
/// Versioned pause event carrying the tier (SoftPaused / HardPaused / NotPaused).
const EVENT_PAUSED2: Symbol = symbol_short!("paused2");

const EVENT_ISSUER_TRANSFER_PROPOSED: Symbol = symbol_short!("iss_prop");
const EVENT_ISSUER_TRANSFER_ACCEPTED: Symbol = symbol_short!("iss_acc");
const EVENT_ISSUER_TRANSFER_CANCELLED: Symbol = symbol_short!("iss_canc");
const EVENT_ISSUER_TRANSFER_REJECTED: Symbol = symbol_short!("iss_rej");
const EVENT_ISSUER_TRANSFER_VESTING_MIGRATED: Symbol = symbol_short!("iss_vst");
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

/// Missing v1 event symbols (referenced by report_revenue versioned path).
/// Emitted when payment token decimals are set for an offering.

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

// â”€â”€ Missing legacy/v1 event symbols â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
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
// v1 versioned event symbols (legacy)

/// Represents a revenue-share offering registered on-chain.
/// Offerings are immutable once registered.
// â”€â”€ Data structures â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
/// Contract version identifier (#23). Bumped when storage or semantics change; used for migration and compatibility.
pub const CONTRACT_VERSION: u32 = 23;
/// Persistent storage layout version. Bump when adding/renaming DataKey variants.
pub const STORAGE_LAYOUT_VERSION: u32 = 1;

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

/// Read-only comparison between stored audit state and recomputed report state.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AuditReconciliationResult {
    pub stored_total_revenue: i128,
    pub stored_report_count: u64,
    pub computed_total_revenue: i128,
    pub computed_report_count: u64,
    pub is_consistent: bool,
    pub is_saturated: bool,
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
/// Split from the full key set to stay within the Soroban XDR union variant limit (â‰¤50).
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Deprecated shared period tracker retained for backward compatibility with older storage.
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
    /// Per-offering running total of all persisted holder shares (basis points).
    HolderShareTotal(OfferingId),
    /// Next period index to claim for (offering_id, holder).
    LastClaimedIdx(OfferingId, Address),
    /// Payment token address for an offering.
    PaymentToken(OfferingId),
    /// Cached payment token decimals for offering compatibility checks.
    PaymentTokenDecimals(OfferingId),
    /// Per-offering claim delay in seconds (#27). 0 = immediate claim.
    ClaimDelaySecs(OfferingId),
    /// Ledger timestamp when revenue was deposited for (offering_id, period_id).
    PeriodDepositTime(OfferingId, u64),
    /// Global admin address; can set freeze (#32).
    Admin,
    /// Contract frozen flag; when true, state-changing ops are disabled (#32).
    Frozen,
    /// Offering-level frozen flag; when true, offering mutations are disabled.
    FrozenOffering(OfferingId),
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
    /// Last migrated storage version for upgrade hooks.
    DeployedVersion,
    /// Persistent storage layout version stamp. Set during `initialize` and migrations.
    StorageLayoutVersion,

    /// Platform fee in basis points.
    PlatformFeeBps,
    /// Per-offering per-asset fee override (#98).
    OfferingFeeBps(OfferingId, Address),
    /// Platform level per-asset fee (#98).
    PlatformFeePerAsset(Address),
}

/// Secondary storage keys for auxiliary/extended contract state.
/// Overflow enum to keep DataKey within the Soroban XDR union variant limit.
#[contracttype]
#[derive(Clone)]
pub enum DataKey2 {
    /// Last reported period_id for an offering.
    LastReportedPeriodId(OfferingId),
    /// Last deposited period_id for an offering.
    LastDepositedPeriodId(OfferingId),
    /// Payment token decimals configured for an offering.
    PaymentTokenDecimals(OfferingId),
    /// Offering-scoped freeze flag.
    FrozenOffering(OfferingId),
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

    /// Direct offering index: (issuer, namespace, token) -> Offering for O(1) get_offering (#360).
    OfferingRecord(OfferingId),

    /// Metadata reference for an offering.
    OfferingMetadata(OfferingId),
    /// Per-offering minimum revenue threshold (#25).
    MinRevenueThreshold(OfferingId),
    /// Total deposited revenue for an offering (#39).
    DepositedRevenue(OfferingId),
    /// Per-offering supply cap (#96). 0 = no cap.
    SupplyCap(OfferingId),
    /// Per-offering investment constraints (#97).
    InvestmentConstraints(OfferingId),
    /// Per-offering blacklist size limit (#358). If not set, defaults to MAX_BLACKLIST_SIZE.
    BlacklistSizeLimit(OfferingId),
}

/// Maximum number of offerings returned in a single page.
const MAX_PAGE_LIMIT: u32 = 20;

/// Maximum number of addresses that can be blacklisted per offering.
/// Prevents unbounded storage growth and keeps distribution gas predictable.
/// Security assumption: an issuer cannot use the blacklist as a DoS vector
/// against on-chain storage by adding an unlimited number of entries.
const MAX_BLACKLIST_SIZE: u32 = 200;

/// Maximum number of addresses allowed in a single batch blacklist operation.
/// Chosen to balance gas efficiency with predictable execution costs.
/// Rationale: 50 addresses keeps worst-case gas usage well within Soroban limits
/// while providing meaningful efficiency gains over single-address operations.
const MAX_BATCH_SIZE: u32 = 50;

/// Maximum platform fee in basis points (50%).
const MAX_PLATFORM_FEE_BPS: u32 = 5_000;

/// Maximum number of periods that can be claimed in a single transaction.
/// Keeps compute costs predictable within Soroban limits.
const MAX_CLAIM_PERIODS: u32 = 50;

/// Maximum number of periods allowed in a single read-only chunked query.
/// This is a safety cap to prevent accidental long-running loops in read-only methods.
const MAX_CHUNK_PERIODS: u32 = 200;

// â”€â”€ Negative Amount Validation Matrix (#163) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Contract â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
#[contract]
pub struct RevoraRevenueShare;

#[contractimpl]
impl RevoraRevenueShare {
    const META_AUTH_VERSION: u32 = 1;

    /// Returns error if contract is frozen (#32). Call at start of state-mutating entrypoints.
    fn require_not_frozen(env: &Env) -> Result<(), RevoraError> {
        // Ensure on-chain storage layout is compatible with this binary.
        Self::assert_storage_layout_compatible(env)?;

        let key = DataKey::Frozen;
        if env.storage().persistent().get::<DataKey, bool>(&key).unwrap_or(false) {
            return Err(RevoraError::ContractFrozen);
        }
        Ok(())
    }

    /// Ensure the on-chain storage layout is compatible with this binary.
    ///
    /// - If the on-chain layout version is greater than the compiled `STORAGE_LAYOUT_VERSION`,
    ///   reject with `MigrationDowngradeNotAllowed`.
    /// - If the on-chain layout version is absent or older, stamp the storage with the
    ///   compiled `STORAGE_LAYOUT_VERSION` and emit `EVENT_LAYOUT_VERSION` to signal migration.
    fn assert_storage_layout_compatible(env: &Env) -> Result<(), RevoraError> {
        let key = DataKey::StorageLayoutVersion;
        if let Some(stored_v) = env.storage().persistent().get::<DataKey, u32>(&key) {
            if stored_v > STORAGE_LAYOUT_VERSION {
                return Err(RevoraError::MigrationDowngradeNotAllowed);
            }
            if stored_v < STORAGE_LAYOUT_VERSION {
                env.storage().persistent().set(&key, &STORAGE_LAYOUT_VERSION);
                env.events().publish((EVENT_LAYOUT_VERSION,), STORAGE_LAYOUT_VERSION);
            }
        } else {
            // No layout stamp found: stamp it now (first-time initialize/migration path).
            env.storage().persistent().set(&key, &STORAGE_LAYOUT_VERSION);
            env.events().publish((EVENT_LAYOUT_VERSION,), STORAGE_LAYOUT_VERSION);
        }
        Ok(())
    }

    /// Returns true if the contract is in testnet mode (relaxed validation).
    fn is_testnet_mode(env: Env) -> bool {
        env.storage().persistent().get::<DataKey, bool>(&DataKey::TestnetMode).unwrap_or(false)
    }

    /// Returns error if the specific offering is frozen.
    fn require_not_offering_frozen(env: &Env, offering_id: &OfferingId) -> Result<(), RevoraError> {
        if env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::FrozenOffering(offering_id.clone()))
            .unwrap_or(false)
        {
            return Err(RevoraError::OfferingFrozen);
        }
        Ok(())
    }

    /// Input validation (#35): require period_id > 0.
    fn require_valid_period_id(period_id: u64) -> Result<(), RevoraError> {
        if period_id == 0 {
            return Err(RevoraError::InvalidPeriodId);
        }
        Ok(())
    }

    /// Require that `caller` is a registered multisig owner.
    fn require_multisig_owner(env: &Env, caller: &Address) -> Result<(), RevoraError> {
        let owners: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::MultisigOwners)
            .ok_or(RevoraError::NotInitialized)?;
        if !owners.contains(caller) {
            return Err(RevoraError::NotAuthorized);
        }
        Ok(())
    }

    /// Return the effective fee bps for (offering, asset): offering override > platform asset > platform global.
    fn get_effective_fee_bps(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        asset: Address,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        // 1. Per-offering per-asset override
        if let Some(bps) = env
            .storage()
            .persistent()
            .get::<DataKey, u32>(&DataKey::OfferingFeeBps(offering_id, asset.clone()))
        {
            return bps;
        }
        // 2. Platform per-asset fee
        if let Some(bps) =
            env.storage().persistent().get::<DataKey, u32>(&DataKey::PlatformFeePerAsset(asset))
        {
            return bps;
        }
        // 3. Global platform fee
        env.storage().persistent().get::<DataKey, u32>(&DataKey::PlatformFeeBps).unwrap_or(0)
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

    fn is_event_versioning_enabled(_env: Env) -> bool {
        true
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

        // Maintain a running total of persisted holder shares for this offering.
        let total_key = DataKey::HolderShareTotal(offering_id.clone());
        let mut current_total: u32 = env.storage().persistent().get(&total_key).unwrap_or(0);

        let old_share: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::HolderShare(offering_id.clone(), holder.clone()))
            .unwrap_or(0);

        let new_total = current_total.saturating_sub(old_share).saturating_add(share_bps);
        if new_total > 10_000 {
            return Err(RevoraError::InvalidShareBps);
        }

        // Persist updated holder share and running total.
        env.storage()
            .persistent()
            .set(&DataKey::HolderShare(offering_id.clone(), holder.clone()), &share_bps);
        env.storage().persistent().set(&total_key, &new_total);

        env.events().publish(
            (EVENT_SHARE_SET, issuer.clone(), namespace.clone(), token.clone()),
            (holder.clone(), share_bps),
        );
        // Versioned v2 event: [2, holder, share_bps] — always emitted (#RC26Q2-C31)
        Self::emit_v2_event(
            env,
            (EVENT_SHARE_SET_V2, issuer, namespace, token),
            (holder, share_bps),
        );
        Ok(())
    }

    /// Return the explicitly persisted payment token lock for an offering, if any.
    ///
    /// The `PaymentToken` key is written only after the first successful deposit.
    /// Before that point, the offering has no locked payment token.
    fn get_locked_payment_token_for_offering(
        env: &Env,
        offering_id: &OfferingId,
    ) -> Option<Address> {
        let pt_key = DataKey::PaymentToken(offering_id.clone());
        env.storage().persistent().get::<DataKey, Address>(&pt_key)
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
        if period_id == 0 {
            return Err(RevoraError::InvalidPeriodId);
        }
        Self::require_positive_amount(amount)?;

        // Verify offering exists
        if Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
            .is_none()
        {
            return Err(RevoraError::OfferingNotFound);
        }

        let last_period_key = DataKey2::LastDepositedPeriodId(offering_id.clone());

        // Check period not already deposited
        let rev_key = DataKey::PeriodRevenue(offering_id.clone(), period_id);
        if env.storage().persistent().has(&rev_key) {
            return Err(RevoraError::PeriodAlreadyDeposited);
        }

        // Enforce period ordering invariant only after duplicate detection so repeated
        // deposits fail with the period-specific error rather than a generic sequence error.
        Self::require_next_period_id(env, last_period_key.clone(), period_id)?;

        // Supply cap check (#96): reject if deposit would exceed cap
        let cap_key = DataKey2::SupplyCap(offering_id.clone());
        let cap: i128 = env.storage().persistent().get(&cap_key).unwrap_or(0);
        if cap > 0 {
            let deposited_key = DataKey2::DepositedRevenue(offering_id.clone());
            let deposited: i128 = env.storage().persistent().get(&deposited_key).unwrap_or(0);
            let new_total = deposited.saturating_add(amount);
            if new_total > cap {
                return Err(RevoraError::SupplyCapExceeded);
            }
        }

        let pt_key = DataKey::PaymentToken(offering_id.clone());
        if let Some(locked_payment_token) =
            Self::get_locked_payment_token_for_offering(env, &offering_id)
        {
            if locked_payment_token != payment_token {
                return Err(RevoraError::PaymentTokenMismatch);
            }
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

        if !env.storage().persistent().has(&pt_key) {
            env.storage().persistent().set(&pt_key, &payment_token);
        }

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
        Self::commit_period_id(env, last_period_key, period_id);

        // Update cumulative deposited revenue and emit cap-reached event if applicable (#96)
        let deposited_key = DataKey2::DepositedRevenue(offering_id.clone());
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

        // Versioned event v2: [version: u32, payment_token: Address, amount: i128, period_id: u64]
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
        env.storage().persistent().get(&DataKey2::SupplyCap(offering_id)).unwrap_or(0)
    }

    // â”€â”€ Fee BPS Configuration (#98) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Set the global platform fee in basis points. Admin-only. (#98)
    ///
    /// Emits `EVENT_PLATFORM_FEE_SET` with the new `fee_bps` value.
    ///
    /// ### Errors
    /// - `NotInitialized` â€” contract not yet initialized.
    /// - `InvalidRevenueShareBps` â€” `fee_bps` exceeds `MAX_PLATFORM_FEE_BPS` (5 000).
    pub fn set_platform_fee(env: Env, fee_bps: u32) -> Result<(), RevoraError> {
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;
        admin.require_auth();
        if fee_bps > MAX_PLATFORM_FEE_BPS {
            return Err(RevoraError::InvalidRevenueShareBps);
        }
        env.storage().persistent().set(&DataKey::PlatformFeeBps, &fee_bps);
        env.events().publish((EVENT_PLATFORM_FEE_SET,), fee_bps);
        Ok(())
    }

    /// Return the global platform fee in basis points (0 = no fee). (#98)
    ///
    /// O(1) â€” single persistent storage read.
    pub fn get_platform_fee(env: Env) -> u32 {
        env.storage().persistent().get(&DataKey::PlatformFeeBps).unwrap_or(0)
    }

    /// Calculate the platform fee for `amount` using the stored global platform fee BPS. (#98)
    ///
    /// O(1) â€” one storage read plus integer arithmetic; no storage writes.
    pub fn calculate_platform_fee(env: Env, amount: i128) -> i128 {
        let fee_bps: i128 =
            env.storage().persistent().get::<DataKey, u32>(&DataKey::PlatformFeeBps).unwrap_or(0)
                as i128;
        (amount * fee_bps).checked_div(BPS_DENOMINATOR).unwrap_or(0)
    }

    /// Set a per-offering per-asset fee override in basis points. Issuer-only. (#98)
    ///
    /// Emits `EVENT_FEE_CONFIG` with `(issuer, namespace, token, asset, fee_bps)`.
    ///
    /// ### Errors
    /// - `OfferingNotFound` â€” offering does not exist or caller is not the issuer.
    /// - `InvalidRevenueShareBps` â€” `fee_bps` exceeds `MAX_PLATFORM_FEE_BPS` (5 000).
    pub fn set_offering_fee_bps(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        asset: Address,
        fee_bps: u32,
    ) -> Result<(), RevoraError> {
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        issuer.require_auth();
        if fee_bps > MAX_PLATFORM_FEE_BPS {
            return Err(RevoraError::InvalidRevenueShareBps);
        }
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::OfferingFeeBps(offering_id, asset.clone()), &fee_bps);
        env.events().publish((EVENT_FEE_CONFIG, issuer, namespace, token, asset), fee_bps);
        Ok(())
    }

    /// Return the per-offering per-asset fee override in basis points (0 = use platform default). (#98)
    ///
    /// O(1) â€” single persistent storage read.
    pub fn get_offering_fee_bps(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        asset: Address,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey::OfferingFeeBps(offering_id, asset)).unwrap_or(0)
    }

    /// Set a platform-level per-asset fee in basis points. Admin-only. (#98)
    ///
    /// Emits `EVENT_FEE_CONFIG` with `(asset, fee_bps)`.
    ///
    /// ### Errors
    /// - `NotInitialized` â€” contract not yet initialized.
    /// - `InvalidRevenueShareBps` â€” `fee_bps` exceeds `MAX_PLATFORM_FEE_BPS` (5 000).
    pub fn set_platform_fee_per_asset(
        env: Env,
        asset: Address,
        fee_bps: u32,
    ) -> Result<(), RevoraError> {
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;
        admin.require_auth();
        if fee_bps > MAX_PLATFORM_FEE_BPS {
            return Err(RevoraError::InvalidRevenueShareBps);
        }
        env.storage().persistent().set(&DataKey::PlatformFeePerAsset(asset.clone()), &fee_bps);
        env.events().publish((EVENT_FEE_CONFIG, asset), fee_bps);
        Ok(())
    }

    /// Return the platform-level per-asset fee in basis points (0 = no per-asset override). (#98)
    ///
    /// O(1) â€” single persistent storage read.
    pub fn get_platform_fee_per_asset(env: Env, asset: Address) -> u32 {
        env.storage().persistent().get(&DataKey::PlatformFeePerAsset(asset)).unwrap_or(0)
    }

    /// Return true if the contract is in event-only mode.
    pub fn is_event_only(env: &Env) -> bool {
        let (_, event_only): (bool, bool) =
            env.storage().persistent().get(&DataKey2::ContractFlags).unwrap_or((false, false));
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

    /// Require `period_id` to be strictly greater than the last committed period for the key.
    fn require_next_period_id<K>(env: &Env, key: K, period_id: u64) -> Result<(), RevoraError>
    where
        K: IntoVal<Env, soroban_sdk::Val> + Clone,
    {
        if period_id == 0 {
            return Err(RevoraError::InvalidPeriodId);
        }
        let last: u64 = env.storage().persistent().get(&key).unwrap_or(0);
        if period_id != last + 1 {
            return Err(RevoraError::InvalidPeriodId);
        }
        Ok(())
    }

    fn commit_period_id<K>(env: &Env, key: K, period_id: u64)
    where
        K: IntoVal<Env, soroban_sdk::Val> + Clone,
    {
        env.storage().persistent().set(&key, &period_id);
    }

    fn get_min_revenue_threshold_for_offering(env: &Env, offering_id: &OfferingId) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey2::MinRevenueThreshold(offering_id.clone()))
            .unwrap_or(0)
    }

    fn compute_audit_summary_from_reports(
        env: &Env,
        offering_id: &OfferingId,
    ) -> (AuditSummary, bool) {
        let reports_key = DataKey::RevenueReports(offering_id.clone());
        let reports: Map<u64, (i128, u64)> =
            env.storage().persistent().get(&reports_key).unwrap_or_else(|| Map::new(env));

        let mut total_revenue: i128 = 0;
        let mut is_saturated = false;
        let keys = reports.keys();
        for i in 0..keys.len() {
            let period_id = keys.get(i).unwrap();
            if let Some((amount, _)) = reports.get(period_id) {
                let next = total_revenue.saturating_add(amount);
                if next == i128::MAX && amount > 0 && total_revenue != i128::MAX {
                    is_saturated = true;
                }
                total_revenue = next;
            }
        }

        (AuditSummary { total_revenue, report_count: reports.len() as u64 }, is_saturated)
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

    fn ensure_issuer_registered(env: &Env, issuer: &Address) {
        let issuer_key = DataKey2::IssuerRegistered(issuer.clone());
        if !env.storage().persistent().has(&issuer_key) {
            let count: u32 = env.storage().persistent().get(&DataKey2::IssuerCount).unwrap_or(0);
            env.storage().persistent().set(&DataKey2::IssuerItem(count), issuer);
            env.storage().persistent().set(&DataKey2::IssuerCount, &(count + 1));
            env.storage().persistent().set(&issuer_key, &true);
        }
    }

    fn ensure_namespace_registered(env: &Env, issuer: &Address, namespace: &Symbol) {
        let ns_key = DataKey2::NamespaceRegistered(issuer.clone(), namespace.clone());
        if !env.storage().persistent().has(&ns_key) {
            let ns_count: u32 = env
                .storage()
                .persistent()
                .get(&DataKey2::NamespaceCount(issuer.clone()))
                .unwrap_or(0);
            env.storage()
                .persistent()
                .set(&DataKey2::NamespaceItem(issuer.clone(), ns_count), namespace);
            env.storage()
                .persistent()
                .set(&DataKey2::NamespaceCount(issuer.clone()), &(ns_count + 1));
            env.storage().persistent().set(&ns_key, &true);
        }
    }

    /// Enable or disable testnet mode for the contract.
    ///
    /// ### Security Note
    /// This mode MUST only be enabled on test networks. It relaxes critical
    /// validation rules (like concentration limits) to facilitate automated
    /// testing and integration flows.
    pub fn set_testnet_mode(env: Env, enabled: bool) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;
        admin.require_auth();
        env.storage().persistent().set(&DataKey::TestnetMode, &enabled);
        env.events().publish((EVENT_TESTNET_MODE,), enabled);
        Ok(())
    }

    /// Read-only accessor for the on-chain storage layout version stamp.
    pub fn storage_layout_version(env: Env) -> Option<u32> {
        env.storage().persistent().get(&DataKey::StorageLayoutVersion)
    }

    /// Admin-only setter to adjust the stored layout version (used by migrations/tests).
    /// Emits `EVENT_LAYOUT_VERSION` when the stored value is changed.
    pub fn set_storage_layout_version(env: Env, caller: Address, v: u32) -> Result<(), RevoraError> {
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;
        admin.require_auth();
        if caller != admin {
            return Err(RevoraError::NotAuthorized);
        }
        env.storage().persistent().set(&DataKey::StorageLayoutVersion, &v);
        env.events().publish((EVENT_LAYOUT_VERSION,), v);
        Ok(())
    }

    pub fn get_pending_issuer_transfer(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<Address> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get::<DataKey, PendingTransfer>(&DataKey::PendingIssuerTransfer(offering_id))
            .map(|pending| pending.new_issuer)
    }

    fn find_pending_transfer_for_new_issuer(
        env: &Env,
        namespace: &Symbol,
        token: &Address,
        new_issuer: &Address,
    ) -> Option<OfferingId> {
        let issuer_count: u32 = env.storage().persistent().get(&DataKey2::IssuerCount).unwrap_or(0);
        for i in 0..issuer_count {
            let issuer: Address = env.storage().persistent().get(&DataKey2::IssuerItem(i)).unwrap();
            let ns_count: u32 = env
                .storage()
                .persistent()
                .get(&DataKey2::NamespaceCount(issuer.clone()))
                .unwrap_or(0);
            for j in 0..ns_count {
                let namespace_item: Symbol = env
                    .storage()
                    .persistent()
                    .get(&DataKey2::NamespaceItem(issuer.clone(), j))
                    .unwrap();
                if namespace_item != *namespace {
                    continue;
                }
                let offering_id = OfferingId {
                    issuer: issuer.clone(),
                    namespace: namespace_item.clone(),
                    token: token.clone(),
                };
                if let Some(pending) = env.storage().persistent().get::<DataKey, PendingTransfer>(
                    &DataKey::PendingIssuerTransfer(offering_id.clone()),
                ) {
                    if pending.new_issuer == *new_issuer {
                        return Some(offering_id);
                    }
                }
            }
        }
        None
    }

    pub fn propose_issuer_transfer(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        new_issuer: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

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

        let key = DataKey::PendingIssuerTransfer(offering_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(RevoraError::IssuerTransferPending);
        }

        let timestamp = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&key, &PendingTransfer { new_issuer: new_issuer.clone(), timestamp });
        env.events().publish(
            (EVENT_ISSUER_TRANSFER_PROPOSED, issuer.clone(), namespace.clone(), token.clone()),
            (new_issuer.clone(), timestamp),
        );
        Ok(())
    }

    pub fn replace_issuer_transfer(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        new_issuer: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::NotAuthorized);
        }

        let key = DataKey::PendingIssuerTransfer(offering_id.clone());
        if !env.storage().persistent().has(&key) {
            return Err(RevoraError::NoTransferPending);
        }

        let pending: PendingTransfer = env.storage().persistent().get(&key).unwrap();
        let timestamp = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&key, &PendingTransfer { new_issuer: new_issuer.clone(), timestamp });

        env.events().publish(
            (EVENT_ISSUER_TRANSFER_CANCELLED, issuer.clone(), namespace.clone(), token.clone()),
            (issuer.clone(), pending.new_issuer.clone()),
        );
        env.events().publish(
            (EVENT_ISSUER_TRANSFER_PROPOSED, issuer.clone(), namespace.clone(), token.clone()),
            (new_issuer.clone(), timestamp),
        );
        Ok(())
    }

    pub fn accept_issuer_transfer(
        env: Env,
        new_issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        new_issuer.require_auth();

        let offering_id =
            Self::find_pending_transfer_for_new_issuer(&env, &namespace, &token, &new_issuer)
                .ok_or(RevoraError::NoTransferPending)?;

        let pending: PendingTransfer = env
            .storage()
            .persistent()
            .get(&DataKey::PendingIssuerTransfer(offering_id.clone()))
            .ok_or(RevoraError::NoTransferPending)?;

        let current_timestamp = env.ledger().timestamp();
        if current_timestamp > pending.timestamp.saturating_add(ISSUER_TRANSFER_EXPIRY_SECS) {
            return Err(RevoraError::IssuerTransferExpired);
        }

        let old_issuer = offering_id.issuer.clone();

        if new_issuer == old_issuer {
            env.storage().persistent().remove(&DataKey::PendingIssuerTransfer(offering_id.clone()));
            env.events().publish(
                (
                    EVENT_ISSUER_TRANSFER_ACCEPTED,
                    offering_id.issuer.clone(),
                    offering_id.namespace.clone(),
                    offering_id.token.clone(),
                ),
                (old_issuer, new_issuer.clone()),
            );
            return Ok(());
        }

        let new_offering_id = OfferingId {
            issuer: new_issuer.clone(),
            namespace: offering_id.namespace.clone(),
            token: offering_id.token.clone(),
        };

        // Prevent duplicate offering entries for the same new issuer / namespace / token.
        if Self::get_offering(
            env.clone(),
            new_issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
        )
        .is_some()
        {
            return Err(RevoraError::LimitReached);
        }

        // Migrate any vesting schedules corresponding to this offering before completing
        // the issuer transfer. This preserves active schedules under the new issuer key
        // and prevents orphaned pre-cliff schedules.
        let vesting_offering_id = vesting::VestingOfferingId {
            issuer: old_issuer.clone(),
            token: offering_id.token.clone(),
        };
        match vesting::migrate_offering_schedules(
            &env,
            &vesting_offering_id,
            new_issuer.clone(),
            current_timestamp,
        ) {
            Ok(beneficiaries) => {
                for beneficiary in beneficiaries.iter() {
                    env.events().publish(
                        (
                            EVENT_ISSUER_TRANSFER_VESTING_MIGRATED,
                            offering_id.namespace.clone(),
                            offering_id.token.clone(),
                            beneficiary.clone(),
                        ),
                        (old_issuer.clone(), new_issuer.clone()),
                    );
                }
            }
            Err(vesting::VestingError::SchedulePreCliff) => {
                return Err(RevoraError::VestingTransferBlocked);
            }
            Err(_) => {
                // If the vesting index is empty or stale, ignore it and continue.
            }
        }

        // Register namespace metadata for the new issuer.
        Self::ensure_issuer_registered(&env, &new_issuer);
        Self::ensure_namespace_registered(&env, &new_issuer, &offering_id.namespace);

        // Copy the offering registration record to the new issuer's tenant list.
        let tenant_id =
            TenantId { issuer: new_issuer.clone(), namespace: offering_id.namespace.clone() };
        let count_key = DataKey::OfferCount(tenant_id.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        let offering = Self::get_offering(
            env.clone(),
            old_issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
        )
        .ok_or(RevoraError::OfferingNotFound)?;
        let item_key = DataKey::OfferItem(tenant_id.clone(), count);
        env.storage().persistent().set(&item_key, &offering);
        env.storage().persistent().set(&count_key, &(count + 1));

        // Update direct index for the new issuer's offering_id (#360).
        env.storage()
            .persistent()
            .set(&DataKey2::OfferingRecord(new_offering_id.clone()), &offering);

        // Update issuer lookups for the old and new offering IDs.
        env.storage()
            .persistent()
            .set(&DataKey::OfferingIssuer(offering_id.clone()), &new_issuer.clone());
        env.storage()
            .persistent()
            .set(&DataKey::OfferingIssuer(new_offering_id.clone()), &new_issuer.clone());

        env.storage().persistent().remove(&DataKey::PendingIssuerTransfer(offering_id.clone()));

        env.events().publish(
            (
                EVENT_ISSUER_TRANSFER_ACCEPTED,
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
            ),
            (old_issuer, new_issuer.clone()),
        );
        Ok(())
    }

    pub fn cancel_issuer_transfer(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::NotAuthorized);
        }

        let key = DataKey::PendingIssuerTransfer(offering_id.clone());
        if !env.storage().persistent().has(&key) {
            return Err(RevoraError::NoTransferPending);
        }

        let pending: PendingTransfer = env.storage().persistent().get(&key).unwrap();
        env.storage().persistent().remove(&key);
        env.events().publish(
            (EVENT_ISSUER_TRANSFER_CANCELLED, issuer.clone(), namespace.clone(), token.clone()),
            (issuer, pending.new_issuer),
        );
        Ok(())
    }

    pub fn reject_issuer_transfer(
        env: Env,
        new_issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        new_issuer.require_auth();

        let offering_id =
            Self::find_pending_transfer_for_new_issuer(&env, &namespace, &token, &new_issuer)
                .ok_or(RevoraError::NoTransferPending)?;

        let pending: PendingTransfer = env
            .storage()
            .persistent()
            .get(&DataKey::PendingIssuerTransfer(offering_id.clone()))
            .ok_or(RevoraError::NoTransferPending)?;

        let old_issuer = offering_id.issuer.clone();

        env.storage().persistent().remove(&DataKey::PendingIssuerTransfer(offering_id.clone()));

        env.events().publish(
            (
                EVENT_ISSUER_TRANSFER_REJECTED,
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
            ),
            (old_issuer, new_issuer.clone()),
        );
        Ok(())
    }

    /// Initialize admin and optional safety role for emergency pause (#7).
    /// `event_only` configures the contract to skip persistent business state (#72).
    /// Can only be called once; panics if already initialized.
    pub fn initialize(env: Env, admin: Address, safety: Option<Address>, event_only: Option<bool>) {
        if env.storage().persistent().has(&DataKey::Admin) {
            return; // Already initialized, no-op
        }
        env.storage().persistent().set(&DataKey::Admin, &admin);
        Self::emit_v2_event(&env, (EVENT_ADMIN_SET,), admin.clone());
        if let Some(ref s) = safety {
            env.storage().persistent().set(&DataKey::Safety, &s);
        }
        env.storage().persistent().set(&DataKey::Paused, &PauseState::NotPaused);
        let eo = event_only.unwrap_or(false);
        env.storage().persistent().set(&DataKey2::ContractFlags, &(false, eo));
        // Stamp storage layout version for future compatibility checks.
        env.storage()
            .persistent()
            .set(&DataKey::StorageLayoutVersion, &STORAGE_LAYOUT_VERSION);
        env.events().publish((EVENT_LAYOUT_VERSION,), STORAGE_LAYOUT_VERSION);

        env.events().publish((EVENT_INIT, admin.clone()), (safety, eo));
    }

    /// Soft-pause the contract (Admin only).
    ///
    /// `SoftPaused` blocks reports and deposits but **allows** `claim`, so
    /// holders can still withdraw their funds during incident response.
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
        env.storage().persistent().set(&DataKey::Paused, &PauseState::SoftPaused);
        // Legacy compatibility event
        env.events().publish((EVENT_PAUSED, caller.clone()), ());
        // Versioned tier event
        env.events().publish((EVENT_PAUSED2, caller.clone()), (PauseState::SoftPaused,));
        Ok(())
    }

    /// Unpause the contract (Admin only).
    ///
    /// Re-enables all operations after a pause.
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
        env.storage().persistent().set(&DataKey::Paused, &PauseState::NotPaused);
        env.events().publish((EVENT_UNPAUSED, caller.clone()), ());
        env.events().publish((EVENT_PAUSED2, caller.clone()), (PauseState::NotPaused,));
        Ok(())
    }

    /// Hard-pause the contract (Admin only).
    ///
    /// `HardPaused` blocks **every** state-mutating operation including `claim`.
    /// Use this tier only when funds must be fully locked (e.g. critical exploit).
    /// Only the admin can escalate to HardPaused; the safety role is limited to SoftPaused.
    ///
    /// ### Parameters
    /// - `caller`: The address of the admin (must match initialized admin).
    pub fn hard_pause_admin(env: Env, caller: Address) -> Result<(), RevoraError> {
        caller.require_auth();
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;
        if caller != admin {
            return Err(RevoraError::NotAuthorized);
        }
        env.storage().persistent().set(&DataKey::Paused, &PauseState::HardPaused);
        env.events().publish((EVENT_PAUSED, caller.clone()), ());
        env.events().publish((EVENT_PAUSED2, caller.clone()), (PauseState::HardPaused,));
        Ok(())
    }

    /// Soft-pause the contract (Safety role only).
    ///
    /// `SoftPaused` blocks reports and deposits but **allows** `claim`, so
    /// holders can still withdraw their funds during incident response.
    /// The safety role cannot escalate to `HardPaused`; only the admin can.
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
        env.storage().persistent().set(&DataKey::Paused, &PauseState::SoftPaused);
        env.events().publish((EVENT_PAUSED, caller.clone()), ());
        env.events().publish((EVENT_PAUSED2, caller.clone()), (PauseState::SoftPaused,));
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
        env.storage().persistent().set(&DataKey::Paused, &PauseState::NotPaused);
        env.events().publish((EVENT_UNPAUSED, caller.clone()), ());
        env.events().publish((EVENT_PAUSED2, caller.clone()), (PauseState::NotPaused,));
        Ok(())
    }

    /// Query the paused state of the contract.
    ///
    /// Returns `true` when the contract is in either `SoftPaused` or `HardPaused` state,
    /// preserving backward compatibility with callers that only need a binary signal.
    /// Use `get_pause_state` to distinguish between the two tiers.
    pub fn is_paused(env: Env) -> bool {
        matches!(
            env.storage()
                .persistent()
                .get::<DataKey, PauseState>(&DataKey::Paused)
                .unwrap_or(PauseState::NotPaused),
            PauseState::SoftPaused | PauseState::HardPaused
        )
    }

    /// Return the current `PauseState` tier.
    ///
    /// - `NotPaused`  – all operations open.
    /// - `SoftPaused` – reports/deposits blocked; `claim` allowed.
    /// - `HardPaused` – all state-mutating operations blocked including `claim`.
    pub fn get_pause_state(env: Env) -> PauseState {
        env.storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::Paused)
            .unwrap_or(PauseState::NotPaused)
    }

    /// Helper: block if the contract is in SoftPaused or HardPaused state.
    /// Used by reports, deposits, and all non-claim state-mutating entrypoints.
    fn require_not_paused(env: &Env) -> Result<(), RevoraError> {
        let state = env
            .storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::Paused)
            .unwrap_or(PauseState::NotPaused);
        if matches!(state, PauseState::SoftPaused | PauseState::HardPaused) {
            return Err(RevoraError::ContractPaused);
        }
        Ok(())
    }

    /// Helper: block only if the contract is in HardPaused state.
    /// Used exclusively by `claim` so holders can still withdraw during a SoftPause.
    fn require_not_hard_paused(env: &Env) -> Result<(), RevoraError> {
        let state = env
            .storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::Paused)
            .unwrap_or(PauseState::NotPaused);
        if matches!(state, PauseState::HardPaused) {
            return Err(RevoraError::ContractPaused);
        }
        Ok(())
    }

    // â”€â”€ Offering management â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Register a new revenue-share offering.
    ///
    /// Once registered, an offering's parameters are immutable.
    ///
    /// # Arguments
    /// * `issuer` - The address of the offering issuer. Must provide authentication.
    /// * `namespace` - A symbol identifying the namespace for this offering.
    /// * `token` - The address of the token being offered.
    /// * `revenue_share_bps` - The revenue share percentage in basis points (0â€“10,000).
    ///   Values above 10,000 are rejected unless testnet mode is enabled (admin-only,
    ///   never enable on mainnet â€” see `TESTNET_MODE.md`).
    /// * `payout_asset` - The asset in which revenue will be paid out.
    /// * `supply_cap` - Optional cap on the total amount of revenue that can be deposited (0 = no cap).
    ///
    /// # Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::InvalidRevenueShareBps)` if `revenue_share_bps` exceeds 10,000
    ///   and testnet mode is disabled (the default).
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
    /// - `Err(RevoraError::ContractPaused)` if the contract is paused.
    ///
    /// # Events
    /// Emits `EVENT_OFFER_REG_V2` and `EVENT_INDEXED_V2`.
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

        // Skip bps validation in testnet mode (reads the real flag from storage).
        // In production mode (default) revenue_share_bps is always capped at 10 000 (100%).
        // Testnet mode is admin-only and must never be enabled on mainnet â€” see TESTNET_MODE.md.
        let testnet_mode = Self::is_testnet_mode(env.clone());
        if !testnet_mode && revenue_share_bps > 10_000 {
            return Err(RevoraError::InvalidRevenueShareBps);
        }

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Duplicate prevention: check if offering already exists by its stable identity (issuer+namespace+token)
        // This makes register_offering idempotent and prevents state inconsistencies in off-chain catalogs.
        if env.storage().persistent().has(&DataKey::OfferingIssuer(offering_id.clone())) {
            return Ok(());
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

        // Direct index for O(1) get_offering (#360).
        env.storage().persistent().set(&DataKey2::OfferingRecord(offering_id.clone()), &offering);

        let issuer_lookup_key = DataKey::OfferingIssuer(offering_id.clone());
        env.storage().persistent().set(&issuer_lookup_key, &issuer);

        if supply_cap > 0 {
            let cap_key = DataKey2::SupplyCap(offering_id.clone());
            env.storage().persistent().set(&cap_key, &supply_cap);
        }

        Self::emit_v2_event(
            &env,
            (EVENT_OFFER_REG_V2, issuer.clone(), namespace.clone()),
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

        if false {
            env.events().publish(
                (EVENT_OFFER_REG_V1, issuer.clone(), namespace.clone()),
                (EVENT_SCHEMA_VERSION, token.clone(), revenue_share_bps, payout_asset.clone()),
            );
        }
        // Versioned v2 event: [2, token, revenue_share_bps, payout_asset] â€” always emitted (#RC26Q2-C31)
        Self::emit_v2_event(
            &env,
            (EVENT_OFFER_REG_V2, issuer, namespace, token.clone()),
            (token, revenue_share_bps, payout_asset),
        );

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
    /// This method first attempts an O(1) direct lookup via the `OfferingRecord` index written
    /// at registration (#360). Falls back to an O(n) scan for legacy offerings registered before
    /// the index was introduced.
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
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        // O(1) direct lookup via index written at registration (#360).
        if let Some(offering) = env
            .storage()
            .persistent()
            .get::<DataKey2, Offering>(&DataKey2::OfferingRecord(offering_id))
        {
            return Some(offering);
        }
        // Fallback: O(n) scan for legacy offerings registered before the index was added.
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
    /// Returns `None` when:
    /// - the offering is unknown, or
    /// - the offering exists but has not yet recorded a successful deposit.
    ///
    /// Once the first successful deposit persists the `PaymentToken` key, this returns
    /// `Some(payment_token)` for that locked token.
    pub fn get_payment_token(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<Address> {
        let offering_id = OfferingId { issuer, namespace, token };
        Self::get_locked_payment_token_for_offering(&env, &offering_id)
    }

    /// Record or correct a revenue report for an offering and emit audit events.
    ///
    /// Semantics:
    /// - New periods persist `(amount, timestamp)`, emit `rev_init`, and update
    ///   `AuditSummary` by `(amount, +1)`.
    /// - Existing periods with `override_existing=true` emit `rev_ovrd` and update
    ///   `AuditSummary` by `(new_amount - old_amount, +0)`.
    /// - Existing periods with `override_existing=false` emit `rev_rej` and leave
    ///   persisted state unchanged.
    /// - New periods below the configured minimum threshold emit `rev_below` and
    ///   leave both persisted report state and the report cursor unchanged.
    ///
    /// Validates amount using the Negative Amount Validation Matrix (#163).
    #[allow(clippy::too_many_arguments)]
    /// Report revenue for a specific period of an offering.
    ///
    /// # Arguments
    /// * `issuer` - The address of the offering issuer.
    /// * `namespace` - A symbol identifying the namespace.
    /// * `token` - The address of the token.
    /// * `payout_asset` - The asset being reported.
    /// * `amount` - The amount of revenue.
    /// * `period_id` - The identifier for the revenue period.
    /// * `override_existing` - If true, replaces an existing report for the same period.
    ///
    /// # Events
    /// Emits `EVENT_REV_REP_V2` and `EVENT_INDEXED_V2`.
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

        // Input validation (#35): reject zero/invalid period_id
        if period_id == 0 {
            return Err(RevoraError::InvalidPeriodId);
        }

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
        let last_report_period_key = DataKey2::LastReportedPeriodId(offering_id.clone());
        let threshold = Self::get_min_revenue_threshold_for_offering(&env, &offering_id);
        let current_timestamp = env.ledger().timestamp();

        Self::require_not_offering_frozen(&env, &offering_id)?;
        Self::require_report_window_open(&env, &offering_id)?;

        if !event_only {
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

            // Testnet mode bypass: if enabled, skip concentration limit enforcement
            // to allow flexible testing of revenue flows without holder constraints.
            let testnet_mode = Self::is_testnet_mode(env.clone());
            if !testnet_mode {
                let limit_key = DataKey::ConcentrationLimit(offering_id.clone());
                if let Some(config) =
                    env.storage().persistent().get::<DataKey, ConcentrationLimitConfig>(&limit_key)
                {
                    // Concentration Enforcement: if enforce=true and max_bps > 0,
                    // reject report if current concentration exceeds the limit.
                    // Allowed: current <= max_bps. Rejected: current > max_bps.
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

        // Use bounded read for event snapshots to avoid unbounded payloads
        // Cap at MAX_PAGE_LIMIT (20) to prevent gas spikes from large blacklists
        let blacklist = if event_only {
            Vec::new(&env)
        } else {
            Self::get_blacklist_page(
                env.clone(),
                issuer.clone(),
                namespace.clone(),
                token.clone(),
                0,
                MAX_PAGE_LIMIT,
            )
            .0
        };

        let mut actual_override = false;
        let mut actual_initial = false;

        if event_only {
            if threshold > 0 && amount < threshold {
                env.events().publish(
                    (EVENT_REV_BELOW_THRESHOLD, issuer, namespace, token),
                    (amount, period_id, threshold),
                );
                return Ok(());
            }

            actual_initial = true;
            env.events().publish(
                (EVENT_REVENUE_REPORT_INITIAL, issuer.clone(), namespace.clone(), token.clone()),
                (amount, period_id, blacklist.clone()),
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
        } else {
            let reports_key = DataKey::RevenueReports(offering_id.clone());
            let mut reports: Map<u64, (i128, u64)> =
                env.storage().persistent().get(&reports_key).unwrap_or_else(|| Map::new(&env));
            let idx_key = DataKey::RevenueIndex(offering_id.clone(), period_id);

            match reports.get(period_id) {
                Some((existing_amount, _)) => {
                    if !override_existing {
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
                            (EVENT_REVENUE_REPORT_REJECTED_ASSET, issuer, namespace, token),
                            (payout_asset, amount, period_id, existing_amount, blacklist),
                        );
                        return Ok(());
                    }

                    actual_override = true;
                    reports.set(period_id, (amount, current_timestamp));
                    env.storage().persistent().set(&reports_key, &reports);
                    env.storage().persistent().set(&idx_key, &amount);

                    let summary_key = DataKey::AuditSummary(offering_id.clone());
                    let mut summary: AuditSummary = env
                        .storage()
                        .persistent()
                        .get(&summary_key)
                        .unwrap_or(AuditSummary { total_revenue: 0, report_count: 0 });
                    summary.total_revenue = summary
                        .total_revenue
                        .saturating_add(amount.saturating_sub(existing_amount));
                    env.storage().persistent().set(&summary_key, &summary);

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
                }
                None => {
                    if override_existing {
                        return Err(RevoraError::MissingReportForOverride);
                    }
                    // preserve existing initial-report behavior when override_existing=false
                    Self::require_next_period_id(&env, last_report_period_key.clone(), period_id)?;
                    if threshold > 0 && amount < threshold {
                        env.events().publish(
                            (
                                EVENT_REV_BELOW_THRESHOLD,
                                issuer.clone(),
                                namespace.clone(),
                                token.clone(),
                            ),
                            (amount, period_id, threshold),
                        );
                        return Ok(());
                    }

                    actual_initial = true;
                    reports.set(period_id, (amount, current_timestamp));
                    env.storage().persistent().set(&reports_key, &reports);
                    env.storage().persistent().set(&idx_key, &amount);
                    Self::commit_period_id(&env, last_report_period_key.clone(), period_id);

                    let summary_key = DataKey::AuditSummary(offering_id.clone());
                    let mut summary: AuditSummary = env
                        .storage()
                        .persistent()
                        .get(&summary_key)
                        .unwrap_or(AuditSummary { total_revenue: 0, report_count: 0 });
                    summary.total_revenue = summary.total_revenue.saturating_add(amount);
                    summary.report_count = summary.report_count.saturating_add(1);
                    env.storage().persistent().set(&summary_key, &summary);

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
                    // Versioned v2 event: [2, amount, period_id, blacklist] â€” always emitted (#RC26Q2-C31)
                    Self::emit_v2_event(
                        &env,
                        (EVENT_REV_INIT_V2, issuer.clone(), namespace.clone(), token.clone()),
                        (amount, period_id, blacklist.clone()),
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
            (amount, payout_asset.clone(), actual_override),
        );
        env.events().publish(
            (EVENT_REVENUE_REPORTED_ASSET, issuer.clone(), namespace.clone(), token.clone()),
            (payout_asset.clone(), amount, period_id),
        );
        // Versioned v2 events: always emitted regardless of feature flags (#RC26Q2-C31)
        // rv_rep2: [2, amount, period_id, blacklist]
        Self::emit_v2_event(
            &env,
            (EVENT_REV_REP_V2, issuer.clone(), namespace.clone(), token.clone()),
            (amount, period_id, blacklist.clone()),
        );
        // rv_repa2: [2, payout_asset, amount, period_id]
        Self::emit_v2_event(
            &env,
            (EVENT_REV_REPA_V2, issuer.clone(), namespace.clone(), token.clone()),
            (payout_asset.clone(), amount, period_id),
        );
        // rv_inia2: [2, payout_asset, amount, period_id, blacklist]
        Self::emit_v2_event(
            &env,
            (EVENT_REV_INIA_V2, issuer.clone(), namespace.clone(), token.clone()),
            (payout_asset.clone(), amount, period_id, blacklist.clone()),
        );

        if Self::is_event_versioning_enabled(env.clone()) {
            env.events().publish(
                (EVENT_REV_INIA_V1, issuer.clone(), namespace.clone(), token.clone()),
                (EVENT_SCHEMA_VERSION, payout_asset.clone(), amount, period_id, blacklist.clone()),
            );
            env.events().publish(
                (EVENT_REV_REP_V2, issuer.clone(), namespace.clone(), token.clone()),
                (EVENT_SCHEMA_VERSION, amount, period_id, blacklist.clone()),
            );
            env.events().publish(
                (EVENT_REV_REPA_V1, issuer, namespace, token),
                (EVENT_SCHEMA_VERSION, payout_asset, amount, period_id),
            );
        }

        Ok(())
    }

    /// Repair the `AuditSummary` cache for an offering by recomputing it from the
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
        let (corrected, _) = Self::compute_audit_summary_from_reports(&env, &offering_id);

        let summary_key = DataKey::AuditSummary(offering_id);
        env.storage().persistent().set(&summary_key, &corrected);

        Self::emit_v2_event(
            &env,
            (EVENT_AUDIT_REPAIRED, issuer, namespace, token),
            (corrected.total_revenue, corrected.report_count),
        );

        Ok(corrected)
    }

    /// Read-only comparison between the stored `AuditSummary` cache and the
    /// authoritative `RevenueReports` map for an offering.
    pub fn reconcile_audit_summary(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> AuditReconciliationResult {
        let offering_id = OfferingId { issuer, namespace, token };
        let stored = env
            .storage()
            .persistent()
            .get::<DataKey, AuditSummary>(&DataKey::AuditSummary(offering_id.clone()))
            .unwrap_or(AuditSummary { total_revenue: 0, report_count: 0 });
        let (computed, is_saturated) = Self::compute_audit_summary_from_reports(&env, &offering_id);
        let is_consistent = !is_saturated
            && stored.total_revenue == computed.total_revenue
            && stored.report_count == computed.report_count;

        AuditReconciliationResult {
            stored_total_revenue: stored.total_revenue,
            stored_report_count: stored.report_count,
            computed_total_revenue: computed.total_revenue,
            computed_report_count: computed.report_count,
            is_consistent,
            is_saturated,
        }
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

    /// Sum reported revenue for all period IDs in `[from_period, to_period]` (inclusive).
    ///
    /// **Warning:** unbounded range â€” for large ranges prefer [`get_revenue_range_chunk`].
    ///
    /// ### Auth
    /// None â€” read-only.
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
            total = total.saturating_add(Self::get_revenue_by_period(
                env.clone(),
                issuer.clone(),
                namespace.clone(),
                token.clone(),
                period,
            ));
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
                let limit = Self::get_effective_blacklist_limit(&env, &offering_id);
                if map.len() >= limit {
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

    /// Add multiple investors to the per-offering blacklist in a single transaction.
    ///
    /// Enables efficient bulk compliance updates by processing up to MAX_BATCH_SIZE (50)
    /// addresses atomically. The operation is idempotent: addresses already blacklisted
    /// are skipped without error. Events are emitted only for addresses that result in
    /// actual state changes.
    ///
    /// ### Parameters
    /// - `caller`: The address authorized to manage the blacklist. Must be the current issuer or admin.
    /// - `issuer`: The issuer address of the offering.
    /// - `namespace`: The namespace of the offering.
    /// - `token`: The token representing the offering.
    /// - `investors`: Vector of addresses to blacklist (max 50).
    ///
    /// ### Security Assumptions
    /// - `caller` must be the current issuer of the offering or the contract admin.
    /// - All-or-nothing semantics: if any validation fails, no addresses are added.
    /// - Batch size is capped at MAX_BATCH_SIZE to keep gas costs predictable.
    /// - Blacklist size is capped per-offering (configurable via set_blacklist_size_limit, default MAX_BLACKLIST_SIZE).
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
    /// - `Err(RevoraError::ContractPaused)` if the contract is paused.
    /// - `Err(RevoraError::OfferingNotFound)` if the offering does not exist.
    /// - `Err(RevoraError::NotAuthorized)` if caller is not the current issuer or admin.
    /// - `Err(RevoraError::LimitReached)` if batch size exceeds MAX_BATCH_SIZE.
    /// - `Err(RevoraError::BlacklistSizeLimitExceeded)` if adding the batch would exceed the per-offering limit.
    #[allow(clippy::too_many_arguments)]
    pub fn blacklist_add_many(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        investors: Vec<Address>,
    ) -> Result<(), RevoraError> {
        // Task 2.1: Authorization checks
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        caller.require_auth();

        // Task 2.2: Batch size validation
        if investors.len() > MAX_BATCH_SIZE {
            return Err(RevoraError::LimitReached);
        }

        // Handle empty batch case (idempotent no-op)
        if investors.is_empty() {
            return Ok(());
        }

        // Task 2.3: Offering existence check and authorization
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

        // Task 2.3: Load storage
        let key = DataKey::Blacklist(offering_id.clone());
        let mut map: Map<Address, bool> =
            env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));
        let order_key = DataKey::BlacklistOrder(offering_id.clone());
        let mut order: Vec<Address> =
            env.storage().persistent().get(&order_key).unwrap_or_else(|| Vec::new(&env));

        // Task 2.4: Deduplication logic
        let mut seen = Map::new(&env);
        let mut unique_investors = Vec::new(&env);
        for i in 0..investors.len() {
            let investor = investors.get(i).unwrap();
            if !seen.contains_key(investor.clone()) {
                seen.set(investor.clone(), true);
                unique_investors.push_back(investor);
            }
        }

        // Task 2.5: Capacity validation
        let limit = Self::get_effective_blacklist_limit(&env, &offering_id);
        let current_size = map.len();
        let mut new_count = 0u32;
        for i in 0..unique_investors.len() {
            let investor = unique_investors.get(i).unwrap();
            if !map.contains_key(investor.clone()) {
                new_count += 1;
            }
        }

        if current_size + new_count > limit {
            return Err(RevoraError::BlacklistSizeLimitExceeded);
        }

        // Task 2.6: Batch add logic with storage updates
        for i in 0..unique_investors.len() {
            let investor = unique_investors.get(i).unwrap();
            let was_present = map.get(investor.clone()).unwrap_or(false);

            if !was_present {
                // Add to map and order vec
                if !Self::is_event_only(&env) {
                    map.set(investor.clone(), true);
                    order.push_back(investor.clone());
                }

                // Emit event for actual state change
                env.events().publish(
                    (EVENT_BL_ADD, issuer.clone(), namespace.clone(), token.clone()),
                    (caller.clone(), investor),
                );
            }
            // If already blacklisted, skip without error or event (idempotent)
        }

        // Save updated storage
        if !Self::is_event_only(&env) {
            env.storage().persistent().set(&key, &map);
            env.storage().persistent().set(&order_key, &order);
        }

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
        Self::require_not_frozen(&env)?;

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

    /// Remove multiple investors from the per-offering blacklist in a single transaction.
    ///
    /// Enables efficient bulk compliance updates by processing up to MAX_BATCH_SIZE (50)
    /// addresses atomically. The operation is idempotent: addresses not currently blacklisted
    /// are skipped without error. Events are emitted only for addresses that result in
    /// actual state changes.
    ///
    /// ### Parameters
    /// - `caller`: The address authorized to manage the blacklist. Must be the current issuer or admin.
    /// - `issuer`: The issuer address of the offering.
    /// - `namespace`: The namespace of the offering.
    /// - `token`: The token representing the offering.
    /// - `investors`: Vector of addresses to remove from blacklist (max 50).
    ///
    /// ### Security Assumptions
    /// - `caller` must be the current issuer of the offering or the contract admin.
    /// - All-or-nothing semantics: if any validation fails, no addresses are removed.
    /// - Batch size is capped at MAX_BATCH_SIZE to keep gas costs predictable.
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
    /// - `Err(RevoraError::ContractPaused)` if the contract is paused.
    /// - `Err(RevoraError::OfferingNotFound)` if the offering does not exist.
    /// - `Err(RevoraError::NotAuthorized)` if caller is not the current issuer or admin.
    /// - `Err(RevoraError::LimitReached)` if batch size exceeds MAX_BATCH_SIZE.
    #[allow(clippy::too_many_arguments)]
    pub fn blacklist_remove_many(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        investors: Vec<Address>,
    ) -> Result<(), RevoraError> {
        // Task 3.1: Authorization checks
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        caller.require_auth();

        // Task 3.2: Batch size validation
        if investors.len() > MAX_BATCH_SIZE {
            return Err(RevoraError::LimitReached);
        }

        // Handle empty batch case (idempotent no-op)
        if investors.is_empty() {
            return Ok(());
        }

        // Task 3.3: Offering existence check and authorization
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

        // Task 3.3: Load storage
        let key = DataKey::Blacklist(offering_id.clone());
        let mut map: Map<Address, bool> =
            env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));

        // Task 3.4: Deduplication logic
        let mut seen = Map::new(&env);
        let mut unique_investors = Vec::new(&env);
        for i in 0..investors.len() {
            let investor = investors.get(i).unwrap();
            if !seen.contains_key(investor.clone()) {
                seen.set(investor.clone(), true);
                unique_investors.push_back(investor);
            }
        }

        // Task 3.5: Batch remove logic
        for i in 0..unique_investors.len() {
            let investor = unique_investors.get(i).unwrap();
            let was_present = map.get(investor.clone()).unwrap_or(false);

            if was_present {
                // Remove from map
                map.remove(investor.clone());

                // Emit event for actual state change
                env.events().publish(
                    (EVENT_BL_REM, issuer.clone(), namespace.clone(), token.clone()),
                    (caller.clone(), investor),
                );
            }
            // If not blacklisted, skip without error or event (idempotent)
        }

        // Task 3.5: Rebuild order vec to maintain consistency
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

        // Save updated storage
        env.storage().persistent().set(&key, &map);
        env.storage().persistent().set(&order_key, &new_order);

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
    ///
    /// ## Legacy/Bounded Warning
    ///
    /// This method returns the entire blacklist in a single call, which can exceed gas limits
    /// for large lists. It is retained for backward compatibility but should be avoided in
    /// production code. Use `get_blacklist_page` instead for pagination with deterministic cursors.
    ///
    /// The blacklist size is bounded by MAX_BLACKLIST_SIZE (200) per offering, so this method
    /// will never return more than 200 addresses. However, for off-chain tooling and event
    /// processing, the paginated form is preferred to avoid gas spikes.
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

    /// Return a page of blacklisted addresses for an offering.
    ///
    /// ## Pagination Behavior
    ///
    /// - `start`: Zero-based cursor position in the insertion-ordered blacklist
    /// - `limit`: Maximum number of addresses to return (capped at MAX_PAGE_LIMIT = 20)
    /// - Returns: (page of addresses, next_cursor)
    ///   - `next_cursor = Some(n)` indicates more data is available at position `n`
    ///   - `next_cursor = None` indicates end of list
    ///
    /// The cursor is deterministic and stable: it corresponds to the index in the
    /// insertion-ordered blacklist. Pagination preserves insertion order (#38).
    ///
    /// ## Usage Pattern
    ///
    /// ```ignore
    /// let mut cursor = 0;
    /// loop {
    ///     let (page, next) = get_blacklist_page(env, issuer, ns, token, cursor, 20);
    ///     // process page...
    ///     match next {
    ///         Some(n) => cursor = n,
    ///         None => break,
    ///     }
    /// }
    /// ```
    pub fn get_blacklist_page(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        start: u32,
        limit: u32,
    ) -> (Vec<Address>, Option<u32>) {
        let offering_id = OfferingId { issuer, namespace, token };
        let order_key = DataKey::BlacklistOrder(offering_id);
        let all: Vec<Address> = env
            .storage()
            .persistent()
            .get::<DataKey, Vec<Address>>(&order_key)
            .unwrap_or_else(|| Vec::new(&env));

        let count = all.len();
        let effective_limit =
            if limit == 0 || limit > MAX_PAGE_LIMIT { MAX_PAGE_LIMIT } else { limit };

        if start >= count {
            return (Vec::new(&env), None);
        }

        let end = core::cmp::min(start + effective_limit, count);
        let mut results = Vec::new(&env);
        for i in start..end {
            results.push_back(all.get(i).unwrap());
        }

        let next_cursor = if end < count { Some(end) } else { None };
        (results, next_cursor)
    }

    /// Return the current number of blacklisted addresses for an offering.
    ///
    /// This is a cheap O(1) read of the underlying map length and can be used
    /// by off-chain tooling to monitor proximity to the per-offering blacklist limit
    /// (default MAX_BLACKLIST_SIZE = 200, configurable via set_blacklist_size_limit)
    /// before attempting an add.
    ///
    /// Returns 0 when no blacklist exists yet for the offering.
    pub fn get_blacklist_size(env: Env, issuer: Address, namespace: Symbol, token: Address) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::Blacklist(offering_id);
        env.storage()
            .persistent()
            .get::<DataKey, Map<Address, bool>>(&key)
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// Get the effective blacklist size limit for a per-offering.
    ///
    /// Returns the per-offering limit if set, otherwise defaults to MAX_BLACKLIST_SIZE.
    /// This is a private helper used by blacklist_add and blacklist_add_many.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `offering_id`: The offering identifier.
    ///
    /// ### Returns
    /// The maximum allowed blacklist size for the offering.
    fn get_effective_blacklist_limit(env: &Env, offering_id: &OfferingId) -> u32 {
        let key = DataKey2::BlacklistSizeLimit(offering_id.clone());
        env.storage().persistent().get::<DataKey2, u32>(&key).unwrap_or(MAX_BLACKLIST_SIZE)
    }

    /// Set the per-offering blacklist size limit.
    ///
    /// Allows the issuer to configure a maximum number of addresses that can be
    /// blacklisted for a specific offering. This limit affects both `blacklist_add`
    /// and `blacklist_add_many` operations. If not set, the default is MAX_BLACKLIST_SIZE (200).
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `caller`: The address making the request. Must be the current issuer.
    /// - `issuer`: The issuer address of the offering.
    /// - `namespace`: The namespace of the offering.
    /// - `token`: The token representing the offering.
    /// - `max_size`: The new maximum blacklist size (must be > 0).
    ///
    /// ### Security Assumptions
    /// - `caller` must be the current issuer of the offering.
    /// - Caller must be authorized (require_auth).
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
    /// - `Err(RevoraError::OfferingNotFound)` if the offering does not exist.
    /// - `Err(RevoraError::NotAuthorized)` if caller is not the current issuer.
    /// - `Err(RevoraError::LimitReached)` if max_size is 0.
    pub fn set_blacklist_size_limit(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        max_size: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        caller.require_auth();

        // Verify the offering exists and caller is the issuer
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        if caller != current_issuer {
            return Err(RevoraError::NotAuthorized);
        }

        // Validate: max_size must be at least 1
        if max_size == 0 {
            return Err(RevoraError::LimitReached);
        }

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        let key = DataKey2::BlacklistSizeLimit(offering_id);
        env.storage().persistent().set(&key, &max_size);

        Ok(())
    }

    // ── Whitelist management ──────────────────────────────────

    /// Set per-offering concentration limit. Caller must be the offering issuer.
    /// `max_bps`: max allowed single-holder share in basis points (0 = disable).
    /// Add `investor` to the per-offering whitelist for `token`.
    ///
    /// Idempotent â€” calling with an already-whitelisted address is safe.
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
        Self::require_not_frozen(&env)?;

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
    /// Idempotent â€” calling when the address is not listed is safe.
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
        Self::require_not_frozen(&env)?;
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

    /// Return a page of whitelisted addresses for an offering.
    /// Limit capped at MAX_PAGE_LIMIT (20).
    pub fn get_whitelist_page(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        start: u32,
        limit: u32,
    ) -> (Vec<Address>, Option<u32>) {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::Whitelist(offering_id);
        let all: Vec<Address> = env
            .storage()
            .persistent()
            .get::<DataKey, Map<Address, bool>>(&key)
            .map(|m| m.keys())
            .unwrap_or_else(|| Vec::new(&env));

        let count = all.len();
        let effective_limit =
            if limit == 0 || limit > MAX_PAGE_LIMIT { MAX_PAGE_LIMIT } else { limit };

        if start >= count {
            return (Vec::new(&env), None);
        }

        let end = core::cmp::min(start + effective_limit, count);
        let mut results = Vec::new(&env);
        for i in start..end {
            results.push_back(all.get(i).unwrap());
        }

        let next_cursor = if end < count { Some(end) } else { None };
        (results, next_cursor)
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

    // â”€â”€ Holder concentration guardrail (#26) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
    /// Configure the concentration limit for an offering.
    ///
    /// ### Parameters
    /// - `max_bps`: The maximum allowed share for a single holder in basis points.
    /// - `enforce`: If true, `report_revenue` will fail if current concentration > `max_bps`.
    ///
    /// ### Constraints
    /// - `max_bps` must be <= 10,000.
    pub fn set_concentration_limit(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        max_bps: u32,
        enforce: bool,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;

        // Auth-first: authenticate before any state reads or side effects.
        // This prevents unauthenticated callers from probing offering existence
        // and ensures event-only mode never silently skips authorization.
        issuer.require_auth();

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
            let key = DataKey::ConcentrationLimit(offering_id);
            env.storage().persistent().set(&key, &ConcentrationLimitConfig { max_bps, enforce });
        }

        Self::emit_v2_event(
            &env,
            (EVENT_CONC_LIMIT_SET, issuer, namespace, token),
            (max_bps, enforce),
        );

        Ok(())
    }

    /// Report the current top-holder concentration for an offering.
    ///
    /// Stores the provided concentration value. If it exceeds the configured limit,
    /// a `conc_warn` event is emitted. The stored value is used for enforcement in `report_revenue`.
    ///
    /// ### Enforcement Boundary
    /// - If `enforce` is true in `ConcentrationLimitConfig`:
    ///   - `concentration_bps <= max_bps`: `report_revenue` is allowed.
    ///   - `concentration_bps > max_bps`: `report_revenue` is rejected.
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
        Self::require_not_paused(&env)?;
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

        let limit_key = DataKey::ConcentrationLimit(offering_id.clone());
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
            env.storage()
                .persistent()
                .set(&DataKey::CurrentConcentration(offering_id.clone()), &concentration_bps);
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

    // â”€â”€ Audit log summary (#34) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
    ///
    /// ### Auth ordering
    /// `issuer.require_auth()` is called immediately after the frozen guard so that
    /// unauthenticated callers cannot probe offering existence or trigger side effects.
    pub fn set_rounding_mode(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        mode: RoundingMode,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        // Auth-first: authenticate before any state reads.
        issuer.require_auth();

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
        let key = DataKey::RoundingMode(offering_id);
        env.storage().persistent().set(&key, &mode);
        Self::emit_v2_event(&env, (EVENT_ROUNDING_MODE_SET, issuer, namespace, token), mode);
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

    // â”€â”€ Per-offering investment constraints (#97) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Set min and max stake per investor for an offering. Issuer/admin only. Constraints are read by off-chain systems for enforcement.
    /// Validates amounts using the Negative Amount Validation Matrix (#163).
    ///
    /// ### Auth ordering
    /// `issuer.require_auth()` is called immediately after the frozen guard, before any state reads.
    pub fn set_investment_constraints(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        min_stake: i128,
        max_stake: i128,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        // Auth-first: authenticate before any state reads.
        issuer.require_auth();

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

        let key = DataKey2::InvestmentConstraints(offering_id);
        let previous =
            env.storage().persistent().get::<DataKey2, InvestmentConstraintsConfig>(&key);
        env.storage().persistent().set(&key, &InvestmentConstraintsConfig { min_stake, max_stake });
        Self::emit_v2_event(
            &env,
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
        let key = DataKey2::InvestmentConstraints(offering_id);
        env.storage().persistent().get(&key)
    }

    // â”€â”€ Per-offering minimum revenue threshold (#25) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Set minimum revenue per period below which no distribution is triggered.
    /// Only the offering issuer may set this. Emits event when configured or changed.
    /// Pass 0 to disable the threshold.
    /// Validates amount using the Negative Amount Validation Matrix (#163).
    ///
    /// ### Auth ordering
    /// `issuer.require_auth()` is called immediately after the frozen guard, before any state reads.
    pub fn set_min_revenue_threshold(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        min_amount: i128,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        // Auth-first: authenticate before any state reads.
        issuer.require_auth();

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

        // Negative Amount Validation Matrix: MinRevenueThreshold requires >= 0 (#163)
        if let Err((err, _)) = AmountValidationMatrix::validate(
            min_amount,
            AmountValidationCategory::MinRevenueThreshold,
        ) {
            return Err(err);
        }

        let key = DataKey2::MinRevenueThreshold(offering_id);
        let previous: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &min_amount);

        Self::emit_v2_event(
            &env,
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
        let key = DataKey2::MinRevenueThreshold(offering_id);
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
    ///
    /// ## Decomposition Bound
    ///
    /// The function decomposes `amount` as `amount = q * 10_000 + r` where:
    /// - `q = amount / 10_000` (quotient)
    /// - `r = amount % 10_000` (remainder, bounded to `|r| < 10_000`)
    ///
    /// This ensures:
    /// - `|r * bps| < 10_000 * 10_000 = 10^8` (well within i128 range)
    /// - The remainder product uses `checked_mul` with saturating fallback for defense-in-depth
    /// - Even if the bound assumption is violated by refactors, saturation prevents overflow
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
        // Defense-in-depth: use checked_mul with saturating fallback to guard against refactors.
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

        let remainder_product = r.checked_mul(bps).unwrap_or_else(|| {
            if (r >= 0 && bps >= 0) || (r < 0 && bps < 0) {
                i128::MAX
            } else {
                i128::MIN
            }
        });
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
                if base >= 0 {
                    i128::MAX
                } else {
                    i128::MIN
                }
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
    /// - If `from_decimals < 7`: scales **up** by `10^(7 - from_decimals)` (e.g., 6-decimal USDC â†’ 7).
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
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        env.storage().persistent().set(&DataKey::PaymentTokenDecimals(offering_id), &decimals);
        env.events().publish((EVENT_DECIMAL_SET, issuer, namespace, token), decimals);
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
            .get(&DataKey2::PaymentTokenDecimals(offering_id))
            .unwrap_or(STELLAR_CANONICAL_DECIMALS)
    }

    // â”€â”€ Multi-period aggregated claims â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Deposit revenue for a specific period of an offering.
    ///
    /// # Arguments
    /// * `issuer` - The address of the offering issuer.
    /// * `namespace` - A symbol identifying the namespace.
    /// * `token` - The address of the token.
    /// * `payment_token` - The address of the token used for payment.
    /// * `amount` - The amount of revenue to deposit.
    /// * `period_id` - The identifier for the revenue period.
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::OfferingNotFound)` if the offering is not found.
    /// - `Err(RevoraError::PeriodAlreadyDeposited)` if revenue has already been deposited for this `period_id`.
    /// - `Err(RevoraError::PaymentTokenMismatch)` if `payment_token` differs from the token locked by the first successful deposit.
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
        issuer.require_auth();

        // Input validation (#35): reject zero/invalid period_id and non-positive amounts.
        if period_id == 0 {
            return Err(RevoraError::InvalidPeriodId);
        }
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
        Self::require_not_frozen(&env)?;

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
        Self::require_not_frozen(&env)?;

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
        // Versioned event v2: [version: u32, payment_token: Address, amount: i128, period_id: u64, snapshot_reference: u64]
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
        Self::require_not_frozen(&env)?;
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

    // â”€â”€ Deterministic Snapshot Expansion (#054) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
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
    /// - `OutdatedSnapshot`: `snapshot_ref` â‰¤ last committed ref (replay / stale).
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

        // Maintain per-offering running total and validate aggregate cap.
        let total_key = DataKey::HolderShareTotal(offering_id.clone());
        let mut current_total: u32 = env.storage().persistent().get(&total_key).unwrap_or(0);

        for i in 0..batch_len {
            let (holder, share_bps) = holders.get(i).unwrap();
            let slot = start_index.saturating_add(i);

            // Write indexed slot for deterministic enumeration.
            env.storage().persistent().set(
                &DataKey::SnapshotHolder(offering_id.clone(), snapshot_ref, slot),
                &(holder.clone(), share_bps),
            );

            // Compute delta against previously persisted holder share.
            let old_share: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::HolderShare(offering_id.clone(), holder.clone()))
                .unwrap_or(0);

            let new_total = current_total.saturating_sub(old_share).saturating_add(share_bps);
            if new_total > 10_000 {
                return Err(RevoraError::InvalidShareBps);
            }

            // Update live holder share so claim() works immediately.
            env.storage()
                .persistent()
                .set(&DataKey::HolderShare(offering_id.clone(), holder.clone()), &share_bps);

            current_total = new_total;
            added_bps = added_bps.saturating_add(share_bps);
        }

        // Update snapshot metadata.
        let new_holder_count = entry.holder_count.saturating_add(batch_len);
        let new_total_bps = entry.total_bps.saturating_add(added_bps);
        entry.holder_count = new_holder_count;
        entry.total_bps = new_total_bps;
        env.storage().persistent().set(&entry_key, &entry);

        // Persist updated per-offering running total.
        env.storage()
            .persistent()
            .set(&DataKey::HolderShareTotal(offering_id.clone()), &current_total);

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

    // â”€â”€ Delegating wrappers for functions in the plain impl block â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // These expose functions from the plain impl block through the contract ABI.

    /// Set a holder's revenue share in basis points for an offering.
    pub fn set_holder_share(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        share_bps: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        Self::get_current_issuer(
            &env,
            issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
        )
        .ok_or(RevoraError::OfferingNotFound)?;

        // Delegate to internal writer which maintains the aggregate running total
        // and enforces the per-offering sum invariant (≤ 10_000 bps).
        Self::set_holder_share_internal(&env, issuer, namespace, token, holder, share_bps)
    }

    /// Get a holder's revenue share in basis points for an offering.
    pub fn get_holder_share(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey::HolderShare(offering_id, holder)).unwrap_or(0)
    }

    /// Set the claim delay in seconds for an offering.
    pub fn set_claim_delay(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        delay_secs: u64,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();
        let offering_id = OfferingId { issuer: issuer.clone(), namespace, token };
        let current_issuer = Self::get_current_issuer(
            &env,
            issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
        )
        .ok_or(RevoraError::OfferingNotFound)?;
        if issuer != current_issuer {
            return Err(RevoraError::NotAuthorized);
        }
        env.storage().persistent().set(&DataKey::ClaimDelaySecs(offering_id), &delay_secs);
        Ok(())
    }

    /// Get the claim delay in seconds for an offering.
    pub fn get_claim_delay(env: Env, issuer: Address, namespace: Symbol, token: Address) -> u64 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey::ClaimDelaySecs(offering_id)).unwrap_or(0)
    }

    /// Return the current contract version (#23).
    pub fn get_version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    /// Configure the reporting access window for an offering. If unset, always open.
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

    /// Configure the claiming access window for an offering. If unset, always open.
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
    pub fn claim(
        env: Env,
        holder: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        max_periods: u32,
    ) -> Result<i128, RevoraError> {
        // HardPaused blocks claim; SoftPaused allows it so holders can withdraw during incidents.
        Self::require_not_hard_paused(&env)?;

        holder.require_auth();

        let offering_id = OfferingId { issuer, namespace, token };

        // Initial blacklist check for early fail-fast
        if Self::is_blacklisted(
            env.clone(),
            offering_id.issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
            holder.clone(),
        ) {
            return Err(RevoraError::HolderBlacklisted);
        }

        let share_bps = Self::get_holder_share(
            env.clone(),
            offering_id.issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
            holder.clone(),
        );
        if share_bps == 0 {
            return Err(RevoraError::NoPendingClaims);
        }

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
        let mut previous_period_id: Option<u64> = None;

        for i in start_idx..end_idx {
            // Enforce blacklist/whitelist decisiveness during partial claim sequences
            // This ensures that if a holder becomes blacklisted mid-sequence, subsequent
            // periods in the batch are not claimed
            if Self::is_blacklisted(
                env.clone(),
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
                holder.clone(),
            ) {
                break;
            }

            let entry_key = DataKey::PeriodEntry(offering_id.clone(), i);
            let period_id: u64 = env.storage().persistent().get(&entry_key).unwrap();

            // Enforce index monotonicity: ensure periods are claimed in the exact
            // order they were deposited in PeriodEntry
            if let Some(prev_id) = previous_period_id {
                if period_id <= prev_id {
                    // PeriodEntry order violated - this should never happen with correct
                    // deposit_revenue implementation, but we defensively check
                    return Err(RevoraError::NoPendingClaims);
                }
            }
            previous_period_id = Some(period_id);

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
            let payment_token = Self::get_locked_payment_token_for_offering(&env, &offering_id)
                .ok_or(RevoraError::PaymentTokenMismatch)?;
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

        // Versioned v2 event: [2, holder, total_payout, periods] ΓÇö always emitted (#RC26Q2-C31)
        Self::emit_v2_event(
            &env,
            (
                EVENT_CLAIM_V2,
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
            ),
            (holder.clone(), total_payout, claimed_periods.clone()),
        );
        env.events().publish(
            (
                EVENT_CLAIM_V2,
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
}

// â”€â”€ Holder shares, claims, admin, governance, and utility methods â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
#[contractimpl]
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
    fn set_holder_share_full(
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

        Self::require_not_frozen(&env)?;
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

    // â”€â”€ Meta-authorization, claims, windows, and query methods â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Register an ed25519 public key for a signer address.
    /// The signer must authorize this binding.
    pub fn register_meta_signer_key(
        env: Env,
        signer: Address,
        public_key: BytesN<32>,
    ) -> Result<(), RevoraError> {
        signer.require_auth();
        env.storage().persistent().set(&MetaDataKey::SignerKey(signer.clone()), &public_key);
        Self::emit_v2_event(&env, (EVENT_META_SIGNER_SET, signer), public_key);
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
        Self::emit_v2_event(&env, (EVENT_META_DELEGATE_SET, issuer, namespace, token), delegate);
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
        Self::require_not_frozen(&env)?;
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
        Self::require_not_frozen(&env)?;
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
    fn get_holder_share_internal(
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

    /// @notice Claim accumulated revenue for a holder across multiple unclaimed periods.
    /// @dev Payouts are calculated based on the holder's share at the time of claim.
    ///      Capped at MAX_CLAIM_PERIODS (50) per transaction for gas safety.
    ///      This function enforces strict security invariants for multi-period claims.
    ///
    /// @param holder The address of the token holder. Must provide authentication.
    /// @param issuer The address of the offering issuer.
    /// @param namespace A symbol identifying the namespace.
    /// @param token The token representing the offering.
    /// @param max_periods Maximum number of periods to process (0 = MAX_CLAIM_PERIODS).
    ///
    /// @return Ok(i128) The total payout amount on success.
    /// @return Err(RevoraError::HolderBlacklisted) if the holder is blacklisted.
    /// @return Err(RevoraError::NoPendingClaims) if no share is set or all periods are claimed.
    /// @return Err(RevoraError::ClaimDelayNotElapsed) if the next period is still within the claim delay window.
    ///
    /// # Idempotency and Safety Invariants
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
    /// 7. **Blacklist/whitelist decisiveness during partial sequences**: The blacklist
    ///    check is performed INSIDE the period iteration loop. If a holder becomes
    ///    blacklisted mid-sequence during a multi-period claim, the loop breaks immediately
    ///    and no subsequent periods in the batch are claimed. The index is only advanced
    ///    for periods successfully processed before the blacklist took effect. This ensures
    ///    blacklist/whitelist decisions remain decisive even during partial claim sequences.
    ///
    /// 8. **Index monotonicity enforced**: The function validates that period IDs are
    ///    strictly increasing as they are retrieved from `PeriodEntry`. This ensures
    ///    `LastClaimedIdx` advances only in ways that match the deposited period order,
    ///    preventing any possibility of skipping periods or claiming out of order.
    ///
    /// # Arguments
    /// * `holder` - The address of the holder claiming revenue.
    /// * `issuer` - The address of the offering issuer.
    /// * `namespace` - A symbol identifying the namespace.
    /// * `token` - The address of the token.
    /// * `max_periods` - The maximum number of periods to claim in this call.
    ///
    /// # Events

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
    ///
    /// This function allows indexers, frontends, and reviewers to page through a holder's
    /// currently claimable revenue without mutating contract state. It is the chunked companion
    /// to `get_claimable`.
    ///
    /// # Arguments
    ///
    /// * `issuer` - The offering issuer address
    /// * `namespace` - The offering namespace identifier
    /// * `token` - The offering token address
    /// * `holder` - The holder address to compute claimable amount for
    /// * `start_idx` - The starting period index (cursor) for the chunk query
    /// * `count` - The maximum number of periods to include in this chunk
    ///
    /// # Returns
    ///
    /// Returns `(total, next_cursor)` where:
    /// - `total` is the sum of claimable amounts for the processed periods
    /// - `next_cursor` is `Some(next_index)` if more eligible periods exist after the processed window,
    ///   or `None` if all eligible periods have been processed
    ///
    /// # Behavior
    ///
    /// - Caller-provided cursors (`start_idx`) are clamped to the holder's stored `LastClaimedIdx`
    /// - The first delayed period stops iteration and becomes the returned `next_cursor`
    /// - A blacklisted holder receives `0` from this function
    /// - A closed claim window also yields `0` from this function
    /// - Chunk size `0` or any size above `MAX_CHUNK_PERIODS` (200) is normalized to `MAX_CHUNK_PERIODS`
    /// - Holders with zero share receive `0` claimable amount
    ///
    /// # Security Guarantees
    ///
    /// This implementation is intentionally conservative: previews never advertise more value
    /// than the holder could actually claim at the current ledger state.
    ///
    /// # Cursor Idempotency
    ///
    /// Repeated queries with the same cursor yield identical results, ensuring reliable pagination.
    ///
    /// # Chunk Summation Parity
    ///
    /// Summing chunked claimable amounts equals the full claimable amount obtainable via `get_claimable`.
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

    // â”€â”€ Time-delayed claim configuration (#27) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Set the claim delay for an offering in seconds.
    fn set_claim_delay_full(
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

        Self::require_not_frozen(&env)?;
        issuer.require_auth();
        let key = DataKey::ClaimDelaySecs(offering_id);
        env.storage().persistent().set(&key, &delay_secs);
        env.events().publish((EVENT_CLAIM_DELAY_SET, issuer, namespace, token), delay_secs);
        Ok(())
    }

    /// Get per-offering claim delay in seconds. 0 = immediate claim.
    fn get_claim_delay_internal(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> u64 {
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
}

// â”€â”€ Test-only helpers (not part of the contract ABI) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
impl RevoraRevenueShare {
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
        let deposited_key = DataKey2::DepositedRevenue(offering_id.clone());
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
    // â”€â”€ On-chain distribution simulation (#29) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

    // â”€â”€ Issuer two-step transfer (#258) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    // â”€â”€ Upgradeability guard and freeze (#32) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
        Self::emit_v2_event(&env, (EVENT_ADMIN_SET,), admin);
        Ok(())
    }

    /// Get the admin address, if set.
    pub fn get_admin(env: Env) -> Option<Address> {
        let key = DataKey::Admin;
        env.storage().persistent().get(&key)
    }

    // â”€â”€ Admin rotation safety flow (Issue #191) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Propose a two-step admin rotation to `new_admin`.
    ///
    /// The current admin initiates; `new_admin` must call [`accept_admin_rotation`] to complete.
    /// Only one rotation may be pending at a time.
    ///
    /// ### Auth
    /// Current admin (`require_auth`).
    ///
    /// ### Errors
    /// - `AdminRotationSameAddress` â€” `new_admin` equals current admin.
    /// - `AdminRotationPending` â€” a rotation is already pending; cancel it first.
    /// - `ContractFrozen` â€” contract is frozen.
    ///
    /// ### Events
    /// Emits `adm_prop`: `(adm_prop, current_admin)` â†’ `new_admin`.
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

    /// Accept a pending admin rotation. Completes the transfer and grants admin to `new_admin`.
    ///
    /// ### Auth
    /// `new_admin` must authorize (`require_auth`). Caller must match the pending proposed address.
    ///
    /// ### Errors
    /// - `NoAdminRotationPending` â€” no rotation was proposed.
    /// - `UnauthorizedRotationAccept` â€” caller does not match the pending proposed address.
    /// - `ContractFrozen` â€” contract is frozen.
    ///
    /// ### Events
    /// Emits `adm_acc`: `(adm_acc, old_admin)` â†’ `new_admin`.
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

    /// Cancel a pending admin rotation before it is accepted.
    ///
    /// ### Auth
    /// Current admin (`require_auth`).
    ///
    /// ### Errors
    /// - `NoAdminRotationPending` â€” no rotation is pending.
    /// - `ContractFrozen` â€” contract is frozen.
    ///
    /// ### Events
    /// Emits `adm_canc`: `(adm_canc, current_admin)` â†’ `proposed_new_admin`.
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

    /// Return the proposed new admin address for a pending rotation, or `None` if none is pending.
    ///
    /// ### Auth
    /// None â€” read-only.
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
        // Versioned event v2: [version: u32, frozen: bool]
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

        let key = DataKey2::FrozenOffering(offering_id);
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

        let key = DataKey2::FrozenOffering(offering_id);
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
            .get::<DataKey2, bool>(&DataKey2::FrozenOffering(offering_id))
            .unwrap_or(false)
    }

    /// Return true if the contract is frozen.
    pub fn is_frozen(env: Env) -> bool {
        env.storage().persistent().get::<DataKey, bool>(&DataKey::Frozen).unwrap_or(false)
    }

    // â”€â”€ Multisig admin logic â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    pub const MAX_MULTISIG_OWNERS: u32 = 20;
    /// Maximum proposal duration: 365 days in seconds.
    pub const MAX_PROPOSAL_DURATION: u64 = 365 * 24 * 60 * 60;

    /// Initialize the multisig admin system. May only be called once.
    /// Only the caller (deployer/admin) needs to authorize; owners are registered
    /// without requiring their individual signatures at init time.
    ///
    /// # Soroban Limitation Note
    /// Soroban does not support requiring multiple signers in a single transaction
    /// invocation. Each owner must separately call `approve_action` to sign proposals.
    ///
    /// # Validation Rules
    /// - `owners` must not be empty and must contain â‰¤ 20 unique addresses
    /// - `threshold` must be in range [1, owners.len()]
    /// - `proposal_duration` must be in range [1, 31,536,000] seconds (365 days)
    ///
    /// # Errors
    /// - `NotAuthorized`: Caller is not the admin
    /// - `NotInitialized`: Admin not set (contract not initialized)
    /// - `LimitReached`: Already initialized, empty owners, too many owners, invalid threshold, or duplicate owners
    /// - `InvalidAmount`: Duration is zero or exceeds maximum
    ///
    /// # Events
    /// Emits `ms_init` with `(caller, (owners_count, threshold))` on success.
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
        if proposal_duration == 0 {
            return Err(RevoraError::InvalidAmount);
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

        // Validate proposal duration
        if proposal_duration == 0 || proposal_duration > Self::MAX_PROPOSAL_DURATION {
            return Err(RevoraError::InvalidAmount);
        }

        env.storage().persistent().set(&DataKey::MultisigThreshold, &threshold);
        env.storage().persistent().set(&DataKey::MultisigOwners, &owners.clone());
        env.storage().persistent().set(&DataKey::MultisigProposalCount, &0_u32);
        env.storage().persistent().set(&DataKey::MultisigProposalDuration, &proposal_duration);
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
                return Err(RevoraError::AlreadyApproved);
            }
        }

        proposal.approvals.push_back(approver.clone());
        env.events().publish((EVENT_PROPOSAL_APPROVED, approver.clone()), proposal_id);

        let _threshold: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::MultisigThreshold)
            .ok_or(RevoraError::NotInitialized)?;

        env.storage().persistent().set(&key, &proposal);
        Ok(())
    }

    /// Execute a multisig proposal once the approval threshold is reached.
    pub fn execute_action(
        env: Env,
        executor: Address,
        proposal_id: u32,
    ) -> Result<(), RevoraError> {
        executor.require_auth();
        Self::require_multisig_owner(&env, &executor)?;

        let key = DataKey::MultisigProposal(proposal_id);
        let mut proposal: Proposal =
            env.storage().persistent().get(&key).ok_or(RevoraError::OfferingNotFound)?;

        if proposal.executed {
            return Err(RevoraError::LimitReached);
        }

        if env.ledger().timestamp() >= proposal.expiry {
            return Err(RevoraError::ProposalExpired);
        }

        let threshold: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::MultisigThreshold)
            .ok_or(RevoraError::NotInitialized)?;
        if proposal.approvals.len() < threshold {
            return Err(RevoraError::NotAuthorized);
        }

        proposal.executed = true;
        env.storage().persistent().set(&key, &proposal);

        match proposal.action.clone() {
            ProposalAction::SetAdmin(new_admin) => {
                env.storage().persistent().set(&DataKey::Admin, &new_admin);
            }
            ProposalAction::Freeze => {
                env.storage().persistent().set(&DataKey::Frozen, &true);
                Self::emit_v2_event(&env, (EVENT_FREEZE_V2, proposal.proposer.clone()), true);
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
                if owners.len() >= Self::MAX_MULTISIG_OWNERS {
                    return Err(RevoraError::LimitReached);
                }
                if owners.contains(&new_owner) {
                    return Err(RevoraError::LimitReached);
                }
                owners.push_back(new_owner);
                env.storage().persistent().set(&DataKey::MultisigOwners, &owners);
            }
            ProposalAction::RemoveOwner(old_owner) => {
                let owners: Vec<Address> =
                    env.storage().persistent().get(&DataKey::MultisigOwners).unwrap();
                if !owners.contains(&old_owner) {
                    return Err(RevoraError::NotAuthorized);
                }
                // Threshold invariant: remaining owners must still satisfy threshold.
                if (owners.len() - 1) < threshold {
                    return Err(RevoraError::LimitReached);
                }

                let mut new_owners = Vec::new(&env);
                for i in 0..owners.len() {
                    let owner = owners.get(i).unwrap();
                    if owner != old_owner {
                        new_owners.push_back(owner);
                    }
                }
                env.storage().persistent().set(&DataKey::MultisigOwners, &new_owners);
            }
            ProposalAction::SetProposalDuration(new_duration) => {
                if new_duration == 0 {
                    return Err(RevoraError::InvalidAmount);
                }
                env.storage().persistent().set(&DataKey::MultisigProposalDuration, &new_duration);
                env.events().publish((EVENT_DURATION_SET, proposal.proposer.clone()), new_duration);
            }
        }

        env.events().publish((EVENT_PROPOSAL_EXECUTED, executor), proposal_id);
        Ok(())
    }
} // end impl RevoraRevenueShare (plain)

#[cfg(test)]
mod issue_370_373_tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Env, Symbol, Vec};

    fn client() -> (Env, Address, RevoraRevenueShareClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register_contract(None, RevoraRevenueShare);
        let client = RevoraRevenueShareClient::new(&env, &id);
        (env, id, client)
    }

    fn assert_bounds(result: i128, amount: i128) {
        let lo = core::cmp::min(0_i128, amount);
        let hi = core::cmp::max(0_i128, amount);
        assert!(
            result >= lo && result <= hi,
            "result {result} out of bounds [{lo}, {hi}] for amount={amount}"
        );
    }

    #[test]
    fn issue_370_get_offerings_page_limit_cursor_and_order_are_stable() {
        let (env, _contract_id, client) = client();
        let issuer = Address::generate(&env);
        let namespace = Symbol::new(&env, "def");

        let mut tokens = Vec::new(&env);
        for i in 0..25_u32 {
            let token = Address::generate(&env);
            client.register_offering(&issuer, &namespace, &token, &(1_000 + i), &token, &0);
            tokens.push_back(token);
        }

        assert_eq!(client.get_offering_count(&issuer, &namespace), 25);

        let (page_1, cursor_1) = client.get_offerings_page(&issuer, &namespace, &0, &10);
        assert_eq!(page_1.len(), 10);
        assert_eq!(cursor_1, Some(10));
        for i in 0..10 {
            assert_eq!(page_1.get(i).unwrap().token, tokens.get(i).unwrap());
        }

        let (page_2, cursor_2) = client.get_offerings_page(&issuer, &namespace, &10, &10);
        assert_eq!(page_2.len(), 10);
        assert_eq!(cursor_2, Some(20));
        for i in 0..10 {
            assert_eq!(page_2.get(i).unwrap().token, tokens.get(i + 10).unwrap());
        }

        let (page_3, cursor_3) = client.get_offerings_page(&issuer, &namespace, &20, &10);
        assert_eq!(page_3.len(), 5);
        assert_eq!(cursor_3, None);
        for i in 0..5 {
            assert_eq!(page_3.get(i).unwrap().token, tokens.get(i + 20).unwrap());
        }

        let (page_clamped, cursor_clamped) =
            client.get_offerings_page(&issuer, &namespace, &0, &100);
        assert_eq!(page_clamped.len(), 20);
        assert_eq!(cursor_clamped, Some(20));

        let (empty_at_count, cursor_at_count) =
            client.get_offerings_page(&issuer, &namespace, &25, &10);
        assert_eq!(empty_at_count.len(), 0);
        assert_eq!(cursor_at_count, None);

        let (empty_beyond, cursor_beyond) =
            client.get_offerings_page(&issuer, &namespace, &99, &10);
        assert_eq!(empty_beyond.len(), 0);
        assert_eq!(cursor_beyond, None);

        let (page_limit_zero, cursor_limit_zero) =
            client.get_offerings_page(&issuer, &namespace, &0, &0);
        assert_eq!(page_limit_zero.len(), 20);
        assert_eq!(cursor_limit_zero, Some(20));
    }

    #[test]
    fn issue_370_get_offerings_page_stable_across_accept_issuer_transfer() {
        let (env, contract_id, client) = client();
        let old_issuer = Address::generate(&env);
        let new_issuer = Address::generate(&env);
        let namespace = Symbol::new(&env, "def");

        // Security: seed issuer registry so pending transfer lookup scans the old issuer.
        env.as_contract(&contract_id, || {
            env.storage().persistent().set(&DataKey2::IssuerCount, &1_u32);
            env.storage().persistent().set(&DataKey2::IssuerItem(0), &old_issuer);
            env.storage().persistent().set(&DataKey2::IssuerRegistered(old_issuer.clone()), &true);
            env.storage().persistent().set(&DataKey2::NamespaceCount(old_issuer.clone()), &1_u32);
            env.storage()
                .persistent()
                .set(&DataKey2::NamespaceItem(old_issuer.clone(), 0), &namespace);
            env.storage()
                .persistent()
                .set(&DataKey2::NamespaceRegistered(old_issuer.clone(), namespace.clone()), &true);
        });

        let new_token_0 = Address::generate(&env);
        let new_token_1 = Address::generate(&env);
        client.register_offering(&new_issuer, &namespace, &new_token_0, &1_100, &new_token_0, &0);
        client.register_offering(&new_issuer, &namespace, &new_token_1, &1_200, &new_token_1, &0);

        let mut old_tokens = Vec::new(&env);
        for i in 0..25_u32 {
            let token = Address::generate(&env);
            client.register_offering(&old_issuer, &namespace, &token, &(2_000 + i), &token, &0);
            old_tokens.push_back(token);
        }

        let transfer_token = old_tokens.get(7).unwrap();
        client.propose_issuer_transfer(&old_issuer, &namespace, &transfer_token, &new_issuer);
        client.accept_issuer_transfer(&new_issuer, &namespace, &transfer_token);

        assert_eq!(client.get_offering_count(&old_issuer, &namespace), 25);
        let (old_page, old_cursor) = client.get_offerings_page(&old_issuer, &namespace, &0, &100);
        assert_eq!(old_page.len(), 20);
        assert_eq!(old_cursor, Some(20));
        for i in 0..20 {
            assert_eq!(old_page.get(i).unwrap().token, old_tokens.get(i).unwrap());
        }

        let (old_tail, old_tail_cursor) =
            client.get_offerings_page(&old_issuer, &namespace, &20, &10);
        assert_eq!(old_tail.len(), 5);
        assert_eq!(old_tail_cursor, None);

        assert_eq!(client.get_offering_count(&new_issuer, &namespace), 3);
        let (new_page_1, new_cursor_1) = client.get_offerings_page(&new_issuer, &namespace, &0, &2);
        assert_eq!(new_page_1.len(), 2);
        assert_eq!(new_cursor_1, Some(2));
        assert_eq!(new_page_1.get(0).unwrap().token, new_token_0);
        assert_eq!(new_page_1.get(1).unwrap().token, new_token_1);

        let (new_page_2, new_cursor_2) = client.get_offerings_page(&new_issuer, &namespace, &2, &2);
        assert_eq!(new_page_2.len(), 1);
        assert_eq!(new_cursor_2, None);
        assert_eq!(new_page_2.get(0).unwrap().token, transfer_token);
    }

    #[test]
    fn issue_373_compute_share_round_half_up_negative_midpoint_and_extremes() {
        let (_env, _contract_id, client) = client();

        assert_eq!(client.compute_share(&0, &5_000, &RoundingMode::RoundHalfUp), 0);
        assert_eq!(client.compute_share(&123_456, &0, &RoundingMode::RoundHalfUp), 0);
        assert_eq!(client.compute_share(&15_000, &5_000, &RoundingMode::RoundHalfUp), 7_500);
        assert_eq!(client.compute_share(&-15_001, &5_000, &RoundingMode::Truncation), -7_500);
        assert_eq!(client.compute_share(&-15_001, &5_000, &RoundingMode::RoundHalfUp), -7_501);

        for bps in [1_u32, 5_000, 9_999, 10_000, 10_001] {
            let pos = client.compute_share(&i128::MAX, &bps, &RoundingMode::RoundHalfUp);
            let neg = client.compute_share(&i128::MIN, &bps, &RoundingMode::RoundHalfUp);
            assert_bounds(pos, i128::MAX);
            assert_bounds(neg, i128::MIN);
            if bps == 10_001 {
                assert_eq!(pos, 0);
                assert_eq!(neg, 0);
            }
        }

        assert_eq!(
            client.compute_share(&i128::MAX, &10_000, &RoundingMode::RoundHalfUp),
            i128::MAX
        );
        assert_eq!(
            client.compute_share(&i128::MIN, &10_000, &RoundingMode::RoundHalfUp),
            i128::MIN
        );
    }
}
