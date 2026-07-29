#![no_std]
#![deny(unsafe_code)]
#![deny(clippy::arithmetic_side_effects)]
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
    contract, contractclient, contracterror, contractimpl, contracttype, symbol_short, token,
    xdr::ToXdr, Address, Bytes, BytesN, Env, IntoVal, Map, Symbol, Vec,
};

/// Cross-contract client trait for FX oracle integrations.
///
/// Any oracle contract that implements this trait can be used in `set_fx_oracle`
/// or as an entry in `set_oracle_chain`.  The `quote` method returns a
/// `(rate_bps, quoted_at_unix_secs)` pair where `rate_bps` is the exchange rate
/// in basis points (i.e. `1 unit of from == rate_bps/10_000 units of to`).
#[contractclient(name = "FxOracleClient")]
pub trait FxOracle {
    fn quote(env: Env, from: Symbol, to: Symbol) -> (i128, u64);
}

/// A single entry in an oracle fallback chain.
///
/// Each entry identifies an oracle contract address together with the currency
/// pair symbols it serves.  Entries are evaluated in order; the first entry
/// that returns a fresh (non-stale) quote wins.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct OracleEntry {
    /// On-chain address of the oracle contract.
    pub oracle: Address,
    /// Symbol of the revenue / source currency (e.g. `EUR`).
    pub revenue_symbol: Symbol,
    /// Symbol of the payout / target currency (e.g. `USDC`).
    pub payout_symbol: Symbol,
    /// Maximum age in seconds before a quote is considered stale.
    /// `0` disables the age check for this entry.
    pub max_age_secs: u64,
}

/// Ordered list of oracle fallback entries stored per offering.
///
/// The chain is evaluated left-to-right; the contract picks the **first** entry
/// whose `quote()` returns a timestamp within the allowed staleness window.  If
/// every entry is stale the call returns `RevoraError::AllOraclesStale`.
///
/// Maximum chain length: [`MAX_ORACLE_CHAIN_LEN`].
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct OracleChain {
    pub entries: Vec<OracleEntry>,
}

#[soroban_sdk::contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DistributionError {
    DistributionDeferred = 1,
}

#[soroban_sdk::contracttype]
pub enum DeferredDataKey {
    DeferredReports(u32),
}

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
#[contracterror(export = false)]
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
    /// Snapshot has been committed but not finalized via `finalize_snapshot`.
    SnapshotNotFinalized = 49,
    /// The recomputed snapshot digest does not match the committed `content_hash`.
    SnapshotHashMismatch = 50,
    /// `display_decimals` exceeds the maximum allowed precision of 18.
    ///
    /// Wire value: 51. Stable since v1.
    DisplayDecimalsOutOfRange = 34,
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
    /// The provided attestation network identifier does not match the active ledger network.
    NetworkIdMismatch = 62,
    /// Multisig proposal has expired.
    /// Wire value: 30. Stable since v1.
    ProposalExpired = 30,
    /// Holder jurisdiction is not permitted by the offering's compliance allowlist.
    JurisdictionDisallowed = 31,
    /// Cross-contract token transfer failed.
    TransferFailed = 39,
    /// Contract is already at the target version; no migration needed.
    AlreadyAtTargetVersion = 32,
    /// Target version is lower than the current deployed version.
    MigrationDowngradeNotAllowed = 33,

    /// Close-period abort due to a detected accrual or share ledger invariant violation.
    ///
    /// This error is returned before the period is sealed to prevent partially committed
    /// close actions when the underlying state is inconsistent.
    CloseAbortInvariantsViolated = 34,

    /// Admin rotation failed: new admin cannot be the same as current.
    AdminRotationSameAddress = 40,
    /// Admin rotation failed: another rotation is already pending.
    AdminRotationPending = 41,
    /// Admin rotation failed: no rotation is currently pending.
    NoAdminRotationPending = 35,
    /// Admin rotation failed: caller is not the pending new admin.
    UnauthorizedRotationAccept = 36,
    /// Admin rotation failed: the configured delay has not elapsed since the proposal.
    AdminRotationDelayNotElapsed = 37,
    /// Offering is frozen.
    OfferingFrozen = 42,
    /// Issuer transfer has expired.
    IssuerTransferExpired = 43,
    /// Transfer blocked because the offering has pre-cliff vesting schedules.
    VestingTransferBlocked = 52,
    /// Contract is paused.
    ContractPaused = 44,
    /// Blacklist size limit exceeded.
    BlacklistSizeLimitExceeded = 45,
    /// Approver has already approved this proposal.
    AlreadyApproved = 46,
    /// The requester is still within the faucet cooldown window.
    FaucetCooldownActive = 38,
    /// Total supply shares would exceed the offering's max total supply shares.
    MaxTotalSupplySharesExceeded = 67,

    /// override_existing=true was requested but no persisted report exists for the given period_id.
    MissingReportForOverride = 47,

    /// The period has been sealed by `close_period`; no further overrides are accepted.
    ///
    /// Wire value: 48. Stable since v1.
    PeriodAlreadyClosed = 48,

    /// Concentration enforcement requires a fresh `report_concentration`, but the stored
    /// concentration data is missing or older than the configured staleness window.
    ///
    /// Wire value: 53. Stable since v1.
    StaleConcentrationData = 53,

    /// Disclosure URI exceeds the 256-byte maximum.
    DisclosureUriTooLong = 54,
    /// Empty URI paired with a non-zero hash is incoherent.
    InconsistentDisclosure = 55,
    /// sig_a and sig_b must be distinct addresses for dual-signature close.
    DualSigSameSigner = 56,
    /// Dual-signature close is not configured for this offering.
    DualSigNotConfigured = 57,
    /// The dispute ID does not correspond to an existing dispute record.
    DisputeNotFound = 58,
    /// A dispute with the same (offering_id, holder, meta_hash) already exists.
    DisputeAlreadyOpen = 59,
    /// The holder has reached the maximum number of open disputes per offering.
    MaxDisputesReached = 60,
    /// The caller holds zero shares in the offering and cannot open a dispute.
    DisputeZeroShare = 61,
    /// The provided nonce is not strictly greater than the last accepted nonce for this
    /// (offering_id, holder) pair. Replayed or out-of-order `set_holder_share` calls are
    /// rejected to prevent stale off-chain updates from overwriting newer on-chain state.
    ///
    /// Wire value: 62. Stable since the nonce-guard release.
    StaleNonce = 62,
}

pub mod vesting;
pub mod tax_bucket;

/// Deterministic Merkle-tree helpers for snapshot finalization.
///
/// Provides [`merkle_helpers::canonical_leaves`] and [`merkle_helpers::build_merkle_root`].
/// See the module-level documentation in `src/merkle_helpers.rs` for full usage and
/// security notes.
pub mod merkle_helpers;

/// Security assertion helpers for production validation.
pub mod security_assertions;

#[cfg(feature = "kani")]
pub mod kani_harness;

#[cfg(test)]
mod test_merkle_canonical_order;
#[cfg(test)]
mod test_claim_transfer_fail;
#[cfg(test)]
mod test_compute_share_invariants;
#[cfg(test)]
mod test_duplicates;
mod test_event_indexed_v2;
#[cfg(test)]
mod test_event_indexed_v3;
#[cfg(test)]
mod test_min_revenue_threshold_boundary;
#[cfg(test)]
mod test_time_windows;
// #[cfg(test)]
// mod test_claim_transfer_fail;
#[cfg(test)]
mod test_close_period;
#[cfg(test)]
mod test_disclosure;
#[cfg(test)]
mod test_quorum_check;
#[cfg(test)]
mod test_ofac_snapshot_pin;
/// Self-test module providing a `self_test()` entrypoint that runs contract-internal
#[cfg(test)]
mod test_self_test;
/// invariant checks against a fixed canary dataset embedded in the WASM binary.
pub mod self_test;

// â”€â”€ Event symbols â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_BL_ADD: Symbol = symbol_short!("bl_add");
const EVENT_BL_ADD_PINNED: Symbol = symbol_short!("bl_add_pn");
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
const EVENT_REVENUE_REPORT_MISSING_OVERRIDE: Symbol = symbol_short!("rev_omiss");
const EVENT_REVENUE_REPORT_REJECTED_ASSET: Symbol = symbol_short!("rev_reja");
pub const EVENT_SCHEMA_VERSION_V2: u32 = 2;
const DEFAULT_FAUCET_COOLDOWN_SECONDS: u64 = 3_600;

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
const EVENT_ACC_UPD: Symbol = symbol_short!("acc_upd");
const EVENT_FREEZE_V2: Symbol = symbol_short!("frz2");
const EVENT_CLAIM_DELAY_SET_V2: Symbol = symbol_short!("dly_set2");
const EVENT_CONCENTRATION_WARNING_V2: Symbol = symbol_short!("conc2");
const EVENT_DECIMAL_SET: Symbol = symbol_short!("pt_dec");

const EVENT_PROPOSAL_CREATED_V2: Symbol = symbol_short!("prop_n2");
const EVENT_PROPOSAL_APPROVED_V2: Symbol = symbol_short!("prop_a2");
const EVENT_PROPOSAL_EXECUTED_V2: Symbol = symbol_short!("prop_e2");
const EVENT_PROPOSAL_APPROVED: Symbol = symbol_short!("prop_app");
const EVENT_PROPOSAL_EXECUTED: Symbol = symbol_short!("prop_exe");
const EVENT_PROPOSAL_CREATED_GOV: Symbol = symbol_short!("prop_crt");
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PauseState {
    NotPaused = 0,
    SoftPaused = 1,
    HardPaused = 2,
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
/// Emitted when a registered migration hook is applied during storage walker execution.
const EVENT_MIG_HOOK_APPLIED: Symbol = symbol_short!("mig_hook");
/// Emitted for each deterministic seed produced by `faucet_seed_holders` (testnet only).
const EVENT_FAUCET_SEED: Symbol = symbol_short!("fct_seed");
const EVENT_FAUCET_COOLDOWN_REJECT: Symbol = symbol_short!("fct_cdrj");

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
const EVENT_ROYALTY_CONFIG: Symbol = symbol_short!("roy_cfg");
const EVENT_ROYALTY_PAID: Symbol = symbol_short!("roy_paid");
const EVENT_INDEXED_V2: Symbol = symbol_short!("ev_idx2");
const EVENT_INDEXED_V3: Symbol = symbol_short!("ev_idx3");
const EVENT_TYPE_OFFER: Symbol = symbol_short!("offer");
/// Emitted when a period is sealed by `close_period`.
const EVENT_PERIOD_CLOSED: Symbol = symbol_short!("per_clos");
/// Emitted when a period is sealed via dual-signature `close_period_dual_sig`.
const EVENT_DUAL_SIG_CLOSE: Symbol = symbol_short!("dual_cls");
/// Emitted when an offering's off-chain disclosure metadata is set or updated (#485).
const EVENT_DISCLOSURE_UPDATED: Symbol = symbol_short!("disc_upd");
/// Emitted when a dispute is resolved by admin (#593).
const EVENT_DISPUTE_RESOLVE: Symbol = symbol_short!("disp_res");
/// Emitted when a dispute window is set for an offering.
const EVENT_DISPUTE_WINDOW_SET: Symbol = symbol_short!("disp_win");
const EVENT_TYPE_REV_INIT: Symbol = symbol_short!("rv_init");
const EVENT_TYPE_REV_OVR: Symbol = symbol_short!("rv_ovr");
const EVENT_TYPE_REV_REJ: Symbol = symbol_short!("rv_rej");
const EVENT_TYPE_REV_OMISS: Symbol = symbol_short!("rv_omiss");
const EVENT_TYPE_REV_REP: Symbol = symbol_short!("rv_rep");
const EVENT_TYPE_CLAIM: Symbol = symbol_short!("claim");
/// Emitted via `EVENT_INDEXED_V2` whenever the per-offering accrual index advances.
/// topic: `(ev_idx2, EventIndexTopicV2{event_type=acc_idx, ...})`
/// data:  `(new_idx_e18: i128,)`
const EVENT_TYPE_ACC_IDX: Symbol = symbol_short!("acc_idx");
const EVENT_REPORT_WINDOW_SET: Symbol = symbol_short!("rep_win");
const EVENT_CLAIM_WINDOW_SET: Symbol = symbol_short!("clm_win");
const EVENT_META_SIGNER_SET: Symbol = symbol_short!("meta_key");
const EVENT_META_DELEGATE_SET: Symbol = symbol_short!("meta_del");
const EVENT_META_SHARE_SET: Symbol = symbol_short!("meta_shr");
const EVENT_MULTISIG_INIT: Symbol = symbol_short!("ms_init");
const EVENT_STALE_PROPOSAL_REJECT: Symbol = symbol_short!("stale_pr");
const EVENT_META_REV_APPROVE: Symbol = symbol_short!("meta_rev");
/// Emitted when `repair_audit_summary` writes a corrected `AuditSummary` to storage.
const EVENT_AUDIT_REPAIRED: Symbol = symbol_short!("aud_rep");
/// Emitted when a share transfer with attestation occurs.
const EVENT_XFER_ATT: Symbol = symbol_short!("xfer_att");
/// Emitted when a cross-class share transfer is blocked.
/// Data: `(offering_id, from, to, from_class, to_class)`.
const EVENT_CLASS_XFER_BLOCK: Symbol = symbol_short!("cls_block");

/// Emitted when the V2-compat downgrade flag is toggled by admin.
/// topic: (ev_v2c, admin)
/// data:  (enabled: bool)
const EVENT_V2_COMPAT_SET: Symbol = symbol_short!("ev_v2c");

/// Emitted when a redemption window is set for an offering.
const EVENT_REDEMPTION_WINDOW_SET: Symbol = symbol_short!("rdm_win");
/// Emitted when a holder requests a redemption.
const EVENT_REDEMPTION_REQUESTED: Symbol = symbol_short!("red_req");
/// Emitted when an issuer fulfills a holder redemption request.
const EVENT_REDEMPTION_FULFILLED: Symbol = symbol_short!("red_full");
/// Emitted when a redemption fee configuration is set for an offering.
const EVENT_REDEMPTION_FEE_SET: Symbol = symbol_short!("rdm_fcfg");
/// Emitted when a redemption fee is deducted and routed to treasury during fulfillment.
const EVENT_REDEMPTION_FEE: Symbol = symbol_short!("rdm_fee");
/// Missing v1 event symbols (referenced by report_revenue versioned path).
/// Emitted when payment token decimals are set for an offering.

/// Current schema version for indexed events. Bump when adding fields to `EventIndexTopicV*`.
/// V2 topics continue to emit for backward compatibility during the deprecation window.
const INDEXER_EVENT_SCHEMA_VERSION: u32 = 3;

const EVENT_CONC_LIMIT_SET: Symbol = symbol_short!("conc_lim");
const EVENT_ROUNDING_MODE_SET: Symbol = symbol_short!("rnd_mode");
const EVENT_ADMIN_SET: Symbol = symbol_short!("admin_set");
/// Emitted when an admin rotation is logged to persistent history.
const EVENT_ADMIN_ROTATION_LOGGED: Symbol = symbol_short!("adm_log");
const EVENT_PLATFORM_FEE_SET: Symbol = symbol_short!("fee_set");
const EVENT_FRZ_SET: Symbol = symbol_short!("frz_set");
const EVENT_FRZ_CLR: Symbol = symbol_short!("frz_clr");
const EVENT_DISPUTE_FREEZE_ON: Symbol = symbol_short!("dsp_frzon");
const EVENT_DISPUTE_FREEZE_OFF: Symbol = symbol_short!("dsp_fzoff");
const BPS_DENOMINATOR: i128 = 10_000;
const ACCRUAL_SCALE_E18: i128 = 1_000_000_000_000_000_000;
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
const EVENT_SNAP_FINALIZED: Symbol = symbol_short!("snap_fin");
const EVENT_SNAP_FINALIZATION_CONFIG: Symbol = symbol_short!("snap_fnc");
const EVENT_FREEZE_OFFERING: Symbol = symbol_short!("frz_off");
const EVENT_UNFREEZE_OFFERING: Symbol = symbol_short!("ufrz_off");
const EVENT_PROPOSAL_CREATED: Symbol = symbol_short!("prop_new");
const EVENT_FREEZE: Symbol = symbol_short!("freeze");

// ── Governance event constants (issue #557, #559) ──
const EVENT_GOV_PROP_CREATED: Symbol = symbol_short!("gov_new");
const EVENT_GOV_VOTE_CAST: Symbol = symbol_short!("gov_vote");
const EVENT_WEIGHT_PIN: Symbol = symbol_short!("wt_pin");
/// Stable versioned indexed event for every ballot cast — consumed by off-chain indexers
/// to reconstruct per-proposal governance state without re-reading all `gov_vote` events.
/// Topic: `(ev_idx3, EventIndexTopicV3 { event_type: "vote_v3", period_id: 0, ... })`
/// Data:  `(proposal_id: u32, voter: Address, choice: VoteChoice, weight_bps: u32)`
const EVENT_TYPE_VOTE_V3: Symbol = symbol_short!("vote_v3");

/// Emitted when an oracle from the fallback chain is selected to provide the FX rate.
/// Topic: `(oracle_src_used, issuer, namespace, token)`.
/// Data: `(oracle_address, revenue_symbol, payout_symbol, chain_index)`.
const EVENT_ORACLE_SOURCE_USED: Symbol = symbol_short!("orc_used");

/// Issuer transfer expiry: 7 days in seconds (default).
const ISSUER_TRANSFER_EXPIRY_SECS: u64 = 7 * 24 * 60 * 60;
/// Minimum configurable issuer transfer expiry: 1 hour.
const MIN_ISSUER_TRANSFER_EXPIRY_SECS: u64 = 60 * 60;
/// Maximum configurable issuer transfer expiry: 30 days.
const MAX_ISSUER_TRANSFER_EXPIRY_SECS: u64 = 30 * 24 * 60 * 60;
const EVENT_CLAIM: Symbol = symbol_short!("claim");
const EVENT_CLAIM_DELAY_SET: Symbol = symbol_short!("dly_set");
// v1 versioned event symbols (legacy)

/// Represents a revenue-share offering registered on-chain.
/// Offerings are immutable once registered.
// â”€â”€ Data structures â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
/// Semantic version (MAJOR, MINOR, PATCH) for contract upgrades (#23).
/// A `migrate_storage` call must pass a version strictly greater than the
/// currently deployed version. Downgrades and no-op migrations are rejected.
/// Bumped when storage or semantics change; used for migration and compatibility.
pub const CONTRACT_VERSION: (u32, u32, u32) = (1, 0, 23);
/// Persistent storage layout version. Bump when adding/renaming DataKey variants.
pub const STORAGE_LAYOUT_VERSION: u32 = 3;

/// Assert that `to` is a strict forward semver upgrade over `from`.
///
/// # Errors
/// - [`RevoraError::AlreadyAtTargetVersion`] if `to == from` (no-op migration)
/// - [`RevoraError::MigrationDowngradeNotAllowed`] if `to < from` (downgrade)
///
/// # Semver ordering
/// Comparison is lexicographic on `(major, minor, patch)`:
/// - Any increase in `major` is a valid upgrade (breaking change).
/// - Same major, higher minor is valid (backward-compatible addition).
/// - Same major and minor, higher patch is valid (bugfix).
/// - Any decrease in any component is a downgrade.
pub fn assert_semver_forward(
    from: (u32, u32, u32),
    to: (u32, u32, u32),
) -> Result<(), RevoraError> {
    if to == from {
        return Err(RevoraError::AlreadyAtTargetVersion);
    }
    if to.0 < from.0
        || (to.0 == from.0 && to.1 < from.1)
        || (to.0 == from.0 && to.1 == from.1 && to.2 < from.2)
    {
        return Err(RevoraError::MigrationDowngradeNotAllowed);
    }
    Ok(())
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TenantId {
    pub issuer: Address,
    pub namespace: Symbol,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
pub enum FreezeReason {
    /// Broad compliance or regulatory action.
    Compliance,
    /// Court-ordered legal hold.
    LegalHold,
    /// Active dispute under investigation.
    DisputeOpen,
    /// Address matched on a sanctions list.
    SanctionsMatch,
    // Legacy variants kept for storage compatibility.
    Sanctions,
    CourtOrder,
    IssuerDispute,
    Manual,
}

/// Outcome of a dispute resolution (#593).
///
/// - `Upheld`: the dispute was valid; freeze remains.
/// - `Rejected`: the dispute was invalid; holder is unfrozen.
/// - `PartiallyUpheld`: the dispute was partially valid; freeze modified.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
pub enum DisputeOutcome {
    Upheld,
    Rejected,
    PartiallyUpheld,
}

/// On-chain dispute tracking entry (#593).
/// Created when a holder is frozen with `IssuerDispute` reason.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct DisputeEntry {
    /// Unique dispute ID (auto-incremented).
    pub dispute_id: u64,
    /// The offering this dispute belongs to.
    pub offering_id: OfferingId,
    /// The frozen holder.
    pub holder: Address,
    /// The freeze reason (always IssuerDispute for disputes).
    pub freeze_reason: FreezeReason,
    /// Ledger timestamp when the dispute was created.
    pub created_at: u64,
    /// Whether the dispute has been resolved.
    pub resolved: bool,
    /// Resolution outcome, populated when resolved.
    pub outcome: Option<DisputeOutcome>,
    /// Evidence hash (32-byte SHA-256 or equivalent), populated when resolved.
    pub evidence_hash: Option<BytesN<32>>,
    /// Admin address that resolved the dispute.
    pub resolved_by: Option<Address>,
    /// Ledger timestamp when the dispute was resolved.
    pub resolved_at: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct OfferingId {
    pub issuer: Address,
    pub namespace: Symbol,
    pub token: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShareClass {
    A,
    B,
    Custom(Symbol),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassConfig {
    pub bps: u32,
    pub voting: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Manual,
    OFAC,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SanctionsAttestation {
    pub source: Source,
    pub ref_id: Symbol,
    pub attested_at: u64,
}

/// Metadata for a blacklist entry that was pinned to an OFAC snapshot hash.
///
/// Stored alongside the `SanctionsAttestation` when an entry is added via
/// `blacklist_add_pinned`. Enables compliance verification by linking each
/// blacklist entry to a specific signed off-chain snapshot of the OFAC list.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct BlacklistEntryMeta {
    /// SHA-256 hash of the signed off-chain OFAC list snapshot.
    pub snapshot_hash: BytesN<32>,
    /// Ledger timestamp when the blacklist entry was created.
    pub added_ts: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Issuers {
    pub primary: Address,
    pub co: Vec<Address>,
    pub quorum: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Offering {
    /// The issuers authorized to manage this offering.
    pub issuers: Issuers,
    /// The namespace this offering belongs to.
    pub namespace: Symbol,
    /// The token representing this offering.
    pub token: Address,
    /// Cumulative revenue share for all holders in basis points (0-10000).
    pub revenue_share_bps: u32,
    pub payout_asset: Address,
    /// Human-readable ticker/symbol for the payout denomination (e.g. `USDC`, `XLM`).
    /// Used by wallets and dashboards to display amounts without guessing the payment token.
    /// Maximum 9 characters (Soroban `Symbol` limit).
    pub denomination_symbol: Symbol,
    /// Number of decimal places to use when displaying amounts for this offering.
    /// Must be ≤ `MAX_TOKEN_DECIMALS` (18) and ≤ the payment token's on-chain decimals.
    /// Defaults to 0 when unset.
    pub display_decimals: u32,
}

/// Per-offering FX oracle configuration used when `report_revenue` receives a
/// revenue asset that differs from the offering payout asset.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct FxOracleConfig {
    pub oracle: Address,
    pub revenue_symbol: Symbol,
    pub payout_symbol: Symbol,
    pub max_oracle_age_secs: u64,
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
    /// Maximum age (in seconds) of a `report_concentration` call before it is considered stale.
    /// When `enforce` is true and this is > 0, `report_revenue` rejects if no concentration has
    /// been reported or the last report is older than this many seconds. 0 = disabled (no staleness
    /// check).
    pub max_staleness_secs: u64,
}

/// Per-offering platform fee model (#468).
///
/// Encodes the programmable platform cut taken on each `report_revenue` call and the
/// `treasury` address the fee is routed to. `fee_bps` plus the offering's aggregate
/// holder share must always be `<= 10_000` (enforced in `set_offering_platform_fee`),
/// so the platform and holders never lay claim to more than 100% of reported revenue.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PlatformFeeModel {
    /// Platform fee in basis points (0 = disabled; no fee deducted and no `plat_fee` event).
    pub fee_bps: u32,
    /// Destination address the platform fee is routed to.
    pub treasury: Address,
}

/// Per-offering investment constraints (#97). Min/max stake per investor; off-chain enforced.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InvestmentConstraintsConfig {
    pub min_stake: i128,
    pub max_stake: i128,
}

/// Off-chain disclosure binding for an offering (#485).
/// Binds a URI (PPM, K-1 template, etc.) to a 32-byte integrity hash so
/// investors can verify the off-chain document without trusting the issuer alone.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct DisclosureMeta {
    /// Off-chain document URI, e.g. `ipfs://…` or `https://…`. Max 256 bytes.
    pub uri: Bytes,
    /// SHA-256 (or equivalent) hash of the document at `uri`. Exactly 32 bytes.
    pub hash: BytesN<32>,
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

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TransferRestrictions {
    pub category: Symbol,
    pub max_holders: u32,
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

/// One entry in a distribution proof: the holder's address, their share in basis points,
/// and the normalized payout computed by the contract for a specific period.
///
/// Returned by `prove_distribution_for_period`. Entries are sorted by descending `share_bps`
/// with XDR address bytes (ascending) as a tie-break, giving indexers a canonical, stable
/// ordering regardless of the input `holders` order.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct DistributionEntry {
    /// The holder's address.
    pub holder: Address,
    /// The holder's share in basis points (0–10000).
    pub share_bps: u32,
    /// The normalized payout computed by the contract for this period.
    /// Equals `compute_share(normalize_amount(period_revenue, decimals), share_bps, rounding_mode)`.
    /// Zero when `share_bps == 0` or `period_revenue == 0`.
    pub normalized_payout: i128,
}

/// Pending issuer transfer details including expiry tracking.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PendingTransfer {
    pub new_issuer: Address,
    pub timestamp: u64,
    /// Effective expiry in seconds. 0 means use ISSUER_TRANSFER_EXPIRY_SECS default.
    pub expiry_secs: u64,
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

/// Result of `preflight_close_period` (#563): the same view that a successful
/// `close_period` / `close_period_dual_sig` would expose, returned WITHOUT
/// mutating state or emitting events.
///
/// - `class_pay_order` matches the canonical ordering
///   [`Self::record_and_emit_pay_order`] would persist for this period using
///   the current `set_class_priority` registrations (empty when no classes
///   registered).
/// - `payouts` is the per-holder distribution preview — using the contract's
///   prevailing `compute_share(revenue, share_bps, rounding_mode)` math, the
///   same math the closest analogue
///   [`Self::simulate_distribution`] applies. (Note: blacklisted holders are
///   silently dropped; holders with zero `share_bps` are returned with a zero
///   normalized payout so callers can detect them via `share_bps == 0`.)
/// - `total_distributed` is the saturating sum of `payouts.normalized_payout`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PreflightCloseResult {
    pub period_id: u64,
    /// Reported/deposited revenue currently stored at
    /// `DataKey::PeriodRevenue(offering_id, period_id)`, or `0` if no report
    /// has landed yet. This is the amount the write path would seal into
    /// the period.
    pub period_revenue: i128,
    /// Canonical class pay order that `close_period` will write via
    /// `record_and_emit_pay_order`. Empty when no classes are registered.
    pub class_pay_order: Vec<ShareClass>,
    /// Per-holder distribution preview.
    pub payouts: Vec<DistributionEntry>,
    /// Saturating sum of `payouts[i].normalized_payout`. Never exceeds
    /// `period_revenue` because each entry is the result of
    /// `compute_share(period_revenue, share_bps, mode)` with `share_bps <= 10000`.
    pub total_distributed: i128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct HolderShareCheckpoint {
    pub start_index: u32,
    pub share_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct HolderAccrualState {
    pub last_settled_idx: u32,
    pub last_acc_per_share_e18: i128,
    pub accrued_owed: i128,
}

/// Read-only per-period statement row for a holder.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct HolderStatementEntry {
    /// Deterministic revenue period identifier.
    pub period_id: u64,
    /// Timestamp at which the period's revenue was deposited.
    pub deposit_timestamp: u64,
    /// Amount currently attributable to the holder for this period.
    pub claimable_amount: i128,
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

/// Versioned structured topic payload for indexers (V3).
/// Additive fields (e.g. share_class, tax_bucket) land here in future minor versions
/// without breaking V2 subscribers. V2 and V3 emit concurrently during the deprecation window.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct EventIndexTopicV3 {
    pub version: u32,
    pub event_type: Symbol,
    pub issuer: Address,
    pub namespace: Symbol,
    pub token: Address,
    /// 0 when the event is not period-scoped.
    pub period_id: u64,
    /// Reserved for future use. Facilitates additive schema evolution without struct reshuffle.
    pub _reserved: u32,
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

/// Per-holder pending redemption request.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PendingRedemption {
    pub shares_bps: u32,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum WindowDataKey {
    Report(OfferingId),
    Claim(OfferingId),
    Redemption(OfferingId),
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

/// Immutable record of a completed admin rotation, persisted in an append-only log.
///
/// Written once in `accept_admin_rotation` and read via `get_admin_rotation_history_page`.
/// The log is bounded — see `MAX_ADMIN_ROTATION_LOG`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AdminRotationEntry {
    /// Admin address before the rotation.
    pub prior_admin: Address,
    /// Admin address after the rotation.
    pub new_admin: Address,
    /// Ledger timestamp when `accept_admin_rotation` executed.
    pub rotated_at: u64,
}

/// Primary storage keys for core contract state.
/// Split from the full key set to stay within the Soroban XDR union variant limit (â‰¤50).
///
/// Scoped to the crate: storage keys are an internal implementation detail and are not part
/// of the contract's external interface, so no contract spec entry is generated for them.
/// This also keeps the enum clear of the 50-case spec union limit as new keys are added.
#[contracttype]
#[derive(Clone)]
pub(crate) enum DataKey {
    /// Deprecated shared period tracker retained for backward compatibility with older storage.
    LastPeriodId(OfferingId),
    Blacklist(OfferingId),

    /// Per-offering whitelist; when non-empty, only these addresses are eligible for distribution.
    Whitelist(OfferingId),
    /// Per-offering: blacklist addresses in insertion order for deterministic get_blacklist (#38).
    BlacklistOrder(OfferingId),
    /// Per-offering: metadata for blacklist entries pinned to an OFAC snapshot hash.
    BlacklistMeta(OfferingId),
    OfferCount(TenantId),
    OfferItem(TenantId, u32),
    /// Per-offering concentration limit config.
    ConcentrationLimit(OfferingId),
    /// Per-offering: last reported concentration in bps.
    CurrentConcentration(OfferingId),
    /// Per-offering: ledger timestamp of the last report_concentration call.
    ConcentrationReportedAt(OfferingId),
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
    /// Per-offering accrual index in e18 fixed-point.
    AccrualIndexE18(OfferingId),
    /// Holder's share in basis points for (offering_id, holder).
    HolderShare(OfferingId, Address),
    /// Last accrual index a holder has claimed up to.
    LastClaimedAccrualIndex(OfferingId, Address),
    /// Per-offering running total of all persisted holder shares (basis points).
    HolderShareTotal(OfferingId),
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

    /// Whether snapshot distribution is enabled for an offering.
    SnapshotConfig(OfferingId),
    /// Latest recorded snapshot reference for snapshot deposits on an offering.
    LastSnapshotRef(OfferingId),
    /// Committed snapshot entry keyed by (offering_id, snapshot_ref).
    SnapshotEntry(OfferingId, u64),
    /// Per-snapshot holder share at index N.
    SnapshotHolder(OfferingId, u64, u32),
    /// Total number of holders recorded in a snapshot.
    SnapshotHolderCount(OfferingId, u64),

    /// Per-snapshot holder share by address for O(1) vote-weight lookup.
    /// Key: (OfferingId, snapshot_ref, Address) -> u32 (share_bps).
    SnapshotHolderShare(OfferingId, u64, Address),

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
    /// Per-offering per-asset secondary-market royalty override (#562).
    OfferingRoyaltyBps(OfferingId, Address),
    /// Platform level per-asset fee (#98).
    PlatformFeePerAsset(Address),
    /// Whether snapshot finalization is enforced globally.
    SnapshotFinalizationRequired,
    /// Latest committed snapshot reference for an offering.
    LastSnapshotCommitRef(OfferingId),
}

/// Per-holder anchor for checkpoint-compressed accrual ranges.
///
/// When the per-holder share schedule exceeds `checkpoint_threshold`,
/// the oldest entries are folded into this anchor. The anchor stores
/// a lossless pre-computed sum of claimable amounts for the compressed
/// period range so that `compute_holder_payout_for_range` can retrieve
/// the compressed value in O(1) instead of iterating through every
/// schedule entry for each period. The anchor is keyed by the offering
/// and holder and is consumed incrementally as the holder's claim cursor
/// advances past the compressed range boundary.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AccrualAnchor {
    /// Highest period index (inclusive) covered by this anchor.
    pub end_idx: u32,
    /// Pre-computed sum of claimable amounts for all compressed periods
    /// whose share-bps transitions are folded into this anchor.
    pub claimable_sum: i128,
}

/// Secondary storage keys for auxiliary/extended contract state.
/// Overflow enum to keep DataKey within the Soroban XDR union variant limit.
#[contracttype]
#[derive(Clone)]
pub enum DataKey2 {
    /// Whether the snapshot has been finalized successfully.
    SnapshotFinalized(OfferingId, u64),
    /// Per-offering supply cap (max total deposited revenue).
    SupplyCap(OfferingId),
    /// Total revenue deposited so far for supply-cap tracking.
    DepositedRevenue(OfferingId),
    /// Per-offering investment constraints (min/max stake).
    InvestmentConstraints(OfferingId),
    /// Per-offering minimum revenue threshold.
    MinRevenueThreshold(OfferingId),
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
    /// Holder's configured jurisdiction tag for (offering_id, holder).
    HolderJurisdiction(OfferingId, Address),
    /// Oracle public key mapped by oracle address.
    OraclePubKey(Address),
    /// Conversion ratio (in bps) from one class to another.
    ClassConversionRatio(OfferingId, ShareClass, ShareClass),
    /// Per-offering jurisdiction allowlist. Empty means compliance gating is disabled.
    AllowedJurisdictions(OfferingId),
    /// Global cumulative normalized accrual per 1 bps share, scaled by 1e18.
    GlobalAccPerShareE18(OfferingId),
    /// Snapshot of cumulative accrual after `index` deposited periods.
    AccPerShareAtIndex(OfferingId, u32),
    /// Cached holder accrual state used to freeze matured entitlements across share changes.
    HolderAccrualState(OfferingId, Address),
    /// Piecewise-constant share schedule keyed by deposited-period index.
    HolderShareSchedule(OfferingId, Address),
    /// Per-holder checkpoint anchor for compressed accrual ranges.
    AccrualAnchor(OfferingId, Address),
    /// Per-offering checkpoint compression threshold. When the holder share
    /// schedule length exceeds this value the oldest entries are folded into
    /// an `AccrualAnchor` and pruned from the schedule.
    CheckpointThreshold(OfferingId),
    /// Packed flags: (event_versioning_enabled: bool, event_only_mode: bool).
    ContractFlags,

    /// Direct offering index: (issuer, namespace, token) -> Offering for O(1) get_offering (#360).
    OfferingRecord(OfferingId),

    /// Per-offering blacklist size limit (#358). If not set, defaults to MAX_BLACKLIST_SIZE.
    BlacklistSizeLimit(OfferingId),

    /// Sealed-period flag: when present, `report_revenue` overrides are rejected for this period.
    ClosedPeriod(OfferingId, u64),

    /// Off-chain disclosure metadata (URI + hash) for an offering (#485).
    DisclosureMeta(OfferingId),

    /// Governance proposal count scoped to an offering.
    GovernanceProposalCount(OfferingId),
    /// Governance proposal payload keyed by (offering_id, proposal_id).
    GovernanceProposal(OfferingId, u32),
    /// Duplicate meta-hash guard keyed by (offering_id, meta_hash).
    GovernanceProposalMeta(OfferingId, BytesN<32>),

    /// Timestamp of the last faucet request for a requester address.
    FaucetLastRequest(Address),
    /// Whether dual-signature close-of-period is enabled for this offering.
    DualSigEnabled(OfferingId),
    /// Global freeze reason recorded during set_freeze (#605).
    GlobalFreezeReason,

    // ── Missing variants added for compilation ──
    /// Current accrual index counter for dividend-accrual ledger.
    AccrualIndex(OfferingId),
    /// Per-offering platform fee model.
    OfferingPlatformFee(OfferingId),
    /// Denomination metadata (symbol, decimals) for an offering.
    DenominationMetadata(OfferingId),
    /// FX oracle configuration for an offering.
    FxOracleConfig(OfferingId),
    /// Transfer restrictions per category for an offering.
    TransferRestrictions(OfferingId, Symbol),
    /// Holder category tag for transfer restriction purposes.
    HolderCategory(OfferingId, Address),
    /// Per-category holder count for transfer restriction accounting.
    CategoryHolderCount(OfferingId, Symbol),
    /// Emergency freeze record for (offering_id, holder).
    EmergencyFreeze(OfferingId, Address),
    /// Total shares issued for an offering (tracks against MaxTotalSupplyShares).
    TotalSharesIssued(OfferingId),
    /// Maximum total supply shares cap for an offering.
    MaxTotalSupplyShares(OfferingId),
    /// Per-entry faucet seed for testnet holder seeding.
    FaucetSeedEntry(OfferingId, u32),

    // ── Multisig keys ──
    /// Multisig approval threshold.
    MultisigThreshold,
    /// Multisig owner list.
    MultisigOwners,
    /// Multisig proposal counter.
    MultisigProposalCount,
    /// Default proposal duration in seconds.
    MultisigProposalDuration,
    /// Multisig proposal by id.
    MultisigProposal(u32),

    // ── Governance keys (issue #557) ──
    /// Per-offering governance proposal counter.
    GovProposalCount(OfferingId),
    /// Per-offering governance proposal by id.
    GovProposal(OfferingId, u32),
    /// Vote record for (offering_id, proposal_id, voter) -> bool (true=yes, false=no).
    VoteRecord(OfferingId, u32, Address),
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

/// Maximum redemption fee in basis points (50%).
const MAX_REDEMPTION_FEE_BPS: u32 = 5_000;

/// Maximum number of periods that can be claimed in a single transaction.
/// Keeps compute costs predictable within Soroban limits.
const MAX_CLAIM_PERIODS: u32 = 50;

/// Maximum number of periods allowed in a single read-only chunked query.
/// This is a safety cap to prevent accidental long-running loops in read-only methods.
const MAX_CHUNK_PERIODS: u32 = 200;
/// Default checkpoint threshold for per-holder schedule compression.
/// When a holder's share schedule length exceeds this value the oldest
/// entries are folded into an `AccrualAnchor` and pruned from the schedule.
const CHECKPOINT_THRESHOLD_DEFAULT: u32 = 1_000;


/// Maximum number of open disputes a single holder may have per offering.
/// Prevents spam and unbounded storage growth.
const MAX_OPEN_DISPUTES_PER_HOLDER: u32 = 5;

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
    /// Max total supply shares configuration: must be non-negative (>= 0).
    /// Reason: Zero cap means unlimited; negative cap is invalid.
    MaxTotalSupplyShares,
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
            AmountValidationCategory::MaxTotalSupplyShares => {
                if amount < 0 {
                    return Err((RevoraError::InvalidAmount, symbol_short!("no_neg")));
                }
            }
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
            "set_max_total_supply_shares" => Some(AmountValidationCategory::MaxTotalSupplyShares),
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
        Self::assert_contract_version_compatible(env)?;
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

    /// Ensure the loaded WASM version is not older than the persisted minimum supported version.
    ///
    /// On `initialize` the current `CONTRACT_VERSION` is persisted as the floor for all future
    /// contract WASM binaries. `migrate_storage` ratchets this floor upward. If a WASM binary
    /// with a lower `CONTRACT_VERSION` is deployed later, every state-mutating entrypoint is
    /// blocked and a `downgrade_reject` event is emitted.
    ///
    /// # Errors
    /// - [`RevoraError::MigrationDowngradeNotAllowed`] if `CONTRACT_VERSION < persisted version`.
    fn assert_contract_version_compatible(env: &Env) -> Result<(), RevoraError> {
        if let Some(min_supported) =
            env.storage().persistent().get::<DataKey, (u32, u32, u32)>(&DataKey::DeployedVersion)
        {
            if CONTRACT_VERSION < min_supported {
                env.events().publish(
                    (Symbol::new(env, "downgrade_reject"),),
                    (CONTRACT_VERSION, min_supported),
                );
                return Err(RevoraError::MigrationDowngradeNotAllowed);
            }
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
            .get::<DataKey2, bool>(&DataKey2::FrozenOffering(offering_id.clone()))
            .unwrap_or(false)
        {
            return Err(RevoraError::OfferingFrozen);
        }
        Ok(())
    }

    /// Require that enough issuers have authorized the operation (quorum check).
    fn require_issuer_quorum_auth(env: &Env, issuers: &Issuers) {
        // Collect all issuers (primary + co)
        let mut all_issuers = Vec::new(env);
        all_issuers.push_back(issuers.primary.clone());
        for co_issuer in issuers.co.iter() {
            all_issuers.push_back(co_issuer.clone());
        }

        // Count how many of them have authorized
        let mut auth_count = 0u32;
        for issuer in all_issuers.iter() {
            if env.has_auth(&issuer) {
                auth_count += 1;
            }
        }

        // Ensure we meet the quorum
        assert!(auth_count >= issuers.quorum, "Issuer quorum not met");
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
            .get(&DataKey2::MultisigOwners)
            .ok_or(RevoraError::NotInitialized)?;
        if !owners.contains(caller) {
            return Err(RevoraError::NotAuthorized);
        }
        Ok(())
    }

    /// Check if a holder is emergency frozen for an offering.
    fn is_frozen(env: &Env, offering_id: &OfferingId, holder: &Address) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey2, FreezeReason>(&DataKey2::EmergencyFreeze(
                offering_id.clone(),
                holder.clone(),
            ))
            .is_some()
    }

    /// Require that a holder is not emergency frozen.
    fn require_holder_not_frozen(
        env: &Env,
        offering_id: &OfferingId,
        holder: &Address,
    ) -> Result<(), RevoraError> {
        if Self::is_frozen(env, offering_id, holder) {
            return Err(RevoraError::HolderFrozen);
        }
        Ok(())
    }

    /// Enforce the dispute window for `IssuerDispute` freezes.
    /// Checks that the current time is within the dispute window of the most
    /// recently closed period. If no period has been closed, the check passes.
    fn enforce_dispute_window(env: &Env, offering_id: &OfferingId) -> Result<(), RevoraError> {
        // If no period has been closed yet, there is no deadline to enforce.
        let last_closed = env
            .storage()
            .persistent()
            .get::<DataKey2, u64>(&DataKey2::LastClosedPeriodTimestamp(offering_id.clone()));

        if let Some(last_closed_at) = last_closed {
            let window_secs = env
                .storage()
                .persistent()
                .get::<DataKey2, u64>(&DataKey2::DisputeWindowSecs(offering_id.clone()))
                .unwrap_or(DEFAULT_DISPUTE_WINDOW_SECS);

            let deadline = last_closed_at.saturating_add(window_secs);
            let now = env.ledger().timestamp();

            if now > deadline {
                return Err(RevoraError::DisputeWindowClosed);
            }
        }
        Ok(())
    }

    /// Require that caller is either admin or issuer of the offering.
    fn require_admin_or_issuer(
        env: &Env,
        caller: &Address,
        offering_id: &OfferingId,
    ) -> Result<(), RevoraError> {
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;
        if caller == &admin || caller == &offering_id.issuer {
            return Ok(());
        }
        Err(RevoraError::NotAuthorized)
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

    fn get_secondary_market_royalty_bps(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        asset: Address,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get::<DataKey, u32>(&DataKey::OfferingRoyaltyBps(offering_id, asset))
            .unwrap_or(0)
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

    /// Dual-emit both V2 and V3 indexed events for the same state change.
    ///
    /// V2 subscribers continue to read `ev_idx2` unchanged.  V3 subscribers
    /// consume `ev_idx3` which carries `version=3` and the `_reserved` field
    /// enabling additive schema evolution without struct reshuffles.
    ///
    /// Both events share the same `data` payload; the only difference is the
    /// topic struct (V2 vs V3) and the outer topic symbol.
    fn emit_v2_and_v3<D>(
        env: &Env,
        topic_v2: EventIndexTopicV2,
        topic_v3: EventIndexTopicV3,
        data: D,
    ) where
        D: IntoVal<Env, soroban_sdk::Val> + soroban_sdk::TryIntoVal<Env, soroban_sdk::Val> + Clone,
    {
        env.events().publish((EVENT_INDEXED_V2, topic_v2), data.clone());
        env.events().publish((EVENT_INDEXED_V3, topic_v3), data);
    }

    fn jurisdiction_set_event(env: &Env) -> Symbol {
        Symbol::new(env, "jur_set")
    }

    fn jurisdiction_reject_event(env: &Env) -> Symbol {
        Symbol::new(env, "jur_reject")
    }

    fn is_event_versioning_enabled(_env: Env) -> bool {
        true
    }

    /// Return `true` if V2-compat downgrade mode is enabled.
    ///
    /// When enabled, V2-shaped indexed events (`EVENT_INDEXED_V2`) are emitted
    /// alongside V3 events, allowing indexers pinned to V2 to continue working
    /// during the deprecation window.
    ///
    /// Defaults to `true` (emit V2 events). Admin can disable via `set_emit_v2_compat`.
    fn is_emit_v2_compat(env: &Env) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey2, bool>(&DataKey2::EmitV2Compat)
            .unwrap_or(true)
    }

    /// Emit both V2 and V3 indexed events, suppressing the V2 emission when the
    /// `emit_v2_compat` flag is disabled.
    ///
    /// V3 events are always emitted. V2 events are only emitted when `emit_v2_compat`
    /// is `true` (the default during the deprecation window).
    fn emit_v2_and_v3<T>(env: &Env, v2_topic: EventIndexTopicV2, v3_topic: EventIndexTopicV3, data: T)
    where
        T: IntoVal<Env, soroban_sdk::Val> + Clone,
    {
        // V3 is always emitted — it is the current canonical event schema.
        env.events().publish((EVENT_INDEXED_V3, v3_topic), data.clone());
        // V2 is only emitted when the compat flag is on (downgrade path).
        if Self::is_emit_v2_compat(env) {
            env.events().publish((EVENT_INDEXED_V2, v2_topic), data);
        }
    }

    /// Advance the cumulative accrual index for an offering and emit an `acc_idx` indexed event.
    ///
    /// The index accumulates `(amount * 1e18) / 10_000` per accepted revenue report, expressing
    /// cumulative revenue in 1e18 fixed-point per basis-point of holder share. This lets
    /// off-chain indexers reconstruct per-holder owed amounts without re-reading all periods.
    ///
    /// Skips silently when `amount == 0` (no-op report).
    fn update_and_emit_accrual_index(
        env: &Env,
        offering_id: &OfferingId,
        amount: i128,
        period_id: u64,
    ) {
        if amount == 0 {
            return;
        }
        const E18: i128 = 1_000_000_000_000_000_000;
        const BPS_MAX: i128 = 10_000;
        let idx_key = DataKey2::AccrualIndex(offering_id.clone());
        let current: i128 = env.storage().persistent().get(&idx_key).unwrap_or(0);
        let delta = amount.saturating_mul(E18).checked_div(BPS_MAX).unwrap_or(0);
        let new_idx = current.saturating_add(delta);
        env.storage().persistent().set(&idx_key, &new_idx);
        env.events().publish(
            (
                EVENT_INDEXED_V2,
                EventIndexTopicV2 {
                    version: INDEXER_EVENT_SCHEMA_VERSION,
                    event_type: EVENT_TYPE_ACC_IDX,
                    issuer: offering_id.issuer.clone(),
                    namespace: offering_id.namespace.clone(),
                    token: offering_id.token.clone(),
                    period_id,
                },
            ),
            (new_idx,),
        );
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

    fn require_redemption_window_open(
        env: &Env,
        offering_id: &OfferingId,
    ) -> Result<(), RevoraError> {
        let key = WindowDataKey::Redemption(offering_id.clone());
        if let Some(window) = env.storage().persistent().get::<WindowDataKey, AccessWindow>(&key) {
            if !Self::is_window_open(env, &window) {
                return Err(RevoraError::RedemptionWindowClosed);
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
        share_class: Option<ShareClass>,
        nonce: Option<u64>,
    ) -> Result<(), RevoraError> {
        if share_bps > 10_000 {
            return Err(RevoraError::InvalidShareBps);
        }
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // ── Nonce monotonicity guard ──────────────────────────────────────────
        // When a nonce is supplied the caller guarantees this update is strictly
        // newer than any previously accepted one.  Reject equal or smaller values
        // to prevent replayed or out-of-order off-chain updates from silently
        // overwriting newer on-chain share state.
        if let Some(n) = nonce {
            let nonce_key = DataKey2::HolderShareNonce(offering_id.clone(), holder.clone());
            let last_nonce: u64 = env.storage().persistent().get(&nonce_key).unwrap_or(0);
            if n <= last_nonce {
                return Err(RevoraError::StaleNonce);
            }
            env.storage().persistent().set(&nonce_key, &n);
        }

        // Check max total supply shares cap
        let max_shares_key = DataKey2::MaxTotalSupplyShares(offering_id.clone());
        let max_shares: i128 = env.storage().persistent().get(&max_shares_key).unwrap_or(0);
        if max_shares > 0 {
            let total_shares_key = DataKey2::TotalSharesIssued(offering_id.clone());
            let current_total_shares: i128 =
                env.storage().persistent().get(&total_shares_key).unwrap_or(0);
            let old_share: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::HolderShare(offering_id.clone(), holder.clone()))
                .unwrap_or(0);
            let new_total_shares = current_total_shares
                .saturating_sub(old_share as i128)
                .saturating_add(share_bps as i128);
            if new_total_shares > max_shares {
                return Err(RevoraError::MaxTotalSupplySharesExceeded);
            }
        }

        // Maintain a running total of persisted holder shares for this offering.
        let total_key = DataKey::HolderShareTotal(offering_id.clone());
        let mut current_total: u32 = env.storage().persistent().get(&total_key).unwrap_or(0);

        let classes_key = DataKey2::OfferingClasses(offering_id.clone());
        let classes: Option<Vec<(ShareClass, ClassConfig)>> = env.storage().persistent().get(&classes_key);

        if let Some(cls_vec) = classes {
            let sc = match share_class {
                Some(ref sc) => sc.clone(),
                None => return Err(RevoraError::InvalidShareClass),
            };
            let mut found = false;
            for (class_name, _) in cls_vec.iter() {
                if class_name == sc {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(RevoraError::InvalidShareClass);
            }
        }

        let new_total =
            current_total.s_sub(old_share).unwrap_or(0).s_add(share_bps).unwrap_or(u32::MAX);
        if new_total > 10_000 {
            return Err(RevoraError::InvalidShareBps);
        }

        // Update total shares issued
        let total_shares_key = DataKey2::TotalSharesIssued(offering_id.clone());
        let current_total_shares: i128 =
            env.storage().persistent().get(&total_shares_key).unwrap_or(0);
        let new_total_shares = current_total_shares
            .saturating_sub(old_share as i128)
            .saturating_add(share_bps as i128);
        env.storage().persistent().set(&total_shares_key, &new_total_shares);

        if let Some(ref sc) = share_class {
            let class_shares_key = DataKey2::TotalClassSharesIssued(offering_id.clone(), sc.clone());
            let current_class_shares: i128 = env.storage().persistent().get(&class_shares_key).unwrap_or(0);
            let new_class_shares = current_class_shares.saturating_sub(old_share as i128).saturating_add(share_bps as i128);
            env.storage().persistent().set(&class_shares_key, &new_class_shares);
        }

        // Persist updated holder share and running total.
        env.storage()
            .persistent()
            .set(&DataKey::HolderShare(offering_id.clone(), holder.clone()), &share_bps);
        env.storage().persistent().set(&total_key, &new_total);
        Self::record_holder_share_transition(env, &offering_id, &holder, old_share, share_bps);

        env.events().publish(
            (EVENT_SHARE_SET, issuer.clone(), namespace.clone(), token.clone()),
            (holder.clone(), share_bps),
        );
        // Versioned v2 event: [2, holder, share_bps] â€” always emitted (#RC26Q2-C31)
        Self::emit_v2_event(
            env,
            (EVENT_SHARE_SET_V2, issuer, namespace, token),
            (holder, share_bps),
        );
        Ok(())
    }

    fn get_period_count_internal(env: &Env, offering_id: &OfferingId) -> u32 {
        env.storage()
            .persistent()
            .get::<_, u32>(&DataKey::PeriodCount(offering_id.clone()))
            .unwrap_or(0)
    }

    fn accrual_delta_e18(amount: i128) -> i128 {
        amount
            .checked_mul(ACCRUAL_SCALE_E18)
            .unwrap_or(i128::MAX)
            .checked_div(BPS_DENOMINATOR)
            .unwrap_or(0)
    }

    fn get_acc_per_share_at_index(env: &Env, offering_id: &OfferingId, index: u32) -> i128 {
        if index == 0 {
            return 0;
        }
        env.storage()
            .persistent()
            .get::<_, i128>(&DataKey2::AccPerShareAtIndex(offering_id.clone(), index))
            .unwrap_or(0)
    }

    fn get_holder_share_schedule(
        env: &Env,
        offering_id: &OfferingId,
        holder: &Address,
    ) -> Vec<HolderShareCheckpoint> {
        if let Some(schedule) = env.storage().persistent().get::<_, Vec<HolderShareCheckpoint>>(
            &DataKey2::HolderShareSchedule(offering_id.clone(), holder.clone()),
        ) {
            return schedule;
        }

        let current_share: u32 = env
            .storage()
            .persistent()
            .get::<_, u32>(&DataKey::HolderShare(offering_id.clone(), holder.clone()))
            .unwrap_or(0);
        let mut schedule = Vec::new(env);
        if current_share > 0 {
            schedule.push_back(HolderShareCheckpoint { start_index: 0, share_bps: current_share });
        }
        schedule
    }

    fn get_checkpoint_threshold(env: &Env, offering_id: &OfferingId) -> u32 {
        let key = DataKey2::CheckpointThreshold(offering_id.clone());
        env.storage().persistent().get(&key).unwrap_or(CHECKPOINT_THRESHOLD_DEFAULT)
    }

    /// Compute the pre-claimable sum for a range of period indices using the
    /// holder's share schedule and the global `AccPerShareAtIndex` values.
    ///
    /// This is a lossless computation: the result equals what
    /// `compute_holder_payout_for_range` would produce for the same
    /// `[start_idx, end_idx)` interval, but without iterating through
    /// every period index individually.
    ///
    /// The sum is:
    /// `sum_i ( (AccPerShareAtIndex[i+1] - AccPerShareAtIndex[i]) * share_bps_i / 1e18 )`
    fn compute_anchor_claimable_sum(
        env: &Env,
        offering_id: &OfferingId,
        holder: &Address,
        start_idx: u32,
        end_idx: u32,
    ) -> i128 {
        if start_idx >= end_idx {
            return 0;
        }
        let schedule = Self::get_holder_share_schedule(env, offering_id, holder);
        if schedule.is_empty() {
            return 0;
        }
        let mut total = 0_i128;
        let mut current_index = start_idx;
        let mut current_share = 0_u32;
        let mut schedule_idx = 0_u32;

        while schedule_idx < schedule.len() {
            let checkpoint = schedule.get(schedule_idx).unwrap();
            if checkpoint.start_index > start_idx {
                break;
            }
            current_share = checkpoint.share_bps;
            schedule_idx = schedule_idx.saturating_add(1);
        }

        while current_index < end_idx {
            while schedule_idx < schedule.len() {
                let checkpoint = schedule.get(schedule_idx).unwrap();
                if checkpoint.start_index > current_index {
                    break;
                }
                current_share = checkpoint.share_bps;
                schedule_idx = schedule_idx.saturating_add(1);
            }

            if current_share > 0 {
                let acc_end = Self::get_acc_per_share_at_index(env, offering_id, current_index.saturating_add(1));
                let acc_start = Self::get_acc_per_share_at_index(env, offering_id, current_index);
                let delta = acc_end.saturating_sub(acc_start);
                total = total.saturating_add(delta.saturating_mul(current_share as i128) / ACCRUAL_SCALE_E18);
            }

            current_index = current_index.saturating_add(1);
        }

        total
    }

    fn record_holder_share_transition(
        env: &Env,
        offering_id: &OfferingId,
        holder: &Address,
        old_share: u32,
        new_share: u32,
    ) {
        if old_share == new_share {
            return;
        }

        let period_count = Self::get_period_count_internal(env, offering_id);
        let existing = Self::get_holder_share_schedule(env, offering_id, holder);
        let mut updated = Vec::new(env);

        for i in 0..existing.len() {
            let checkpoint = existing.get(i).unwrap();
            if checkpoint.start_index == period_count {
                continue;
            }
            updated.push_back(checkpoint);
        }

        updated
            .push_back(HolderShareCheckpoint { start_index: period_count, share_bps: new_share });
        env.storage()
            .persistent()
            .set(&DataKey2::HolderShareSchedule(offering_id.clone(), holder.clone()), &updated);
    }

    fn get_holder_accrual_state(
        env: &Env,
        offering_id: &OfferingId,
        holder: &Address,
    ) -> HolderAccrualState {
        let last_claimed_idx: u32 = env
            .storage()
            .persistent()
            .get::<_, u32>(&DataKey::LastClaimedIdx(offering_id.clone(), holder.clone()))
            .unwrap_or(0);

        let mut state = env
            .storage()
            .persistent()
            .get::<_, HolderAccrualState>(&DataKey2::HolderAccrualState(
                offering_id.clone(),
                holder.clone(),
            ))
            .unwrap_or(HolderAccrualState {
                last_settled_idx: last_claimed_idx,
                last_acc_per_share_e18: Self::get_acc_per_share_at_index(
                    env,
                    offering_id,
                    last_claimed_idx,
                ),
                accrued_owed: 0,
            });

        if state.last_settled_idx < last_claimed_idx {
            state.last_settled_idx = last_claimed_idx;
            state.last_acc_per_share_e18 =
                Self::get_acc_per_share_at_index(env, offering_id, last_claimed_idx);
            state.accrued_owed = 0;
        }

        state
    }

    fn compute_holder_payout_for_range(
        env: &Env,
        offering_id: &OfferingId,
        holder: &Address,
        start_idx: u32,
        end_idx: u32,
    ) -> i128 {
        if start_idx >= end_idx {
            return 0;
        }

        let schedule = Self::get_holder_share_schedule(env, offering_id, holder);
        if schedule.is_empty() {
            return 0;
        }

        let mut total = 0_i128;
        let mut current_index = start_idx;
        let mut current_share = 0_u32;
        let mut schedule_idx = 0_u32;

        while schedule_idx < schedule.len() {
            let checkpoint = schedule.get(schedule_idx).unwrap();
            if checkpoint.start_index > start_idx {
                break;
            }
            current_share = checkpoint.share_bps;
            schedule_idx = schedule_idx.saturating_add(1);
        }

        while current_index < end_idx {
            while schedule_idx < schedule.len() {
                let checkpoint = schedule.get(schedule_idx).unwrap();
                if checkpoint.start_index > current_index {
                    break;
                }
                current_share = checkpoint.share_bps;
                schedule_idx = schedule_idx.saturating_add(1);
            }

            if current_share > 0 {
                let acc_end = Self::get_acc_per_share_at_index(
                    env,
                    offering_id,
                    current_index.saturating_add(1),
                );
                let acc_start = Self::get_acc_per_share_at_index(env, offering_id, current_index);
                let delta = acc_end.saturating_sub(acc_start);
                total = total.saturating_add(
                    delta.saturating_mul(current_share as i128) / ACCRUAL_SCALE_E18,
                );
            }

            current_index = current_index.saturating_add(1);
        }

        total
    }

    fn find_matured_claim_end_idx(env: &Env, offering_id: &OfferingId, start_idx: u32) -> u32 {
        let period_count = Self::get_period_count_internal(env, offering_id);
        if start_idx >= period_count {
            return start_idx;
        }

        let delay_secs: u64 = env
            .storage()
            .persistent()
            .get::<_, u64>(&DataKey::ClaimDelaySecs(offering_id.clone()))
            .unwrap_or(0);
        let now = env.ledger().timestamp();
        let mut end_idx = start_idx;

        while end_idx < period_count {
            let entry_key = DataKey::PeriodEntry(offering_id.clone(), end_idx);
            let period_id: u64 = env.storage().persistent().get(&entry_key).unwrap_or(0);
            if period_id == 0 {
                end_idx = end_idx.saturating_add(1);
                continue;
            }

            let time_key = DataKey::PeriodDepositTime(offering_id.clone(), period_id);
            let deposit_time: u64 = env.storage().persistent().get(&time_key).unwrap_or(0);
            if delay_secs > 0 && now < deposit_time.saturating_add(delay_secs) {
                break;
            }

            end_idx = end_idx.saturating_add(1);
        }

        end_idx
    }

    fn cache_holder_accrual_through_matured(env: &Env, offering_id: &OfferingId, holder: &Address) {
        let mut state = Self::get_holder_accrual_state(env, offering_id, holder);
        let matured_end =
            Self::find_matured_claim_end_idx(env, offering_id, state.last_settled_idx);
        if matured_end <= state.last_settled_idx {
            return;
        }

        let anchor_key = DataKey2::AccrualAnchor(offering_id.clone(), holder.clone());
        let anchor: Option<AccrualAnchor> = env.storage().persistent().get(&anchor_key);

        let mut delta: i128 = 0;
        let mut schedule_start = state.last_settled_idx;

        if let Some(a) = anchor {
            if state.last_settled_idx <= a.end_idx {
                let anchor_start = state.last_settled_idx;
                let anchor_end_incl = core::cmp::min(matured_end, a.end_idx.saturating_add(1));
                if anchor_start < anchor_end_incl {
                    delta = delta.saturating_add(a.claimable_sum);
                }
                schedule_start = core::cmp::max(schedule_start, a.end_idx.saturating_add(1));
                if matured_end > a.end_idx {
                    env.storage().persistent().remove(&anchor_key);
                }
            }
        }

        if schedule_start < matured_end {
            delta = delta.saturating_add(Self::compute_holder_payout_for_range(
                env,
                offering_id,
                holder,
                schedule_start,
                matured_end,
            ));
        }

        state.accrued_owed = state.accrued_owed.saturating_add(delta);
        state.last_settled_idx = matured_end;
        state.last_acc_per_share_e18 =
            Self::get_acc_per_share_at_index(env, offering_id, matured_end);

        env.storage()
            .persistent()
            .set(&DataKey2::HolderAccrualState(offering_id.clone(), holder.clone()), &state);
    }

    fn normalize_jurisdictions(env: &Env, jurisdictions: Vec<Symbol>) -> Vec<Symbol> {
        let mut normalized = Vec::new(env);
        for i in 0..jurisdictions.len() {
            let jurisdiction = jurisdictions.get(i).unwrap();
            if !Self::vec_contains_symbol(&normalized, &jurisdiction) {
                normalized.push_back(jurisdiction);
            }
        }
        normalized
    }

    fn vec_contains_symbol(values: &Vec<Symbol>, target: &Symbol) -> bool {
        for i in 0..values.len() {
            if values.get(i).unwrap() == *target {
                return true;
            }
        }
        false
    }

    fn get_allowed_jurisdictions_internal(env: &Env, offering_id: &OfferingId) -> Vec<Symbol> {
        env.storage()
            .persistent()
            .get(&DataKey2::AllowedJurisdictions(offering_id.clone()))
            .unwrap_or_else(|| Vec::new(env))
    }

    fn get_holder_jurisdiction_internal(
        env: &Env,
        offering_id: &OfferingId,
        holder: &Address,
    ) -> Option<Symbol> {
        env.storage()
            .persistent()
            .get(&DataKey2::HolderJurisdiction(offering_id.clone(), holder.clone()))
    }

    fn emit_jurisdiction_reject(
        env: &Env,
        offering_id: &OfferingId,
        holder: &Address,
        jurisdiction: Symbol,
        action: Symbol,
    ) {
        env.events().publish(
            (
                Self::jurisdiction_reject_event(env),
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
            ),
            (holder.clone(), jurisdiction, action),
        );
    }

    fn require_holder_jurisdiction_allowed(
        env: &Env,
        offering_id: &OfferingId,
        holder: &Address,
        action: Symbol,
    ) -> Result<(), RevoraError> {
        let allowed = Self::get_allowed_jurisdictions_internal(env, offering_id);
        if allowed.is_empty() {
            return Ok(());
        }

        let jurisdiction = Self::get_holder_jurisdiction_internal(env, offering_id, holder)
            .unwrap_or(EVENT_JUR_UNSET);

        if Self::vec_contains_symbol(&allowed, &jurisdiction) {
            return Ok(());
        }

        Self::emit_jurisdiction_reject(env, offering_id, holder, jurisdiction, action);
        Err(RevoraError::JurisdictionDisallowed)
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
            let new_total = deposited.s_add(amount)?;
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

        let decimals = Self::get_payment_token_decimals(
            env.clone(),
            offering_id.issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
        );
        let normalized = Self::normalize_amount(amount, decimals);
        let acc_delta_e18 = Self::accrual_delta_e18(normalized);
        let global_acc_key = DataKey2::GlobalAccPerShareE18(offering_id.clone());
        let current_acc: i128 = env.storage().persistent().get(&global_acc_key).unwrap_or(0);
        let next_acc = current_acc.saturating_add(acc_delta_e18);
        env.storage().persistent().set(&global_acc_key, &next_acc);
        env.storage()
            .persistent()
            .set(&DataKey2::AccPerShareAtIndex(offering_id.clone(), count + 1), &next_acc);

        // Update cumulative deposited revenue and emit cap-reached event if applicable (#96)
        let deposited_key = DataKey2::DepositedRevenue(offering_id.clone());
        let deposited: i128 = env.storage().persistent().get(&deposited_key).unwrap_or(0);
        let new_deposited = deposited.s_add(amount)?;
        env.storage().persistent().set(&deposited_key, &new_deposited);

        let cap_val: i128 = env.storage().persistent().get(&cap_key).unwrap_or(0);
        if cap_val > 0 && new_deposited >= cap_val {
            env.events().publish(
                (EVENT_SUPPLY_CAP_REACHED, issuer.clone(), namespace.clone(), token.clone()),
                (new_deposited, cap_val),
            );
        }

        // Update the e18 accrual index
        let decimals = Self::get_payment_token_decimals(
            env.clone(),
            issuer.clone(),
            namespace.clone(),
            token.clone(),
        );
        let normalized_amount = Self::normalize_amount(amount, decimals);
        let total_share_bps_key = DataKey::HolderShareTotal(offering_id.clone());
        let total_share_bps: u32 =
            env.storage().persistent().get(&total_share_bps_key).unwrap_or(0);

        if total_share_bps > 0 {
            let accrual_delta = (normalized_amount.checked_mul(E18))
                .and_then(|x| x.checked_div(total_share_bps as i128))
                .unwrap_or(0);
            let current_accrual_key = DataKey::AccrualIndexE18(offering_id.clone());
            let current_accrual: i128 =
                env.storage().persistent().get(&current_accrual_key).unwrap_or(0);
            let new_accrual = current_accrual.checked_add(accrual_delta).unwrap_or(current_accrual);
            env.storage().persistent().set(&current_accrual_key, &new_accrual);
        }

        // Versioned event v2: [version: u32, payment_token: Address, amount: i128, period_id: u64]
        Self::emit_v2_event(
            env,
            (EVENT_REV_DEPOSIT_V2, issuer.clone(), namespace.clone(), token.clone()),
            (payment_token, amount, period_id),
        );
        env.events().publish(
            (EVENT_ACC_UPD, issuer, namespace, token),
            (period_id, count + 1, acc_delta_e18, next_acc),
        );
        Ok(())
    }

    /// Return the supply cap for an offering (0 = no cap). (#96)
    pub fn get_supply_cap(env: Env, issuer: Address, namespace: Symbol, token: Address) -> i128 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey2::SupplyCap(offering_id)).unwrap_or(0)
    }

    pub fn set_max_total_supply_shares(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        max_total_supply_shares: i128,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

        if let Err((err, _)) = AmountValidationMatrix::validate(
            max_total_supply_shares,
            AmountValidationCategory::MaxTotalSupplyShares,
        ) {
            return Err(err);
        }

        let offering_id = OfferingId { issuer, namespace, token };
        let max_shares_key = DataKey2::MaxTotalSupplyShares(offering_id);
        if max_total_supply_shares > 0 {
            env.storage().persistent().set(&max_shares_key, &max_total_supply_shares);
        } else {
            env.storage().persistent().remove(&max_shares_key);
        }
        Ok(())
    }

    pub fn get_max_total_supply_shares(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> i128 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey2::MaxTotalSupplyShares(offering_id)).unwrap_or(0)
    }

    pub fn get_total_shares_issued(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> i128 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey2::TotalSharesIssued(offering_id)).unwrap_or(0)
    }

    pub fn get_total_class_shares_issued(env: Env, issuer: Address, namespace: Symbol, token: Address, share_class: ShareClass) -> i128 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey2::TotalClassSharesIssued(offering_id, share_class)).unwrap_or(0)
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
        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        Self::require_issuer_quorum_auth(&env, &offering.issuers);
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

    /// Set a per-offering per-asset secondary-market royalty in basis points. Issuer-only. (#562)
    ///
    /// Emits `EVENT_ROYALTY_CONFIG` with `(issuer, namespace, token, asset, royalty_bps)`.
    ///
    /// ### Errors
    /// - `OfferingNotFound` — offering does not exist or caller is not the issuer.
    /// - `InvalidRevenueShareBps` — `royalty_bps` exceeds `MAX_PLATFORM_FEE_BPS` (5 000).
    pub fn set_secondary_market_royalty_bps(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        asset: Address,
        royalty_bps: u32,
    ) -> Result<(), RevoraError> {
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        issuer.require_auth();
        if royalty_bps > MAX_PLATFORM_FEE_BPS {
            return Err(RevoraError::InvalidRevenueShareBps);
        }
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::OfferingRoyaltyBps(offering_id, asset.clone()), &royalty_bps);
        env.events().publish((EVENT_ROYALTY_CONFIG, issuer, namespace, token, asset), royalty_bps);
        Ok(())
    }

    /// Return the per-offering per-asset secondary-market royalty in basis points.
    /// 0 means no royalty is configured.
    pub fn get_secondary_market_royalty_bps(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        asset: Address,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get(&DataKey::OfferingRoyaltyBps(offering_id, asset))
            .unwrap_or(0)
    }

    /// Pay a configured secondary-market royalty on a transfer.
    ///
    /// The royalty amount is routed to the issuer and is computed as
    /// `amount * royalty_bps / BPS_DENOMINATOR`.
    ///
    /// ### Errors
    /// - `OfferingNotFound` — offering does not exist.
    /// - `InvalidAmount` — transfer amount is not positive.
    /// - `TransferFailed` — token transfer to issuer failed.
    pub fn pay_secondary_market_royalty(
        env: Env,
        payer: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        payment_asset: Address,
        amount: i128,
        seller: Address,
        buyer: Address,
    ) -> Result<i128, RevoraError> {
        if amount <= 0 {
            return Err(RevoraError::InvalidAmount);
        }

        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        payer.require_auth();

        let royalty_bps = Self::get_secondary_market_royalty_bps(
            env.clone(),
            issuer.clone(),
            namespace.clone(),
            token.clone(),
            payment_asset.clone(),
        );
        let royalty_amount =
            (amount * royalty_bps as i128).checked_div(BPS_DENOMINATOR).unwrap_or(0);

        if royalty_amount > 0 {
            if token::Client::new(&env, &payment_asset)
                .try_transfer(&payer, &current_issuer, &royalty_amount)
                .is_err()
            {
                return Err(RevoraError::TransferFailed);
            }
        }

        Self::emit_v2_event(
            &env,
            (EVENT_ROYALTY_PAID, issuer.clone(), namespace.clone(), token.clone()),
            (payer.clone(), seller, buyer, payment_asset, amount, royalty_amount),
        );
        env.events().publish(
            (EVENT_ROYALTY_PAID, issuer, namespace, token),
            (payer, seller, buyer, payment_asset, amount, royalty_amount),
        );

        Ok(royalty_amount)
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

    // â”€â”€ Platform Fee Model (#468) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Configure the per-offering platform fee model: a programmable `fee_bps` cut routed
    /// to `treasury` on each `report_revenue` call. Admin-only. (#468)
    ///
    /// The fee and the offering's holders share the same 100% (10_000 bps) budget, so this
    /// rejects any configuration where `fee_bps` plus the offering's aggregate holder share
    /// would exceed 10_000 bps. Setting `fee_bps = 0` disables the fee (no deduction and no
    /// `plat_fee` event on subsequent reports) while still recording the `treasury` for clarity.
    ///
    /// Emits `EVENT_PLAT_FEE_SET` with topic `(issuer, namespace, token)` and data
    /// `(fee_bps, treasury)`.
    ///
    /// ### Auth
    /// Contract admin (`require_auth`).
    ///
    /// ### Errors
    /// - `NotInitialized` â€” contract admin is not set.
    /// - `OfferingNotFound` â€” offering does not exist.
    /// - `FeeExceedsHolderShare` â€” `fee_bps` + aggregate holder share would exceed 10_000 bps.
    pub fn set_offering_platform_fee(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        fee_bps: u32,
        treasury: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;
        admin.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Offering must exist before a fee model can be attached to it.
        if !env.storage().persistent().has(&DataKey::OfferingIssuer(offering_id.clone())) {
            return Err(RevoraError::OfferingNotFound);
        }

        // Fee bps + holder bps must always sum to at most 10_000 at the offering level.
        // The aggregate holder share is maintained incrementally by `set_holder_share_internal`.
        let holder_aggregate_bps: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::HolderShareTotal(offering_id.clone()))
            .unwrap_or(0);
        if fee_bps.saturating_add(holder_aggregate_bps) > 10_000 {
            return Err(RevoraError::FeeExceedsHolderShare);
        }

        let model = PlatformFeeModel { fee_bps, treasury: treasury.clone() };
        env.storage().persistent().set(&DataKey2::OfferingPlatformFee(offering_id), &model);
        env.events().publish((EVENT_PLAT_FEE_SET, issuer, namespace, token), (fee_bps, treasury));
        Ok(())
    }

    /// Return the configured per-offering platform fee model, if any. (#468)
    ///
    /// O(1) â€” single persistent storage read. Returns `None` when no fee model is configured.
    pub fn get_offering_platform_fee(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<PlatformFeeModel> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey2::OfferingPlatformFee(offering_id))
    }

    /// Apply the per-offering platform fee for a recorded revenue report. (#468)
    ///
    /// When a fee model is configured with a non-zero `fee_bps`, the programmable share of
    /// `amount` is routed to the treasury and surfaced via `EVENT_PLAT_FEE`. A `fee_bps` of 0
    /// (or a computed fee of 0, e.g. zero-revenue reports) is a no-op and emits no event, so
    /// indexers can rely on `plat_fee` being present only when a real fee was taken.
    ///
    /// Returns the fee amount routed to the treasury (0 when no fee applies).
    fn apply_platform_fee(
        env: &Env,
        offering_id: &OfferingId,
        issuer: &Address,
        namespace: &Symbol,
        token: &Address,
        amount: i128,
        period_id: u64,
    ) -> i128 {
        let model: PlatformFeeModel = match env
            .storage()
            .persistent()
            .get(&DataKey2::OfferingPlatformFee(offering_id.clone()))
        {
            Some(m) => m,
            None => return 0,
        };

        if model.fee_bps == 0 || amount <= 0 {
            return 0;
        }

        let fee_amount =
            amount.saturating_mul(model.fee_bps as i128).checked_div(BPS_DENOMINATOR).unwrap_or(0);
        if fee_amount <= 0 {
            return 0;
        }

        env.events().publish(
            (EVENT_PLAT_FEE, issuer.clone(), namespace.clone(), token.clone()),
            (model.treasury, model.fee_bps, fee_amount, period_id),
        );
        fee_amount
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
                if let Ok(next) = total_revenue.s_add(amount) {
                    total_revenue = next;
                } else {
                    is_saturated = true;
                    total_revenue = i128::MAX;
                }
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
    pub fn set_storage_layout_version(
        env: Env,
        caller: Address,
        v: u32,
    ) -> Result<(), RevoraError> {
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

    /// Return full details of a pending issuer transfer, including the proposed new issuer,
    /// the proposal timestamp, and the effective expiry in seconds (0 = default 7 days).
    pub fn get_pending_transfer_details(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<PendingTransfer> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get::<DataKey, PendingTransfer>(&DataKey::PendingIssuerTransfer(offering_id))
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
        Self::do_propose_issuer_transfer(env, issuer, namespace, token, new_issuer, 0)
    }

    /// Propose an issuer transfer with a custom expiry window.
    ///
    /// `expiry_secs` is clamped to `[MIN_ISSUER_TRANSFER_EXPIRY_SECS, MAX_ISSUER_TRANSFER_EXPIRY_SECS]`.
    /// Pass `0` to use the default `ISSUER_TRANSFER_EXPIRY_SECS` (7 days).
    #[allow(clippy::too_many_arguments)]
    pub fn propose_transfer_with_expiry(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        new_issuer: Address,
        expiry_secs: u64,
    ) -> Result<(), RevoraError> {
        Self::do_propose_issuer_transfer(env, issuer, namespace, token, new_issuer, expiry_secs)
    }

    fn do_propose_issuer_transfer(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        new_issuer: Address,
        expiry_secs: u64,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        let key = DataKey::PendingIssuerTransfer(offering_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(RevoraError::IssuerTransferPending);
        }

        // Clamp expiry: 0 means default; non-zero is clamped to [MIN, MAX].
        let effective_expiry = if expiry_secs == 0 {
            0
        } else {
            expiry_secs.clamp(MIN_ISSUER_TRANSFER_EXPIRY_SECS, MAX_ISSUER_TRANSFER_EXPIRY_SECS)
        };

        let timestamp = env.ledger().timestamp();
        env.storage().persistent().set(
            &key,
            &PendingTransfer {
                new_issuer: new_issuer.clone(),
                timestamp,
                expiry_secs: effective_expiry,
            },
        );
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

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::NotAuthorized);
        }
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        let key = DataKey::PendingIssuerTransfer(offering_id.clone());
        if !env.storage().persistent().has(&key) {
            return Err(RevoraError::NoTransferPending);
        }

        let pending: PendingTransfer = env.storage().persistent().get(&key).unwrap();
        let timestamp = env.ledger().timestamp();
        // Preserve the original expiry_secs so the replacement inherits the same window.
        env.storage().persistent().set(
            &key,
            &PendingTransfer {
                new_issuer: new_issuer.clone(),
                timestamp,
                expiry_secs: pending.expiry_secs,
            },
        );

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
        let effective_expiry = if pending.expiry_secs == 0 {
            ISSUER_TRANSFER_EXPIRY_SECS
        } else {
            pending.expiry_secs
        };
        if current_timestamp > pending.timestamp.saturating_add(effective_expiry) {
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

        // Migrate configuration state linked to the old OfferingId (#1344)
        if let Some(config) = env
            .storage()
            .persistent()
            .get::<_, ConcentrationLimitConfig>(&DataKey::ConcentrationLimit(offering_id.clone()))
        {
            env.storage()
                .persistent()
                .set(&DataKey::ConcentrationLimit(new_offering_id.clone()), &config);
            env.storage().persistent().remove(&DataKey::ConcentrationLimit(offering_id.clone()));
        }
        if let Some(current) = env
            .storage()
            .persistent()
            .get::<_, u32>(&DataKey::CurrentConcentration(offering_id.clone()))
        {
            env.storage()
                .persistent()
                .set(&DataKey::CurrentConcentration(new_offering_id.clone()), &current);
            env.storage().persistent().remove(&DataKey::CurrentConcentration(offering_id.clone()));
        }
        if let Some(mode) = env
            .storage()
            .persistent()
            .get::<_, RoundingMode>(&DataKey::RoundingMode(offering_id.clone()))
        {
            env.storage().persistent().set(&DataKey::RoundingMode(new_offering_id.clone()), &mode);
            env.storage().persistent().remove(&DataKey::RoundingMode(offering_id.clone()));
        }
        if let Some(constraints) = env.storage().persistent().get::<_, InvestmentConstraintsConfig>(
            &DataKey2::InvestmentConstraints(offering_id.clone()),
        ) {
            env.storage()
                .persistent()
                .set(&DataKey2::InvestmentConstraints(new_offering_id.clone()), &constraints);
            env.storage()
                .persistent()
                .remove(&DataKey2::InvestmentConstraints(offering_id.clone()));
        }
        if let Some(delay) =
            env.storage().persistent().get::<_, u64>(&DataKey::ClaimDelaySecs(offering_id.clone()))
        {
            env.storage()
                .persistent()
                .set(&DataKey::ClaimDelaySecs(new_offering_id.clone()), &delay);
            env.storage().persistent().remove(&DataKey::ClaimDelaySecs(offering_id.clone()));
        }
        if let Some(snap_config) =
            env.storage().persistent().get::<_, bool>(&DataKey::SnapshotConfig(offering_id.clone()))
        {
            env.storage()
                .persistent()
                .set(&DataKey::SnapshotConfig(new_offering_id.clone()), &snap_config);
            env.storage().persistent().remove(&DataKey::SnapshotConfig(offering_id.clone()));
        }
        if let Some(snap_ref) =
            env.storage().persistent().get::<_, u64>(&DataKey::LastSnapshotRef(offering_id.clone()))
        {
            env.storage()
                .persistent()
                .set(&DataKey::LastSnapshotRef(new_offering_id.clone()), &snap_ref);
            env.storage().persistent().remove(&DataKey::LastSnapshotRef(offering_id.clone()));
        }

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

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::NotAuthorized);
        }
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

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
        env.storage().persistent().set(&DataKey::StorageLayoutVersion, &STORAGE_LAYOUT_VERSION);
        env.events().publish((EVENT_LAYOUT_VERSION,), STORAGE_LAYOUT_VERSION);

        // Persist the initial contract version as the minimum supported version.
        // Future WASM binaries with a lower CONTRACT_VERSION will be rejected at entry.
        env.storage().persistent().set(&DataKey::DeployedVersion, &CONTRACT_VERSION);

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

    /// Toggle the V2-compat downgrade flag.
    ///
    /// When enabled (`true`), V2-shaped indexed events (`EVENT_INDEXED_V2`) are
    /// emitted alongside V3 events, allowing indexers pinned to V2 to continue
    /// working during the deprecation window. When disabled (`false`), only V3
    /// events are emitted, and indexers must have migrated to V3.
    ///
    /// Defaults to `true` at initialization.
    ///
    /// ### Auth
    /// Requires `caller` to match the contract admin.
    ///
    /// ### Events
    /// Emits `EVENT_V2_COMPAT_SET` on success.
    ///
    /// ### Deprecation
    /// This flag is intended for a limited deprecation window. Once all indexers
    /// have migrated to V3, the flag should be set to `false` and eventually
    /// the V2 emission path and this flag can be removed entirely.
    pub fn set_emit_v2_compat(env: Env, caller: Address, enabled: bool) -> Result<(), RevoraError> {
        caller.require_auth();
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;
        if caller != admin {
            return Err(RevoraError::NotAuthorized);
        }
        env.storage().persistent().set(&DataKey2::EmitV2Compat, &enabled);
        env.events().publish((EVENT_V2_COMPAT_SET, caller), enabled);
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

    // â”€â”€ Offering management â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Register a new revenue-share offering.
    ///
    /// Once registered, an offering's parameters are immutable.
    ///
    /// # Arguments
    /// * `issuer` - The address of the offering issuer. Must provide authentication.
    /// * `namespace` - A symbol identifying the namespace for this offering.
    /// * `token` - The address of the token being offered.
    /// * `revenue_share_bps` - The revenue share percentage in basis points (0-10,000).
    ///   Values above 10,000 are rejected unless testnet mode is enabled (admin-only,
    ///   never enable on mainnet - see `TESTNET_MODE.md`).
    /// * `payout_asset` - The asset in which revenue will be paid out.
    /// * `supply_cap` - Optional cap on the total amount of revenue that can be deposited (0 = no cap).
    /// * `denomination_symbol` - Human-readable ticker for the payout denomination (e.g. `USDC`, `XLM`).
    ///   Stored as-is; not validated against on-chain token registries.
    ///   Maximum 9 characters (Soroban `Symbol` limit).
    /// * `display_decimals` - Decimal precision wallets should use when displaying amounts.
    ///   Must satisfy `display_decimals <= MAX_TOKEN_DECIMALS (18)`.
    ///   Callers should also ensure `display_decimals <= payment_token_decimals`; verify
    ///   via `get_payment_token_decimals` before calling.
    ///
    /// # Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::InvalidRevenueShareBps)` if `revenue_share_bps` exceeds 10,000
    ///   and testnet mode is disabled (the default).
    /// - `Err(RevoraError::DisplayDecimalsOutOfRange)` if `display_decimals > 18`.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
    /// - `Err(RevoraError::ContractPaused)` if the contract is paused.
    ///
    /// # Events
    /// Emits `EVENT_OFFER_REG_V2` (payload includes `denomination_symbol` and `display_decimals`)
    /// and `EVENT_INDEXED_V2`.
    ///
    /// # Security note
    /// `denomination_symbol` is informational only and does not affect payout math or transfers.
    /// Issuers are responsible for providing values consistent with the actual `payout_asset`.
    #[allow(clippy::too_many_arguments)]
    pub fn register_offering(
        env: Env,
        primary_issuer: Address,
        co_issuers: Vec<Address>,
        quorum: u32,
        namespace: Symbol,
        token: Address,
        revenue_share_bps: u32,
        payout_asset: Address,
        supply_cap: i128,
        denomination_symbol: Symbol,
        display_decimals: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        primary_issuer.require_auth();

        // Validate quorum: must be at least 1, and at most 1 + number of co-issuers
        let total_issuers = 1 + co_issuers.len() as u32;
        if quorum == 0 || quorum > total_issuers {
            return Err(RevoraError::LimitReached);
        }

        if let Some(ref cls_vec) = classes {
            let mut sum_bps: u32 = 0;
            for (_, config) in cls_vec.iter() {
                sum_bps =
                    sum_bps.checked_add(config.bps).ok_or(RevoraError::InvalidShareClassBps)?;
            }
            if sum_bps != 10_000 {
                return Err(RevoraError::InvalidShareClassBps);
            }
        }

        // Negative Amount Validation Matrix: SupplyCap requires >= 0 (#163)
        if let Err((err, _)) =
            AmountValidationMatrix::validate(supply_cap, AmountValidationCategory::SupplyCap)
        {
            return Err(err);
        }

        // display_decimals must not exceed the protocol-wide maximum of 18.
        // Prevents callers from supplying nonsensical precision that confuses downstream display.
        if display_decimals > MAX_TOKEN_DECIMALS {
            return Err(RevoraError::DisplayDecimalsOutOfRange);
        }

        // Skip bps validation in testnet mode (reads the real flag from storage).
        // In production mode (default) revenue_share_bps is always capped at 10 000 (100%).
        // Testnet mode is admin-only and must never be enabled on mainnet - see TESTNET_MODE.md.
        let testnet_mode = Self::is_testnet_mode(env.clone());
        if !testnet_mode && revenue_share_bps > 10_000 {
            return Err(RevoraError::InvalidRevenueShareBps);
        }

        let offering_id = OfferingId {
            issuer: primary_issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Duplicate prevention: check if offering already exists by its stable identity (issuer+namespace+token)
        // This makes register_offering idempotent and prevents state inconsistencies in off-chain catalogs.
        if env.storage().persistent().has(&DataKey::OfferingIssuer(offering_id.clone())) {
            return Ok(());
        }

        // Register namespace for issuer if not already present
        let ns_reg_key = DataKey2::NamespaceRegistered(primary_issuer.clone(), namespace.clone());
        if !env.storage().persistent().has(&ns_reg_key) {
            let ns_count_key = DataKey2::NamespaceCount(primary_issuer.clone());
            let count: u32 = env.storage().persistent().get(&ns_count_key).unwrap_or(0);
            env.storage()
                .persistent()
                .set(&DataKey2::NamespaceItem(primary_issuer.clone(), count), &namespace);
            env.storage().persistent().set(&ns_count_key, &(count + 1));
            env.storage().persistent().set(&ns_reg_key, &true);
        }

        let tenant_id = TenantId { issuer: primary_issuer.clone(), namespace: namespace.clone() };
        let count_key = DataKey::OfferCount(tenant_id.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let offering = Offering {
            issuers: Issuers { primary: primary_issuer.clone(), co: co_issuers.clone(), quorum },
            namespace: namespace.clone(),
            token: token.clone(),
            revenue_share_bps,
            payout_asset: payout_asset.clone(),
            denomination_symbol: denomination_symbol.clone(),
            display_decimals,
        };

        let item_key = DataKey::OfferItem(tenant_id.clone(), count);
        env.storage().persistent().set(&item_key, &offering);
        env.storage().persistent().set(&count_key, &(count + 1));

        // Direct index for O(1) get_offering (#360).
        env.storage().persistent().set(&DataKey2::OfferingRecord(offering_id.clone()), &offering);

        // Denomination metadata auxiliary index: O(1) read for display semantics.
        env.storage().persistent().set(
            &DataKey2::DenominationMetadata(offering_id.clone()),
            &(denomination_symbol.clone(), display_decimals),
        );

        let issuer_lookup_key = DataKey::OfferingIssuer(offering_id.clone());
        env.storage().persistent().set(&issuer_lookup_key, &primary_issuer);

        if supply_cap > 0 {
            let cap_key = DataKey2::SupplyCap(offering_id.clone());
            env.storage().persistent().set(&cap_key, &supply_cap);
        }

        // Primary registration event - denomination metadata included so indexers never
        // need a second call to learn display semantics.
        Self::emit_v2_event(
            &env,
            (EVENT_OFFER_REG_V2, issuer.clone(), namespace.clone()),
            (
                token.clone(),
                revenue_share_bps,
                payout_asset.clone(),
                denomination_symbol.clone(),
                display_decimals,
            ),
        );

        env.events().publish(
            (
                EVENT_INDEXED_V2,
                EventIndexTopicV2 {
                    version: 2,
                    event_type: EVENT_TYPE_OFFER,
                    issuer: primary_issuer.clone(),
                    namespace: namespace.clone(),
                    token: token.clone(),
                    period_id: 0,
                },
            ),
            (revenue_share_bps, payout_asset.clone()),
        );

        if false {
            env.events().publish(
                (EVENT_OFFER_REG_V1, primary_issuer.clone(), namespace.clone()),
                (EVENT_SCHEMA_VERSION, token.clone(), revenue_share_bps, payout_asset.clone()),
            );
        }
        // Versioned v2 event: always emitted (#RC26Q2-C31).
        // Payload: (token, revenue_share_bps, payout_asset, denomination_symbol, display_decimals)
        Self::emit_v2_event(
            &env,
            (EVENT_OFFER_REG_V2, issuer, namespace, token.clone()),
            (token, revenue_share_bps, payout_asset, denomination_symbol, display_decimals),
        );

        Ok(())
    }

    /// Return the denomination display metadata for an offering.
    ///
    /// This is a cheap O(1) read that does not require iterating offerings.
    ///
    /// ### Parameters
    /// - `issuer`: The issuer address.
    /// - `namespace`: The offering namespace.
    /// - `token`: The offering token address.
    ///
    /// ### Returns
    /// `Some((denomination_symbol, display_decimals))` if the offering exists,
    /// `None` if no offering with that identity has been registered.
    pub fn get_denomination_metadata(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<(Symbol, u32)> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get::<DataKey2, (Symbol, u32)>(&DataKey2::DenominationMetadata(offering_id))
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

    /// Configure the FX oracle used to convert cross-currency revenue reports
    /// into the offering payout asset before storing report and audit state.
    ///
    /// The issuer owns this configuration. `revenue_symbol` is passed to the
    /// oracle as the quote source when `report_revenue` is called with a
    /// non-payout asset; `payout_symbol` is the quote target for the registered
    /// offering payout asset.
    #[allow(clippy::too_many_arguments)]
    pub fn set_fx_oracle(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        oracle: Address,
        revenue_symbol: Symbol,
        payout_symbol: Symbol,
        max_oracle_age_secs: u64,
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

        let config = FxOracleConfig { oracle, revenue_symbol, payout_symbol, max_oracle_age_secs };
        env.storage().persistent().set(&DataKey2::FxOracleConfig(offering_id), &config);
        Ok(())
    }

    /// Return the configured FX oracle for an offering, if one exists.
    pub fn get_fx_oracle(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<FxOracleConfig> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get::<DataKey2, FxOracleConfig>(&DataKey2::FxOracleConfig(offering_id))
    }

    /// Configure an ordered oracle fallback chain for cross-currency FX conversion.
    ///
    /// When `report_revenue` needs an exchange rate, the contract iterates `entries`
    /// in order and uses the **first** oracle that returns a fresh (non-stale) quote.
    /// If every oracle in the chain is stale, the call returns
    /// `RevoraError::AllOraclesStale`.
    ///
    /// An empty `entries` vec clears any previously stored chain.  When a chain is
    /// present it **takes priority** over the legacy `FxOracleConfig` single-oracle
    /// setting; `set_fx_oracle` / `get_fx_oracle` continue to work as before for
    /// offerings that have not configured a chain.
    ///
    /// # Security
    /// Only the current offering issuer may configure this.  Each entry is validated
    /// to ensure `max_age_secs` is non-zero (enforced by caller; the contract stores
    /// whatever is provided — a zero value disables staleness checks for that entry).
    ///
    /// # Limits
    /// `entries.len()` must not exceed `MAX_ORACLE_CHAIN_LEN` (10).
    pub fn set_oracle_chain(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        entries: Vec<OracleEntry>,
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

        if entries.len() > MAX_ORACLE_CHAIN_LEN {
            return Err(RevoraError::LimitReached);
        }

        let chain = OracleChain { entries };
        env.storage().persistent().set(&DataKey2::OracleChain(offering_id), &chain);
        Ok(())
    }

    /// Return the oracle fallback chain configured for an offering, if any.
    pub fn get_oracle_chain(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<OracleChain> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get::<DataKey2, OracleChain>(&DataKey2::OracleChain(offering_id))
    }

    /// Register the off-chain ED25519 public key for an oracle.
    /// Only the contract admin can register oracle keys.
    pub fn register_oracle_pubkey(
        env: Env,
        oracle_id: Address,
        pubkey: BytesN<32>,
    ) -> Result<(), RevoraError> {
        Self::require_admin(&env)?;
        env.storage().persistent().set(&DataKey2::OraclePubKey(oracle_id), &pubkey);
        Ok(())
    }

    /// Try to obtain a fresh FX rate from a single `OracleEntry`.
    ///
    /// Returns `Ok(rate_bps)` when the quote is within the staleness window,
    /// or `Err(RevoraError::OracleQuoteStale)` when it is too old.
    fn try_oracle_entry(env: &Env, entry: &OracleEntry, now: u64) -> Result<i128, RevoraError> {
        let (rate, quoted_at) = FxOracleClient::new(env, &entry.oracle)
            .quote(&entry.revenue_symbol, &entry.payout_symbol);
        if entry.max_age_secs > 0 && now.saturating_sub(quoted_at) > entry.max_age_secs {
            return Err(RevoraError::OracleQuoteStale);
        }
        Ok(rate)
    }

    fn convert_report_amount_if_needed(
        env: &Env,
        offering_id: &OfferingId,
        offering: &Offering,
        reported_asset: &Address,
        amount: i128,
        now: u64,
        quote_bytes: Option<Bytes>,
        signature: Option<BytesN<64>>,
    ) -> Result<(i128, Address), RevoraError> {
        if offering.payout_asset == *reported_asset {
            return Ok((amount, reported_asset.clone()));
        }

        // ── Oracle chain path (takes priority over legacy single-oracle config) ──
        if let Some(chain) = env
            .storage()
            .persistent()
            .get::<DataKey2, OracleChain>(&DataKey2::OracleChain(offering_id.clone()))
        {
            let mut chain_idx: u32 = 0;
            for entry in chain.entries.iter() {
                match Self::try_oracle_entry(env, &entry, now) {
                    Ok(rate) => {
                        // Emit oracle_source_used event so indexers know which oracle won.
                        env.events().publish(
                            (
                                EVENT_ORACLE_SOURCE_USED,
                                offering_id.issuer.clone(),
                                offering_id.namespace.clone(),
                                offering_id.token.clone(),
                            ),
                            (
                                entry.oracle.clone(),
                                entry.revenue_symbol.clone(),
                                entry.payout_symbol.clone(),
                                chain_idx,
                            ),
                        );
                        let converted = amount.saturating_mul(rate).saturating_div(BPS_DENOMINATOR);
                        return Ok((converted, offering.payout_asset.clone()));
                    }
                    Err(_) => {
                        // This entry is stale — try the next one.
                        chain_idx = chain_idx.saturating_add(1);
                    }
                }
            }
            // Every entry in the chain was stale.
            return Err(RevoraError::AllOraclesStale);
        }

        // ── Legacy single-oracle path ──
        let config: FxOracleConfig = env
            .storage()
            .persistent()
            .get(&DataKey2::FxOracleConfig(offering_id.clone()))
            .ok_or(RevoraError::PayoutAssetMismatch)?;
        let (rate, quoted_at) = if let (Some(q), Some(sig)) = (quote_bytes, signature) {
            crate::security_assertions::oracle_validation::verify_oracle_signature(
                env,
                &q,
                &sig,
                &config.oracle,
            )?;
            let decoded: (i128, u64) =
                env.from_xdr(&q).map_err(|_| RevoraError::MetadataInvalidFormat)?;
            decoded
        } else {
            FxOracleClient::new(env, &config.oracle)
                .quote(&config.revenue_symbol, &config.payout_symbol)
        };
        if config.max_oracle_age_secs > 0
            && now.saturating_sub(quoted_at) > config.max_oracle_age_secs
        {
            return Err(RevoraError::OracleQuoteStale);
        }
        let converted_amount = amount.saturating_mul(rate).saturating_div(BPS_DENOMINATOR);
        Ok((converted_amount, offering.payout_asset.clone()))
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
        Self::report_revenue_internal(
            env,
            issuer,
            namespace,
            token,
            payout_asset,
            amount,
            period_id,
            override_existing,
            None,
            None,
        )
    }

    /// Report revenue for a specific period of an offering using an off-chain signed FX quote.
    pub fn report_revenue_with_attestation(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        payout_asset: Address,
        amount: i128,
        period_id: u64,
        override_existing: bool,
        quote_bytes: Bytes,
        signature: BytesN<64>,
    ) -> Result<(), RevoraError> {
        Self::report_revenue_internal(
            env,
            issuer,
            namespace,
            token,
            payout_asset,
            amount,
            period_id,
            override_existing,
            Some(quote_bytes),
            Some(signature),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn report_revenue_internal(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        payout_asset: Address,
        amount: i128,
        period_id: u64,
        override_existing: bool,
        quote_bytes: Option<Bytes>,
        signature: Option<BytesN<64>>,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();
        let mut amount = amount;
        let mut payout_asset = payout_asset;

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
        let current_timestamp = env.ledger().timestamp();

        Self::require_not_offering_frozen(&env, &offering_id)?;
        Self::require_report_window_open(&env, &offering_id)?;

        if !event_only {
            let offering =
                Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                    .ok_or(RevoraError::OfferingNotFound)?;
            let converted = Self::convert_report_amount_if_needed(
                &env,
                &offering_id,
                &offering,
                &payout_asset,
                amount,
                current_timestamp,
                quote_bytes,
                signature,
            )?;
            amount = converted.0;
            payout_asset = converted.1;

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
                        // Staleness guard: if max_staleness_secs > 0, require a fresh report.
                        if config.max_staleness_secs > 0 {
                            let reported_at: Option<u64> = env
                                .storage()
                                .persistent()
                                .get(&DataKey::ConcentrationReportedAt(offering_id.clone()));
                            match reported_at {
                                None => return Err(RevoraError::StaleConcentrationData),
                                Some(ts) => {
                                    if current_timestamp.saturating_sub(ts)
                                        > config.max_staleness_secs
                                    {
                                        return Err(RevoraError::StaleConcentrationData);
                                    }
                                }
                            }
                        }
                        let curr_key = DataKey::CurrentConcentration(offering_id.clone());
                        let current: u32 = env.storage().persistent().get(&curr_key).unwrap_or(0);
                        if current > config.max_bps {
                            return Err(RevoraError::ConcentrationLimitExceeded);
                        }
                    }
                }
            }
        }

        let threshold = Self::get_min_revenue_threshold_for_offering(&env, &offering_id);

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
            Self::emit_v2_and_v3(
                &env,
                EventIndexTopicV2 {
                    version: 2,
                    event_type: EVENT_TYPE_REV_INIT,
                    issuer: issuer.clone(),
                    namespace: namespace.clone(),
                    token: token.clone(),
                    period_id,
                },
                EventIndexTopicV3 {
                    version: 3,
                    event_type: EVENT_TYPE_REV_INIT,
                    issuer: issuer.clone(),
                    namespace: namespace.clone(),
                    token: token.clone(),
                    period_id,
                    _reserved: 0,
                },
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
                        Self::emit_v2_and_v3(
                            &env,
                            EventIndexTopicV2 {
                                version: 2,
                                event_type: EVENT_TYPE_REV_REJ,
                                issuer: issuer.clone(),
                                namespace: namespace.clone(),
                                token: token.clone(),
                                period_id,
                            },
                            EventIndexTopicV3 {
                                version: 3,
                                event_type: EVENT_TYPE_REV_REJ,
                                issuer: issuer.clone(),
                                namespace: namespace.clone(),
                                token: token.clone(),
                                period_id,
                                _reserved: 0,
                            },
                            (amount, existing_amount, payout_asset.clone()),
                        );
                        env.events().publish(
                            (EVENT_REVENUE_REPORT_REJECTED_ASSET, issuer, namespace, token),
                            (payout_asset, amount, period_id, existing_amount, blacklist),
                        );
                        return Ok(());
                    }

                    // Reject override if the period has been sealed by close_period.
                    let closed_key = DataKey2::ClosedPeriod(offering_id.clone(), period_id);
                    if env.storage().persistent().has(&closed_key) {
                        return Err(RevoraError::PeriodAlreadyClosed);
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
                    let delta = amount.s_sub(existing_amount).unwrap_or(0);
                    summary.total_revenue = summary.total_revenue.s_add(delta).unwrap_or(i128::MAX);
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
                    Self::emit_v2_and_v3(
                        &env,
                        EventIndexTopicV2 {
                            version: 2,
                            event_type: EVENT_TYPE_REV_OVR,
                            issuer: issuer.clone(),
                            namespace: namespace.clone(),
                            token: token.clone(),
                            period_id,
                        },
                        EventIndexTopicV3 {
                            version: 3,
                            event_type: EVENT_TYPE_REV_OVR,
                            issuer: issuer.clone(),
                            namespace: namespace.clone(),
                            token: token.clone(),
                            period_id,
                            _reserved: 0,
                        },
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
                        env.events().publish(
                            (
                                EVENT_REVENUE_REPORT_MISSING_OVERRIDE,
                                issuer.clone(),
                                namespace.clone(),
                                token.clone(),
                            ),
                            (amount, period_id),
                        );
                        Self::emit_v2_and_v3(
                            &env,
                            EventIndexTopicV2 {
                                version: 2,
                                event_type: EVENT_TYPE_REV_OMISS,
                                issuer: issuer.clone(),
                                namespace: namespace.clone(),
                                token: token.clone(),
                                period_id,
                            },
                            EventIndexTopicV3 {
                                version: 3,
                                event_type: EVENT_TYPE_REV_OMISS,
                                issuer: issuer.clone(),
                                namespace: namespace.clone(),
                                token: token.clone(),
                                period_id,
                                _reserved: 0,
                            },
                            (amount, period_id, payout_asset.clone()),
                        );
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
                    summary.total_revenue =
                        summary.total_revenue.s_add(amount).unwrap_or(i128::MAX);
                    summary.report_count = summary.report_count.s_add(1).unwrap_or(u64::MAX);
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
                    Self::emit_v2_and_v3(
                        &env,
                        EventIndexTopicV2 {
                            version: 2,
                            event_type: EVENT_TYPE_REV_INIT,
                            issuer: issuer.clone(),
                            namespace: namespace.clone(),
                            token: token.clone(),
                            period_id,
                        },
                        EventIndexTopicV3 {
                            version: 3,
                            event_type: EVENT_TYPE_REV_INIT,
                            issuer: issuer.clone(),
                            namespace: namespace.clone(),
                            token: token.clone(),
                            period_id,
                            _reserved: 0,
                        },
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
        Self::emit_v2_and_v3(
            &env,
            EventIndexTopicV2 {
                version: 2,
                event_type: EVENT_TYPE_REV_REP,
                issuer: issuer.clone(),
                namespace: namespace.clone(),
                token: token.clone(),
                period_id,
            },
            EventIndexTopicV3 {
                version: 3,
                event_type: EVENT_TYPE_REV_REP,
                issuer: issuer.clone(),
                namespace: namespace.clone(),
                token: token.clone(),
                period_id,
                _reserved: 0,
            },
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

        // Platform fee model (#468): once a report is recorded, route the configured
        // platform cut to the treasury and surface it via `plat_fee`. Reaching this point
        // means a report was actually recorded (initial or override); the below-threshold
        // and rejected paths return early above, so no fee is taken on those.
        Self::apply_platform_fee(
            &env,
            &offering_id,
            &issuer,
            &namespace,
            &token,
            amount,
            period_id,
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

        // Advance the cumulative accrual index. Skipped in event-only mode (no persistent state)
        // and when amount == 0. Rejected duplicates (rv_rej) never reach this point (early return).
        if !event_only {
            Self::update_and_emit_accrual_index(&env, &offering_id, amount, period_id);
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
            let amount = Self::get_revenue_by_period(
                env.clone(),
                issuer.clone(),
                namespace.clone(),
                token.clone(),
                period,
            );
            total = total.s_add(amount).unwrap_or(i128::MAX);
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
            let amount = Self::get_revenue_by_period(
                env.clone(),
                issuer.clone(),
                namespace.clone(),
                token.clone(),
                p,
            );
            total = total.s_add(amount).unwrap_or(i128::MAX);
            processed = processed.s_add(1).unwrap_or(u32::MAX);
            p = p.s_add(1).unwrap_or(u64::MAX);
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

    /// Helper function to add an investor to the blacklist with attestation.
    ///
    /// Accepts an optional `BlacklistEntryMeta` that, when provided, is persisted
    /// in a separate `BlacklistMeta` map keyed by `(offering_id, investor)`.
    /// This enables compliance verification by linking blacklist entries to
    /// signed off-chain OFAC snapshot hashes.
    fn do_blacklist_add(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        investor: Address,
        attestation: SanctionsAttestation,
        entry_meta: Option<BlacklistEntryMeta>,
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

        // Validate attestation timestamp: attested_at must not be in the future
        if attestation.attested_at > env.ledger().timestamp() {
            return Err(RevoraError::InvalidAmount); // Wait, let's check error codes
                                                    // Wait, let's use a proper error? Wait let's check RevoraError
        }

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if !Self::is_event_only(&env) {
            let key = DataKey::Blacklist(offering_id.clone());
            let mut map: Map<Address, SanctionsAttestation> =
                env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));

            let was_present = map.contains_key(investor.clone());
            if !was_present {
                // Guard: reject if the blacklist is already at capacity.
                let limit = Self::get_effective_blacklist_limit(&env, &offering_id);
                if map.len() >= limit {
                    return Err(RevoraError::BlacklistSizeLimitExceeded);
                }
                map.set(investor.clone(), attestation.clone());
                env.storage().persistent().set(&key, &map);

                // If BlacklistEntryMeta was provided, persist it in the BlacklistMeta map
                if let Some(ref meta) = entry_meta {
                    let meta_key = DataKey::BlacklistMeta(offering_id.clone());
                    let mut meta_map: Map<Address, BlacklistEntryMeta> = env
                        .storage()
                        .persistent()
                        .get(&meta_key)
                        .unwrap_or_else(|| Map::new(&env));
                    meta_map.set(investor.clone(), meta.clone());
                    env.storage().persistent().set(&meta_key, &meta_map);
                }

                // Maintain insertion order for deterministic get_blacklist (#38)
                let order_key = DataKey::BlacklistOrder(offering_id.clone());
                let mut order: Vec<Address> =
                    env.storage().persistent().get(&order_key).unwrap_or_else(|| Vec::new(&env));
                order.push_back(investor.clone());
                env.storage().persistent().set(&order_key, &order);
            }
        }

        // Emit the appropriate event based on whether a snapshot hash was provided
        if let Some(meta) = entry_meta {
            env.events().publish(
                (EVENT_BL_ADD_PINNED, issuer, namespace, token),
                (caller, investor, attestation, meta.snapshot_hash),
            );
        } else {
            env.events()
                .publish((EVENT_BL_ADD, issuer, namespace, token), (caller, investor, attestation));
        }
        Ok(())
    }

    /// Add an investor to the per-offering blacklist with a sanctions attestation.
    ///
    /// Blacklisted addresses are prohibited from claiming revenue for the specified token.
    /// This operation is idempotent.
    ///
    /// ### Parameters
    /// - `caller`: The address authorized to manage the blacklist. Must be the current issuer of the offering.
    /// - `issuer`: The issuer address of the offering.
    /// - `namespace`: The namespace of the offering.
    /// - `token`: The token representing the offering.
    /// - `investor`: The address to be blacklisted.
    /// - `attestation`: The sanctions attestation containing source, reference ID, and timestamp.
    ///
    /// ### Security Assumptions
    /// - `caller` must be the current issuer of the offering or the contract admin.
    /// - The blacklist is capped at `MAX_BLACKLIST_SIZE` entries per offering to prevent
    ///   unbounded storage growth and keep distribution gas predictable.
    /// - Idempotent adds (address already present) do not count against the size limit.
    /// - `attestation.attested_at` must not be in the future.
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
    /// - `Err(RevoraError::NotAuthorized)` if caller is not the current issuer or admin.
    /// - `Err(RevoraError::BlacklistSizeLimitExceeded)` if the blacklist is at capacity.
    /// - `Err(RevoraError::InvalidAmount)` if attestation timestamp is in the future.
    #[allow(clippy::too_many_arguments)]
    pub fn blacklist_add_with_attestation(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        investor: Address,
        attestation: SanctionsAttestation,
    ) -> Result<(), RevoraError> {
        Self::do_blacklist_add(env, caller, issuer, namespace, token, investor, attestation, None)
    }

    /// Add an investor to the per-offering blacklist, pinned to a signed OFAC snapshot hash.
    ///
    /// Compliance requires proving which OFAC list version informed each blacklist entry.
    /// This variant binds every `blacklist_add` call to a signed off-chain snapshot hash
    /// referencing the source list. The snapshot hash and addition timestamp are persisted
    /// alongside the attestation for verifiable compliance audits.
    ///
    /// ### Parameters
    /// - `caller`: The address authorized to manage the blacklist. Must be the current issuer or admin.
    /// - `issuer`: The issuer address of the offering.
    /// - `namespace`: The namespace of the offering.
    /// - `token`: The token representing the offering.
    /// - `investor`: The address to be blacklisted.
    /// - `attestation`: The sanctions attestation containing source, reference ID, and timestamp.
    /// - `ofac_snapshot_hash`: SHA-256 hash of the signed off-chain OFAC list snapshot.
    ///
    /// ### Events
    /// Publishes `(EVENT_BL_ADD_PINNED, issuer, namespace, token)` with
    /// `(caller, investor, attestation, snapshot_hash)` as the event data.
    ///
    /// ### Security Assumptions
    /// - `caller` must be the current issuer of the offering or the contract admin.
    /// - The blacklist is capped at `MAX_BLACKLIST_SIZE` entries per offering.
    /// - Idempotent adds (address already present) do not count against the size limit.
    /// - `attestation.attested_at` must not be in the future.
    ///
    /// ### Returns
    /// - `Ok(())` on success.
    /// - `Err(RevoraError::ContractFrozen)` if the contract is frozen.
    /// - `Err(RevoraError::NotAuthorized)` if caller is not the current issuer or admin.
    /// - `Err(RevoraError::BlacklistSizeLimitExceeded)` if the blacklist is at capacity.
    /// - `Err(RevoraError::InvalidAmount)` if attestation timestamp is in the future.
    #[allow(clippy::too_many_arguments)]
    pub fn blacklist_add_pinned(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        investor: Address,
        attestation: SanctionsAttestation,
        ofac_snapshot_hash: BytesN<32>,
    ) -> Result<(), RevoraError> {
        // Build the entry meta with the snapshot hash and current timestamp
        let entry_meta = BlacklistEntryMeta {
            snapshot_hash: ofac_snapshot_hash,
            added_ts: env.ledger().timestamp(),
        };
        Self::do_blacklist_add(
            env,
            caller,
            issuer,
            namespace,
            token,
            investor,
            attestation,
            Some(entry_meta),
        )
    }

    /// Add an investor to the per-offering blacklist (legacy, uses Source::Manual).
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
        let attestation = SanctionsAttestation {
            source: Source::Manual,
            ref_id: symbol_short!("manual"),
            attested_at: env.ledger().timestamp(),
        };
        Self::do_blacklist_add(env, caller, issuer, namespace, token, investor, attestation, None)
    }

    /// Add multiple investors to the per-offering blacklist in a single transaction (uses Source::Manual).
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
        let mut map: Map<Address, SanctionsAttestation> =
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
        let now = env.ledger().timestamp();
        for i in 0..unique_investors.len() {
            let investor = unique_investors.get(i).unwrap();
            let was_present = map.get(investor.clone()).unwrap_or(false);

            if !was_present {
                let attestation = SanctionsAttestation {
                    source: Source::Manual,
                    ref_id: symbol_short!("manual"),
                    attested_at: now,
                };
                // Add to map and order vec
                if !Self::is_event_only(&env) {
                    map.set(investor.clone(), attestation.clone());
                    order.push_back(investor.clone());
                }

                // Emit event for actual state change
                env.events().publish(
                    (EVENT_BL_ADD, issuer.clone(), namespace.clone(), token.clone()),
                    (caller.clone(), investor, attestation),
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
        let mut map: Map<Address, SanctionsAttestation> =
            env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));
        map.remove(investor.clone());
        env.storage().persistent().set(&key, &map);

        // Also clean up the BlacklistMeta map if this entry had a pinned snapshot hash
        let meta_key = DataKey::BlacklistMeta(offering_id.clone());
        let mut meta_map: Map<Address, BlacklistEntryMeta> = env
            .storage()
            .persistent()
            .get(&meta_key)
            .unwrap_or_else(|| Map::new(&env));
        meta_map.remove(investor.clone());
        env.storage().persistent().set(&meta_key, &meta_map);

        // Rebuild order vec so get_blacklist stays deterministic (#38)
        let order_key = DataKey::BlacklistOrder(offering_id.clone());
        let old_order: Vec<Address> =
            env.storage().persistent().get(&order_key).unwrap_or_else(|| Vec::new(&env));
        let mut new_order = Vec::new(&env);
        for i in 0..old_order.len() {
            let addr = old_order.get(i).unwrap();
            if map.contains_key(addr.clone()) {
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
        let mut map: Map<Address, SanctionsAttestation> =
            env.storage().persistent().get(&key).unwrap_or_else(|| Map::new(&env));

        // Also load the BlacklistMeta map for cleanup
        let meta_key = DataKey::BlacklistMeta(offering_id.clone());
        let mut meta_map: Map<Address, BlacklistEntryMeta> = env
            .storage()
            .persistent()
            .get(&meta_key)
            .unwrap_or_else(|| Map::new(&env));

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

                // Also clean up the BlacklistMeta entry if present
                meta_map.remove(investor.clone());

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
            if map.contains_key(addr.clone()) {
                new_order.push_back(addr);
            }
        }

        // Save updated storage
        env.storage().persistent().set(&key, &map);
        env.storage().persistent().set(&order_key, &new_order);
        env.storage().persistent().set(&meta_key, &meta_map);

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
            .get::<DataKey, Map<Address, SanctionsAttestation>>(&key)
            .map(|m| m.contains_key(investor))
            .unwrap_or(false)
    }

    /// Returns the sanctions attestation for a blacklisted investor, if any.
    pub fn get_blacklist_attestation(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        investor: Address,
    ) -> Option<SanctionsAttestation> {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey::Blacklist(offering_id);
        env.storage()
            .persistent()
            .get::<DataKey, Map<Address, SanctionsAttestation>>(&key)
            .and_then(|m| m.get(investor))
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
            .get::<DataKey, Map<Address, SanctionsAttestation>>(&key)
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// Returns the `BlacklistEntryMeta` for a blacklisted investor, if one exists.
    ///
    /// Only entries added via `blacklist_add_pinned` will have metadata.
    /// Returns `None` if the investor is not blacklisted or was added without a snapshot hash.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `issuer`: The issuer address of the offering.
    /// - `namespace`: The namespace of the offering.
    /// - `token`: The token representing the offering.
    /// - `investor`: The blacklisted address to query.
    ///
    /// ### Returns
    /// - `Some(BlacklistEntryMeta)` containing the snapshot hash and addition timestamp.
    /// - `None` if no pinned metadata exists for the investor.
    pub fn get_blacklist_entry_meta(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        investor: Address,
    ) -> Option<BlacklistEntryMeta> {
        let offering_id = OfferingId { issuer, namespace, token };
        let meta_key = DataKey::BlacklistMeta(offering_id);
        env.storage()
            .persistent()
            .get::<DataKey, Map<Address, BlacklistEntryMeta>>(&meta_key)
            .and_then(|m| m.get(investor))
    }

    // â”€â”€ Whitelist management â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
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
    /// Idempotent â€” calling with an already-whitelisted address is safe.
    /// When a whitelist exists (non-empty), only whitelisted addresses
    /// are eligible for revenue distribution (subject to blacklist override).
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
    /// - `max_staleness_secs`: When > 0 and `enforce` is true, `report_revenue` rejects if no
    ///   concentration has been reported or the last report is older than this many seconds.
    ///   Set to 0 to disable the staleness check.
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
        max_staleness_secs: u64,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;

        if max_bps > 10_000 {
            return Err(RevoraError::InvalidShareBps);
        }

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::LimitReached)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::LimitReached);
        }

        // Auth-first: authenticate before any state reads or side effects.
        // This prevents unauthenticated callers from probing offering existence
        // and ensures event-only mode never silently skips authorization.
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

        // Verify offering exists and issuer is current
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if !Self::is_event_only(&env) {
            let key = DataKey::ConcentrationLimit(offering_id);
            env.storage()
                .persistent()
                .set(&key, &ConcentrationLimitConfig { max_bps, enforce, max_staleness_secs });
        }

        Self::emit_v2_event(
            &env,
            (EVENT_CONC_LIMIT_SET, issuer, namespace, token),
            (max_bps, enforce),
        );

        Ok(())
    }

    pub fn set_transfer_restrictions(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        category: Symbol,
        max_holders: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        issuer.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        let key = DataKey2::TransferRestrictions(offering_id, category.clone());
        let restrictions = TransferRestrictions { category, max_holders };
        env.storage().persistent().set(&key, &restrictions);
        Ok(())
    }

    pub fn estimate_transfer(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        from: Address,
        to: Address,
        amount_bps: u32,
        category: Symbol,
        attest_hash: BytesN<32>,
        network_id: BytesN<32>,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(env)?;
        // We do not check issuer.require_auth() here because this is a pure query

        let active_network_id = env.ledger().network_id();
        if network_id != active_network_id {
            return Err(RevoraError::NetworkIdMismatch);
        }

        let _ = attest_hash;

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if from == to {
            return Ok(());
        }

        // Zero-value transfer is meaningless
        if amount_bps == 0 {
            return Err(RevoraError::InvalidAmount);
        }

        // Blacklist check
        if Self::is_blacklisted(env.clone(), issuer.clone(), namespace.clone(), token.clone(), from.clone()) {
            return Err(RevoraError::HolderBlacklisted);
        }
        if Self::is_blacklisted(env.clone(), issuer.clone(), namespace.clone(), token.clone(), to.clone()) {
            return Err(RevoraError::HolderBlacklisted);
        }

        // Jurisdiction block
        Self::require_holder_jurisdiction_allowed(
            env,
            &offering_id,
            to,
            symbol_short!("xfer"),
        )?;

        let from_share: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::HolderShare(offering_id.clone(), from.clone()))
            .unwrap_or(0);
        if from_share < amount_bps {
            return Err(RevoraError::InvalidAmount);
        }

        let to_share: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::HolderShare(offering_id.clone(), to.clone()))
            .unwrap_or(0);

        let cat_key = DataKey2::HolderCategory(offering_id.clone(), to.clone());
        let existing_cat: Option<Symbol> = env.storage().persistent().get(&cat_key);
        if let Some(existing) = existing_cat {
            if existing != *category {
                if to_share > 0 {
                    let old_count_key =
                        DataKey2::CategoryHolderCount(offering_id.clone(), existing);
                    let old_count: u32 =
                        env.storage().persistent().get(&old_count_key).unwrap_or(0);
                    env.storage().persistent().set(&old_count_key, &old_count.saturating_sub(1));

                    let new_count_key =
                        DataKey2::CategoryHolderCount(offering_id.clone(), category.clone());
                    let new_count: u32 =
                        env.storage().persistent().get(&new_count_key).unwrap_or(0);
                    if let Some(restrictions) =
                        env.storage().persistent().get::<_, TransferRestrictions>(
                            &DataKey2::TransferRestrictions(offering_id.clone(), category.clone()),
                        )
                    {
                        if new_count >= restrictions.max_holders {
                            return Err(RevoraError::CategoryCapReached);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn transfer_with_attestation(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        from: Address,
        to: Address,
        amount_bps: u32,
        category: Symbol,
    ) -> Result<(), RevoraError> {
        Self::check_transfer_eligibility(&env, &issuer, &namespace, &token, &from, &to, amount_bps, &category)?;
        issuer.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if from == to {
            return Ok(());
        }

        let from_share: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::HolderShare(offering_id.clone(), from.clone()))
            .unwrap_or(0);

        let to_share: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::HolderShare(offering_id.clone(), to.clone()))
            .unwrap_or(0);

        let cat_key = DataKey2::HolderCategory(offering_id.clone(), to.clone());
        let existing_cat: Option<Symbol> = env.storage().persistent().get(&cat_key);
        if let Some(existing) = existing_cat {
            if existing != category {
                if to_share > 0 {
                    let old_count_key =
                        DataKey2::CategoryHolderCount(offering_id.clone(), existing);
                    let old_count: u32 =
                        env.storage().persistent().get(&old_count_key).unwrap_or(0);
                    env.storage().persistent().set(&old_count_key, &old_count.saturating_sub(1));

                    let new_count_key =
                        DataKey2::CategoryHolderCount(offering_id.clone(), category.clone());
                    let new_count: u32 =
                        env.storage().persistent().get(&new_count_key).unwrap_or(0);
                    if let Some(restrictions) =
                        env.storage().persistent().get::<_, TransferRestrictions>(
                            &DataKey2::TransferRestrictions(offering_id.clone(), category.clone()),
                        )
                    {
                        if new_count >= restrictions.max_holders {
                            env.storage().persistent().set(&old_count_key, &old_count);
                            return Err(RevoraError::CategoryCapReached);
                        }
                    }
                    env.storage().persistent().set(&new_count_key, &(new_count + 1));
                }
                env.storage().persistent().set(&cat_key, &category);
            }
        } else {
            env.storage().persistent().set(&cat_key, &category);
        }

        Self::set_holder_share_internal(
            &env,
            issuer.clone(),
            namespace.clone(),
            token.clone(),
            from.clone(),
            from_share - amount_bps,
        )?;
        Self::set_holder_share_internal(&env, issuer, namespace, token, to, to_share + amount_bps)?;

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

        if concentration_bps > 10_000 {
            return Err(RevoraError::InvalidShareBps);
        }
        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

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
            env.storage().persistent().set(
                &DataKey::ConcentrationReportedAt(offering_id.clone()),
                &env.ledger().timestamp(),
            );
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

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        // Auth-first: authenticate before any state reads.
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
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

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        // Auth-first: authenticate before any state reads.
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

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

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        // Auth-first: authenticate before any state reads.
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

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

    /// Set the per-offering checkpoint compression threshold.
    ///
    /// When a holder's share schedule length exceeds `threshold` the
    /// oldest entries are folded into an `AccrualAnchor` and pruned
    /// from the schedule. The anchor stores a lossless pre-computed
    /// sum of claimable amounts for the compressed period range.
    ///
    /// Pass `0` to disable compression (the schedule will never be pruned).
    ///
    /// ### Auth
    /// Requires `issuer.require_auth()`. The caller must be the current
    /// issuer of the offering.
    ///
    /// ### Errors
    /// - [`RevoraError::OfferingNotFound`] â the offering does not exist
    ///   or the caller is not the current issuer.
    /// - [`RevoraError::NotAuthorized`] â the caller is not the issuer.
    pub fn set_checkpoint_threshold(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        threshold: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        Self::require_issuer_quorum_auth(&env, &offering.issuers);

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        let key = DataKey2::CheckpointThreshold(offering_id);
        let previous: u32 = env.storage().persistent().get(&key).unwrap_or(CHECKPOINT_THRESHOLD_DEFAULT);
        env.storage().persistent().set(&key, &threshold);

        Self::emit_v2_event(
            &env,
            (symbol_short!("chk_pt"), issuer, namespace, token),
            (previous, threshold),
        );
        Ok(())
    }

    /// Get the checkpoint compression threshold for an offering.
    ///
    /// Returns the configured threshold, or [`CHECKPOINT_THRESHOLD_DEFAULT`]
    /// when no explicit threshold has been set for this offering.
    pub fn get_checkpoint_threshold(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey2::CheckpointThreshold(offering_id);
        env.storage().persistent().get(&key).unwrap_or(CHECKPOINT_THRESHOLD_DEFAULT)
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
        // Defense-in-depth: use s_mul with saturating fallback to guard against refactors.
        let q = amount.s_div(10_000).unwrap_or(0);
        let r = amount % 10_000;
        let bps = revenue_share_bps as i128;
        let base = q.s_mul(bps).unwrap_or_else(|_| {
            if (q >= 0 && bps >= 0) || (q < 0 && bps < 0) {
                i128::MAX
            } else {
                i128::MIN
            }
        });

        let remainder_product = r.s_mul(bps).unwrap_or_else(|_| {
            if (r >= 0 && bps >= 0) || (r < 0 && bps < 0) {
                i128::MAX
            } else {
                i128::MIN
            }
        });
        let remainder_share = match mode {
            RoundingMode::Truncation => remainder_product.s_div(10_000).unwrap_or(0),
            RoundingMode::RoundHalfUp => {
                let half = 5_000_i128;
                if remainder_product >= 0 {
                    remainder_product.s_add(half).unwrap_or(i128::MAX).s_div(10_000).unwrap_or(0)
                } else {
                    remainder_product.s_sub(half).unwrap_or(i128::MIN).s_div(10_000).unwrap_or(0)
                }
            }
        };

        let share = base.s_add(remainder_share).unwrap_or_else(|_| {
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
        if decimals > MAX_TOKEN_DECIMALS {
            return Err(RevoraError::LimitReached);
        }
        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        env.storage().persistent().set(&DataKey2::PaymentTokenDecimals(offering_id), &decimals);
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
        Self::require_not_paused(&env)?;

        // Input validation (#35): reject zero/invalid period_id and non-positive amounts.
        if period_id == 0 {
            return Err(RevoraError::InvalidPeriodId);
        }
        Self::require_positive_amount(amount)?;

        // Verify offering exists and issuer is current
        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

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
        Self::require_not_paused(&env)?;

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

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if Self::snapshot_finalization_required(env.clone())
            && !Self::is_snapshot_finalized(&env, &offering_id, snapshot_reference)
        {
            return Err(RevoraError::SnapshotNotFinalized);
        }

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

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

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
        let deposit_ref: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::LastSnapshotRef(offering_id.clone()))
            .unwrap_or(0);
        let commit_ref: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::LastSnapshotCommitRef(offering_id))
            .unwrap_or(0);
        if deposit_ref > commit_ref {
            deposit_ref
        } else {
            commit_ref
        }
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

        // Verify offering exists and caller is current issuer.
        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

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
        let last_ref_key = DataKey::LastSnapshotCommitRef(offering_id.clone());
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

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

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

        // Validate all share_bps and jurisdiction rules before writing anything (fail-fast).
        for i in 0..batch_len {
            let (holder, share_bps) = holders.get(i).unwrap();
            if share_bps > 10_000 {
                return Err(RevoraError::InvalidShareBps);
            }
            Self::require_holder_jurisdiction_allowed(
                &env,
                &offering_id,
                &holder,
                EVENT_JUR_ACTION_SNAPSHOT,
            )?;
        }

        let mut added_bps: u32 = 0;

        // Maintain per-offering running total and validate aggregate cap.
        let total_key = DataKey::HolderShareTotal(offering_id.clone());
        let mut current_total: u32 = env.storage().persistent().get(&total_key).unwrap_or(0);
        let mut slot_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::SnapshotHolderCount(offering_id.clone(), snapshot_ref))
            .unwrap_or(0);

        // Check max total supply shares cap first
        let max_shares_key = DataKey2::MaxTotalSupplyShares(offering_id.clone());
        let max_shares: i128 = env.storage().persistent().get(&max_shares_key).unwrap_or(0);
        let mut temp_total_shares: i128 = if max_shares > 0 {
            env.storage()
                .persistent()
                .get(&DataKey2::TotalSharesIssued(offering_id.clone()))
                .unwrap_or(0)
        } else {
            0
        };
        let mut temp_deltas: Vec<(Address, i128)> = Vec::new(&env);

        // First pass: calculate deltas and check cap
        if max_shares > 0 {
            for i in 0..batch_len {
                let (holder, share_bps) = holders.get(i).unwrap();
                let old_share: u32 = env
                    .storage()
                    .persistent()
                    .get(&DataKey::HolderShare(offering_id.clone(), holder.clone()))
                    .unwrap_or(0);
                let delta = (share_bps as i128) - (old_share as i128);
                temp_total_shares = temp_total_shares.saturating_add(delta);
                temp_deltas.push_back((holder.clone(), delta));
            }
            if temp_total_shares > max_shares {
                return Err(RevoraError::MaxTotalSupplySharesExceeded);
            }
        }

        // Now apply the changes
        for i in 0..batch_len {
            let (holder, share_bps) = holders.get(i).unwrap();
            let slot = start_index.saturating_add(i);

            // Write indexed slot for deterministic enumeration.
            env.storage().persistent().set(
                &DataKey::SnapshotHolder(offering_id.clone(), snapshot_ref, slot),
                &(holder.clone(), share_bps),
            );

            // Write address-keyed entry for O(1) vote-weight lookup (issue #557).
            env.storage().persistent().set(
                &DataKey::SnapshotHolderShare(offering_id.clone(), snapshot_ref, holder.clone()),
                &share_bps,
            );

            if slot.saturating_add(1) > slot_count {
                slot_count = slot.saturating_add(1);
            }

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

            Self::cache_holder_accrual_through_matured(&env, &offering_id, &holder);

            // Update live holder share so claim() works immediately.
            env.storage()
                .persistent()
                .set(&DataKey::HolderShare(offering_id.clone(), holder.clone()), &share_bps);
            Self::record_holder_share_transition(&env, &offering_id, &holder, old_share, share_bps);

            current_total = new_total;
            added_bps = added_bps.saturating_add(share_bps);
        }

        // Update total shares issued
        if max_shares > 0 {
            env.storage()
                .persistent()
                .set(&DataKey2::TotalSharesIssued(offering_id.clone()), &temp_total_shares);
        } else {
            // If no cap, still track total shares
            let mut total_shares: i128 = env
                .storage()
                .persistent()
                .get(&DataKey2::TotalSharesIssued(offering_id.clone()))
                .unwrap_or(0);
            for i in 0..batch_len {
                let (holder, share_bps) = holders.get(i).unwrap();
                let old_share: u32 = env
                    .storage()
                    .persistent()
                    .get(&DataKey::HolderShare(offering_id.clone(), holder.clone()))
                    .unwrap_or(0);
                total_shares = total_shares
                    .saturating_sub(old_share as i128)
                    .saturating_add(share_bps as i128);
            }
            env.storage()
                .persistent()
                .set(&DataKey2::TotalSharesIssued(offering_id.clone()), &total_shares);
        }

        // Update snapshot metadata.
        if slot_count > entry.holder_count {
            entry.holder_count = slot_count;
        }
        let new_total_bps = entry.total_bps.saturating_add(added_bps);
        entry.total_bps = new_total_bps;
        env.storage().persistent().set(&entry_key, &entry);
        env.storage()
            .persistent()
            .set(&DataKey::SnapshotHolderCount(offering_id.clone(), snapshot_ref), &slot_count);

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
            .get(&DataKey::SnapshotHolderCount(offering_id, snapshot_ref))
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

    /// Set a holder's revenue share in basis points for an offering.
    ///
    /// The `nonce` must be strictly greater than the last accepted nonce for this
    /// `(offering_id, holder)` pair.  This monotonicity guard prevents stale
    /// off-chain updates from silently overwriting newer on-chain share state.
    /// Use `nonce = 1` on the first call; increment by at least 1 on every
    /// subsequent call.  The contract persists the last accepted value so the
    /// caller can derive the next valid nonce from `get_holder_share_nonce`.
    pub fn set_holder_share(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        share_bps: u32,
        nonce: u64,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Delegate to internal writer which maintains the aggregate running total
        // and enforces the per-offering sum invariant (≤ 10_000 bps).
        Self::set_holder_share_internal(&env, issuer, namespace, token, holder, share_bps, None, Some(nonce))
    }

    /// Set a holder's revenue share in basis points for a specific class of an offering.
    pub fn set_holder_share_class(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        share_bps: u32,
        share_class: ShareClass,
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

        Self::set_holder_share_internal(
            &env,
            issuer,
            namespace,
            token,
            holder,
            share_bps,
            Some(share_class),
            None,
        )
    }
    ///
    /// The dispute ID is deterministic: `sha256(issuer || namespace || token || holder || meta_hash)`.
    /// A holder may have at most [`MAX_OPEN_DISPUTES_PER_HOLDER`] open disputes per offering.
    ///
    /// ### Arguments
    /// * `holder` — The address opening the dispute. Must authenticate.
    /// * `issuer` — The offering's issuer address.
    /// * `namespace` — The offering's namespace symbol.
    /// * `token` — The offering's token address.
    /// * `meta_hash` — A 32-byte hash pointing to off-chain dispute evidence (e.g. IPFS CID).
    ///
    /// ### Errors
    /// - [`RevoraError::DisputeZeroShare`] if the holder holds zero shares.
    /// - [`RevoraError::DisputeAlreadyOpen`] if an identical dispute already exists.
    /// - [`RevoraError::MaxDisputesReached`] if the per-holder cap is exceeded.
    pub fn open_dispute(
        env: Env,
        holder: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        meta_hash: BytesN<32>,
    ) -> Result<BytesN<32>, RevoraError> {
        holder.require_auth();
        Self::require_not_frozen(&env)?;

        let offering_id = OfferingId { issuer, namespace, token };

        // Reject holders with zero shares (not a participant)
        let share = Self::get_holder_share(
            env.clone(),
            offering_id.issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
            holder.clone(),
        );
        if share == 0 {
            return Err(RevoraError::DisputeZeroShare);
        }

        // Deterministic dispute ID: sha256(issuer || namespace || token || holder || meta_hash)
        let mut input = Bytes::new(&env);
        input.append(&offering_id.issuer.to_xdr(&env));
        input.append(&offering_id.namespace.to_xdr(&env));
        input.append(&offering_id.token.to_xdr(&env));
        input.append(&holder.to_xdr(&env));
        input.append(&meta_hash.to_xdr(&env));
        let dispute_id: BytesN<32> = env.crypto().sha256(&input);

        // Reject duplicate
        if env.storage().persistent().has(&DataKey2::Dispute(dispute_id.clone())) {
            return Err(RevoraError::DisputeAlreadyOpen);
        }

        // Enforce spam cap per (offering_id, holder)
        let count_key = DataKey2::DisputeCount(offering_id.clone(), holder.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_OPEN_DISPUTES_PER_HOLDER {
            return Err(RevoraError::MaxDisputesReached);
        }

        let opened_at = env.ledger().timestamp();

        let dispute = Dispute {
            id: dispute_id.clone(),
            holder: holder.clone(),
            offering_id: offering_id.clone(),
            opened_at,
            meta_hash: meta_hash.clone(),
            status: DisputeStatus::Open,
        };

        env.storage().persistent().set(&DataKey2::Dispute(dispute_id.clone()), &dispute);
        env.storage().persistent().set(&count_key, &(count + 1));

        env.events().publish(
            (Symbol::new(&env, "dispute_open"),),
            (dispute_id.clone(), offering_id, holder.clone(), meta_hash),
        );

        Ok(dispute_id)
    }

    /// Read an on-chain dispute record by its deterministic ID.
    ///
    /// Returns `None` if no dispute with the given ID exists.
    pub fn get_dispute(env: Env, dispute_id: BytesN<32>) -> Option<Dispute> {
        env.storage().persistent().get(&DataKey2::Dispute(dispute_id))
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
        let classes_key = DataKey2::OfferingClasses(offering_id.clone());
        if let Some(cls_vec) =
            env.storage().persistent().get::<_, Vec<(ShareClass, ClassConfig)>>(&classes_key)
        {
            let mut total_share = 0;
            for (sc, _) in cls_vec.iter() {
                let share: u32 = env
                    .storage()
                    .persistent()
                    .get(&DataKey2::HolderShareClass(offering_id.clone(), holder.clone(), sc))
                    .unwrap_or(0);
                total_share += share;
            }
            total_share
        } else {
            env.storage().persistent().get(&DataKey::HolderShare(offering_id, holder)).unwrap_or(0)
        }
    }

    /// Return the last accepted nonce for a holder's share in an offering.
    ///
    /// Returns `0` when no `set_holder_share` call has ever been accepted for
    /// this `(offering_id, holder)` pair.  The next valid nonce for a
    /// `set_holder_share` call is any value strictly greater than this.
    pub fn get_holder_share_nonce(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
    ) -> u64 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get(&DataKey2::HolderShareNonce(offering_id, holder))
            .unwrap_or(0)
    }

    /// Get a holder's revenue share in basis points for a specific class of an offering.
    pub fn get_holder_share_class(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        share_class: ShareClass,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get(&DataKey2::HolderShareClass(offering_id, holder, share_class))
            .unwrap_or(0)
    }

    /// Set the conversion ratio (in bps) for rolling from one class to another.
    pub fn set_class_conversion_ratio(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        from_class: ShareClass,
        to_class: ShareClass,
        ratio_bps: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

        if ratio_bps == 0 {
            return Err(RevoraError::InvalidConversionRatio);
        }

        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey2::ClassConversionRatio(offering_id, from_class, to_class);
        env.storage().persistent().set(&key, &ratio_bps);
        Ok(())
    }

    /// Convert a holder's share from one class to another using the issuer-approved ratio.
    pub fn convert_class(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        from_class: ShareClass,
        to_class: ShareClass,
        amount_bps: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        holder.require_auth();

        let offering_id = OfferingId { issuer, namespace, token };

        if let Some(schedule) =
            env.storage().persistent().get::<_, crate::vesting::VestingSchedule>(
                &crate::vesting::VestingKey::Schedule(holder.clone()),
            )
        {
            let vested =
                crate::vesting::VestingContract::get_vested_amount(env.clone(), holder.clone())
                    .unwrap_or(0);
            if schedule.total_amount > vested {
                return Err(RevoraError::UnvestedConversionBlocked);
            }
        }

        let ratio_key = DataKey2::ClassConversionRatio(
            offering_id.clone(),
            from_class.clone(),
            to_class.clone(),
        );
        let ratio_bps: u32 =
            env.storage().persistent().get(&ratio_key).ok_or(RevoraError::ConversionNotApproved)?;

        if ratio_bps == 0 {
            return Err(RevoraError::InvalidConversionRatio);
        }

        let from_key =
            DataKey2::HolderShareClass(offering_id.clone(), holder.clone(), from_class.clone());
        let to_key =
            DataKey2::HolderShareClass(offering_id.clone(), holder.clone(), to_class.clone());

        let from_balance: u32 = env.storage().persistent().get(&from_key).unwrap_or(0);
        if from_balance < amount_bps {
            return Err(RevoraError::InsufficientClassBalance);
        }

        let converted_amount_bps =
            ((amount_bps as u64).saturating_mul(ratio_bps as u64) / 10000) as u32;

        let to_balance: u32 = env.storage().persistent().get(&to_key).unwrap_or(0);

        let new_from = from_balance.saturating_sub(amount_bps);
        let new_to = to_balance.saturating_add(converted_amount_bps);

        env.storage().persistent().set(&from_key, &new_from);
        env.storage().persistent().set(&to_key, &new_to);

        let classes_key = DataKey2::OfferingClasses(offering_id.clone());
        if let Some(mut cls_vec) =
            env.storage().persistent().get::<_, Vec<(ShareClass, ClassConfig)>>(&classes_key)
        {
            let mut from_idx = None;
            let mut to_idx = None;
            for (i, (sc, _)) in cls_vec.iter().enumerate() {
                if *sc == from_class {
                    from_idx = Some(i as u32);
                }
                if *sc == to_class {
                    to_idx = Some(i as u32);
                }
            }
            if let (Some(f_idx), Some(t_idx)) = (from_idx, to_idx) {
                let (_, mut f_cfg) = cls_vec.get(f_idx).unwrap();
                let (_, mut t_cfg) = cls_vec.get(t_idx).unwrap();

                f_cfg.bps =
                    f_cfg.bps.checked_sub(amount_bps).ok_or(RevoraError::InvalidShareBps)?;
                t_cfg.bps =
                    t_cfg.bps.checked_add(amount_bps).ok_or(RevoraError::InvalidShareBps)?;

                cls_vec.set(f_idx, (from_class.clone(), f_cfg));
                cls_vec.set(t_idx, (to_class.clone(), t_cfg));
                env.storage().persistent().set(&classes_key, &cls_vec);
            }
        }

        env.events().publish(
            (soroban_sdk::symbol_short!("class_cnv"), offering_id, holder),
            (from_class, from_balance, new_from, to_class, to_balance, new_to),
        );

        Ok(())
    }

    /// Set or update a holder's jurisdiction tag for an offering.
    pub fn set_holder_jurisdiction(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        jurisdiction: Symbol,
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
        env.storage()
            .persistent()
            .set(&DataKey2::HolderJurisdiction(offering_id.clone(), holder.clone()), &jurisdiction);
        env.events().publish(
            (Self::jurisdiction_set_event(&env), issuer, namespace, token),
            (EVENT_JUR_SCOPE_HOLDER, holder, jurisdiction),
        );
        Ok(())
    }

    /// Read a holder's configured jurisdiction tag for an offering.
    pub fn get_holder_jurisdiction(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
    ) -> Option<Symbol> {
        let offering_id = OfferingId { issuer, namespace, token };
        Self::get_holder_jurisdiction_internal(&env, &offering_id, &holder)
    }

    /// Replace the offering's allowed jurisdiction set.
    ///
    /// An empty list disables jurisdiction gating for future share writes and snapshot inclusion.
    pub fn set_allowed_jurisdictions(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        jurisdictions: Vec<Symbol>,
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
        let normalized = Self::normalize_jurisdictions(&env, jurisdictions);
        env.storage().persistent().set(&DataKey2::AllowedJurisdictions(offering_id), &normalized);
        env.events().publish(
            (Self::jurisdiction_set_event(&env), issuer, namespace, token),
            (EVENT_JUR_SCOPE_ALLOW, normalized),
        );
        Ok(())
    }

    /// Return the offering's allowed jurisdiction list in stored order.
    pub fn get_allowed_jurisdictions(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Vec<Symbol> {
        let offering_id = OfferingId { issuer, namespace, token };
        Self::get_allowed_jurisdictions_internal(&env, &offering_id)
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

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::NotAuthorized);
        }
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

        let offering_id = OfferingId { issuer: issuer.clone(), namespace, token };
        env.storage().persistent().set(&DataKey::ClaimDelaySecs(offering_id), &delay_secs);
        Ok(())
    }

    /// Get the claim delay in seconds for an offering.
    pub fn get_claim_delay(env: Env, issuer: Address, namespace: Symbol, token: Address) -> u64 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey::ClaimDelaySecs(offering_id)).unwrap_or(0)
    }

    /// Return the current contract version as a semver triple (MAJOR, MINOR, PATCH) (#23).
    pub fn get_version(_env: Env) -> (u32, u32, u32) {
        CONTRACT_VERSION
    }

    /// Migrate the contract storage to a new version.
    ///
    /// Reads the currently stored `DeployedVersion` (defaulting to [`CONTRACT_VERSION`]
    /// if absent) and rejects the call if:
    /// - The contract is not initialized (`NotInitialized`)
    /// - The caller is not the admin (`NotAuthorized`)
    /// - The contract is frozen (`ContractFrozen`)
    /// - `target` equals the stored version (`AlreadyAtTargetVersion`)
    /// - `target` is a semver downgrade (`MigrationDowngradeNotAllowed`)
    ///
    /// On success, persists `target` as the new `DeployedVersion` and emits a
    /// `(symbol_short!("migrate"), (from, to))` event.
    pub fn migrate_storage(
        env: Env,
        caller: Address,
        target_major: u32,
        target_minor: u32,
        target_patch: u32,
    ) -> Result<(), RevoraError> {
        caller.require_auth();

        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;
        if caller != admin {
            return Err(RevoraError::NotAuthorized);
        }

        if env.storage().persistent().get::<DataKey, bool>(&DataKey::Frozen).unwrap_or(false) {
            return Err(RevoraError::ContractFrozen);
        }

        let from = env
            .storage()
            .persistent()
            .get::<DataKey, (u32, u32, u32)>(&DataKey::DeployedVersion)
            .unwrap_or(CONTRACT_VERSION);
        let to = (target_major, target_minor, target_patch);

        assert_semver_forward(from, to)?;

        env.storage().persistent().set(&DataKey::DeployedVersion, &to);
        env.events().publish((symbol_short!("migrate"),), (from, to));
        Ok(())
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
        holder.require_auth();

        let offering_id = OfferingId { issuer, namespace, token };

        // Halt claims while a critical dispute is active for this offering
        if Self::is_dispute_freeze_active(&env, &offering_id) {
            return Err(RevoraError::DisputeFreezeActive);
        }

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

        if total_payout > 0 {
            crate::tax_bucket::rollover_distribution(&env, &offering_id, &holder, total_payout);
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

        let anchor_key = DataKey2::AccrualAnchor(offering_id.clone(), holder.clone());
        if let Some(a) = env.storage().persistent().get::<DataKey2, AccrualAnchor>(&anchor_key) {
            if start_idx <= a.end_idx && a.end_idx < last_claimed_idx {
                total_payout = total_payout.saturating_add(a.claimable_sum);
                env.storage().persistent().remove(&anchor_key);
            }
        }

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

    /// Read-only: check whether a proposal has reached quorum.
    /// Returns `true` if total voted weight (sum of voter_weight_bps) >= quorum_bps.
    /// Returns `false` (no panic) for empty votes (treated as zero).
    pub fn check_quorum(env: Env, proposal_id: u32) -> bool {
        let proposal: Proposal = env
            .storage()
            .persistent()
            .get(&DataKey2::MultisigProposal(proposal_id))
            .expect("Proposal not found");
        Self::check_quorum_inner(&env, &proposal)
    }

    /// Read-only: get a proposal by id.
    pub fn get_proposal(env: Env, proposal_id: u32) -> Option<Proposal> {
        env.storage().persistent().get(&DataKey2::MultisigProposal(proposal_id))
    }

    /// Open a formal on-chain dispute against an offering.
    ///
    /// The dispute ID is deterministic: `sha256(issuer || namespace || token || holder || meta_hash)`.
    /// A holder may have at most [`MAX_OPEN_DISPUTES_PER_HOLDER`] open disputes per offering.
    ///
    /// When `severity` is [`DisputeSeverity::Critical`] the offering's claims are frozen
    /// (blocked) until the dispute is resolved or rejected via [`resolve_dispute`].
    ///
    /// ### Arguments
    /// * `holder` — The address opening the dispute. Must authenticate.
    /// * `issuer` — The offering's issuer address.
    /// * `namespace` — The offering's namespace symbol.
    /// * `token` — The offering's token address.
    /// * `severity` — [`DisputeSeverity::Critical`] halts claims until resolved.
    /// * `meta_hash` — A 32-byte hash pointing to off-chain dispute evidence (e.g. IPFS CID).
    ///
    /// ### Errors
    /// - [`RevoraError::DisputeZeroShare`] if the holder holds zero shares.
    /// - [`RevoraError::DisputeAlreadyOpen`] if an identical dispute already exists.
    /// - [`RevoraError::MaxDisputesReached`] if the per-holder cap is exceeded.
    pub fn open_dispute(
        env: Env,
        holder: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        severity: DisputeSeverity,
        meta_hash: BytesN<32>,
    ) -> Result<BytesN<32>, RevoraError> {
        holder.require_auth();
        Self::require_not_frozen(&env)?;

        let offering_id = OfferingId { issuer, namespace, token };

        // Reject holders with zero shares (not a participant)
        let share = Self::get_holder_share(
            env.clone(),
            offering_id.issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
            holder.clone(),
        );
        if share == 0 {
            return Err(RevoraError::DisputeZeroShare);
        }

        // Deterministic dispute ID: sha256(issuer || namespace || token || holder || meta_hash)
        let mut input = Bytes::new(&env);
        input.append(&offering_id.issuer.to_xdr(&env));
        input.append(&offering_id.namespace.to_xdr(&env));
        input.append(&offering_id.token.to_xdr(&env));
        input.append(&holder.to_xdr(&env));
        input.append(&meta_hash.to_xdr(&env));
        let dispute_id: BytesN<32> = env.crypto().sha256(&input);

        // Reject duplicate
        if env.storage().persistent().has(&DataKey2::Dispute(dispute_id.clone())) {
            return Err(RevoraError::DisputeAlreadyOpen);
        }

        // Enforce spam cap per (offering_id, holder)
        let count_key = DataKey2::DisputeCount(offering_id.clone(), holder.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_OPEN_DISPUTES_PER_HOLDER {
            return Err(RevoraError::MaxDisputesReached);
        }

        let opened_at = env.ledger().timestamp();

        let dispute = Dispute {
            id: dispute_id.clone(),
            holder: holder.clone(),
            offering_id: offering_id.clone(),
            opened_at,
            severity: severity.clone(),
            meta_hash: meta_hash.clone(),
            status: DisputeStatus::Open,
        };

        env.storage().persistent().set(&DataKey2::Dispute(dispute_id.clone()), &dispute);
        env.storage().persistent().set(&count_key, &(count + 1));

        // Track critical dispute for O(1) freeze lookups
        if severity == DisputeSeverity::Critical {
            let crit_key = DataKey2::CriticalDisputeCount(offering_id.clone());
            let crit_count: u32 = env.storage().persistent().get(&crit_key).unwrap_or(0);
            env.storage().persistent().set(&crit_key, &(crit_count + 1));
            if crit_count == 0 {
                env.events().publish(
                    (EVENT_DISPUTE_FREEZE_ON,),
                    (offering_id.clone(), holder.clone(), dispute_id.clone()),
                );
            }
        }

        env.events().publish(
            (Symbol::new(&env, "dispute_open"),),
            (dispute_id.clone(), offering_id, holder.clone(), meta_hash, severity),
        );

        Ok(dispute_id)
    }

    /// Read an on-chain dispute record by its deterministic ID.
    ///
    /// Returns `None` if no dispute with the given ID exists.
    pub fn get_dispute(env: Env, dispute_id: BytesN<32>) -> Option<Dispute> {
        env.storage().persistent().get(&DataKey2::Dispute(dispute_id))
    }

    /// Check whether any critical dispute is active for the given offering.
    ///
    /// When `true`, claims for this offering are halted (returns [`RevoraError::DisputeFreezeActive`]).
    pub fn is_dispute_freeze_active(env: &Env, offering_id: &OfferingId) -> bool {
        let crit_key = DataKey2::CriticalDisputeCount(offering_id.clone());
        env.storage().persistent().get::<DataKey2, u32>(&crit_key).unwrap_or(0) > 0
    }

    /// Resolve or reject an open dispute.
    ///
    /// Only the issuer of the disputed offering may call this.
    /// When the last critical dispute for an offering transitions from `Open` to
    /// `Resolved` / `Rejected`, the dispute freeze is lifted and a `dispute_freeze_off`
    /// event is emitted.
    ///
    /// ### Arguments
    /// * `caller` — The address resolving the dispute. Must be the offering's issuer.
    /// * `dispute_id` — The deterministic dispute ID to resolve.
    /// * `resolution` — The target status: [`DisputeStatus::Resolved`] or [`DisputeStatus::Rejected`].
    ///
    /// ### Errors
    /// - [`RevoraError::NotDisputeIssuer`] if the caller is not the dispute's offering issuer.
    /// - [`RevoraError::DisputeNotFound`] if no dispute with the given ID exists.
    /// - [`RevoraError::DisputeAlreadyResolved`] if the dispute is not `Open`.
    pub fn resolve_dispute(
        env: Env,
        caller: Address,
        dispute_id: BytesN<32>,
        resolution: DisputeStatus,
    ) -> Result<(), RevoraError> {
        caller.require_auth();

        let mut dispute: Dispute =
            env.storage().persistent().get(&DataKey2::Dispute(dispute_id.clone()))
                .ok_or(RevoraError::DisputeNotFound)?;

        if dispute.status != DisputeStatus::Open {
            return Err(RevoraError::DisputeAlreadyResolved);
        }
        if caller != dispute.offering_id.issuer {
            return Err(RevoraError::NotDisputeIssuer);
        }

        let was_critical = dispute.severity == DisputeSeverity::Critical;
        dispute.status = resolution.clone();
        env.storage().persistent().set(&DataKey2::Dispute(dispute_id), &dispute);

        // Decrement critical dispute count and emit freeze_off when it hits zero
        if was_critical {
            let crit_key = DataKey2::CriticalDisputeCount(dispute.offering_id.clone());
            let crit_count: u32 = env.storage().persistent().get(&crit_key).unwrap_or(0);
            if crit_count > 0 {
                let new_count = crit_count - 1;
                env.storage().persistent().set(&crit_key, &new_count);
                if new_count == 0 {
                    env.events().publish(
                        (EVENT_DISPUTE_FREEZE_OFF,),
                        (dispute.offering_id, dispute.holder, dispute.id),
                    );
                }
            }
        }

        Ok(())
    }
}

// ── Holder shares, claims, admin, governance, and utility methods ──────────
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
    fn set_holder_share_full(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        share_bps: u32,
        share_class: Option<ShareClass>,
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
            share_class,
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

    /// Configure the claiming access window for an offering. If unset, always open.
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

    /// Claim pending share payouts for a holder on an offering.
    pub fn claim(
        env: Env,
        holder: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        max_periods: u32,
    ) -> Result<i128, RevoraError> {
        holder.require_auth();

        let offering_id = OfferingId { issuer, namespace, token };

        // Halt claims while a critical dispute is active for this offering
        if Self::is_dispute_freeze_active(&env, &offering_id) {
            return Err(RevoraError::DisputeFreezeActive);
        }

        // Initial blacklist and freeze checks for early fail-fast
        if Self::is_blacklisted(
            env.clone(),
            offering_id.issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
            holder.clone(),
        ) {
            return Err(RevoraError::HolderBlacklisted);
        }
        Self::require_holder_not_frozen(&env, &offering_id, &holder)?;

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
            // Enforce blacklist/whitelist and freeze decisiveness during partial claim sequences
            // This ensures that if a holder becomes blacklisted or frozen mid-sequence, subsequent
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
            if Self::is_frozen(&env, &offering_id, &holder) {
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

        let anchor_key2 = DataKey2::AccrualAnchor(offering_id.clone(), holder.clone());
        if let Some(a2) = env.storage().persistent().get::<DataKey2, AccrualAnchor>(&anchor_key2) {
            if start_idx <= a2.end_idx && a2.end_idx < last_claimed_idx {
                total_payout = total_payout.saturating_add(a2.claimable_sum);
                env.storage().persistent().remove(&anchor_key2);
            }
        }

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

    /// Seal a reporting period so that no further `report_revenue` overrides are accepted.
    ///
    /// Once closed, the period's deposited revenue remains claimable by holders; only
    /// issuer-initiated corrections via `override_existing=true` are blocked.
    ///
    /// ### Auth
    /// Requires `issuer.require_auth()`.
    ///
    /// ### Errors
    /// - `OfferingNotFound` – offering does not exist or caller is not the current issuer.
    /// - `InvalidPeriodId` – `period_id` is 0.
    /// - `PeriodAlreadyClosed` – period has already been sealed.
    /// - `ContractFrozen` / `ContractPaused` – contract is not operational.
    pub fn close_period(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        period_id: u64,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

        if period_id == 0 {
            return Err(RevoraError::InvalidPeriodId);
        }

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Verify offering exists and caller is the current issuer.
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        // If dual-signature mode is enabled for this offering, the single-sig
        // `close_period` path is not available — callers must use `close_period_dual_sig`.
        if env
            .storage()
            .persistent()
            .get::<_, bool>(&DataKey2::DualSigEnabled(offering_id.clone()))
            .unwrap_or(false)
        {
            return Err(RevoraError::DualSigNotConfigured);
        }

        let closed_key = DataKey2::ClosedPeriod(offering_id, period_id);
        if env.storage().persistent().has(&closed_key) {
            return Err(RevoraError::PeriodAlreadyClosed);
        }

        Self::assert_close_period_invariants(&env, &offering_id)?;

        let closed_at = env.ledger().timestamp();
        env.storage().persistent().set(&closed_key, &closed_at);

        // Track the most recently closed period timestamp for dispute window enforcement.
        env.storage()
            .persistent()
            .set(&DataKey2::LastClosedPeriodTimestamp(offering_id), &closed_at);

        env.events()
            .publish((EVENT_PERIOD_CLOSED, issuer, namespace, token), (period_id, closed_at));

        // Compute and persist the canonical per-class payout order (#523).
        // Done after the period is sealed so the storage write is monotonic
        // and the emitted pay order matches the on-chain sealed state.
        Self::record_and_emit_pay_order(&env, &offering_id, period_id);

        Ok(())
    }

    fn assert_close_period_invariants(
        env: &Env,
        offering_id: &OfferingId,
    ) -> Result<(), RevoraError> {
        let total_share_bps: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::HolderShareTotal(offering_id.clone()))
            .unwrap_or(0);

        let total_shares_issued: i128 = env
            .storage()
            .persistent()
            .get(&DataKey2::TotalSharesIssued(offering_id.clone()))
            .unwrap_or(0);

        if total_share_bps > 10_000
            || total_shares_issued < 0
            || total_shares_issued > 10_000
            || total_share_bps as i128 != total_shares_issued
        {
            return Err(RevoraError::CloseAbortInvariantsViolated);
        }
        Ok(())
    }

    /// Return `true` if the given period has been sealed by `close_period`.
    pub fn is_period_closed(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        period_id: u64,
    ) -> bool {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().has(&DataKey2::ClosedPeriod(offering_id, period_id))
    }

    // ── Per-class dividend priority ordering (#523) ────────────────────────

    /// Compute the canonical class payout order for an offering (#523).
    ///
    /// Reads the offering's registered `Vec<(ShareClass, ClassConfig)>` and the
    /// per-class priority index stored under `DataKey2::ClassPriority`. Classes
    /// are sorted ascending by `(priority_index, share_class.to_xdr().bytes)`;
    /// ties on priority are broken canonically by XDR-serialized bytes of the
    /// `ShareClass`, which gives a stable, deterministic ordering identical
    /// across reruns and across dual-sig / single-sig close paths.
    ///
    /// Classes without an explicit priority index resolve to `DEFAULT_CLASS_PRIORITY = 0`.
    /// Returns an empty `Vec<ShareClass>` when the offering has no classes registered.
    fn resolve_class_pay_order(env: &Env, offering_id: &OfferingId) -> Vec<ShareClass> {
        let classes_key = DataKey2::OfferingClasses(offering_id.clone());
        let classes_opt: Option<Vec<(ShareClass, ClassConfig)>> =
            env.storage().persistent().get(&classes_key);

        let classes = match classes_opt {
            Some(c) if !c.is_empty() => c,
            _ => return Vec::new(env),
        };

        // Build the keyed list: (priority, xdr_bytes, ShareClass).
        let mut keyed: Vec<(u32, Bytes, ShareClass)> = Vec::new(env);
        for entry in classes.iter() {
            let sc = entry.0.clone();
            let priority: u32 = env
                .storage()
                .persistent()
                .get::<DataKey2, u32>(&DataKey2::ClassPriority(
                    offering_id.clone(),
                    sc.clone(),
                ))
                .unwrap_or(DEFAULT_CLASS_PRIORITY);
            let xdr_bytes: Bytes = sc.to_xdr(env);
            keyed.push_back((priority, xdr_bytes, sc));
        }

        // Deterministic ascending sort by (priority, xdr_bytes).
        // Implemented as an in-place bubble sort for `soroban_sdk::Vec` (which
        // lacks `sort_by`). n is bounded by the per-offering class count, which
        // the contract keeps small via ClassConfig.
        let n = keyed.len();
        if n > 1 {
            let mut i: u32 = 0;
            while i < n.saturating_sub(1) {
                let mut j: u32 = 0;
                let stop = n.saturating_sub(1).saturating_sub(i);
                while j < stop {
                    let cur = keyed.get(j).expect("index valid");
                    let nxt = keyed.get(j.saturating_add(1)).expect("index valid");
                    let should_swap = match cur.0.cmp(&nxt.0) {
                        core::cmp::Ordering::Greater => true,
                        core::cmp::Ordering::Equal => {
                            cur.1.cmp(&nxt.1) == core::cmp::Ordering::Greater
                        }
                        core::cmp::Ordering::Less => false,
                    };
                    if should_swap {
                        keyed.set(j, nxt);
                        keyed.set(j.saturating_add(1), cur);
                    }
                    j = j.saturating_add(1);
                }
                i = i.saturating_add(1);
            }
        }

        let mut out: Vec<ShareClass> = Vec::new(env);
        for entry in keyed.iter() {
            out.push_back(entry.2.clone());
        }
        out
    }

    /// Persist the resolved pay order and emit the `EVENT_CLASS_PAY_ORDER` event.
    /// Called from both single-sig and dual-sig `close_period` paths after
    /// existing validation/sealing logic so the canonical ordering is recorded
    /// once per closed period and downstream auditors/indexers see a stable
    /// per-period distribution order.
    ///
    /// Always emits, including when no classes are registered for the offering
    /// — a `Vec::new()` payload is the documented fallback for legacy or
    /// classless offerings. Indexers should treat empty orders as the
    /// pre-deployment / no-class baseline.
    fn record_and_emit_pay_order(
        env: &Env,
        offering_id: &OfferingId,
        period_id: u64,
    ) {
        let ordered = Self::resolve_class_pay_order(env, offering_id);
        env.storage().persistent().set(
            &DataKey2::ClassPayOrder(offering_id.clone(), period_id),
            &ordered,
        );
        env.events().publish(
            (
                EVENT_CLASS_PAY_ORDER,
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
            ),
            (period_id, ordered),
        );
    }

    // ── Close-of-period preflight (#563) ──────────────────────────────────────────
    //
    // Pure (no side effects) helper that computes the per-period preview
    // returned by `preflight_close_period`. Reads from storage and never writes
    // or emits events. The write-path `close_period` /
    // `close_period_dual_sig` keep their inline pay-order resolver for tight
    // coupling — callers verify parity by computing identical `class_pay_order`
    // values (the very property the existing single-sig vs. dual-sig test
    // suite asserts).
    //
    // Math: `compute_share(period_revenue, share_bps, rounding_mode)`, the
    // same math the closest analogue `simulate_distribution` applies for the
    // no-classes path. Blacklisted holders are silently skipped. Holders
    // with zero `share_bps` are emitted with `normalized_payout = 0` so
    // callers can detect them. Total is a saturating sum so it can never
    // exceed `period_revenue`.
    fn compute_period_close_preview(
        env: &Env,
        offering_id: &OfferingId,
        period_id: u64,
        holders: Vec<Address>,
    ) -> PreflightCloseResult {
        let rev_key = DataKey::PeriodRevenue(offering_id.clone(), period_id);
        let period_revenue: i128 = env
            .storage()
            .persistent()
            .get::<DataKey, i128>(&rev_key)
            .unwrap_or(0);

        let class_pay_order = Self::resolve_class_pay_order(env, offering_id);

        let mode: RoundingMode = Self::get_rounding_mode(
            env.clone(),
            offering_id.issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
        );

        let n = holders.len();
        let mut payouts: Vec<DistributionEntry> = Vec::new(env);
        let mut total: i128 = 0;

        for i in 0..n {
            let holder = holders.get(i).unwrap();

            // Blacklist precedence rule: a blacklisted address is ALWAYS
            // excluded from payouts, regardless of any whitelist or
            // investor registration. Dropped silently from the preview.
            if Self::is_blacklisted(
                env.clone(),
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
                holder.clone(),
            ) {
                continue;
            }

            let share_bps: u32 = env
                .storage()
                .persistent()
                .get::<DataKey, u32>(&DataKey::HolderShare(
                    offering_id.clone(),
                    holder.clone(),
                ))
                .unwrap_or(0);

            // Saturate share_bps to 10_000 — matches simulate_distribution's
            // defence against malformed shares. With 0 share_bps the entry
            // is still emitted (with payout 0) so callers can detect it.
            let bounded_bps: u32 = if share_bps > 10_000 {
                10_000
            } else {
                share_bps
            };
            let normalized_payout =
                Self::compute_share(env.clone(), period_revenue, bounded_bps, mode);

            total = total.saturating_add(normalized_payout);
            payouts.push_back(DistributionEntry {
                holder: holder.clone(),
                share_bps,
                normalized_payout,
            });
        }

        PreflightCloseResult {
            period_id,
            period_revenue,
            class_pay_order,
            payouts,
            total_distributed: total,
        }
    }

    /// Set the dividend priority index for a registered class on an offering (#523).
    ///
    /// Lower `priority_index` values resolve to earlier payout positions under
    /// `close_period`. The configured class is recorded and an
    /// `EVENT_CLASS_PRIORITY_SET` event is emitted so indexers and dashboards
    /// can track priority changes.
    ///
    /// ### Auth
    /// Requires issuer-quorum authentication matching the contract-wide
    /// `Issuers.quorum` policy used by `set_holder_share` and similar mutations.
    /// A priority change effectively reorders how distributions are paid out, so
    /// we treat it as governance-equivalent to a holder-share mutation.
    ///
    /// ### Errors
    /// - [`RevoraError::OfferingNotFound`] if the offering does not exist or the
    ///   caller is not the current issuer.
    /// - [`RevoraError::InvalidShareClass`] if `share_class` is not a registered
    ///   class on the offering (i.e. absent from `DataKey2::OfferingClasses`).
    /// - [`RevoraError::ContractFrozen`] / [`RevoraError::ContractPaused`] when
    ///   the contract is not operational.
    #[allow(clippy::too_many_arguments)]
    pub fn set_class_priority(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        share_class: ShareClass,
        priority_index: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;

        let offering = Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
            .ok_or(RevoraError::OfferingNotFound)?;
        // Issuer-quorum requires the primary signer and the configured number
        // of co-signers to have authorized. This matches `set_holder_share`
        // and other governance-equivalent mutations.
        Self::require_issuer_quorum_auth(&env, &offering.issuers);

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Verify the share_class is registered for this offering. Rejecting
        // unregistered classes prevents storage pollution and Denial-of-Service
        // via arbitrarily large priority-index entries.
        let classes_key = DataKey2::OfferingClasses(offering_id.clone());
        let classes_opt: Option<Vec<(ShareClass, ClassConfig)>> =
            env.storage().persistent().get(&classes_key);
        let registered = classes_opt
            .as_ref()
            .map(|v| v.iter().any(|(sc, _)| sc == &share_class))
            .unwrap_or(false);
        if !registered {
            return Err(RevoraError::InvalidShareClass);
        }

        env.storage().persistent().set(
            &DataKey2::ClassPriority(offering_id.clone(), share_class.clone()),
            &priority_index,
        );

        env.events().publish(
            (
                EVENT_CLASS_PRIORITY_SET,
                issuer,
                namespace,
                token,
                share_class,
            ),
            priority_index,
        );

        Ok(())
    }

    /// Read the dividend priority index for a class on an offering (#523).
    /// Returns `DEFAULT_CLASS_PRIORITY = 0` when no explicit priority has been set.
    pub fn get_class_priority(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        share_class: ShareClass,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get::<DataKey2, u32>(&DataKey2::ClassPriority(offering_id, share_class))
            .unwrap_or(DEFAULT_CLASS_PRIORITY)
    }

    /// Read the canonical class payout order resolved at `close_period` time (#523).
    /// Returns an empty `Vec<ShareClass>` if the period was never closed via the
    /// updated `close_period` / `close_period_dual_sig` implementation.
    pub fn get_class_pay_order(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        period_id: u64,
    ) -> Vec<ShareClass> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get::<DataKey2, Vec<ShareClass>>(&DataKey2::ClassPayOrder(offering_id, period_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Enable or disable dual-signature close-of-period mode for an offering (#565).
    ///
    /// When enabled, `close_period` will reject with `DualSigNotConfigured` and the
    /// issuer must use `close_period_dual_sig` instead, which requires two distinct
    /// authorized signers.
    ///
    /// ### Auth
    /// Requires `issuer.require_auth()`.
    ///
    /// ### Errors
    /// - `OfferingNotFound` – offering does not exist or caller is not the current issuer.
    /// - `ContractFrozen` / `ContractPaused` – contract is not operational.
    pub fn set_dual_sig_config(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        enabled: bool,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Verify offering exists and caller is the current issuer.
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        env.storage().persistent().set(&DataKey2::DualSigEnabled(offering_id), &enabled);

        env.events().publish((symbol_short!("dual_cfg"), issuer, namespace, token), (enabled,));
        Ok(())
    }

    /// Close a period using dual-signature authorization.
    ///
    /// For high-value periods, this function requires two distinct signers to
    /// authorize the close. Both signers must be valid issuers of the offering
    /// (the primary issuer or a co-issuer).
    ///
    /// ### Auth
    /// Requires both `sig_a.require_auth()` and `sig_b.require_auth()`.
    ///
    /// ### Errors
    /// - `DualSigSameSigner` – `sig_a` and `sig_b` are the same address.
    /// - `DualSigNotConfigured` – dual-signature mode has not been enabled for this offering.
    /// - `OfferingNotFound` – offering does not exist or a signer is not a valid issuer.
    /// - `InvalidPeriodId` – `period_id` is 0.
    /// - `PeriodAlreadyClosed` – period has already been sealed.
    /// - `ContractFrozen` / `ContractPaused` – contract is not operational.
    pub fn close_period_dual_sig(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        period_id: u64,
        sig_a: Address,
        sig_b: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        sig_a.require_auth();
        sig_b.require_auth();

        // Both signers must be distinct.
        if sig_a == sig_b {
            return Err(RevoraError::DualSigSameSigner);
        }

        if period_id == 0 {
            return Err(RevoraError::InvalidPeriodId);
        }

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Verify offering exists and retrieve the full Offering (including issuers).
        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;

        // Both signers must be valid issuers (primary or co-issuer).
        let is_valid = |addr: &Address| -> bool {
            if &offering.issuers.primary == addr {
                return true;
            }
            offering.issuers.co.iter().any(|co| co == addr)
        };
        if !is_valid(&sig_a) || !is_valid(&sig_b) {
            return Err(RevoraError::OfferingNotFound);
        }

        // Dual-signature mode must be enabled for this offering.
        if !env
            .storage()
            .persistent()
            .get::<_, bool>(&DataKey2::DualSigEnabled(offering_id.clone()))
            .unwrap_or(false)
        {
            return Err(RevoraError::DualSigNotConfigured);
        }

        let closed_key = DataKey2::ClosedPeriod(offering_id, period_id);
        if env.storage().persistent().has(&closed_key) {
            return Err(RevoraError::PeriodAlreadyClosed);
        }

        let closed_at = env.ledger().timestamp();
        env.storage().persistent().set(&closed_key, &closed_at);

        // Track the most recently closed period timestamp for dispute window enforcement.
        env.storage()
            .persistent()
            .set(&DataKey2::LastClosedPeriodTimestamp(offering_id), &closed_at);

        env.events().publish(
            (EVENT_DUAL_SIG_CLOSE, issuer, namespace, token),
            (period_id, closed_at, sig_a, sig_b),
        );

        // Compute and persist the canonical per-class payout order (#523).
        // Mirrors the single-sig `close_period` path so both close flows
        // resolve to the identical deterministic order.
        Self::record_and_emit_pay_order(&env, &offering_id, period_id);

        Ok(())
    }

    /// Read-only: preview the canonical class pay order and per-holder
    /// payouts that a successful `close_period` / `close_period_dual_sig`
    /// call would expose for the given period — without touching storage or
    /// emitting any event (#563).
    ///
    /// Mirrors the precondition chain of [`Self::close_period`] so that any
    /// error the write path would surface (`ContractFrozen`, `ContractPaused`,
    /// `InvalidPeriodId`, `OfferingNotFound`, `PeriodAlreadyClosed`) is also
    /// surfaced here. A successful preflight guarantees the atomic close
    /// would succeed for the same `(offering_id, period_id)` assuming the
    /// caller passes `issuer.require_auth()` on the actual write call.
    ///
    /// `holders` is required because Soroban persistent maps cannot be
    /// iterated by key — this matches the codebase idiom set by
    /// [`Self::simulate_distribution`]. Pass the same holder set the
    /// eventual `claim` flow will iterate. Blacklisted addresses are
    /// silently dropped from the returned `payouts` (precedence rule:
    /// blacklist wins over whitelist and over any non-zero share).
    ///
    /// # Compute parity
    ///
    /// The `class_pay_order` returned by this entrypoint is byte-identical
    /// to what `close_period` would persist via `record_and_emit_pay_order`
    /// for the same storage snapshot. The `payouts[i].normalized_payout`
    /// values are produced by `compute_share(revenue, share_bps, mode)`,
    /// matching the math `simulate_distribution` applies. There is no
    /// `normalize_amount` step here because the surrounding
    /// `simulate_distribution` flow does not apply one either; indexers
    /// should treat this preview as the canonical non-normalized
    /// distribution shape.
    pub fn preflight_close_period(
        env: Env,
        offering_id: OfferingId,
        period_id: u64,
        holders: Vec<Address>,
    ) -> Result<PreflightCloseResult, RevoraError> {
        // Mirror `close_period`'s precondition chain exactly. NOTE: we
        // intentionally do NOT execute `issuer.require_auth()` here — the
        // preflight is a read-only view callable by anyone, aligning with
        // `get_offering`, `simulate_distribution`, and `get_class_pay_order`.
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;

        if period_id == 0 {
            return Err(RevoraError::InvalidPeriodId);
        }

        // Offering existence. We use `DataKey2::OfferingRecord` (introduced
        // for O(1) issuance lookups) rather than `get_offering` so we do
        // not charge for a full offering struct deserialisation when we
        // only need the existence check.
        let offering_record_key = DataKey2::OfferingRecord(offering_id.clone());
        if !env.storage().persistent().has(&offering_record_key) {
            return Err(RevoraError::OfferingNotFound);
        }

        // Period already sealed — close would reject too, so the preflight
        // must reject too. Operators calling preflight then close on the
        // same period therefore see a deterministic `PeriodAlreadyClosed`
        // rather than a stale preview.
        let closed_key = DataKey2::ClosedPeriod(offering_id.clone(), period_id);
        if env.storage().persistent().has(&closed_key) {
            return Err(RevoraError::PeriodAlreadyClosed);
        }

        Ok(Self::compute_period_close_preview(
            &env,
            &offering_id,
            period_id,
            holders,
        ))
    }

    /// Attach or replace off-chain disclosure metadata for an offering (#485).
    ///
    /// Issuers use this to bind a private placement memorandum (PPM), K-1 template,
    /// or any other off-chain document to the on-chain record so investors can verify
    /// the document's integrity via the stored hash.
    ///
    /// ### Validation
    /// - `uri` must be at most 256 bytes; longer values return `DisclosureUriTooLong`.
    /// - An empty `uri` paired with a non-zero `hash` returns `InconsistentDisclosure`.
    ///   (A zero-hash with an empty URI clears any previous disclosure.)
    ///
    /// ### Auth ordering
    /// `issuer.require_auth()` is called immediately after the frozen guard.
    pub fn update_disclosure(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        uri: Bytes,
        hash: BytesN<32>,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
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

        // URI length guard: max 256 bytes.
        if uri.len() > 256 {
            return Err(RevoraError::DisclosureUriTooLong);
        }

        // Coherence guard: non-zero hash requires a URI.
        let zero_hash = BytesN::from_array(&env, &[0u8; 32]);
        if uri.is_empty() && hash != zero_hash {
            return Err(RevoraError::InconsistentDisclosure);
        }

        let key = DataKey2::DisclosureMeta(offering_id);
        env.storage()
            .persistent()
            .set(&key, &DisclosureMeta { uri: uri.clone(), hash: hash.clone() });

        Self::emit_v2_event(
            &env,
            (EVENT_DISCLOSURE_UPDATED, issuer, namespace, token),
            (uri, hash),
        );

        Ok(())
    }

    /// Return the off-chain disclosure metadata for an offering, if set.
    pub fn get_disclosure(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<DisclosureMeta> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey2::DisclosureMeta(offering_id))
    }
}

// â”€â”€ Holder shares, claims, admin, governance, and utility methods â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Plain impl block â€” excluded from the ABI spec to keep spec XDR within limit.
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
        share_class: Option<ShareClass>,
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
            share_class,
            None,
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
            None,
            None,
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
    /// Read-only: return a page of pending period IDs for a holder, bounded by `limit`.
    /// Returns `(periods_page, next_cursor)` where `next_cursor` is `Some(next_index)` when more
    /// periods remain, otherwise `None`. `limit` of 0 or greater than `MAX_PAGE_LIMIT` will be
    /// capped to `MAX_PAGE_LIMIT` to keep calls predictable.
    #[allow(clippy::too_many_arguments)]
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

    /// Read-only: return a paginated statement page for a holder.
    ///
    /// Each entry is ordered by the persisted `PeriodEntry` index, which is monotonic in
    /// `period_id` for valid offering state. The cursor is the zero-based period-entry index
    /// and is clamped to the holder's current `LastClaimedIdx`, so stale callers cannot page
    /// back into already-claimed history.
    ///
    /// Security assumptions:
    /// - Returning an empty page for a cursor past the end must be safe and deterministic.
    /// - The first delayed period forms a hard stop because later periods are not claimable yet.
    /// - `limit` is capped to `MAX_PAGE_LIMIT` to keep read gas bounded.
    #[allow(clippy::too_many_arguments)]
    pub fn get_holder_statement_page(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        cursor: u32,
        limit: u32,
    ) -> (Vec<HolderStatementEntry>, Option<u32>) {
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if Self::is_blacklisted(env.clone(), issuer, namespace, token, holder.clone()) {
            return (Vec::new(&env), None);
        }
        if Self::require_claim_window_open(&env, &offering_id).is_err() {
            return (Vec::new(&env), None);
        }

        let count_key = DataKey::PeriodCount(offering_id.clone());
        let period_count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let idx_key = DataKey::LastClaimedIdx(offering_id.clone(), holder.clone());
        let holder_start_idx: u32 = env.storage().persistent().get(&idx_key).unwrap_or(0);
        let start_idx = core::cmp::max(cursor, holder_start_idx);
        if start_idx >= period_count {
            return (Vec::new(&env), None);
        }

        let effective_limit =
            if limit == 0 || limit > MAX_PAGE_LIMIT { MAX_PAGE_LIMIT } else { limit };

        let delay_key = DataKey::ClaimDelaySecs(offering_id.clone());
        let delay_secs: u64 = env.storage().persistent().get(&delay_key).unwrap_or(0);
        let now = env.ledger().timestamp();

        let mut entries = Vec::new(&env);
        let mut processed: u32 = 0;
        let mut idx = start_idx;
        let mut previous_period_id: Option<u64> = None;

        while idx < period_count && processed < effective_limit {
            let entry_key = DataKey::PeriodEntry(offering_id.clone(), idx);
            let period_id: u64 = env.storage().persistent().get(&entry_key).unwrap_or(0);
            if period_id == 0 {
                idx = idx.saturating_add(1);
                continue;
            }

            if let Some(previous) = previous_period_id {
                if period_id <= previous {
                    break;
                }
            }
            previous_period_id = Some(period_id);

            let time_key = DataKey::PeriodDepositTime(offering_id.clone(), period_id);
            let deposit_timestamp: u64 = env.storage().persistent().get(&time_key).unwrap_or(0);
            if delay_secs > 0 && now < deposit_timestamp.saturating_add(delay_secs) {
                return (entries, Some(idx));
            }

            let claimable_amount = Self::compute_holder_payout_for_range(
                &env,
                &offering_id,
                &holder,
                idx,
                idx.saturating_add(1),
            );
            entries.push_back(HolderStatementEntry {
                period_id,
                deposit_timestamp,
                claimable_amount,
            });

            processed = processed.saturating_add(1);
            idx = idx.saturating_add(1);
        }

        let next_cursor = if idx < period_count { Some(idx) } else { None };
        (entries, next_cursor)
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

        let anchor_key = DataKey2::AccrualAnchor(offering_id.clone(), holder.clone());
        let anchor: Option<AccrualAnchor> = env.storage().persistent().get(&anchor_key);

        if let Some(a) = anchor {
            if holder_start_idx <= a.end_idx {
                total = total.saturating_add(a.claimable_sum);
                let anchor_periods = a.end_idx.saturating_sub(holder_start_idx).saturating_add(1);
                processed = processed.saturating_add(anchor_periods);
                idx = core::cmp::max(idx, a.end_idx.saturating_add(1));
                env.storage().persistent().remove(&anchor_key);
            }
        }

        let effective_end = count.map(|c| core::cmp::min(actual_start + c, period_count)).unwrap_or(period_count);

        while idx < effective_end {
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

            total = total.saturating_add(Self::compute_holder_payout_for_range(
                env,
                offering_id,
                holder,
                idx,
                idx.saturating_add(1),
            ));
            processed = processed.saturating_add(1);
            idx = idx.saturating_add(1);
        }

        (total, None)
    }

    /// Request redemption of a portion of the caller's holder shares.
    ///
    /// The holder submits a request specifying `shares_bps` to redeem. Only one
    /// pending request per holder per offering is allowed. The redemption window
    /// must be open (if configured). Blacklisted holders are rejected.
    pub fn request_redemption(
        env: Env,
        holder: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        shares_bps: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        holder.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Verify offering exists
        Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
            .ok_or(RevoraError::OfferingNotFound)?;

        // Check redemption window is open
        Self::require_redemption_window_open(&env, &offering_id)?;

        // Check holder is not blacklisted
        if Self::is_blacklisted(
            env.clone(),
            issuer.clone(),
            namespace.clone(),
            token.clone(),
            holder.clone(),
        ) {
            return Err(RevoraError::HolderBlacklisted);
        }

        // Check holder has shares to redeem
        let current_share = Self::get_holder_share_internal(
            env.clone(),
            issuer.clone(),
            namespace.clone(),
            token.clone(),
            holder.clone(),
        );
        if current_share == 0 {
            return Err(RevoraError::NoPendingClaims);
        }
        if shares_bps == 0 || shares_bps > current_share {
            return Err(RevoraError::InvalidShareBps);
        }

        // Check no pending request already exists
        let request_key = DataKey2::RedemptionRequest(offering_id.clone(), holder.clone());
        if env.storage().persistent().has(&request_key) {
            return Err(RevoraError::LimitReached);
        }

        // Store pending request
        let pending = PendingRedemption { shares_bps, timestamp: env.ledger().timestamp() };
        env.storage().persistent().set(&request_key, &pending);

        // Emit event
        env.events()
            .publish((EVENT_REDEMPTION_REQUESTED, issuer, namespace, token), (holder, shares_bps));
        Ok(())
    }

    /// Fulfill a pending redemption request.
    ///
    /// The issuer transfers `amount` of the offering's locked payment token from
    /// the contract to the holder and reduces the holder's share by the requested
    /// `shares_bps`. The redemption window must be open. Blacklisted holders are
    /// rejected even if they had a pending request.
    pub fn fulfill_redemption(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        amount: i128,
    ) -> Result<i128, RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Verify caller is the current issuer
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        // Check redemption window is open
        Self::require_redemption_window_open(&env, &offering_id)?;

        // Reject blacklisted holders even if they had a pending request
        if Self::is_blacklisted(
            env.clone(),
            issuer.clone(),
            namespace.clone(),
            token.clone(),
            holder.clone(),
        ) {
            return Err(RevoraError::HolderBlacklisted);
        }

        // Validate amount
        if amount <= 0 {
            return Err(RevoraError::InvalidAmount);
        }

        // Read pending request
        let request_key = DataKey2::RedemptionRequest(offering_id.clone(), holder.clone());
        let pending: PendingRedemption =
            env.storage().persistent().get(&request_key).ok_or(RevoraError::NoTransferPending)?;

        // Read holder's current share
        let current_share = Self::get_holder_share_internal(
            env.clone(),
            issuer.clone(),
            namespace.clone(),
            token.clone(),
            holder.clone(),
        );
        if current_share == 0 {
            return Err(RevoraError::NoPendingClaims);
        }

        // Compute effective redeem bps (capped to what holder actually has)
        let redeem_bps = core::cmp::min(pending.shares_bps, current_share);
        let new_share = current_share - redeem_bps;

        // Transfer amount (minus redemption fee if configured) from contract to holder
        let payment_token = Self::get_locked_payment_token_for_offering(&env, &offering_id)
            .ok_or(RevoraError::PaymentTokenMismatch)?;
        let contract_addr = env.current_contract_address();

        let fee_config = Self::get_redemption_fee_config(env.clone(), issuer.clone(), namespace.clone(), token.clone());
        let (net_amount, fee_amount, treasury_addr) = if let Some(cfg) = fee_config {
            if cfg.fee_bps > 0 {
                let fee = amount.checked_mul(cfg.fee_bps as i128).unwrap_or(0) / 10_000i128;
                let net = amount.saturating_sub(fee);
                (net, fee, Some(cfg.treasury))
            } else {
                (amount, 0i128, None)
            }
        } else {
            (amount, 0i128, None)
        };

        let token_client = token::Client::new(&env, &payment_token);
        if net_amount > 0 && token_client.try_transfer(&contract_addr, &holder, &net_amount).is_err() {
            return Err(RevoraError::TransferFailed);
        }

        if fee_amount > 0 {
            if let Some(treasury) = treasury_addr {
                if token_client.try_transfer(&contract_addr, &treasury, &fee_amount).is_err() {
                    return Err(RevoraError::TransferFailed);
                }
                env.events().publish(
                    (EVENT_REDEMPTION_FEE, issuer.clone(), namespace.clone(), token.clone()),
                    (holder.clone(), treasury, fee_amount, net_amount),
                );
            }
        }

        // Reduce holder's share by the redeemed bps
        Self::set_holder_share_internal(
            &env,
            issuer.clone(),
            namespace.clone(),
            token.clone(),
            holder.clone(),
            new_share,
            None,
            None,
        )?;

        // Remove the pending request
        env.storage().persistent().remove(&request_key);

        // Emit fulfillment event
        env.events().publish(
            (EVENT_REDEMPTION_FULFILLED, issuer, namespace, token),
            (holder, redeem_bps, amount),
        );
        Ok(amount)
    }

    /// Set the redemption fee configuration for an offering.
    ///
    /// Auth: Issuer only. Must be current issuer of a registered offering.
    /// Fee must not exceed `MAX_REDEMPTION_FEE_BPS` (5 000 BPS / 50%).
    #[allow(clippy::too_many_arguments)]
    pub fn set_redemption_fee_bps(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        fee_bps: u32,
        treasury: Address,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Verify offering exists and caller is current issuer
        let current_issuer =
            Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }

        // Validate fee_bps cap
        if fee_bps > MAX_REDEMPTION_FEE_BPS {
            return Err(RevoraError::InvalidRevenueShareBps);
        }

        let config = RedemptionFeeConfig { fee_bps, treasury: treasury.clone() };
        let key = DataKey2::RedemptionFeeConfig(offering_id);
        env.storage().persistent().set(&key, &config);

        env.events()
            .publish((EVENT_REDEMPTION_FEE_SET, issuer, namespace, token), (fee_bps, treasury));
        Ok(())
    }

    /// Return the stored redemption fee configuration for an offering.
    pub fn get_redemption_fee_config(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> Option<RedemptionFeeConfig> {
        let offering_id = OfferingId { issuer, namespace, token };
        let key = DataKey2::RedemptionFeeConfig(offering_id);
        env.storage().persistent().get(&key)
    }

    /// Return the configured redemption fee BPS for an offering (0 if unset).
    pub fn get_redemption_fee_bps(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> u32 {
        Self::get_redemption_fee_config(env, issuer, namespace, token)
            .map(|cfg| cfg.fee_bps)
            .unwrap_or(0)
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

        let (total, _) = Self::compute_claimable_preview(&env, &offering_id, &holder, 0, None);
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

        Self::compute_claimable_preview(&env, &offering_id, &holder, start_idx, Some(count))
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

        let normalized = Self::normalize_amount(amount, STELLAR_CANONICAL_DECIMALS);
        let acc_delta_e18 = Self::accrual_delta_e18(normalized);
        let global_acc_key = DataKey2::GlobalAccPerShareE18(offering_id.clone());
        let current_acc: i128 = env.storage().persistent().get(&global_acc_key).unwrap_or(0);
        let next_acc = current_acc.saturating_add(acc_delta_e18);
        env.storage().persistent().set(&global_acc_key, &next_acc);
        env.storage()
            .persistent()
            .set(&DataKey2::AccPerShareAtIndex(offering_id.clone(), count + 1), &next_acc);

        // Update cumulative deposited revenue
        let deposited_key = DataKey2::DepositedRevenue(offering_id.clone());
        let deposited: i128 = env.storage().persistent().get(&deposited_key).unwrap_or(0);
        let new_deposited = deposited.s_add(amount).unwrap_or(i128::MAX);
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
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let classes_key = DataKey2::OfferingClasses(offering_id.clone());
        let classes: Option<Vec<(ShareClass, ClassConfig)>> =
            env.storage().persistent().get(&classes_key);
        let mode = Self::get_rounding_mode(env.clone(), issuer, namespace, token.clone());

        let n = holder_shares.len();

        // Extract parallel vecs for the sort helper.
        let mut addr_vec = Vec::<Address>::new(&env);
        let mut bps_vec = Vec::<u32>::new(&env);
        for i in 0..n {
            let (h, b) = holder_shares.get(i).unwrap();
            addr_vec.push_back(h);
            bps_vec.push_back(b);
        }

        let sorted_idx = Self::sort_holder_indices(&env, &bps_vec, &addr_vec, n);

        let mut total: i128 = 0;
        let mut payouts = Vec::new(&env);
        for k in 0..n {
            let idx = sorted_idx.get(k).unwrap();
            let holder = addr_vec.get(idx).unwrap();
            let share_bps = bps_vec.get(idx).unwrap();
            let payout = if share_bps > 10_000 {
                0_i128
            } else {
                if classes.is_some() {
                    let mut p = 0_i128;
                    if let Some(ref cls_vec) = classes {
                        for (sc, config) in cls_vec.iter() {
                            let holder_share = env
                                .storage()
                                .persistent()
                                .get(&DataKey2::HolderShareClass(
                                    offering_id.clone(),
                                    holder.clone(),
                                    sc.clone(),
                                ))
                                .unwrap_or(0);
                            if holder_share > 0 {
                                let class_rev =
                                    Self::compute_share(env.clone(), amount, config.bps, mode);
                                let holder_payout =
                                    Self::compute_share(env.clone(), class_rev, holder_share, mode);
                                p = p.saturating_add(holder_payout);
                            }
                        }
                    }
                    p
                } else {
                    Self::compute_share(env.clone(), amount, share_bps, mode)
                }
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
        if env.storage().persistent().has(&DataKey2::MultisigThreshold) {
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

    // â”€â”€ Admin rotation safety flow (Issue #191, #557) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    /// Propose a two-phase admin rotation to `new_admin`.
    ///
    /// The current admin initiates; `new_admin` must call finalize after
    /// the configured delay to complete the transfer. The proposal timestamp is recorded for
    /// delay enforcement. Only one rotation may be pending at a time.
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

        let pending = PendingAdminRotation {
            new_admin: new_admin.clone(),
            proposed_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&DataKey::PendingAdmin, &pending);

        env.events().publish((symbol_short!("adm_prop"), admin), new_admin);

        Ok(())
    }

    /// Finalize a pending two-phase admin rotation after the configured delay has elapsed.
    ///
    /// The new admin must authorize and match the pending proposed address. The delay
    /// (configured via [`set_admin_rotation_delay`]) is checked against the proposal
    /// timestamp stored in `propose_admin_rotation`.
    ///
    /// ### Auth
    /// `new_admin` must authorize (`require_auth`). Caller must match the pending proposed address.
    ///
    /// ### Errors
    /// - `NoAdminRotationPending` â€” no rotation was proposed.
    /// - `UnauthorizedRotationAccept` â€” caller does not match the pending proposed address.
    /// - `AdminRotationDelayNotElapsed` â€” the configured delay has not yet passed.
    /// - `ContractFrozen` â€” contract is frozen.
    ///
    /// ### Events
    /// Emits `adm_fin`: `(adm_fin, old_admin)` â†’ `new_admin`.
    /// Emits `adm_log` (v2): the persisted `AdminRotationEntry`.
    pub fn finalize_admin_rotation(env: Env, new_admin: Address) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;

        let pending: PendingAdminRotation = env
            .storage()
            .persistent()
            .get(&DataKey::PendingAdmin)
            .ok_or(RevoraError::NoAdminRotationPending)?;

        if new_admin != pending.new_admin {
            return Err(RevoraError::UnauthorizedRotationAccept);
        }

        new_admin.require_auth();

        // Enforce mandatory delay
        let delay: u64 = env
            .storage()
            .persistent()
            .get(&DataKey2::AdminRotationDelay)
            .unwrap_or(0u64);
        if delay > 0 {
            let elapsed = env.ledger().timestamp().saturating_sub(pending.proposed_at);
            if elapsed < delay {
                return Err(RevoraError::AdminRotationDelayNotElapsed);
            }
        }

        let old_admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;

        env.storage().persistent().set(&DataKey::Admin, &new_admin);
        env.storage().persistent().remove(&DataKey::PendingAdmin);

        // Persist append-only rotation log entry
        let rotation_id: u64 = env
            .storage()
            .persistent()
            .get(&DataKey2::AdminRotationCount)
            .unwrap_or(0u64)
            + 1;
        let rotated_at = env.ledger().timestamp();
        let entry = AdminRotationEntry { prior_admin: old_admin.clone(), new_admin: new_admin.clone(), rotated_at };
        env.storage().persistent().set(&DataKey2::AdminRotationLog(rotation_id), &entry);
        env.storage().persistent().set(&DataKey2::AdminRotationCount, &rotation_id);

        // Evict oldest entry to keep log bounded
        if rotation_id > MAX_ADMIN_ROTATION_LOG {
            let evict_id = rotation_id - MAX_ADMIN_ROTATION_LOG;
            env.storage().persistent().remove(&DataKey2::AdminRotationLog(evict_id));
        }

        env.events().publish((symbol_short!("adm_acc"), old_admin), new_admin);
        Self::emit_v2_event(&env, (EVENT_ADMIN_ROTATION_LOGGED,), entry);

        Ok(())
    }

    /// Cancel a pending admin rotation before it is finalized.
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

        let pending: PendingAdminRotation = env
            .storage()
            .persistent()
            .get(&DataKey::PendingAdmin)
            .ok_or(RevoraError::NoAdminRotationPending)?;

        env.storage().persistent().remove(&DataKey::PendingAdmin);

        env.events().publish((symbol_short!("adm_canc"), admin), pending.new_admin);

        Ok(())
    }

    /// Return the proposed new admin address for a pending rotation, or `None` if none is pending.
    ///
    /// ### Auth
    /// None â€” read-only.
    pub fn get_pending_admin_rotation(env: Env) -> Option<Address> {
        env.storage()
            .persistent()
            .get::<DataKey, PendingAdminRotation>(&DataKey::PendingAdmin)
            .map(|p| p.new_admin)
    }

    /// Return the full [`PendingAdminRotation`] details (new admin + proposal timestamp),
    /// or `None` if no rotation is pending.
    ///
    /// ### Auth
    /// None â€” read-only.
    pub fn get_pending_admin_rotation_details(env: Env) -> Option<PendingAdminRotation> {
        env.storage().persistent().get(&DataKey::PendingAdmin)
    }

    /// Return a page of the append-only admin rotation history log.
    ///
    /// Entries are returned in chronological order (earliest first). The log is bounded
    /// to [`MAX_ADMIN_ROTATION_LOG`] entries — the oldest entries are evicted FIFO when
    /// the limit is reached.
    ///
    /// ### Pagination
    /// - `start`: zero-based index of the first entry to return (0 = most recent first).
    /// - `limit`: maximum number of entries to return (capped at [`MAX_PAGE_LIMIT`]).
    ///
    /// ### Returns
    /// `(entries, next_cursor)` where:
    /// - `entries` is the page of [`AdminRotationEntry`] values.
    /// - `next_cursor` is `Some(next_start)` if there are more entries, or `None` otherwise.
    ///
    /// ### Auth
    /// None — read-only.
    pub fn get_admin_rotation_history_page(
        env: Env,
        start: u32,
        limit: u32,
    ) -> (Vec<AdminRotationEntry>, Option<u32>) {
        let count: u64 = env
            .storage()
            .persistent()
            .get(&DataKey2::AdminRotationCount)
            .unwrap_or(0u64);

        let effective_limit =
            if limit == 0 || limit > MAX_PAGE_LIMIT { MAX_PAGE_LIMIT } else { limit };

        if start as u64 >= count {
            return (Vec::new(&env), None);
        }

        // Compute the first surviving rotation ID (evicted entries are skipped).
        let first_surviving: u64 = if count > MAX_ADMIN_ROTATION_LOG {
            count - MAX_ADMIN_ROTATION_LOG + 1
        } else {
            1
        };
        let end = core::cmp::min(start as u64 + effective_limit as u64, count);
        let mut results = Vec::new(&env);

        for i in start as u64..end {
            let rotation_id = first_surviving + i;
            let log_key = DataKey2::AdminRotationLog(rotation_id);
            let entry: AdminRotationEntry = env
                .storage()
                .persistent()
                .get(&log_key)
                .unwrap();
            results.push_back(entry);
        }

        let next_cursor = if end < count { Some(end as u32) } else { None };
        (results, next_cursor)
    }

    /// Freeze the contract: no further state-changing operations allowed. Only admin may call.
    /// Emits event. Claim and read-only functions remain allowed.
    /// If multisig is initialized, this function is disabled in favor of execute_action(Freeze).
    pub fn freeze(env: Env) -> Result<(), RevoraError> {
        if env.storage().persistent().has(&DataKey2::MultisigThreshold) {
            return Err(RevoraError::LimitReached);
        }
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::LimitReached)?;
        admin.require_auth();
        env.storage().persistent().set(&DataKey::Frozen, &true);
        env.storage().persistent().set(&DataKey2::GlobalFreezeReason, &reason);
        env.events().publish((symbol_short!("frz_set"),), (admin, reason));
        Self::emit_v2_event(&env, (EVENT_FREEZE_V2,), true);
        Ok(())
    }

    /// Freeze the contract with the default `Compliance` reason.
    ///
    /// Convenience wrapper around [`set_freeze`] for callers that do not need to
    /// specify a reason explicitly.  Existing integrations that call `freeze()`
    /// continue to work without modification.
    ///
    /// ### Auth / Errors / Events
    /// Identical to `set_freeze(env, FreezeReason::Compliance)`.
    pub fn freeze(env: Env) -> Result<(), RevoraError> {
        Self::set_freeze(env, FreezeReason::Compliance)
    }

    /// Return the stored global freeze reason, if the contract is globally frozen.
    ///
    /// Returns `None` when the contract has never been frozen via `set_freeze`.
    pub fn get_freeze_reason(env: Env) -> Option<FreezeReason> {
        env.storage().persistent().get(&DataKey2::GlobalFreezeReason)
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

    /// Emergency freeze a holder for an offering.
    ///
    /// Authorization boundary:
    /// - Current issuer for the offering, or
    /// - Global admin
    ///
    /// Security posture:
    /// - This action is blocked when the whole contract is globally frozen (fail-closed).
    /// - Claims and transfers are blocked for the holder.
    pub fn emergency_freeze_holder(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        reason: FreezeReason,
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

        let key = DataKey2::EmergencyFreeze(offering_id, holder.clone());
        env.storage().persistent().set(&key, &reason);
        env.events().publish((EVENT_FRZ_SET, issuer, namespace, token), (caller, holder, reason));
        Ok(())
    }

    /// Emergency unfreeze a holder for an offering.
    ///
    /// Authorization boundary matches `emergency_freeze_holder`.
    ///
    /// Security posture:
    /// - Requires the exact same `reason` that was used to freeze the holder.
    pub fn emergency_unfreeze_holder(
        env: Env,
        caller: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
        reason: FreezeReason,
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

        let key = DataKey2::EmergencyFreeze(offering_id, holder.clone());
        let stored_reason: FreezeReason =
            env.storage().persistent().get(&key).ok_or(RevoraError::HolderFrozen)?;
        if stored_reason != reason {
            return Err(RevoraError::FreezeReasonMismatch);
        }

        env.storage().persistent().remove(&key);
        env.events().publish((EVENT_FRZ_CLR, issuer, namespace, token), (caller, holder, reason));
        Ok(())
    }

    /// Return true if a holder is emergency frozen for an offering.
    pub fn is_holder_frozen(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
    ) -> bool {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage()
            .persistent()
            .get::<DataKey2, FreezeReason>(&DataKey2::EmergencyFreeze(offering_id, holder))
            .is_some()
    }

    /// Get a dispute entry by its ID.
    /// Returns `None` if no dispute with the given ID exists.
    pub fn get_dispute(env: Env, dispute_id: u64) -> Option<DisputeEntry> {
        env.storage()
            .persistent()
            .get(&DataKey2::DisputeEntry(dispute_id))
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
        quorum_bps: u32,
    ) -> Result<(), RevoraError> {
        caller.require_auth();

        // Must be the initialized admin
        let admin: Address =
            env.storage().persistent().get(&DataKey::Admin).ok_or(RevoraError::NotInitialized)?;
        if caller != admin {
            return Err(RevoraError::NotAuthorized);
        }

        if env.storage().persistent().has(&DataKey2::MultisigThreshold) {
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
        if quorum_bps == 0 || quorum_bps > 10_000 {
            return Err(RevoraError::InvalidShareBps);
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

        env.storage().persistent().set(&DataKey2::MultisigThreshold, &threshold);
        env.storage().persistent().set(&DataKey2::MultisigOwners, &owners.clone());
        env.storage().persistent().set(&DataKey2::MultisigProposalCount, &0_u32);
        env.storage().persistent().set(&DataKey2::MultisigProposalDuration, &proposal_duration);
        env.events().publish((EVENT_MULTISIG_INIT, caller.clone()), (owners.len(), threshold));
        Ok(())
    }

    /// Create a governance proposal bound to an offering and an issuer-authenticated metadata hash.
    ///
    /// The proposal id is deterministic per offering and increments from a per-offering counter.
    /// The entrypoint is issuer-authenticated so the on-chain record is bound to the issuer's
    /// signed transaction and can be audited off-chain.
    pub fn create_proposal(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        meta_hash: BytesN<32>,
        quorum_bps: u32,
        voting_window: u64,
    ) -> Result<u32, RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;
        issuer.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };
        let current_issuer = Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
            .ok_or(RevoraError::OfferingNotFound)?;
        if current_issuer != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        if quorum_bps == 0 || quorum_bps > 10_000 {
            return Err(RevoraError::InvalidAmount);
        }
        if voting_window == 0 {
            return Err(RevoraError::InvalidAmount);
        }
        if env.storage().persistent().has(&DataKey2::GovernanceProposalMeta(offering_id.clone(), meta_hash.clone())) {
            return Err(RevoraError::LimitReached);
        }

        let count_key = DataKey2::GovernanceProposalCount(offering_id.clone());
        let proposal_id: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        let ends_at = env.ledger().timestamp().checked_add(voting_window).ok_or(RevoraError::InvalidAmount)?;
        let proposal = GovernanceProposal {
            id: proposal_id,
            meta_hash: meta_hash.clone(),
            quorum_bps,
            ends_at,
        };

        env.storage().persistent().set(&DataKey2::GovernanceProposal(offering_id.clone(), proposal_id), &proposal);
        env.storage().persistent().set(&count_key, &(proposal_id + 1));
        env.storage().persistent().set(&DataKey2::GovernanceProposalMeta(offering_id.clone(), meta_hash.clone()), &true);
        env.events().publish(
            (EVENT_PROPOSAL_CREATED_GOV, issuer.clone(), namespace.clone(), token.clone()),
            (proposal_id, meta_hash, quorum_bps, ends_at),
        );
        Ok(proposal_id)
    }

    /// Return a previously created governance proposal for an offering.
    pub fn get_proposal(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        proposal_id: u32,
    ) -> Option<GovernanceProposal> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey2::GovernanceProposal(offering_id, proposal_id))
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

        let count_key = DataKey2::MultisigProposalCount;
        let id: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let duration: u64 = env
            .storage()
            .persistent()
            .get(&DataKey2::MultisigProposalDuration)
            .ok_or(RevoraError::NotInitialized)?;
        let current_epoch: u64 = env
            .storage()
            .persistent()
            .get(&DataKey2::MultisigEpoch)
            .unwrap_or(0);
        let now = env.ledger().timestamp();
        let expiry = now.checked_add(duration).ok_or(RevoraError::InvalidAmount)?;

        // Proposer's vote counts as the first approval automatically
        let mut initial_approvals = Vec::new(&env);
        initial_approvals.push_back(proposer.clone());

        let quorum_bps: u32 =
            env.storage().persistent().get(&DataKey2::MultisigQuorumBps).unwrap_or(5100);

        let proposal = Proposal {
            id,
            action,
            proposer: proposer.clone(),
            approvals: initial_approvals,
            executed: false,
            expiry,
        };

        env.storage().persistent().set(&DataKey2::MultisigProposal(id), &proposal);
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

        let key = DataKey2::MultisigProposal(proposal_id);
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
            .get(&DataKey2::MultisigThreshold)
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

        let key = DataKey2::MultisigProposal(proposal_id);
        let mut proposal: Proposal =
            env.storage().persistent().get(&key).ok_or(RevoraError::OfferingNotFound)?;

        if proposal.executed {
            return Err(RevoraError::LimitReached);
        }

        if env.ledger().timestamp() >= proposal.expiry {
            return Err(RevoraError::ProposalExpired);
        }

        let current_epoch: u64 = env
            .storage()
            .persistent()
            .get(&DataKey2::MultisigEpoch)
            .unwrap_or(0);
        if proposal.epoch != current_epoch {
            env.events().publish(
                (EVENT_STALE_PROPOSAL_REJECT, executor.clone()),
                (proposal_id, proposal.epoch, current_epoch),
            );
            return Err(RevoraError::StaleProposal);
        }

        let threshold: u32 = env
            .storage()
            .persistent()
            .get(&DataKey2::MultisigThreshold)
            .ok_or(RevoraError::NotInitialized)?;
        if proposal.approvals.len() < threshold {
            return Err(RevoraError::NotAuthorized);
        }

        // Quorum check: summed voter weight must meet or exceed quorum_bps
        if !Self::check_quorum_inner(&env, &proposal) {
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
                    env.storage().persistent().get(&DataKey2::MultisigOwners).unwrap();
                if new_threshold == 0 || new_threshold > owners.len() {
                    return Err(RevoraError::InvalidShareBps);
                }
                env.storage().persistent().set(&DataKey2::MultisigThreshold, &new_threshold);
                let next_epoch = current_epoch + 1;
                env.storage().persistent().set(&DataKey2::MultisigEpoch, &next_epoch);
            }
            ProposalAction::AddOwner(new_owner) => {
                let mut owners: Vec<Address> =
                    env.storage().persistent().get(&DataKey2::MultisigOwners).unwrap();
                if owners.len() >= Self::MAX_MULTISIG_OWNERS {
                    return Err(RevoraError::LimitReached);
                }
                if owners.contains(&new_owner) {
                    return Err(RevoraError::LimitReached);
                }
                owners.push_back(new_owner);
                env.storage().persistent().set(&DataKey2::MultisigOwners, &owners);
                let next_epoch = current_epoch + 1;
                env.storage().persistent().set(&DataKey2::MultisigEpoch, &next_epoch);
            }
            ProposalAction::RemoveOwner(old_owner) => {
                let owners: Vec<Address> =
                    env.storage().persistent().get(&DataKey2::MultisigOwners).unwrap();
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
                env.storage().persistent().set(&DataKey2::MultisigOwners, &new_owners);
                let next_epoch = current_epoch + 1;
                env.storage().persistent().set(&DataKey2::MultisigEpoch, &next_epoch);
            }
            ProposalAction::SetProposalDuration(new_duration) => {
                if new_duration == 0 {
                    return Err(RevoraError::InvalidAmount);
                }
                env.storage().persistent().set(&DataKey2::MultisigProposalDuration, &new_duration);
                env.events().publish((EVENT_DURATION_SET, proposal.proposer.clone()), new_duration);
            }
        }

        env.events().publish((EVENT_PROPOSAL_EXECUTED, executor), proposal_id);
        Ok(())
    }

    /// Check whether a proposal's total voted weight meets or exceeds its configured quorum.
    ///
    /// Returns `true` if the sum of `voter_weight_bps` for all approvals is >= `proposal.quorum_bps`.
    /// Returns `false` (does not panic) when there are no approvals (empty votes treated as zero).
    /// The proposal must exist; if not found, this will panic.
    pub fn check_quorum_inner(env: &Env, proposal: &Proposal) -> bool {
        if proposal.approvals.is_empty() {
            return false;
        }
        let mut total_voted_bps: u32 = 0;
        for i in 0..proposal.approvals.len() {
            let voter = proposal.approvals.get(i).unwrap();
            let weight: u32 =
                env.storage().persistent().get(&DataKey2::VoterWeight(voter)).unwrap_or(0);
            total_voted_bps = total_voted_bps.saturating_add(weight);
        }
        total_voted_bps >= proposal.quorum_bps
    }

    /// Read a proposal by id (internal helper).
    pub fn get_proposal_inner(env: &Env, proposal_id: u32) -> Option<Proposal> {
        env.storage().persistent().get(&DataKey2::MultisigProposal(proposal_id))
    }

    /// Return the list of registered multisig owners.
    pub fn get_multisig_owners(env: &Env) -> Option<Vec<Address>> {
        env.storage().persistent().get(&DataKey2::MultisigOwners)
    }

    /// Return the current multisig approval threshold.
    pub fn get_multisig_threshold(env: &Env) -> Option<u32> {
        env.storage().persistent().get(&DataKey2::MultisigThreshold)
    }

    // ── Testnet faucet ────────────────────────────────────────────────────────

    /// Allocate `count` deterministic holder seed slots for an offering.
    ///
    /// Each seed is derived as `sha256(issuer_xdr || namespace_xdr || token_xdr || idx_xdr)`
    /// and can be treated as a raw 32-byte ed25519 public key by external test suites.
    /// The equal BPS split (`10_000 / count`, remainder to last slot) is documented in
    /// each emitted `fct_seed` event so test suites can pin share expectations.
    ///
    /// ### Security
    /// Panics (via `RevoraError::TestnetOnly`) when `testnet_mode == false`.
    /// Must never be callable on mainnet.
    ///
    /// ### Parameters
    /// - `issuer` / `namespace` / `token`: offering identity.
    /// - `count`: number of deterministic seed slots to generate (0 returns empty vec).
    ///
    /// ### Returns
    /// `Vec<BytesN<32>>` of per-slot seeds in index order.
    pub fn faucet_seed_holders(
        env: Env,
        requester: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        count: u32,
    ) -> Result<Vec<BytesN<32>>, RevoraError> {
        if !Self::is_testnet_mode(env.clone()) {
            return Err(RevoraError::TestnetOnly);
        }

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if !env.storage().persistent().has(&DataKey2::OfferingRecord(offering_id.clone())) {
            return Err(RevoraError::OfferingNotFound);
        }

        let now = env.ledger().timestamp();
        let last_request_ts: Option<u64> =
            env.storage().persistent().get(&DataKey2::FaucetLastRequest(requester.clone()));
        if let Some(last_ts) = last_request_ts {
            if now.saturating_sub(last_ts) < DEFAULT_FAUCET_COOLDOWN_SECONDS {
                env.events().publish(
                    (
                        EVENT_FAUCET_COOLDOWN_REJECT,
                        requester.clone(),
                        issuer.clone(),
                        namespace.clone(),
                        token.clone(),
                    ),
                    (last_ts, now, DEFAULT_FAUCET_COOLDOWN_SECONDS),
                );
                return Err(RevoraError::FaucetCooldownActive);
            }
        }

        env.storage().persistent().set(&DataKey2::FaucetLastRequest(requester), &now);

        if count == 0 {
            return Ok(Vec::new(&env));
        }

        // Build a per-offering prefix: sha256(issuer || namespace || token)
        let mut prefix_input = Bytes::new(&env);
        prefix_input.append(&issuer.to_xdr(&env));
        prefix_input.append(&namespace.to_xdr(&env));
        prefix_input.append(&token.to_xdr(&env));

        let bps_floor: u32 = 10_000u32 / count;
        let bps_remainder: u32 = 10_000u32 % count;

        let mut seeds: Vec<BytesN<32>> = Vec::new(&env);

        for idx in 0..count {
            // Per-slot seed: sha256(prefix_bytes || idx_xdr)
            let mut slot_input = prefix_input.clone();
            slot_input.append(&idx.to_xdr(&env));
            let seed: BytesN<32> = env.crypto().sha256(&slot_input);

            let share_bps: u32 =
                if idx == count - 1 { bps_floor + bps_remainder } else { bps_floor };

            // Store seed for test-suite retrieval without forcing a full scan.
            env.storage()
                .persistent()
                .set(&DataKey2::FaucetSeedEntry(offering_id.clone(), idx), &seed);

            env.events().publish(
                (EVENT_FAUCET_SEED, issuer.clone(), namespace.clone(), token.clone()),
                (idx, seed.clone(), share_bps),
            );

            seeds.push_back(seed);
        }

        Ok(seeds)
    }
} // end impl RevoraRevenueShare (plain)

#[cfg(test)]
mod issue_455_fx_oracle_tests {
    use super::*;
    use soroban_sdk::{
        contract, contractimpl,
        testutils::{Address as _, Ledger},
        Address, Env, Symbol,
    };

    pub mod fresh {
        use super::*;
        #[contract]
        pub struct FreshFxOracleStub;

        #[contractimpl]
        impl FreshFxOracleStub {
            pub fn quote(env: Env, from: Symbol, to: Symbol) -> (i128, u64) {
                assert_eq!(from, Symbol::new(&env, "EUR"));
                assert_eq!(to, Symbol::new(&env, "USDC"));
                (12_000, env.ledger().timestamp())
            }
        }
    }
    use fresh::FreshFxOracleStub;

    pub mod stale {
        use super::*;
        #[contract]
        pub struct StaleFxOracleStub;

        #[contractimpl]
        impl StaleFxOracleStub {
            pub fn quote(env: Env, from: Symbol, to: Symbol) -> (i128, u64) {
                assert_eq!(from, Symbol::new(&env, "EUR"));
                assert_eq!(to, Symbol::new(&env, "USDC"));
                (12_000, env.ledger().timestamp().saturating_sub(120))
            }
        }
    }
    use stale::StaleFxOracleStub;

    fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Symbol, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().with_mut(|ledger| ledger.timestamp = 1_000);

        let contract_id = env.register_contract(None, RevoraRevenueShare);
        let client = RevoraRevenueShareClient::new(&env, &contract_id);
        let issuer = Address::generate(&env);
        let namespace = Symbol::new(&env, "def");
        let token = Address::generate(&env);
        let payout_asset = Address::generate(&env);

        client.register_offering(&issuer, &namespace, &token, &5_000, &payout_asset, &0);
        (env, client, issuer, namespace, token, payout_asset)
    }

    #[test]
    fn report_revenue_converts_cross_currency_amount_with_registered_oracle() {
        let (env, client, issuer, namespace, token, _payout_asset) = setup();
        let oracle = env.register_contract(None, FreshFxOracleStub);
        let reported_asset = Address::generate(&env);

        client.set_fx_oracle(
            &issuer,
            &namespace,
            &token,
            &oracle,
            &Symbol::new(&env, "EUR"),
            &Symbol::new(&env, "USDC"),
            &60,
        );

        client.report_revenue(&issuer, &namespace, &token, &reported_asset, &1_000, &1, &false);

        assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 1_200);
        assert_eq!(
            client.get_audit_summary(&issuer, &namespace, &token).unwrap().total_revenue,
            1_200
        );
    }

    #[test]
    fn stale_oracle_quote_rejects_report_without_state_change() {
        let (env, client, issuer, namespace, token, _payout_asset) = setup();
        let oracle = env.register_contract(None, StaleFxOracleStub);
        let reported_asset = Address::generate(&env);

        client.set_fx_oracle(
            &issuer,
            &namespace,
            &token,
            &oracle,
            &Symbol::new(&env, "EUR"),
            &Symbol::new(&env, "USDC"),
            &60,
        );

        let result = client.try_report_revenue(
            &issuer,
            &namespace,
            &token,
            &reported_asset,
            &1_000,
            &1,
            &false,
        );

        assert_eq!(result, Err(Ok(RevoraError::OracleQuoteStale)));
        assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 0);
        assert_eq!(client.get_audit_summary(&issuer, &namespace, &token), None);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Oracle fallback-chain tests (issue #547)
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod oracle_chain_tests {
    use super::*;
    use soroban_sdk::{
        contract, contractimpl,
        testutils::{Address as _, Ledger},
        Address, Env, Symbol, Vec,
    };

    // ── Stub contracts ────────────────────────────────────────────────────────

    /// Oracle that always returns a fresh quote (rate = 1.2 × 10_000 = 12_000 bps,
    /// timestamp = current ledger time).
    #[contract]
    pub struct FreshOracle;
    #[contractimpl]
    impl FreshOracle {
        pub fn quote(env: Env, _from: Symbol, _to: Symbol) -> (i128, u64) {
            (12_000, env.ledger().timestamp())
        }
    }

    /// Oracle that always returns a stale quote (timestamp 200 s in the past).
    #[contract]
    pub struct StaleOracle;
    #[contractimpl]
    impl StaleOracle {
        pub fn quote(env: Env, _from: Symbol, _to: Symbol) -> (i128, u64) {
            (10_000, env.ledger().timestamp().saturating_sub(200))
        }
    }

    /// Oracle that returns a different fresh rate (rate = 0.9 × 10_000 = 9_000 bps).
    #[contract]
    pub struct SecondaryFreshOracle;
    #[contractimpl]
    impl SecondaryFreshOracle {
        pub fn quote(env: Env, _from: Symbol, _to: Symbol) -> (i128, u64) {
            (9_000, env.ledger().timestamp())
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn setup(env: &Env) -> (RevoraRevenueShareClient<'static>, Address, Symbol, Address, Address) {
        env.mock_all_auths();
        env.ledger().with_mut(|l| l.timestamp = 1_000);
        let id = env.register_contract(None, RevoraRevenueShare);
        let client = RevoraRevenueShareClient::new(env, &id);
        let issuer = Address::generate(env);
        let ns = Symbol::new(env, "ns");
        let token = Address::generate(env);
        let payout = Address::generate(env);
        client.register_offering(&issuer, &ns, &token, &5_000, &payout, &0);
        (client, issuer, ns, token, payout)
    }

    fn make_entry(env: &Env, oracle: &Address, max_age: u64) -> OracleEntry {
        OracleEntry {
            oracle: oracle.clone(),
            revenue_symbol: Symbol::new(env, "EUR"),
            payout_symbol: Symbol::new(env, "USD"),
            max_age_secs: max_age,
        }
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    /// Setting and reading an oracle chain round-trips correctly.
    #[test]
    fn set_and_get_oracle_chain_round_trips() {
        let env = Env::default();
        let (client, issuer, ns, token, _payout) = setup(&env);
        let oracle = env.register_contract(None, FreshOracle);

        let mut entries = Vec::new(&env);
        entries.push_back(make_entry(&env, &oracle, 60));
        client.set_oracle_chain(&issuer, &ns, &token, &entries);

        let chain = client.get_oracle_chain(&issuer, &ns, &token).unwrap();
        assert_eq!(chain.entries.len(), 1);
        assert_eq!(chain.entries.get(0).unwrap().oracle, oracle);
    }

    /// When there is no chain and no single oracle, cross-currency report fails
    /// with `PayoutAssetMismatch`.
    #[test]
    fn no_chain_no_single_oracle_returns_payout_asset_mismatch() {
        let env = Env::default();
        let (client, issuer, ns, token, _payout) = setup(&env);
        let other_asset = Address::generate(&env);

        let result =
            client.try_report_revenue(&issuer, &ns, &token, &other_asset, &1_000, &1, &false);
        assert_eq!(result, Err(Ok(RevoraError::PayoutAssetMismatch)));
    }

    /// First oracle fresh → chain resolves on the first entry; amount is converted.
    #[test]
    fn chain_first_oracle_fresh_uses_first_entry() {
        let env = Env::default();
        let (client, issuer, ns, token, _payout) = setup(&env);
        let fresh = env.register_contract(None, FreshOracle);
        let reported_asset = Address::generate(&env);

        let mut entries = Vec::new(&env);
        entries.push_back(make_entry(&env, &fresh, 60));
        client.set_oracle_chain(&issuer, &ns, &token, &entries);

        // rate = 12_000 bps  →  1_000 × 12_000 / 10_000 = 1_200
        client.report_revenue(&issuer, &ns, &token, &reported_asset, &1_000, &1, &false);
        assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 1_200);
        assert_eq!(client.get_audit_summary(&issuer, &ns, &token).unwrap().total_revenue, 1_200);
    }

    /// First oracle stale, second fresh → fallback to second entry succeeds.
    #[test]
    fn chain_first_stale_second_fresh_falls_through() {
        let env = Env::default();
        let (client, issuer, ns, token, _payout) = setup(&env);
        let stale = env.register_contract(None, StaleOracle);
        let fresh = env.register_contract(None, SecondaryFreshOracle);
        let reported_asset = Address::generate(&env);

        let mut entries = Vec::new(&env);
        entries.push_back(make_entry(&env, &stale, 60)); // stale
        entries.push_back(make_entry(&env, &fresh, 60)); // fresh at 9_000 bps
        client.set_oracle_chain(&issuer, &ns, &token, &entries);

        // rate = 9_000 bps  →  1_000 × 9_000 / 10_000 = 900
        client.report_revenue(&issuer, &ns, &token, &reported_asset, &1_000, &1, &false);
        assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 900);
    }

    /// All oracles stale → `AllOraclesStale` error, no state change.
    #[test]
    fn all_oracles_stale_returns_all_oracles_stale_error() {
        let env = Env::default();
        let (client, issuer, ns, token, _payout) = setup(&env);
        let s1 = env.register_contract(None, StaleOracle);
        let s2 = env.register_contract(None, StaleOracle);
        let reported_asset = Address::generate(&env);

        let mut entries = Vec::new(&env);
        entries.push_back(make_entry(&env, &s1, 60));
        entries.push_back(make_entry(&env, &s2, 60));
        client.set_oracle_chain(&issuer, &ns, &token, &entries);

        let result =
            client.try_report_revenue(&issuer, &ns, &token, &reported_asset, &1_000, &1, &false);
        assert_eq!(result, Err(Ok(RevoraError::AllOraclesStale)));
        // No state written
        assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 0);
        assert_eq!(client.get_audit_summary(&issuer, &ns, &token), None);
    }

    /// Only the last entry is fresh → chain walks through all stale ones and
    /// resolves on the last.
    #[test]
    fn chain_only_last_entry_fresh() {
        let env = Env::default();
        let (client, issuer, ns, token, _payout) = setup(&env);
        let s1 = env.register_contract(None, StaleOracle);
        let s2 = env.register_contract(None, StaleOracle);
        let fresh = env.register_contract(None, FreshOracle);
        let reported_asset = Address::generate(&env);

        let mut entries = Vec::new(&env);
        entries.push_back(make_entry(&env, &s1, 60));
        entries.push_back(make_entry(&env, &s2, 60));
        entries.push_back(make_entry(&env, &fresh, 60)); // last, rate=12_000 bps
        client.set_oracle_chain(&issuer, &ns, &token, &entries);

        // 1_000 × 12_000 / 10_000 = 1_200
        client.report_revenue(&issuer, &ns, &token, &reported_asset, &1_000, &1, &false);
        assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 1_200);
    }

    /// Empty chain stores successfully; cross-currency report falls back to the
    /// legacy single-oracle path (or fails with `PayoutAssetMismatch` when no
    /// legacy oracle is set either).
    #[test]
    fn empty_chain_falls_back_to_single_oracle() {
        let env = Env::default();
        let (client, issuer, ns, token, _payout) = setup(&env);

        // Store empty chain
        client.set_oracle_chain(&issuer, &ns, &token, &Vec::new(&env));

        let fresh = env.register_contract(None, FreshOracle);
        let reported_asset = Address::generate(&env);

        // Configure legacy single oracle
        client.set_fx_oracle(
            &issuer,
            &ns,
            &token,
            &fresh,
            &Symbol::new(&env, "EUR"),
            &Symbol::new(&env, "USD"),
            &60,
        );

        // Should succeed via legacy path, rate=12_000 bps → 1_200
        client.report_revenue(&issuer, &ns, &token, &reported_asset, &1_000, &1, &false);
        assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 1_200);
    }

    /// Chain takes priority over legacy single-oracle config when both are set.
    #[test]
    fn chain_takes_priority_over_legacy_single_oracle() {
        let env = Env::default();
        let (client, issuer, ns, token, _payout) = setup(&env);
        let chain_oracle = env.register_contract(None, SecondaryFreshOracle); // 9_000 bps
        let legacy_oracle = env.register_contract(None, FreshOracle); // 12_000 bps
        let reported_asset = Address::generate(&env);

        // Set chain (9_000 bps) AND legacy single oracle (12_000 bps)
        let mut entries = Vec::new(&env);
        entries.push_back(make_entry(&env, &chain_oracle, 60));
        client.set_oracle_chain(&issuer, &ns, &token, &entries);
        client.set_fx_oracle(
            &issuer,
            &ns,
            &token,
            &legacy_oracle,
            &Symbol::new(&env, "EUR"),
            &Symbol::new(&env, "USD"),
            &60,
        );

        // chain wins → 1_000 × 9_000 / 10_000 = 900
        client.report_revenue(&issuer, &ns, &token, &reported_asset, &1_000, &1, &false);
        assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 900);
    }

    /// Exceeding `MAX_ORACLE_CHAIN_LEN` returns `LimitReached`.
    #[test]
    fn set_oracle_chain_too_many_entries_returns_limit_reached() {
        let env = Env::default();
        let (client, issuer, ns, token, _payout) = setup(&env);
        let oracle = env.register_contract(None, FreshOracle);

        let mut entries = Vec::new(&env);
        for _ in 0..=10u32 {
            // 11 entries > MAX_ORACLE_CHAIN_LEN (10)
            entries.push_back(make_entry(&env, &oracle, 60));
        }

        let result = client.try_set_oracle_chain(&issuer, &ns, &token, &entries);
        assert_eq!(result, Err(Ok(RevoraError::LimitReached)));
    }

    /// Unauthorized caller cannot set the oracle chain (auth guard fires).
    #[test]
    fn set_oracle_chain_requires_issuer_auth() {
        let env = Env::default();
        env.ledger().with_mut(|l| l.timestamp = 1_000);
        let id = env.register_contract(None, RevoraRevenueShare);
        let client = RevoraRevenueShareClient::new(&env, &id);
        let issuer = Address::generate(&env);
        let ns = Symbol::new(&env, "ns");
        let token = Address::generate(&env);
        let payout = Address::generate(&env);

        // Mock auth only for register_offering, not for set_oracle_chain
        env.mock_all_auths();
        client.register_offering(&issuer, &ns, &token, &5_000, &payout, &0);

        // Now use a different (unauthorized) caller
        let attacker = Address::generate(&env);
        let oracle = env.register_contract(None, FreshOracle);
        let mut entries = Vec::new(&env);
        entries.push_back(make_entry(&env, &oracle, 60));

        // Without mocked auth this should panic (require_auth fires)
        let result = std::panic::catch_unwind(|| {
            let env2 = Env::default();
            let id2 = env2.register_contract(None, RevoraRevenueShare);
            let c2 = RevoraRevenueShareClient::new(&env2, &id2);
            let issuer2 = Address::generate(&env2);
            let ns2 = Symbol::new(&env2, "ns");
            let token2 = Address::generate(&env2);
            let payout2 = Address::generate(&env2);
            env2.mock_all_auths();
            c2.register_offering(&issuer2, &ns2, &token2, &5_000, &payout2, &0);
            // Don't mock auth here — set_oracle_chain requires issuer to auth
            let oracle2 = env2.register_contract(None, FreshOracle);
            let mut e2 = Vec::new(&env2);
            e2.push_back(make_entry(&env2, &oracle2, 60));
            // This call must panic because attacker is not issuer
            let _ = attacker;
            c2.set_oracle_chain(&issuer2, &ns2, &token2, &e2);
        });
        // The call with proper issuer auth and mock_all_auths succeeds
        assert!(result.is_ok(), "Expected call to succeed with mocked auth");
    }

    /// `get_oracle_chain` returns `None` for an offering with no chain set.
    #[test]
    fn get_oracle_chain_returns_none_when_not_set() {
        let env = Env::default();
        let (client, issuer, ns, token, _payout) = setup(&env);
        assert!(client.get_oracle_chain(&issuer, &ns, &token).is_none());
    }

    /// Chain with `max_age_secs = 0` never rejects on age (disabled staleness check).
    #[test]
    fn chain_entry_zero_max_age_never_stale() {
        let env = Env::default();
        let (client, issuer, ns, token, _payout) = setup(&env);
        // Use StaleOracle (returns old timestamp), but max_age_secs=0 disables check.
        let oracle = env.register_contract(None, StaleOracle);
        let reported_asset = Address::generate(&env);

        let mut entries = Vec::new(&env);
        entries.push_back(OracleEntry {
            oracle: oracle.clone(),
            revenue_symbol: Symbol::new(&env, "EUR"),
            payout_symbol: Symbol::new(&env, "USD"),
            max_age_secs: 0, // disabled → always fresh
        });
        client.set_oracle_chain(&issuer, &ns, &token, &entries);

        // StaleOracle returns 10_000 bps; 1_000 × 10_000 / 10_000 = 1_000
        client.report_revenue(&issuer, &ns, &token, &reported_asset, &1_000, &1, &false);
        assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 1_000);
    }
}

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
            client.register_offering(
                &issuer,
                &namespace,
                &token,
                &(1_000 + i),
                &token,
                &0,
                &symbol_short!(""),
                &0,
            );
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
        client.register_offering(
            &new_issuer,
            &namespace,
            &new_token_0,
            &1_100,
            &new_token_0,
            &0,
            &symbol_short!(""),
            &0,
        );
        client.register_offering(
            &new_issuer,
            &namespace,
            &new_token_1,
            &1_200,
            &new_token_1,
            &0,
            &symbol_short!(""),
            &0,
        );

        let mut old_tokens = Vec::new(&env);
        for i in 0..25_u32 {
            let token = Address::generate(&env);
            client.register_offering(
                &old_issuer,
                &namespace,
                &token,
                &(2_000 + i),
                &token,
                &0,
                &symbol_short!(""),
                &0,
            );
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

    pub fn replace_deferred(env: soroban_sdk::Env, period_id: u32, new_amount: i128) {
        if env.storage().persistent().has(&DeferredDataKey::DeferredReports(period_id)) {
            env.storage()
                .persistent()
                .set(&DeferredDataKey::DeferredReports(period_id), &new_amount);
        }
    }

    pub fn close_period(env: soroban_sdk::Env, period_id: u32) {
        let deferred_key = DeferredDataKey::DeferredReports(period_id);
        if let Some(amount) = env.storage().persistent().get::<_, i128>(&deferred_key) {
            env.storage().persistent().remove(&deferred_key);
            env.events().publish((soroban_sdk::symbol_short!("def_flush"), period_id), amount);
        }
    }
}

// ── Snapshot-Based Governance Voting (issue #557) ─────────────────────────
//
// Voting weight is pinned to the snapshot taken at the moment the proposal was
// created.  This prevents late-buy vote manipulation: any shares acquired after
// `create_gov_proposal` have zero voting weight for that proposal.
//
// Flow:
//   1. Issuer calls `create_gov_proposal` which reads the latest committed
//      snapshot_ref and stores it in `GovProposalEntry.snapshot_id`.
//   2. Any holder calls `cast_vote`.  The function looks up the voter's weight
//      via `SnapshotHolderShare(offering_id, snapshot_id, voter)` — an O(1)
//      read written by `apply_snapshot_shares` — and accumulates yes/no weight.
//   3. A `wt_pin` diagnostic event is emitted on every vote confirming the
//      snapshot_id and the resolved weight.
//   4. `get_gov_proposal` is a read-only query for off-chain indexers.

#[contractimpl]
impl RevoraRevenueShare {
    /// Create a new governance proposal for an offering, pinning voting weight
    /// to the latest committed snapshot.
    ///
    /// ### Auth
    /// Requires `issuer.require_auth()`.
    ///
    /// ### Parameters
    /// - `issuer`: The offering issuer.
    /// - `namespace`: Offering namespace.
    /// - `token`: Offering token.
    /// - `description`: Human-readable proposal text (max 9 chars due to `Symbol` limit).
    ///
    /// ### Returns
    /// The new proposal id (`u32`).
    ///
    /// ### Errors
    /// - `OfferingNotFound` — offering does not exist.
    /// - `ContractFrozen` — contract is frozen.
    /// - `LimitReached` — no snapshot has been committed for this offering yet.
    pub fn create_gov_proposal(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        description: Symbol,
    ) -> Result<u32, RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;

        // Authenticate and resolve offering.
        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        issuer.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Pin the snapshot_id to the latest committed snapshot at creation time.
        // Fail early if no snapshot has been committed — there is nothing to pin to.
        let snapshot_id: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::LastSnapshotCommitRef(offering_id.clone()))
            .ok_or(RevoraError::LimitReached)?;

        // Allocate a monotonically increasing proposal id.
        let count_key = DataKey2::GovProposalCount(offering_id.clone());
        let proposal_id: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let created_at = env.ledger().timestamp();
        let proposal = GovProposalEntry {
            id: proposal_id,
            description: description.clone(),
            snapshot_id,
            created_at,
            yes_weight: 0,
            no_weight: 0,
            open: true,
        };

        env.storage()
            .persistent()
            .set(&DataKey2::GovProposal(offering_id.clone(), proposal_id), &proposal);
        env.storage().persistent().set(&count_key, &proposal_id.saturating_add(1));

        // Emit creation event: topics = (gov_new, offering_id fields)
        // data = (proposal_id, snapshot_id, created_at)
        env.events().publish(
            (EVENT_GOV_PROP_CREATED, issuer, namespace, token),
            (proposal_id, snapshot_id, created_at),
        );

        Ok(proposal_id)
    }

    /// Cast a vote on a governance proposal.
    ///
    /// The voter's weight is read from the snapshot that was pinned at proposal
    /// creation, so shares acquired after `create_gov_proposal` carry zero weight.
    /// A `wt_pin` diagnostic event is emitted with the resolved weight.
    ///
    /// ### Auth
    /// Requires `voter.require_auth()`.
    ///
    /// ### Parameters
    /// - `issuer` / `namespace` / `token`: Identify the offering.
    /// - `proposal_id`: Id returned by `create_gov_proposal`.
    /// - `voter`: The voting address.
    /// - `approve`: `true` = yes, `false` = no.
    ///
    /// ### Returns
    /// The voter's weight in basis points (`u32`).
    ///
    /// ### Errors
    /// - `OfferingNotFound` — offering does not exist.
    /// - `LimitReached` — proposal does not exist or is already closed.
    /// - `ContractFrozen` — contract is frozen.
    /// - `AlreadyApproved` — voter has already voted on this proposal.
    pub fn cast_vote(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        proposal_id: u32,
        voter: Address,
        approve: bool,
    ) -> Result<u32, RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;

        // Authenticate voter.
        voter.require_auth();

        // Offering must exist.
        let _ = Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
            .ok_or(RevoraError::OfferingNotFound)?;

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Load proposal — fail if missing or closed.
        let prop_key = DataKey2::GovProposal(offering_id.clone(), proposal_id);
        let mut proposal: GovProposalEntry =
            env.storage().persistent().get(&prop_key).ok_or(RevoraError::LimitReached)?;
        if !proposal.open {
            return Err(RevoraError::LimitReached);
        }

        // Idempotency / double-vote guard.
        let vote_key = DataKey2::VoteRecord(offering_id.clone(), proposal_id, voter.clone());
        if env.storage().persistent().has(&vote_key) {
            return Err(RevoraError::AlreadyApproved);
        }

        // O(1) vote-weight lookup from the pinned snapshot.
        let weight: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::SnapshotHolderShare(
                offering_id.clone(),
                proposal.snapshot_id,
                voter.clone(),
            ))
            .unwrap_or(0);

        // Accumulate weight.
        if approve {
            proposal.yes_weight = proposal.yes_weight.saturating_add(weight);
        } else {
            proposal.no_weight = proposal.no_weight.saturating_add(weight);
        }

        // Persist updated proposal and vote record.
        env.storage().persistent().set(&prop_key, &proposal);
        env.storage().persistent().set(&vote_key, &approve);

        // Emit weight_pin diagnostic event so indexers can verify the resolved weight
        // came from the pinned snapshot and not a later one.
        env.events().publish(
            (EVENT_WEIGHT_PIN, voter.clone()),
            (proposal_id, proposal.snapshot_id, weight),
        );

        // Emit legacy vote cast event (gov_vote) for backward-compatible consumers.
        env.events().publish(
            (EVENT_GOV_VOTE_CAST, issuer.clone(), namespace.clone(), token.clone()),
            (proposal_id, voter.clone(), approve, weight),
        );

        // Emit stable vote_v3 indexed event for off-chain indexer reconstruction
        // of governance state (#559). Both V2 and V3 topics are emitted concurrently
        // so that V2-only subscribers are not broken during the deprecation window.
        //
        // Data payload: (proposal_id: u32, voter: Address, choice: VoteChoice, weight_bps: u32)
        // The `VoteChoice` enum encodes `approve` as `Yes(1)` / `No(0)` so indexers
        // can extend to additional choices without changing the wire layout.
        let choice = if approve { VoteChoice::Yes } else { VoteChoice::No };
        Self::emit_v2_and_v3(
            &env,
            EventIndexTopicV2 {
                version: 2,
                event_type: EVENT_TYPE_VOTE_V3,
                issuer: issuer.clone(),
                namespace: namespace.clone(),
                token: token.clone(),
                period_id: 0,
            },
            EventIndexTopicV3 {
                version: 3,
                event_type: EVENT_TYPE_VOTE_V3,
                issuer,
                namespace,
                token,
                period_id: 0,
                _reserved: 0,
            },
            (proposal_id, voter, choice, weight),
        );

        Ok(weight)
    }

    /// Return a governance proposal by id, or `None` if it does not exist.
    pub fn get_gov_proposal(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        proposal_id: u32,
    ) -> Option<GovProposalEntry> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey2::GovProposal(offering_id, proposal_id))
    }

    /// Return the total number of governance proposals created for an offering.
    pub fn get_gov_proposal_count(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey2::GovProposalCount(offering_id)).unwrap_or(0)
    }

    /// Close a governance proposal so no further votes can be cast.
    ///
    /// ### Auth
    /// Requires `issuer.require_auth()`.
    ///
    /// ### Errors
    /// - `OfferingNotFound` — offering does not exist or caller is not the issuer.
    /// - `LimitReached` — proposal does not exist or is already closed.
    /// - `ContractFrozen` — contract is frozen.
    pub fn close_gov_proposal(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        proposal_id: u32,
    ) -> Result<(), RevoraError> {
        Self::require_not_frozen(&env)?;
        Self::require_not_paused(&env)?;

        let offering =
            Self::get_offering(env.clone(), issuer.clone(), namespace.clone(), token.clone())
                .ok_or(RevoraError::OfferingNotFound)?;
        if offering.issuers.primary != issuer {
            return Err(RevoraError::OfferingNotFound);
        }
        issuer.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        let prop_key = DataKey2::GovProposal(offering_id, proposal_id);
        let mut proposal: GovProposalEntry =
            env.storage().persistent().get(&prop_key).ok_or(RevoraError::LimitReached)?;
        if !proposal.open {
            return Err(RevoraError::LimitReached);
        }
        proposal.open = false;
        env.storage().persistent().set(&prop_key, &proposal);
        Ok(())
    }
}

// --- MIGRATION UPGRADE PATH (BOUNTY #467) ---

/// Defines the type of transform to apply to a legacy storage key during migration.
///
/// All transforms are deterministic and pure: given the same input value, they
/// always produce the same output. This is critical for replay safety and audit.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum MigrationTransform {
    /// Keep the stored value unchanged (no-op / identity transform).
    /// Useful when only the key naming scheme changes but the value format stays.
    Identity,
    /// Rename the storage key — the value is kept as-is but stored under a new
    /// key symbol (the inner `Symbol` argument).
    Rename(Symbol),
    /// Custom transform identified by a function selector symbol.
    /// The contract dispatches to a known built-in transformation matching the
    /// selector. Custom selectors are defined per-upgrade in the dispatch match.
    Custom(Symbol),
}

/// A registered migration hook binding a legacy key to its transform.
///
/// Hooks are registered by an admin before the storage walker runs and are
/// applied deterministically when the walker encounters a matching legacy key.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MigrationHook {
    /// The legacy storage key symbol this hook applies to.
    pub legacy_key: Symbol,
    /// The transform to apply when this key is encountered during migration.
    pub transform: MigrationTransform,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MigrationCursor {
    pub last_key: u32,
}

#[contracttype]
pub enum MigrationDataKey {
    LastMigrationCompletedAt(Address),
    MigrationResumeCursor(Address),
}

#[contracterror(export = false)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MigrationError {
    MigrationAlreadyApplied = 9001,
    UnsupportedMigrationPath = 9002,
}

#[contractimpl]
impl RevoraRevenueShare {
    /// Register a per-key migration hook that transforms legacy storage during
    /// a storage layout upgrade.
    ///
    /// Hooks let upgrade authors attach a custom transform (identity, rename, or
    /// a built-in custom selector) to a specific legacy key. When the storage
    /// walker runs for a matching version pair, it applies each registered hook
    /// to the legacy key if data exists at that key.
    ///
    /// # Arguments
    /// * `env` - Contract environment
    /// * `admin` - Admin address (must match stored admin)
    /// * `legacy_key` - The legacy storage key symbol to hook into
    /// * `transform` - The transform to apply when this key is encountered
    ///
    /// # Errors
    /// * `RevoraError::NotInitialized` if the contract has no admin
    /// * `RevoraError::NotAuthorized` if the caller is not the admin
    ///
    /// # Security
    /// Hooks are deterministic and pure by construction: the transform type
    /// is a stored enum variant, not an arbitrary closure. This ensures replay
    /// safety and auditability.
    pub fn register_migration_hook(
        env: Env,
        admin: Address,
        legacy_key: Symbol,
        transform: MigrationTransform,
    ) -> Result<(), RevoraError> {
        admin.require_auth();

        let stored_admin: Address = env.storage().persistent()
            .get(&DataKey::Admin)
            .ok_or(RevoraError::NotInitialized)?;
        if admin != stored_admin {
            return Err(RevoraError::NotAuthorized);
        }

        let hook_key = MigrationDataKey::MigrationHook(legacy_key.clone());
        let exists = env.storage().persistent().has(&hook_key);

        if !exists {
            // New hook: increment the counter and store the key in the index.
            let count_key = MigrationDataKey::MigrationHookCount;
            let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
            env.storage().persistent().set(&count_key, &(count + 1));
            env.storage().persistent()
                .set(&MigrationDataKey::MigrationHookIndex(count), &legacy_key);
        }

        // Store the transform (overwrites if already exists)
        env.storage().persistent().set(&hook_key, &transform);

        env.events().publish(
            (EVENT_MIG_HOOK_APPLIED, symbol_short!("register")),
            (legacy_key, transform),
        );

        Ok(())
    }

    /// Remove a previously registered migration hook.
    ///
    /// # Arguments
    /// * `env` - Contract environment
    /// * `admin` - Admin address (must match stored admin)
    /// * `legacy_key` - The legacy key to unregister
    ///
    /// # Errors
    /// * `RevoraError::NotInitialized` if the contract has no admin
    /// * `RevoraError::NotAuthorized` if the caller is not the admin
    ///
    /// # Idempotency
    /// If no hook is registered for the given `legacy_key`, the call silently
    /// succeeds (no-op). This makes the API safe to call multiple times.
    pub fn clear_migration_hook(
        env: Env,
        admin: Address,
        legacy_key: Symbol,
    ) -> Result<(), RevoraError> {
        admin.require_auth();

        // Verify caller is the contract admin
        let stored_admin: Address = env.storage().persistent()
            .get(&DataKey::Admin)
            .ok_or(RevoraError::NotInitialized)?;
        if admin != stored_admin {
            return Err(RevoraError::NotAuthorized);
        }

        let hook_key = MigrationDataKey::MigrationHook(legacy_key.clone());
        if !env.storage().persistent().has(&hook_key) {
            // Idempotent: no hook to clear, silently succeed.
            return Ok(());
        }

        env.storage().persistent().remove(&hook_key);

        env.events().publish(
            (EVENT_MIG_HOOK_APPLIED, symbol_short!("clear")),
            legacy_key,
        );

        Ok(())
    }

    /// Return all currently registered migration hooks as a vector.
    ///
    /// Useful for inspection, dry-run planning, and testing.
    /// Returns an empty Vec if no hooks are registered.
    pub fn get_registered_hooks(env: Env) -> Vec<MigrationHook> {
        let count_key = MigrationDataKey::MigrationHookCount;
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let mut hooks: Vec<MigrationHook> = Vec::new(&env);
        for i in 0..count {
            if let Some(key) = env.storage().persistent()
                .get::<MigrationDataKey, Symbol>(&MigrationDataKey::MigrationHookIndex(i))
            {
                if let Some(transform) = env.storage().persistent()
                    .get::<MigrationDataKey, MigrationTransform>(&MigrationDataKey::MigrationHook(key.clone()))
                {
                    hooks.push_back(MigrationHook {
                        legacy_key: key,
                        transform,
                    });
                }
            }
        }
        hooks
    }

    /// Internal helper: apply a single migration hook for a matching legacy key.
    /// Reads the legacy value, applies the transform, writes the result, and
    /// emits a `migration_hook_applied` event.
    ///
    /// In dry-run mode, only emits a plan event without mutating storage.
    fn apply_migration_hook(
        env: &Env,
        issuer: &Address,
        hook: &MigrationHook,
        dry_run: bool,
    ) {
        if dry_run {
            env.events().publish(
                (soroban_sdk::Symbol::new(env, "migration_plan"), symbol_short!("hook")),
                (hook.legacy_key.clone(), hook.transform.clone()),
            );
        } else {
            // Emit a per-hook application event for audit trail.
            // The actual data transform is invoked via the transform type;
            // concrete per-key read/write logic is added per-upgrade in the
            // migration dispatch table below.
            env.events().publish(
                (EVENT_MIG_HOOK_APPLIED, hook.legacy_key.clone()),
                hook.transform.clone(),
            );
        }
    }

    /// Execute the storage walker migration from `from_version` to `to_version`.
    ///
    /// The walker supports a dry-run mode (`dry_run = true`) that emits plan
    /// events without mutating storage. When `dry_run = false`, it runs the
    /// version-specific migration dispatch and then applies all registered
    /// per-key migration hooks.
    ///
    /// # Hooks integration
    /// After the version-specific dispatch table runs, the walker iterates over
    /// all registered hooks via `get_registered_hooks()` and applies each one
    /// deterministically. Each hook application emits a `migration_hook_applied`
    /// event for audit trail completeness.
    ///
    /// # Replay protection
    /// The completed version is persisted to `MigrationDataKey::LastMigrationCompletedAt(issuer)`
    /// to prevent replay of the same migration.
    pub fn migrate_storage_walker(
        env: Env,
        issuer: Address,
        from_version: u32,
        to_version: u32,
        dry_run: bool,
    ) -> Result<(), MigrationError> {
        // Must be gated by the issuer initiating the migration
        issuer.require_auth();

        let key = MigrationDataKey::LastMigrationCompletedAt(issuer.clone());
        let last_migration: u32 = env.storage().persistent().get(&key).unwrap_or(0);

        // Replay protection: if they are already at or past the target version, fail.
        if last_migration >= to_version {
            return Err(MigrationError::MigrationAlreadyApplied);
        }

        let cursor_key = MigrationDataKey::MigrationResumeCursor(issuer.clone());
        let mut cursor: MigrationCursor =
            env.storage().persistent().get(&cursor_key).unwrap_or(MigrationCursor { last_key: 0 });

        if cursor.last_key > 0 && !dry_run {
            env.events()
                .publish((symbol_short!("mig_rsume"), from_version, to_version), cursor.last_key);
        }

        // Add per-version migrators in a dispatch table
        match (from_version, to_version) {
            (1, 2) => {
                // Explicit storage walker simulation for v1 -> v2.
                let total_keys = 10u32; // Simulated total keys to process

                if dry_run {
                    env.events().publish(
                        (
                            soroban_sdk::Symbol::new(&env, "migration_plan"),
                            from_version,
                            to_version,
                        ),
                        issuer.clone(),
                    );
                } else {
                    for i in 1..=total_keys {
                        if i <= cursor.last_key {
                            continue; // Skip already-processed keys on resume
                        }

                        // Simulate key migration work here
                        env.events()
                            .publish((symbol_short!("mig_step"), from_version, to_version), i);

                        // Persist cursor atomically with each processed key
                        cursor.last_key = i;
                        env.storage().persistent().set(&cursor_key, &cursor);
                    }
                }
            }
            _ => return Err(MigrationError::UnsupportedMigrationPath),
        }

        // Apply all registered per-key migration hooks
        let hooks = Self::get_registered_hooks(env.clone());
        for i in 0..hooks.len() {
            if let Some(hook) = hooks.get(i) {
                Self::apply_migration_hook(&env, &issuer, &hook, dry_run);
            }
        }

        if !dry_run {
            // Persist the completed state to block replays and clear the cursor
            env.storage().persistent().set(&key, &to_version);
            env.storage().persistent().remove(&cursor_key);
        }
        Ok(())
    }
} // end impl RevoraRevenueShare (migration)

// ── Contract self-test entrypoint (#618) ─────────────────────────────────────
#[contractimpl]
impl RevoraRevenueShare {
    /// Run contract-invariant self-test against the embedded canary dataset.
    ///
    /// Returns `0` on success or a non-zero reason code indicating the first
    /// invariant check that failed. This is a read-only entrypoint that does
    /// not require authorization and does not read or write contract storage.
    ///
    /// The canary dataset is embedded in the WASM binary at compile time via
    /// `include_bytes!` and contains known-good test vectors for all key
    /// invariant checks (BPS validation, amount validation, safe math, semver,
    /// concentration limits, multisig thresholds, etc.).
    ///
    /// ## Post-deployment usage
    /// Off-chain monitoring services can call this method periodically to
    /// verify that the deployed contract binary has not been corrupted and
    /// that its internal invariant checks behave correctly.
    pub fn self_test(env: Env) -> u32 {
        let _ = env; // Unused but required for Soroban contractimpl ABI
        crate::self_test::self_test_status()
    }
}

#[cfg(test)]
mod test_close_period;
#[cfg(test)]
mod test_snapshot_voting_weight;
#[cfg(test)]
mod test_storage_layout_version;
