# Freeze Reason V1 Event

Issue: #606

## Summary

Adds a `freeze_reason_v1` event (`frz_rsn`) emitted on every `set_freeze` call so indexers can categorize halts without inspecting storage.

## Event Schema

| Field       | Type          | Description                          |
|-------------|---------------|--------------------------------------|
| topic[0]    | `Symbol`      | `frz_rsn`                            |
| topic[1]    | `Address`     | Admin who called `set_freeze`        |
| data[0]     | `u32`         | Schema version (`EVENT_SCHEMA_VERSION_V2 = 2`) |
| data[1]     | `FreezeReason`| Reason for the freeze                |

## Emission Points

- `set_freeze(env, reason)` — global freeze with explicit reason
- `freeze(env)` — convenience wrapper, emits with `FreezeReason::Compliance`

## FreezeReason Enum

| Variant         | Description                               |
|-----------------|-------------------------------------------|
| Compliance      | Broad compliance or regulatory action     |
| LegalHold       | Court-ordered legal hold                  |
| DisputeOpen     | Active dispute under investigation        |
| SanctionsMatch  | Address matched on sanctions list         |
| Sanctions       | Legacy variant (storage compatibility)   |
| CourtOrder      | Legacy variant                           |
| IssuerDispute   | Legacy variant                           |
| Manual          | Legacy variant                           |

## Indexer Guidance

Indexers should subscribe to the `frz_rsn` topic and validate `data[0] == 2` (schema version). The reason enum and admin address allow categorizing halts without storage inspection.
