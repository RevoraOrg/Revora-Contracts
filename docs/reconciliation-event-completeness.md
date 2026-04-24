# Reconciliation Event Completeness (#188)

## Overview

This document describes the **Reconciliation Event Completeness** capability shipped with this PR. The feature ensures that every persistent state mutation in `RevoraRevenueShare` emits a deterministic on-chain `env.events().publish(...)` call, allowing off-chain indexers, accounting systems, and auditing tools to reconstruct contract state entirely from the event log.

## Motivation

Prior to this feature, 8 critical configuration-level functions wrote to persistent storage without emitting observable events. Any indexer or reconciliation job that relied solely on events would experience blind spots, leading to state drift between on-chain data and off-chain models.

## New Events

| Event Constant | Function | Emitted Data |
|---|---|---|
| `EVENT_CONC_LIMIT_SET` | `set_concentration_limit` | `(max_bps, enforce)` |
| `EVENT_ROUNDING_MODE_SET` | `set_rounding_mode` | `mode` |
| `EVENT_MULTISIG_INIT` | `init_multisig` | `(members, threshold)` |
| `EVENT_ADMIN_SET` | `initialize` / `set_admin` | `admin` |
| `EVENT_PLATFORM_FEE_SET` | `set_platform_fee` | `fee_bps` |
| `EVENT_MIN_REV_THRESHOLD_SET` | `set_min_revenue_threshold` | `threshold` |
| `EVENT_CLAIM_DELAY_SET` | `set_claim_delay` | `delay_seconds` |
| `EVENT_METADATA_SET` | `set_offering_metadata` | `metadata` |
| `EVENT_METADATA_UPDATED` | `set_offering_metadata` | `metadata` |

## V2 Indexed Events (Standardized)

All core state mutations now also emit a standardized `EVENT_INDEXED_V2` event with an `EventIndexTopicV2` structure for robust off-chain indexing.

| Event Type (v2) | Function | Purpose |
|---|---|---|
| `offer` | `register_offering` | Offering registration |
| `fee_cfg` | `set_fee_configuration` | Fee configuration changes |
| `min_rev` | `set_min_revenue_threshold` | Minimum revenue threshold updates |
| `round` | `set_rounding_mode` | Rounding mode updates |
| `conc` | `set_concentration_limit` | Concentration limit updates |
| `delay` | `set_claim_delay` | Claim delay updates |
| `ms_init` | `init_multisig` | Multisig initialization |
| `meta_set` | `set_offering_metadata` | Initial metadata attachment |
| `meta_upd` | `set_offering_metadata` | Metadata updates |
| `inv_con` | `set_investment_constraints` | Investment constraints updates |
| `adm_set` | `set_admin` | Global admin initialization |
| `plat_fee` | `set_platform_fee` / `set_platform_fee_per_asset` | Platform fee updates |

## Security Assumptions

- Events are **informational only** — they carry no authority. They cannot be used to replay or spoof state changes.
- All existing authorization requirements (`issuer.require_auth()`, multisig threshold checks, etc.) remain in force before an event can be emitted.
- Decimal normalization now also applies to `AuditSummary.total_revenue` so reconciliation figures match payout math exactly.

## Testing

All event emissions are covered by the `test_reconciliation_completeness` module in `src/test.rs`. Tests assert that calling each mutating function strictly increases the event count.

```
cargo test --features testutils test_reconciliation_completeness
```

All 7 tests pass.
