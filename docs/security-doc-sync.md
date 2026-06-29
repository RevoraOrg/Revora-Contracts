# Security Doc Sync

Issue: #194, #255

## Summary

This change adds a deterministic on-chain payload to keep security documentation synchronized with contract reality.

Implemented in:
- src/lib.rs
- src/test_security_doc_sync.rs

## New API

`get_security_doc_sync() -> Map<Symbol, u32>`

Returned keys:
- `ver`: contract version (currently 4)
- `ev_sch`: event schema version
- `idx_sch`: indexer schema version
- `err_sh_bps`: InvalidRevenueShareBps
- `err_limit`: LimitReached
- `err_conc`: ConcentrationLimitExceeded
- `err_no_off`: OfferingNotFound
- `err_dep`: PeriodAlreadyDeposited
- `err_no_clm`: NoPendingClaims
- `err_bl`: HolderBlacklisted
- `err_ish_bps`: InvalidShareBps
- `err_ptm`: PaymentTokenMismatch
- `err_frz`: ContractFrozen
- `err_dly`: ClaimDelayNotElapsed
- `err_snap_e`: SnapshotNotEnabled
- `err_snap_o`: OutdatedSnapshot
- `err_asset`: PayoutAssetMismatch
- `err_tx_p`: IssuerTransferPending
- `err_tx_n`: NoTransferPending
- `err_tx_u`: UnauthorizedTransferAccept
- `err_meta_l`: MetadataTooLarge
- `err_auth`: NotAuthorized
- `err_init`: NotInitialized
- `err_amt`: InvalidAmount
- `err_per`: InvalidPeriodId
- `err_cap`: SupplyCapExceeded
- `err_meta_f`: MetadataInvalidFormat
- `err_win_r`: ReportingWindowClosed
- `err_win_c`: ClaimWindowClosed
- `err_sig_e`: SignatureExpired
- `err_sig_r`: SignatureReplay
- `err_sig_k`: SignerKeyNotRegistered
- `err_prop`: ProposalExpired
- `err_xfer`: TransferFailed
- `err_ver`: AlreadyAtTargetVersion
- `err_mig`: MigrationDowngradeNotAllowed
- `err_rot_s`: AdminRotationSameAddress
- `err_rot_p`: AdminRotationPending

## Why

Security docs often drift from implementation details (error codes, schema versions, and guarantees). This API provides a machine-readable source of truth that docs tooling can validate in CI.

## Security Notes

- Read-only method; no state mutation.
- Deterministic output for consistent doc checks.
- Enables explicit detection of silent breaking changes in event/error schema.

## Tests

Added deterministic tests:
- `security_doc_sync_returns_expected_markers`
- `security_doc_sync_is_deterministic`

These verify key presence, expected values, and stable payload shape.
