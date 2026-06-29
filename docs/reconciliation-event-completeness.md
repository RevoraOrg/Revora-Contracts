# Reconciliation Event Completeness (#188 / #289)

## Overview

This document describes the **Reconciliation Event Completeness** capability and its
extension for on-chain milestone signal integrity (issue #289). The feature ensures that
every persistent state mutation in `RevoraRevenueShare` emits a deterministic on-chain
`env.events().publish(...)` call, allowing off-chain indexers, accounting systems, and
`hardenedMilestoneValidation` consumers to reconstruct contract state entirely from the
event log.

## Motivation

Prior to this feature, 8 critical configuration-level functions wrote to persistent
storage without emitting observable events. Any indexer or reconciliation job that relied
solely on events would experience blind spots, leading to state drift between on-chain
data and off-chain models.

## Events Emitted on Every State Mutation

| Event constant | Function | Emitted data |
|---|---|---|
| `EVENT_CONC_LIMIT_SET` | `set_concentration_limit` | `(max_bps, enforce)` |
| `EVENT_ROUNDING_MODE_SET` | `set_rounding_mode` | `mode` |
| `EVENT_META_SIGNER_SET` | `register_meta_signer_key` | `pub_key` |
| `EVENT_META_DELEGATE_SET` | `set_meta_delegate` | `delegate` |
| `EVENT_MULTISIG_INIT` | `init_multisig` | `(members, threshold)` |
| `EVENT_ADMIN_SET` | `initialize` / `set_admin` | `admin` |
| `EVENT_PLATFORM_FEE_SET` | `set_platform_fee` | `fee_bps` |
| `EVENT_PLATFORM_FEE_ASSET_SET` | `set_platform_fee_per_asset` | `fee_bps` |
| `EVENT_OFFERING_FEE_SET` | `set_offering_fee_bps` | `fee_bps` |
| `EVENT_PROPOSAL_EXECUTED_V2` | `execute_action` | `proposal_id` |
| `EVENT_AUDIT_REPAIRED` | `repair_audit_summary` | `(total_revenue, report_count)` |
| `EVENT_OFFER_REG_V2` | `register_offering` | `(token, share_bps, payout_asset)` |
| `EVENT_REV_REP_V2` | `report_revenue` | `(amount, period_id, blacklist)` |
| `EVENT_REV_DEPOSIT_V2` | `deposit_revenue` | `(payment_token, amount, period_id)` |
| `EVENT_CLAIM_V2` | `claim` | `(holder, amount, periods)` |
| `EVENT_SHARE_SET_V2` | `set_holder_share` | `(holder, share_bps)` |

## Security Assumptions & Risk Note

### Security Assumptions
- **Events are Informational**: On-chain events are strictly for off-chain reconstruction and auditing. They do not grant authority and cannot be used to modify contract state.
- **Authorization Enforcement**: Every state mutation requires valid authorization (e.g., `issuer.require_auth()`, admin signatures, or multisig threshold approval) before an event is emitted.
- **Deterministic State**: The combination of persistent storage and events ensures that the contract state can be audited and verified by independent parties.
- **AuditSummary Integrity**: Decimal normalization and saturation logic in `AuditSummary` prevent arithmetic issues while maintaining a verifiable total of all revenue transitions.

### Risk Note
- **Indexer Dependency**: Off-chain systems relying on these events must handle potential network delays or re-orgs (though Soroban's finality minimizes this).
- **V1/V2 Coexistence**: While v2 events provide a more robust schema, legacy v1 events are maintained for backward compatibility. Consumers should prioritize v2 events for new integrations.
- **Event-Only Mode**: In event-only mode, storage mutations are skipped, and only events are emitted. This is intended for high-throughput reporting where on-chain state persistence is not required.

## Testing Strategy

All event emissions are covered by automated tests, including:
- `test_reconciliation_completeness`: Asserts that all 8+ critical config-level functions emit events.
- Revenue lifecycle tests: Verify that `register_offering`, `report_revenue`, `deposit_revenue`, and `claim` emit correctly versioned v2 events.
- Edge cases: Tests cover supply cap enforcement, blacklist checks, and unauthorized attempts to trigger mutations.

```bash
cargo test test_reconciliation_completeness
```

All tests pass, ensuring full parity between documented requirements and implementation.
