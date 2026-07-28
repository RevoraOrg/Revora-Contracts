# Core Event Version Field (v2)

## Purpose
Production-grade deterministic versioning for all core events. Enables:

1. **Schema evolution** - Indexers parse version-first data tuples
2. **Security** - Reject malformed/replay events, enforce schema compliance
3. **Determinism** - Always emit v2 events; no feature flags, no conditional paths
4. **Auditability** - Versioned events for historical reconstruction

## Schema Rules (Production Requirements)
```
ALL v2 events MUST:
- Emit Symbol topic[0] = EVENT_FOO_V2 (e.g. "ofr_reg2")
- Data[0] = EVENT_SCHEMA_VERSION_V2 = 2u32 (position 0, ALWAYS first)
- Data[1...] = payload tuple (unchanged semantics)
- Emit unconditionally — no is_event_versioning_enabled guard
```

**Indexer Enforcement** (off-chain):
```
if event.topic[0] not in known_v2_topics:
  reject event
if event.data[0] != 2u32:
  reject event (schema version mismatch)
parse data[1..] with v2 schema for that topic
```

## Core Events Schema Table

All flows below emit their v2 event **unconditionally** (no feature flags).

| Flow | V2 Topic | Rust Constant | Data Schema (position 0=version) |
|------|----------|---------------|----------------------------------|
| **register_offering** | `ofr_reg2` | `EVENT_OFFER_REG_V2` | `[2, token:Address, revenue_share_bps:u32, payout_asset:Address]` |
| **report_revenue init** | `rv_init2` | `EVENT_REV_INIT_V2` | `[2, amount:i128, period_id:u64, blacklist:Vec<Address>]` |
| **report_revenue init-asset** | `rv_inia2` | `EVENT_REV_INIA_V2` | `[2, payout_asset:Address, amount:i128, period_id:u64, blacklist:Vec<Address>]` |
| **report_revenue generic** | `rv_rep2` | `EVENT_REV_REP_V2` | `[2, amount:i128, period_id:u64, blacklist:Vec<Address>]` |
| **report_revenue asset** | `rv_repa2` | `EVENT_REV_REPA_V2` | `[2, payout_asset:Address, amount:i128, period_id:u64]` |
| **deposit_revenue** | `rev_dep2` | `EVENT_REV_DEPOSIT_V2` | `[2, payment_token:Address, amount:i128, period_id:u64]` |
| **deposit_revenue snapshot** | `rev_snp2` | `EVENT_REV_DEP_SNAP_V2` | `[2, payment_token:Address, amount:i128, period_id:u64, snapshot_reference:u64]` |
| **set_holder_share** | `sh_set2` | `EVENT_SHARE_SET_V2` | `[2, holder:Address, share_bps:u32]` |
| **claim** | `claim2` | `EVENT_CLAIM_V2` | `[2, holder:Address, total_payout:i128, periods:Vec<u64>]` |
| **freeze** | `frz2` | `EVENT_FREEZE_V2` | `[2, frozen:bool]` |

> **Implementation note**: Each flow uses the `emit_v2_event` helper, which
> automatically prepends `EVENT_SCHEMA_VERSION_V2 = 2u32` to the data tuple.

## Dual-Stream Architecture

Each core flow emits **two parallel event streams**:

1. **Indexed structured stream** (`ev_idx2` topic + `EventIndexTopicV2` struct):
   Used by the Revora-Backend for efficient keyed indexing of offer/revenue/claim flows.

2. **Direct v2 stream** (e.g. `ofr_reg2`, `rv_init2`, …):
   Used by downstream indexers that need the full payload with the version guard at data[0].

Both streams are emitted unconditionally. Legacy v1 streams (`ofr_reg1`, `rv_init1`, …)
continue to emit only when `is_event_versioning_enabled()` returns true (backward compat,
deprecated 90 days post v2 rollout).

## Security Properties

1. **Position 0 Version**: Schema-breaking changes bump version. Legacy indexers ignore v2+.
2. **Deterministic Emission**: No flags — ALL core events emit their v2 tuple every invocation.
3. **Topic Schema Mapping**: Off-chain indexers validate topic → schema.
4. **Replay Protection**: Version + ledger context + deterministic data prevents replays.
5. **Constant Guard**: `EVENT_SCHEMA_VERSION_V2 = 2u32` is tested in `test_indexer_fixtures.rs`
   to prevent accidental mutation from silently breaking all downstream indexers.

**Off-chain Rejection Logic**:
```rust
match event.topic {
  "ofr_reg2" => if data[0] != 2 || data.len() != 4 { reject }
  "rv_init2" => if data[0] != 2 || data.len() != 4 { reject }
  "rv_inia2" => if data[0] != 2 || data.len() != 5 { reject }
  "rv_rep2"  => if data[0] != 2 || data.len() != 4 { reject }
  "rv_repa2" => if data[0] != 2 || data.len() != 4 { reject }
  "rev_dep2" => if data[0] != 2 || data.len() != 4 { reject }
  "rev_snp2" => if data[0] != 2 || data.len() != 5 { reject }
  "sh_set2"  => if data[0] != 2 || data.len() != 3 { reject }
  "claim2"   => if data[0] != 2 || data.len() != 4 { reject }
  "frz2"     => if data[0] != 2 || data.len() != 2 { reject }
}
```

## Migration Guide (v1 → v2)

| Legacy | Status | Replacement | Notes |
|--------|--------|-------------|-------|
| `ofr_reg1` | **deprecated** | `ofr_reg2` | v1 still emitted when versioning flag enabled |
| `rv_init1` | **deprecated** | `rv_init2` | v1 still emitted when versioning flag enabled |
| `rv_inia1` | **deprecated** | `rv_inia2` | v2 now unconditional (was previously flag-gated) |
| conditional `emit_v2_event` | **removed** | always emit | `is_event_versioning_enabled` no longer gates v2 events |

**Backward Compatibility**: v1 events still emitted via `if is_event_versioning_enabled()`.
v2 indexers ignore v1 events. v1 deprecated 90 days post-v2 production rollout.

## Indexer Best Practices

1. **Version Validation**: Always check `data[0] == 2` for v2 events
2. **Topic Whitelist**: Only process known V2 topics
3. **Data Length**: Enforce exact tuple length per topic (see rejection logic above)
4. **Storage Replay**: Use version+period_id+ledger as dedup key
5. **Dual Index**: Process both `ev_idx2` (structured) and direct v2 topics in parallel
6. **Symbol Collision**: All 10 v2 topic symbols are guaranteed distinct (tested in fixtures)

## Test Coverage (RC26Q2-C31)

Tests live in `src/test_indexer_fixtures.rs`.

| Test | Description |
|---|---|
| `fixture_topics_have_stable_order_and_shape` | 6 fixture structs in stable order |
| `fixture_topics_bind_to_requested_identity` | issuer/namespace/token/version=2 on all fixtures |
| `event_schema_version_v2_constant_is_2` | Guard constant value against accidental change |
| `register_offering_emits_ofr_reg2_v2_event` | ofr_reg2 topic present after register_offering |
| `register_offering_v2_event_data_starts_with_version_2` | data[0] == 2 for ofr_reg2 |
| `report_revenue_emits_rv_init2_on_initial_report` | rv_init2 emitted on first period report |
| `report_revenue_emits_rv_rep2_unconditionally` | rv_rep2 emitted on every report |
| `report_revenue_emits_rv_repa2_unconditionally` | rv_repa2 emitted on every report |
| `report_revenue_emits_rv_inia2_unconditionally_without_versioning_flag` | rv_inia2 not flag-gated |
| `set_holder_share_emits_sh_set2_v2_event` | sh_set2 emitted by set_holder_share |
| `v2_event_symbols_are_all_distinct` | No symbol collision among 10 v2 topics |
| `all_fixture_topics_carry_version_2` | All fixture structs have version = 2 |
| `fixture_period_id_zero_for_non_period_scoped_events` | offer/claim fixtures have period_id = 0 |
| `fixture_period_scoped_events_carry_requested_period_id` | rv_init/ovr/rej/rep carry requested period_id |

## Verification Steps

```
1. cargo test --test-threads=1
2. Deploy testnet → smoke test all core flows (register, report, deposit, claim, set_share, freeze)
3. Indexers: verify v2 parsing on test events from testnet
4. Mainnet: deploy + monitor event emission for both ev_idx2 and direct v2 topics
```

**Success Criteria**: 100% core events emit `(2u32, ...v2_data)` with correct topic, verified
by automated tests in `src/test_indexer_fixtures.rs`.

**Upgrade Path**: v3 bumps EVENT_SCHEMA_VERSION_V2 → 3 when storage schemas change.
See `INDEXER_EVENT_SCHEMA_VERSION = 3` and the `EventIndexTopicV3` struct for the active schema.

## Indexer Event Versioning (V2 → V3)

### What Changed

- Added `EVENT_INDEXED_V3` topic constant (`ev_idx3`).
- Added `EventIndexTopicV3` struct — same shape as V2 plus a `_reserved: u32` field for future additive fields.
- Added `emit_v2_and_v3()` helper that publishes both V2 and V3 events from all state-changing entries.
- Bumped `INDEXER_EVENT_SCHEMA_VERSION` from 2 to 3.

### Dual-Emission Strategy

All state-changing entries now call `emit_v2_and_v3()` which publishes:
1. **V2 event** at `EVENT_INDEXED_V2` (`ev_idx2`) with `version=2` — unchanged for existing subscribers.
2. **V3 event** at `EVENT_INDEXED_V3` (`ev_idx3`) with `version=3` — for forward-compatible indexers.

### Deprecation Policy

- V2 events continue to emit for at least **two contract minor versions** after V3 introduction.
- V2-only subscribers are safe during this window.
- After the deprecation window, V2 emission may be removed.
- V3 subscribers should validate `version == 3` and reject mismatches.

### V2-Compat Downgrade Flag

A `set_emit_v2_compat` admin method controls whether V2-indexed events (`ev_idx2`) are
emitted alongside V3 events:

- **Default: `true`** — V2 events are emitted (backward-compatible behavior).
- **`false`** — Only V3 events are emitted; indexers must have migrated.

The flag is stored under `DataKey2::EmitV2Compat` as a `bool`. Admin can toggle it at
any time. The flag is intended for a limited deprecation window; once all indexers have
migrated to V3, the flag should be set to `false` and eventually the V2 emission path
and this flag can be removed entirely.

**Timeline:**
1. **Current phase (V2+V3 dual-emission)**: Default `emit_v2_compat = true`. Both V2 and V3 events are emitted.
2. **Migration phase**: Admin disables compat flag for test indexers; validates V3-only behavior.
3. **Deprecation completion**: All indexers on V3 → admin sets `emit_v2_compat = false` permanently.
4. **Cleanup**: Future contract version removes V2 event emission and the `emit_v2_compat` flag.

### Migration Table

| Aspect | V2 | V3 |
|--------|----|----|
| Topic symbol | `ev_idx2` | `ev_idx3` |
| Struct | `EventIndexTopicV2` | `EventIndexTopicV3` |
| `version` field | `2` | `3` |
| `_reserved` | N/A | `u32` (always `0`) |
| Emission | `env.events().publish((EVENT_INDEXED_V2, ...), ...)` | dual-emitted via `emit_v2_and_v3` |
| Subscriber impact | Unchanged | New topic, validate version |

**Upgrade Path**: v3 will bump `EVENT_SCHEMA_VERSION_V2 → 3` when storage schemas change;
the constant guard test will catch any accidental early bump.
