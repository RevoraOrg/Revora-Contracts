# PR: Emit Jurisdiction Migration Events with Grace Period

Closes #539

---

## Summary

Holders relocating between jurisdictions now receive a documented grace period during which they can divest before claims are blocked. This PR introduces a structured jurisdiction migration event, a per-offering configurable grace period, and a compliance deadline enforced in the claim path.

---

## Motivation

When a holder relocates from an allowed jurisdiction to one not in the offering's allowlist, there is currently no grace period — the holder's claims can be immediately blocked once the jurisdiction change takes effect. This is unfair to holders who may need time to divest their position.

This PR introduces:

1. **A scheduled migration model**: jurisdiction changes can be scheduled with a future `effective_ts`, giving the holder advance notice.
2. **A per-offering configurable grace period**: issuers can set how long holders have after the effective date before enforcement begins.
3. **A structured `jur_mig` event**: indexers and off-chain systems receive a machine-readable migration event with the old jurisdiction, new jurisdiction, effective timestamp, and compliance deadline.
4. **Deadline enforcement in the claim path**: after the grace period expires, claims are blocked if the target jurisdiction is disallowed — with a specific `JurisdictionMigrationDeadlineExceeded` (76) error code.

---

## Design

### State machine

```
┌──────────────┐  set_holder_jurisdiction(new_jur, effective_ts=0)  ┌──────────────────┐
│  No current   │ ─────────────────────────────────────────────────▶│  Jurisdiction     │
│  jurisdiction │                                                   │  applied          │
│  (jur_none)   │                                                   │  immediately      │
└──────┬───────┘                                                   └──────────────────┘
       │
       │ set_holder_jurisdiction(new_jur, effective_ts > now)
       ▼
┌──────────────────┐
│  Migration       │
│  scheduled       │  ← JurisdictionMigrationState stored
│  (grace active)  │  ← Claims continue normally
└────────┬─────────┘
         │
         │ now >= deadline
         ▼
    ┌─────────────┐    new_jur allowed?  ┌──────────────────┐
    │  Deadline    │─── YES ────────────▶│  Migration        │
    │  reached     │                     │  finalized:       │
    │              │                     │  -jurisdiction set│
    │              │                     │  -migration cleared│
    └──────┬──────┘                     └──────────────────┘
           │
           │ new_jur disallowed?
           ▼
    ┌──────────────────────────────┐
    │  JurisdictionMigrationDeadline│
    │  Exceeded (error 76)         │
    │  Claims blocked until:       │
    │  - holder relocates, or      │
    │  - issuer updates allowlist  │
    └──────────────────────────────┘
```

### Key design decisions

- **Migration is a claim-gate, not an immediate jurisdiction change.** The holder's stored jurisdiction remains unchanged during the grace period, allowing claims to proceed normally. Enforcement happens only at claim time once the deadline passes.
- **Automatic finalization on allowed jurisdiction.** When the deadline passes and the target jurisdiction *is* allowed, the migration is automatically finalized: the holder's jurisdiction is updated and the migration state is cleared in the same claim transaction.
- **No automatic cleanup on disallowed.** If the holder's target jurisdiction is disallowed, the migration state persists — continuing to block claims until the holder relocates to an allowed jurisdiction or the issuer updates the allowlist.

---

## Files Changed

| File | Change |
|------|--------|
| `src/lib.rs` | Core implementation — event constants, error code, struct, storage keys, functions, claim enforcement |
| `src/test_jurisdiction.rs` | Updated 4 existing tests + added 10 new tests |
| `src/structured_error_tests.rs` | Added error code 76 to all error tables |
| `tools/storage_layout_schema.rs` | Added 2 new DataKey2 entries |

---

## Detailed Changes

### 1. New Event Constants (`src/lib.rs`)

```rust
/// Emitted when a holder's jurisdiction change is scheduled with a grace period.
/// Topic: (jur_mig, issuer, namespace, token)
/// Data:   (holder, old_jur, new_jur, effective_ts, deadline)
const EVENT_JUR_MIGRATION: Symbol = symbol_short!("jur_mig");

/// Emitted when per-offering jurisdiction grace period is configured.
/// Topic: (jur_grace, issuer, namespace, token)
/// Data:   (grace_secs,)
const EVENT_JUR_GRACE_SET: Symbol = symbol_short!("jur_grace");

// Timing constants
const DEFAULT_JURISDICTION_GRACE_SECS: u64 = 7 * 24 * 60 * 60;  // 7 days
const MIN_JURISDICTION_GRACE_SECS: u64 = 60 * 60;                // 1 hour
const MAX_JURISDICTION_GRACE_SECS: u64 = 90 * 24 * 60 * 60;     // 90 days
```

### 2. New Error Code

```rust
/// Holder's jurisdiction migration grace period has expired and the
/// new jurisdiction is disallowed for this offering.
JurisdictionMigrationDeadlineExceeded = 76,
```

### 3. New Struct — `JurisdictionMigrationState`

```rust
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct JurisdictionMigrationState {
    pub old_jurisdiction: Symbol,  // jurisdiction migrating from
    pub new_jurisdiction: Symbol,  // target jurisdiction
    pub effective_ts: u64,         // when the change takes effect
    pub deadline: u64,             // effective_ts + grace_period_secs
}
```

### 4. New Storage Keys (`DataKey2`)

```rust
/// Per-offering jurisdiction migration grace period in seconds.
JurisdictionGracePeriod(OfferingId),

/// Pending jurisdiction migration for (offering_id, holder).
JurisdictionMigration(OfferingId, Address),
```

### 5. Modified `set_holder_jurisdiction`

**Signature change** — added `effective_ts: u64` parameter.

| Condition | Behavior |
|-----------|----------|
| `effective_ts == 0 \|\| effective_ts <= now` | Immediate jurisdiction change (backward compatible) |
| `effective_ts > now` | Scheduled migration: stores `JurisdictionMigrationState`, emits `EVENT_JUR_MIGRATION`, jurisdiction unchanged until effective_ts |

**Breaking change note:** All callers must now pass an `effective_ts` argument. Passing `0` preserves the existing immediate-set behavior.

### 6. New Public Functions

| Function | Purpose |
|----------|---------|
| `set_jurisdiction_grace_period(issuer, namespace, token, grace_secs)` | Configure per-offering grace period (1h–90d). Requires issuer quorum auth. |
| `get_jurisdiction_grace_period(issuer, namespace, token)` | Read the grace period. Returns 7-day default when not configured. |
| `get_jurisdiction_migration(issuer, namespace, token, holder)` | Read pending migration state. Returns `None` when no migration is pending. |

### 7. Internal Helper — `require_jurisdiction_migration_not_expired`

Called from both `claim()` functions after blacklist/freeze checks. Logic:

1. If no pending migration → pass through
2. If `now < deadline` → pass through (grace period active)
3. If deadline passed and `new_jurisdiction` is allowed → **finalize**: update `HolderJurisdiction`, clear migration, pass through
4. If deadline passed and `new_jurisdiction` is disallowed → emit `jur_reject` event, return `JurisdictionMigrationDeadlineExceeded`

### 8. Claim Path Enforcement

Added to both `claim()` functions in `src/lib.rs`:
- Line ~8095 (first claim function)
- Line ~8500 (second claim function with freeze checks)

```rust
// Jurisdiction migration deadline enforcement
Self::require_jurisdiction_migration_not_expired(
    &env,
    &offering_id,
    &holder,
    symbol_short!("claim"),
)?;
```

### 9. Storage Layout Bump

`STORAGE_LAYOUT_VERSION` bumped from **3** → **4** to reflect new `DataKey2` variants.

---

## Test Coverage

### Updated existing tests (4)

All existing jurisdiction tests updated to pass `&0u64` as the new `effective_ts` parameter, preserving identical behavior.

### New tests (10)

| # | Test | What it covers |
|---|------|----------------|
| 1 | `jurisdiction_migration_with_future_ts_emits_event_and_stores_state` | Future effective_ts stores `JurisdictionMigrationState`, emits `jur_mig` event, jurisdiction unchanged |
| 2 | `jurisdiction_migration_immediate_sets_jurisdiction_directly` | `effective_ts=0` sets jurisdiction immediately, no migration state |
| 3 | `claim_during_grace_period_succeeds` | Holders can claim while grace period is active even if target jurisdiction is disallowed |
| 4 | `claim_after_grace_period_with_disallowed_jurisdiction_fails` | Claims blocked with `JurisdictionMigrationDeadlineExceeded` after deadline |
| 5 | `claim_after_grace_period_with_allowed_jurisdiction_succeeds` | Migration finalized (jurisdiction updated, state cleared) when target is allowed |
| 6 | `migration_into_disallowed_jurisdiction_at_exact_grace_end` | **Edge case**: deadline is inclusive — claim fails at `effective_ts + grace_secs` |
| 7 | `configurable_grace_period_is_honored` | Per-offering grace period is stored and reflected in deadline computation |
| 8 | `default_grace_period_is_seven_days` | Unconfigured offering gets the 7-day default |
| 9 | `claim_with_no_pending_migration_succeeds_normally` | No migration = no interference with normal claiming |
| 10 | `migration_with_no_allowlist_never_blocks_claims` | Empty allowlist = jurisdiction gating disabled, migration never blocks |
| 11 | `get_jurisdiction_migration_returns_none_when_no_migration_pending` | Getter returns `None` for clean state |

---

## Security Considerations

1. **No storage bloat from stale migrations.** When the target jurisdiction is allowed, the migration is finalized and the `JurisdictionMigration` entry is removed in the same transaction.
2. **Frozen/paused contract gating.** `set_holder_jurisdiction` and `set_jurisdiction_grace_period` both enforce `require_not_frozen` and `require_not_paused`.
3. **Issuer authentication.** Only the authorized issuer (with quorum) can set jurisdictions or grace periods.
4. **Grace period bounds.** Minimum 1 hour, maximum 90 days — prevents misconfiguration (e.g., 0-second grace or multi-year grace).
5. **Saturating arithmetic.** `deadline = effective_ts.saturating_add(grace_secs)` — prevents overflow for extreme values.
6. **Fail-fast.** Jurisdiction migration enforcement happens before share checks and payout computation, preventing wasted computation.
7. **Claim idempotency preserved.** If a claim succeeds and the migration is finalized, subsequent claims see no migration and proceed normally.

---

## Backward Compatibility

- **Breaking API change:** `set_holder_jurisdiction` now requires `effective_ts: u64`. Passing `0` restores the previous immediate-set behavior.
- **Storage compatibility:** New `DataKey2` variants are additive. Existing storage keys are unchanged.
- **Error code stability:** `JurisdictionMigrationDeadlineExceeded = 76` is a new wire value, not a renumber of any existing code.
- **Event backward compat:** The existing `jur_set` and `jur_reject` events are unchanged. Two new events (`jur_mig`, `jur_grace`) are emitted only when the new feature is used.
- **`STORAGE_LAYOUT_VERSION`** bumped from 3 to 4.

---

## Migration Guide for Integrators

### For issuers

```rust
// To set jurisdiction immediately (backward compatible):
set_holder_jurisdiction(issuer, namespace, token, holder, "us", 0);

// To schedule a migration with grace period:
set_holder_jurisdiction(issuer, namespace, token, holder, "ky", future_effective_ts);

// To configure a custom grace period:
set_jurisdiction_grace_period(issuer, namespace, token, 72 * 3600); // 72 hours
```

### For indexers

Subscribe to the new event topic:

```
Topic: jur_mig
Data format: (holder: Address, old_jur: Symbol, new_jur: Symbol, effective_ts: u64, deadline: u64)
```

The `jur_grace` event signals grace period configuration:

```
Topic: jur_grace
Data format: (grace_secs: u64)
```

### For holders

Monitor `get_jurisdiction_migration(issuer, namespace, token, holder)` for active migrations. A `None` return means no migration is pending. A `Some(migration)` with `now >= migration.deadline` means the holder should consider relocating or divesting before their next claim.

---

## Checklist

- [x] `EVENT_JUR_MIGRATION` and `EVENT_JUR_GRACE_SET` event constants
- [x] `JurisdictionMigrationDeadlineExceeded = 76` error code
- [x] `JurisdictionMigrationState` struct with `#[contracttype]`
- [x] `DataKey2::JurisdictionGracePeriod` and `DataKey2::JurisdictionMigration`
- [x] Modified `set_holder_jurisdiction` with `effective_ts` parameter
- [x] `set_jurisdiction_grace_period` with issuer quorum auth
- [x] `get_jurisdiction_grace_period` and `get_jurisdiction_migration` getters
- [x] `require_jurisdiction_migration_not_expired` with automatic finalization
- [x] Enforcement in both `claim()` functions
- [x] `STORAGE_LAYOUT_VERSION` → 4
- [x] Updated `storage_layout_schema.rs` with new entries
- [x] Updated `structured_error_tests.rs` with error code 76
- [x] 4 existing tests updated for new signature
- [x] 11 new tests covering all edge cases
- [x] Grace period bounds validation (1h–90d)
- [x] Saturating arithmetic on deadline computation

---

## Example Commit Message

```
feat: emit jurisdiction migration events with grace period (#539)

- Add set_holder_jurisdiction(holder, jur, effective_ts) with migration scheduling
- Emit jur_mig event with old/new jurisdiction, effective_ts, and deadline
- Add per-offering configurable grace period (default 7 days, range 1h–90d)
- Add JurisdictionMigrationDeadlineExceeded error (76) with enforced deadline
- Enforce migration deadline in both claim() paths
- Automatic migration finalization when target jurisdiction is allowed
- Add 11 new tests covering all edge cases
- Bump STORAGE_LAYOUT_VERSION to 4
```
