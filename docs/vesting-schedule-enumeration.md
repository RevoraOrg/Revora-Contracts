# Vesting Schedule Enumeration

## Overview

The vesting schedule enumeration API provides callers with efficient, bounded read access to the
full set of schedules managed by a `RevoraVesting` contract. Three complementary patterns are
supported:

| Pattern | Function | Use case |
|---------|----------|----------|
| Admin-scoped pagination | `list_schedules_page` | Tooling / back-office; iterate all schedules for an admin |
| Beneficiary lookup | `list_schedules_for_beneficiary` | Wallet / front-end; show a user all their grants |
| Beneficiary active filter | `active_schedules_for_beneficiary` | Dashboard; show only schedules with remaining value |

All enumeration functions are **read-only** and require **no authentication**. schedule data is
already permissionlessly queried via `get_schedule`, so enumeration is a pure convenience layer.

---

## Storage Key Schema

### Primary index (pre-existing)

| Key | Type | Description |
|-----|------|-------------|
| `VestingDataKey::ScheduleCount(admin)` | `u32` | Number of schedules created by `admin` |
| `VestingDataKey::Schedule(admin, index)` | `VestingSchedule` | Schedule at `index` (0-based) |

### Secondary beneficiary index (new)

| Key | Type | Description |
|-----|------|-------------|
| `VestingDataKey::BeneficiaryScheduleCount(admin, beneficiary)` | `u32` | Number of index entries for the `(admin, beneficiary)` pair |
| `VestingDataKey::BeneficiaryScheduleItem(admin, beneficiary, i)` | `u32` | The **global schedule index** stored at secondary-index position `i` |

The secondary index is **append-only**: every call to `create_schedule` atomically appends a new
entry. Because schedules are never deleted (only cancelled via a flag), the index never needs
compaction. Lookups resolve in two storage reads: one to read the global schedule index, one to
read the schedule itself.

---

## Function Reference

### `list_schedules_page`

```rust
pub fn list_schedules_page(
    env:   Env,
    admin: Address,
    start: u32,   // first index to return (0-based)
    limit: u32,   // items per page; 0 → capped at MAX_SCHEDULES_PAGE (50)
) -> (Vec<VestingSchedule>, Option<u32>)
```

**Returns** `(schedules, Some(next_start))` when a following page exists, `(schedules, None)` on
the final page.

**Bounds**:
- `limit` is clamped to `MAX_SCHEDULES_PAGE = 50`.
- `start >= total_count` returns `([], None)`.

**Example — iterate all pages**:
```rust
let mut cursor: u32 = 0;
loop {
    let (page, next) = client.list_schedules_page(&admin, &cursor, &50);
    for schedule in page.iter() { /* process */ }
    match next {
        Some(c) => cursor = c,
        None    => break,
    }
}
```

---

### `get_beneficiary_schedule_count`

```rust
pub fn get_beneficiary_schedule_count(
    env:         Env,
    admin:       Address,
    beneficiary: Address,
) -> u32
```

Returns the number of schedules assigned to `beneficiary` under `admin`. Returns `0` when none
exist. This mirrors `get_schedule_count` but is scoped to a single beneficiary.

---

### `list_schedules_for_beneficiary`

```rust
pub fn list_schedules_for_beneficiary(
    env:         Env,
    admin:       Address,
    beneficiary: Address,
) -> Vec<VestingSchedule>
```

Returns **all** schedules for `beneficiary`, including cancelled and fully-claimed ones, in
creation order (ascending schedule index).

**Bounds**: capped at `MAX_SCHEDULES_PER_BENEFICIARY = 100`.

---

### `active_schedules_for_beneficiary`

```rust
pub fn active_schedules_for_beneficiary(
    env:         Env,
    admin:       Address,
    beneficiary: Address,
) -> Vec<VestingSchedule>
```

Returns only **active** schedules. A schedule is active when:

1. `cancelled == false`, **and**
2. `claimable_amount > 0` **or** `now < end_time`

This definition captures:
- Schedules that have not yet started (all tokens still in the future).
- Schedules mid-vesting (some tokens claimable, some still locked).
- Schedules that ended but still have an unclaimed residue (due to partial prior claims).

A schedule where the beneficiary has claimed every token **and** the vesting window has closed is
excluded (fully settled).

**Bounds**: capped at `MAX_SCHEDULES_PER_BENEFICIARY = 100`.

---

## Security Assumptions

| Assumption | Rationale |
|-----------|----------|
| **No auth required** | All data is already readable via `get_schedule`; enumeration is additive, not privileged. |
| **Append-only secondary index** | `create_schedule` atomically appends to both the primary and secondary index in the same transaction. An interrupted write cannot leave a dangling pointer because Soroban's contract execution is atomic. |
| **Bounded result sets** | Both caps (`50` and `100`) are enforced in contract logic, preventing unbounded gas consumption regardless of caller input. |
| **Namespace isolation** | The `admin` parameter is part of every storage key; different admins cannot read or corrupt each other's indexes. |
| **Admin key is immutable per contract** | `initialize_vesting` is one-shot; there is no admin-rotation mechanism, so the admin used as a namespace key is stable for the lifetime of the contract. |

---

## Abuse and Failure Paths

### Oversized `limit` in `list_schedules_page`
A caller passing `limit = u32::MAX` or `limit = 0` receives at most `MAX_SCHEDULES_PAGE = 50`
results. No additional validation is needed by the caller.

### `start` beyond total count
`list_schedules_page` returns `([], None)` immediately when `start >= count`. No error is raised.

### Cancelled schedules in beneficiary list
`list_schedules_for_beneficiary` includes cancelled schedules (the client may need to display them
as "forfeited"). Use `list_active_schedules_for_beneficiary` to exclude them automatically.

### Fully-claimed schedules
A schedule where `claimed_amount == total_amount` after `end_time` is **not** active. The active
filter checks `claimable > 0 || now < end_time`; when both are false, the schedule is excluded.

### Pre-existing schedules (before index introduction)
The secondary beneficiary index only tracks schedules created after this feature was deployed.
`list_schedules_page` is unaffected (it uses the primary index, which has always existed).
`list_schedules_for_beneficiary` and `active_schedules_for_beneficiary` will return only
schedules created after the upgrade. This is acceptable for a fresh deployment.

### Schedule index spoofing
`BeneficiaryScheduleItem` values are set exclusively by `create_schedule`. No public mutation
function exists for these keys; they cannot be forged or overwritten by an external caller.

---

## Integration Example

**Scenario**: A wallet UI wants to display all active vesting grants for a user.

```javascript
// Pseudocode — adapt to your Soroban SDK bindings
const active = await contract.active_schedules_for_beneficiary({
  admin:       ADMIN_ADDRESS,
  beneficiary: USER_WALLET_ADDRESS,
});

for (const sched of active) {
  console.log(`Token: ${sched.token}`);
  console.log(`Claimable: ${await contract.get_claimable_vesting({ admin: ADMIN_ADDRESS, schedule_index: scheduleIndex })}`);
}
```

**Scenario**: A back-office tool pages through every schedule for an admin.

```javascript
let cursor = 0;
while (true) {
  const [page, next] = await contract.list_schedules_page({
    admin: ADMIN_ADDRESS, start: cursor, limit: 50,
  });
  processBatch(page);
  if (next === null) break;
  cursor = next;
}
```
