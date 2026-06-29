# Snapshot / Override Reporting — Negative Test Matrix [RC26Q2-C18]

## Overview

This document describes the negative test matrix for snapshot-based distribution
and override reporting flows in the Revora Soroban contract.

## Error Coverage

| Error | Code | Trigger Condition |
|---|---|---|
| `OutdatedSnapshot` | 13 | `snapshot_ref` ≤ `last_snapshot_ref` |
| `SnapshotNotEnabled` | 12 | Snapshot feature not enabled for offering |
| `PayoutAssetMismatch` | 14 | `payout_asset` arg ≠ offering's registered asset |

## Event Symbols (from lib.rs)

| Symbol | Constant | Meaning |
|---|---|---|
| `snap_cfg` | `EVENT_SNAP_CONFIG` | Snapshot config toggled |
| `snap_cmt` | `EVENT_SNAP_COMMIT` | Snapshot committed |
| `snap_shr` | `EVENT_SNAP_SHARES_APPLIED` | Holder shares applied |
| `rev_snp2` | `EVENT_REV_DEP_SNAP_V2` | Versioned snapshot deposit (v2) |
| `rev_ovrd` | `EVENT_REVENUE_REPORT_OVERRIDE` | Revenue report overridden |
| `rev_ovra` | `EVENT_REVENUE_REPORT_OVERRIDE_ASSET` | Override with asset detail |
| `rv_ovr` | `EVENT_TYPE_REV_OVR` | v2 indexed override event type |
| `rev_rej` | `EVENT_REVENUE_REPORT_REJECTED` | Report rejected (no override flag) |

## Test Matrix

| Test | Error Expected | State Mutation | Notes |
|---|---|---|---|
| `snapshot_deposit_fails_when_ref_equals_last_ref` | `OutdatedSnapshot` | None | Replay prevention |
| `snapshot_deposit_fails_when_ref_less_than_last_ref` | `OutdatedSnapshot` | None | Stale ref |
| `snapshot_deposit_fails_with_zero_ref` | `InvalidAmount` | None | Zero is invalid per validation matrix |
| `commit_snapshot_fails_when_ref_equals_last_ref` | `OutdatedSnapshot` | None | Write-once semantics |
| `commit_snapshot_fails_when_ref_less_than_last_ref` | `OutdatedSnapshot` | None | Monotonicity |
| `apply_snapshot_shares_fails_for_non_existent_snapshot` | `OutdatedSnapshot` | None | Must commit before apply |
| `deposit_with_snapshot_fails_when_snapshots_disabled` | `SnapshotNotEnabled` | None | Default-off |
| `commit_snapshot_fails_when_snapshots_disabled` | `SnapshotNotEnabled` | None | Default-off |
| `apply_snapshot_shares_fails_when_snapshots_disabled` | `SnapshotNotEnabled` | None | Default-off |
| `snapshot_operations_fail_after_disabling` | `SnapshotNotEnabled` | None | Re-disable path |
| `report_revenue_fails_with_wrong_payout_asset` | `PayoutAssetMismatch` | None | Asset guard |
| `report_revenue_override_fails_with_wrong_payout_asset` | `PayoutAssetMismatch` | None | Override asset guard |
| `deposit_revenue_with_snapshot_fails_with_wrong_payout_asset` | `PaymentTokenMismatch` | None | Token lock |
| `failed_snapshot_deposit_does_not_update_last_ref` | `OutdatedSnapshot` | None | Atomicity |
| `failed_commit_snapshot_does_not_update_last_ref` | `OutdatedSnapshot` | None | Atomicity |
| `failed_report_revenue_does_not_update_period_count` | `PayoutAssetMismatch` | None | Atomicity |
| `override_with_wrong_asset_preserves_original_amount` | `PayoutAssetMismatch` | None | No partial write |
| `rejected_report_without_override_does_not_mutate_state` | None (rev_rej event) | None | Rejection ≠ error |
| `snapshot_deposit_fails_when_offering_frozen` | Frozen error | None | Cross-feature |
| `commit_snapshot_fails_when_contract_frozen` | `ContractFrozen` | None | Global freeze |
| `report_revenue_fails_when_contract_paused` | Paused error | None | Pause guard |

## Security Notes

- **Monotonicity is enforced per offering** — replay of a snapshot ref is impossible once committed.
- **Write-once semantics** — `commit_snapshot` stores an entry keyed by `(offering_id, snapshot_ref)`; a second call with the same ref is rejected before any write.
- **Atomicity** — all failed operations leave `last_snapshot_ref` and `period_count` unchanged.
- **content_hash is caller-supplied** — the contract stores it verbatim and does NOT verify it matches on-chain holder entries. Off-chain consumers must recompute and compare.
- **Payout asset is locked at offering registration** — any mismatch in `report_revenue` or override calls is rejected before state mutation.

## Implementation Status

All branches are implemented in this build. No gaps.
