### Vesting Partial Claim Tracking

This document describes the partial-claim capability added to the vesting contract and its security assumptions, data model, and test coverage.

#### Overview
- The vesting contract (`src/vesting.rs`) supports explicit partial claims via `claim_vesting_partial(beneficiary, admin, schedule_index, amount)`.
- Each successful partial claim:
  - Transfers `amount` tokens from the contract to the `beneficiary`.
  - Increases `claimed_amount` on the `VestingSchedule`.
  - Appends a claim record `(timestamp, amount)` to on-chain history.
  - Emits both `vest_pcl` and `vst_pcl1` events for backward-compatible indexing.
- Existing full-claim flow (`claim_vesting`) remains unchanged and does not write to the partial-claim ledger.

#### Cursor / Ledger Model
- `ClaimCount(admin, schedule_index)` is the append-only cursor for a schedule.
- The cursor always equals the number of successful partial claims stored for that schedule.
- `ClaimRecord(admin, schedule_index, claim_index)` is the immutable ledger row for that cursor position.
- Records are written sequentially from index `0` upward; failed claims do not advance the cursor.
- A later full claim settles the remaining claimable balance without adding a duplicate partial record.

#### Storage
- `VestingDataKey::ClaimCount(Address admin, u32 schedule_index)` -> `u32` cursor / record count.
- `VestingDataKey::ClaimRecord(Address admin, u32 schedule_index, u32 claim_index)` -> `(u64 timestamp, i128 amount)` ledger entry.

These keys allow deterministic enumeration of a schedule's claim history.

#### Query Methods
- `get_partial_claim_count(admin, schedule_index)` -> `u32`
- `get_partial_claim_record(admin, schedule_index, claim_index)` -> `Option<(u64, i128)>`

#### Events
- `vest_pcl` (legacy partial claim) is emitted with topics `(vest_pcl, beneficiary, admin)` and data `(schedule_index, token, amount, claim_index)`.
- `vst_pcl1` is emitted in parallel with schema version `1` as the first data field.
- Legacy event `vest_clm` (full claim) is unchanged.
- Creation, cancellation, and full-claim vesting events follow the same versioned-event rules documented in `docs/vesting-event-schema-versioning.md`.

#### Validation and Errors
- `amount` must be `> 0`, else `InvalidAmount`.
- Cannot exceed currently claimable (vested - claimed), else `InvalidAmount`.
- Before cliff or if nothing is currently claimable, returns `NothingToClaim`.
- Cancelled schedules and schedule/beneficiary mismatches return `ScheduleNotFound` (consistent with existing behavior masking unauthorized access to schedule metadata).

#### Security Assumptions
- Auth:
  - `beneficiary.require_auth()` is enforced for all claiming operations.
  - `admin.require_auth()` is enforced for schedule creation/cancellation.
- Token balances:
  - The contract must hold sufficient token balance to fulfill claims. Tests fund the contract using the asset contract's `mint(...)` method.
- Invariants:
  - `claimed_amount` never exceeds `total_amount`.
  - `claimed_amount` increases monotonically.
  - Partial-claim records are append-only, indexed from `0..count-1`.
  - The partial-claim cursor equals the ledger length, so gaps cannot be introduced by failed claims.
- Time:
  - Vesting uses ledger time with cliff and linear vesting until `end_time`.
  - Cancelled schedules are non-claimable.

#### Failure/Abuse Paths Considered
- Attempt to claim before cliff -> rejected with `NothingToClaim`.
- Attempt to claim more than claimable -> rejected with `InvalidAmount`.
- Attempt to claim with zero/negative amount -> rejected with `InvalidAmount`.
- Attempt to claim on cancelled or mismatched schedule -> `ScheduleNotFound` to avoid oracle leakage.

#### Testing
Comprehensive tests are included in `src/vesting_test.rs`:
- Happy path partial claim with balance updates and history recording.
- Zero-amount partial claim is rejected.
- Partial claim exceeding claimable is rejected.
- Partial claim before cliff is rejected.
- The partial-claim cursor advances by one per success and full claims do not append duplicate partial records.
- Event emission is versioned and includes the legacy plus v1 partial-claim topics.

General vesting behavior (create, cancel, claimable math) is also covered. The full test suite is intended to keep aggregate project coverage >=95%.

#### Notes
- Partial-claim history is stored compactly as `(timestamp, amount)`. Consumers can reconstruct aggregate flows via enumeration.
- `claim_vesting` (full-claim/all-available) remains as-is for convenience; it does not record into the partial-claim history to avoid duplication of events.
