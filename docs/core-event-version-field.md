# Core Event Version Field (v2)

## Purpose
Production-grade deterministic versioning for all core events. Enables:

1. **Schema evolution** - Indexers parse version-first data tuples
2. **Security** - Reject malformed/replay events, enforce schema compliance  
3. **Determinism** - Always emit v2 events, no feature flags
4. **Auditability** - Versioned events for historical reconstruction

## Schema Rules (Production Requirements)
```
ALL v2 events MUST:
- Emit Symbol topic[0] = EVENT_FOO_V2 (e.g. "ofr_reg2")
- Data[0] = EVENT_SCHEMA_VERSION_V2 = 2u32 (position 0, ALWAYS first)
- Data[1...] = legacy data tuple (unchanged semantics)
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

| Flow | V2 Topic | Data Schema (position 0=version) |
|------|----------|----------------------------------|
| **register_offering** | `ofr_reg2` | `[2, token:Address, revenue_share_bps:u32, payout_asset:Address]` |
| **report_revenue init** | `rv_init2` | `[2, amount:i128, period_id:u64, blacklist:Vec<Address>]` |
| **report_revenue init-asset** | `rv_inia2` | `[2, payout_asset:Address, amount:i128, period_id:u64, blacklist:Vec<Address>]` |
| **report_revenue generic** | `rv_rep2` | `[2, amount:i128, period_id:u64, blacklist:Vec<Address>]` |
| **report_revenue asset** | `rv_repa2` | `[2, payout_asset:Address, amount:i128, period_id:u64]` |
| **deposit_revenue** | `rev_dep2` | `[2, payment_token:Address, amount:i128, period_id:u64]` |
| **deposit_revenue snapshot** | `rev_snp2` | `[2, payment_token:Address, amount:i128, period_id:u64, snapshot_reference:u64]` |
| **set_holder_share** | `sh_set2` | `[2, holder:Address, share_bps:u32]` |
| **claim** | `claim2` | `[2, holder:Address, total_payout:i128, periods:Vec<u64>]` |
| **freeze** | `frz2` | `[2, frozen:bool]` |

## Security Properties

1. **Position 0 Version**: Schema-breaking changes bump version. Legacy indexers ignore v2+.
2. **Deterministic Emission**: No flags - ALL core events emit v2 tuple.
3. **Topic Schema Mapping**: Off-chain indexers validate topic → schema.
4. **Replay Protection**: Version + ledger context + deterministic data prevents replays.

**Off-chain Rejection Logic**:
```rust
match event.topic {
  "ofr_reg2" => if data[0] != 2 || data.len() != 4 { reject }
  "rv_init2" => if data[0] != 2 || data.len() != 4 { reject }
  // etc...
}
```

## Migration Guide (v1 → v2)

| Legacy | Status | Replacement |
|--------|--------|-------------|
| `ofr_reg1` | **deprecated** | `ofr_reg2` |
| `rv_init1` | **deprecated** | `rv_init2` |
| conditional flag | **removed** | always emit v2 |

**Backward Compatibility**: v1 events still emitted via legacy paths. v2 indexers ignore v1.

## Indexer Best Practices

1. **Version Validation**: Always check `data[0] == 2` for v2 events
2. **Topic Whitelist**: Only process known V2 topics  
3. **Data Length**: Enforce exact tuple length per topic
4. **Storage Replay**: Use version+period_id+ledger as dedup key
5. **Dual Index**: Parallel v1/v2 streams during migration (v1 deprecated 90d post-v2)

## Verification Steps

```
1. cargo test
2. Deploy testnet → smoke test all core flows
3. Indexers: verify v2 parsing on test events
4. Mainnet: deploy + monitor event emission
```

**Success Criteria**: 100% core events emit `(2u32, ...v2_data)` with correct topic.

**Upgrade Path**: v3 will bump EVENT_SCHEMA_VERSION_V2 → 3 when storage schemas change.

