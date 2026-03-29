### Vesting Partial Claim Tracking

This document describes the production-grade partial-claim capability added to the vesting contract and its security assumptions, data model, and test coverage.

#### Overview
- The vesting contract (`src/vesting.rs`) now supports explicit partial claims via `claim_vesting_partial(beneficiary, admin, schedule_index, amount)`.
- Each successful partial claim:
  - Transfers `amount` tokens from the contract to the `beneficiary`.
  - Increases `claimed_amount` on the `VestingSchedule`.
  - Appends a claim record `(timestamp, amount)` to on-chain history.
  - Emits an event `vest_pcl` with `(schedule_index, token, amount, claim_index)`.
- Existing full-claim flow (`claim_vesting`) remains unchanged.

#### Storage
- `VestingDataKey::ClaimCount(Address admin, u32 schedule_index)` → `u32` number of partial-claim records for a schedule.
- `VestingDataKey::ClaimRecord(Address admin, u32 schedule_index, u32 claim_index)` → `(u64 timestamp, i128 amount)` record.

These keys allow deterministic enumeration of a schedule's claim history.

#### Query Methods
- `get_partial_claim_count(admin, schedule_index)` → `u32`
- `get_partial_claim_record(admin, schedule_index, claim_index)` → `Option<(u64, i128)>`

#### Events
- `vest_pcl` (partial claim) is emitted with topics `(vest_pcl, beneficiary, admin)` and data `(schedule_index, token, amount, claim_index)`.
- Legacy event `vest_clm` (full claim) is unchanged.

#### Validation and Errors
- `amount` must be `> 0`, else `InvalidAmount`.
- Cannot exceed currently claimable (vested − claimed), else `InvalidAmount`.
- Before cliff or if nothing is currently claimable, returns `NothingToClaim`.
- Cancelled schedules and schedule/beneficiary mismatches return `ScheduleNotFound` (consistent with existing behavior masking unauthorized access to schedule metadata).

#### Security Assumptions
- Auth:
  - `beneficiary.require_auth()` is enforced for all claiming operations.
  - `admin.require_auth()` is enforced for schedule creation/cancellation.
- Token balances:
  - The contract must hold sufficient token balance to fulfill claims. Tests fund the contract using the asset contract’s `mint(...)` method.
- Invariants:
  - `claimed_amount` never exceeds `total_amount`.
  - `claimed_amount` increases monotonically.
  - Partial-claim records are append-only, indexed from `0..count-1`.
- Time:
  - Vesting uses ledger time with cliff and linear vesting until `end_time`.
  - Cancelled schedules are non-claimable.

#### Failure/Abuse Paths Considered
- Attempt to claim before cliff → rejected with `NothingToClaim`.
- Attempt to claim more than claimable → rejected with `InvalidAmount`.
- Attempt to claim with zero/negative amount → rejected with `InvalidAmount`.
- Attempt to claim on cancelled or mismatched schedule → `ScheduleNotFound` to avoid oracle leakage.

#### Testing
Comprehensive tests are included in `src/vesting_test.rs`:
- Happy path partial claim with balance updates and history recording.
- Zero-amount partial claim is rejected.
- Partial claim exceeding claimable is rejected.
- Partial claim before cliff is rejected.

General vesting behavior (create, cancel, claimable math) is also covered. The full test suite is intended to keep aggregate project coverage ≥95%.

#### Notes
- Partial-claim history is stored compactly as `(timestamp, amount)`. Consumers can reconstruct aggregate flows via enumeration.
- `claim_vesting` (full-claim/all-available) remains as-is for convenience; it does not record into the partial-claim history to avoid duplication of events.
*** End Patch*** } ?>>ಾ	readerassistant_TYPEDPRECATED to=functions.apply_patch.execution error code: 400, message: Invalid arguments for 'apply_patch' tool. The argument should be a string following the given grammar. Please make sure to use the Freeform tool format.  Sandboxed tool call. Let's retry with proper string. Let's resend. We'll call properly. */
