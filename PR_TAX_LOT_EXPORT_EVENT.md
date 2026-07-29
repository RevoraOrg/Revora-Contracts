# PR: Emit `tax_lot_v1` Event for Tax-Lot Reconstruction

**Closes #536**

---

## Summary

Indexers need a stable per-holder per-bucket event topic to reconstruct tax lots off-chain. This PR adds a `tax_lt1` event emitted on every tax-bucket update during `claim()`, providing indexers with the exact decomposition of each payout into return of capital (non-taxable) and capital gains (taxable), along with the associated period and ledger timestamp.

---

## Motivation

Off-chain tax-lot accounting requires knowing, for each holder payout, how much was return of capital (cost basis recovery) versus capital gains (taxable income). Without a dedicated event, indexers would need to replay the entire `rollover_distribution` logic to reconstruct this decomposition, which is fragile and computationally expensive.

The new `tax_lt1` event makes this decomposition observable directly from the event stream — indexers simply subscribe to `topics[0] == "tax_lt1"` and get the exact per-claim breakdown.

---

## New Event Schema

### `tax_lt1` (version 1)

**Topic**: `(tax_lt1, issuer, namespace, token)`

**Data tuple** (field order for indexer deserialization):

| Index | Field | Type | Description |
|-------|-------|------|-------------|
| 0 | `holder` | `Address` | The holder whose tax bucket was updated |
| 1 | `return_of_capital` | `i128` | Amount treated as return of capital (non-taxable) |
| 2 | `capital_gains` | `i128` | Amount treated as capital gains (taxable) |
| 3 | `amount` | `i128` | Total payout amount (`return_of_capital + capital_gains`) |
| 4 | `period_id` | `u64` | The period associated with this distribution |
| 5 | `timestamp` | `u64` | Ledger timestamp at claim time |

**Invariant**: `return_of_capital + capital_gains == amount` (lossless decomposition).

**Emitted by**: `rollover_distribution()` in `src/tax_bucket.rs`, called during `claim()`.

**Frequency**: One event per successful `claim()` call with a positive payout.

---

## Architecture

### Before

```
claim()
  └─ rollover_distribution(env, offering_id, holder, amount)
       ├─ Updates remaining_basis
       └─ Emits tax_roll (only on capital gains)
```

The existing `rollover_distribution` only emitted `tax_roll` in one branch (when `remaining_basis < amount`). There was no per-claim observable decomposition, making tax-lot reconstruction impossible from event data alone.

### After

```
claim()
  ├─ Computes total_payout across N periods
  └─ rollover_distribution(env, offering_id, holder, amount, period_id, timestamp)
       ├─ Updates remaining_basis
       ├─ Emits tax_roll (only on capital gains) — unchanged
       └─ Emits tax_lt1 (always, on every bucket update)
            data: (holder, return_of_capital, capital_gains, amount, period_id, timestamp)
```

The existing `tax_roll` event is preserved for backward compatibility. The new `tax_lt1` event is additive and emitted unconditionally on every tax-bucket update.

---

## Changes Made

### `src/tax_bucket.rs` — Core event emission

| Change | Details |
|--------|---------|
| `EVENT_TAX_LOT_V1` constant | `symbol_short!("tax_lt1")` with full doc-comment documenting topic, data tuple, and field order |
| `rollover_distribution` signature | Added `period_id: u64` and `timestamp: u64` parameters |
| Event emission | Unconditional `tax_lt1` publish after every bucket update, before returning `TaxBucketResult` |

### `src/lib.rs` — Call site update

| Change | Details |
|--------|---------|
| Claim function call site | Updated to pass `previous_period_id.expect(...)` and `now` (ledger timestamp) to `rollover_distribution` |

### `src/test_indexer_fixtures.rs` — Tests (5 new tests)

| Test | What it verifies |
|------|------------------|
| `fixture_tax_lot_v1_topic_symbol_is_stable` | `EVENT_TAX_LOT_V1` constant equals `symbol_short!("tax_lt1")` — pins the symbol against accidental rename |
| `fixture_tax_lot_v1_data_tuple_shape` | Full data tuple deserializes correctly on a successful claim; asserts `return_of_capital > 0` and `return_of_capital + capital_gains == amount` |
| `fixture_tax_lot_v1_capital_gains_when_basis_exhausted` | When `remaining_basis < amount`, `capital_gains > 0` and `return_of_capital` equals the remaining basis |
| `fixture_tax_lot_v1_zero_payout_emits_no_event` | Claim with `share_bps = 0` fails with `NoPendingClaims`; no `tax_lt1` event is emitted |
| `fixture_tax_lot_v1_burst_emits_n_events` | N separate claim calls produce exactly N `tax_lt1` events |

### `docs/tax-lot-export-event.md` — Documentation

Full specification of the `tax_lt1` event schema, field order, security invariants, and indexer integration guide.

---

## Test Coverage

### Edge Case: Empty Period — Zero Events

```
Scenario:
- Offering registered with revenue deposited
- Holder has NO share set (share_bps = 0)
- Call claim()

Expected:
- claim() fails with NoPendingClaims
- No tax_lt1 event emitted
```

**Test**: `fixture_tax_lot_v1_zero_payout_emits_no_event`

### Edge Case: Burst Period — N Events

```
Scenario:
- Offering registered with 3 periods of revenue deposited
- Holder has 50% share and sufficient cost basis
- Claim periods 1+2 (first call)
- Claim period 3 (second call)

Expected:
- 2 tax_lt1 events emitted (one per successful claim)
- Each event has correct holder, decomposition, period_id, and timestamp
```

**Test**: `fixture_tax_lot_v1_burst_emits_n_events`

### Capital Gains Coverage

```
Scenario:
- Holder has 100% share but only 1,000 cost basis
- Revenue deposited: 100,000
- Claim: payout = 100,000, basis = 1,000

Expected:
- return_of_capital = 1,000 (remaining basis)
- capital_gains > 0 (the excess)
- return_of_capital + capital_gains == amount
```

**Test**: `fixture_tax_lot_v1_capital_gains_when_basis_exhausted`

---

## Security Considerations

1. **Event guard on positive payout**: `tax_lt1` is only emitted when `total_payout > 0`. Failed claims (zero share, blacklisted, delay not elapsed) never emit the event, preventing spurious events.

2. **Lossless decomposition**: The invariant `return_of_capital + capital_gains == amount` is asserted in tests, ensuring the decomposition is always complete.

3. **Tamper-resistant timestamps**: `timestamp` is sourced from `env.ledger().timestamp()`, which is the authoritative Soroban ledger timestamp — not caller-supplied.

4. **Period integrity**: `period_id` is sourced from the claimed period's on-chain entry, not from caller input.

5. **Symbol distinctness**: The `tax_lt1` symbol is distinct from all other event symbols. The existing `v2_event_symbols_are_all_distinct` test picks up the new symbol (via its test list, which would need the new symbol added if it were an `ev_idx2` event type — as a standalone event it has no collision risk).

6. **Existing tax_roll preserved**: The pre-existing `EVENT_TAX_ROLLOVER` event is still emitted on capital gains, ensuring backward compatibility with any indexers consuming that event.

---

## Backward Compatibility

- **Fully backward compatible**: No existing interfaces or storage layouts are modified.
- **Existing events unchanged**: `EVENT_TAX_ROLLOVER` continues to be emitted with the same schema.
- **Additive change**: The new `tax_lt1` event is purely additive — existing indexers that ignore unknown topics are unaffected.
- **New parameters**: `rollover_distribution` signature changed (added `period_id` and `timestamp`), but this is a `pub fn` only used internally — no external callers.
- **No storage layout change**: `STORAGE_LAYOUT_VERSION` is not bumped.

---

## Files Changed

| File | Status | Lines |
|------|--------|-------|
| `src/tax_bucket.rs` | Modified | +28 / -22 |
| `src/lib.rs` | Modified | +6 / -1 |
| `src/test_indexer_fixtures.rs` | Modified | +237 / -1 |
| `docs/tax-lot-export-event.md` | Added | +82 |

---

## Checklist

- [x] `EVENT_TAX_LOT_V1` constant defined and documented
- [x] `tax_lt1` event emitted unconditionally on every tax-bucket update
- [x] Event data includes `(holder, return_of_capital, capital_gains, amount, period_id, timestamp)`
- [x] Field order and encoding documented in doc-comment
- [x] `rollover_distribution` call site updated with `period_id` and `timestamp`
- [x] Symbol stability test pins the event symbol
- [x] Data shape test validates full tuple deserialization
- [x] Capital gains test covers `remaining_basis < amount` path
- [x] Empty period edge case (no events)
- [x] Burst period edge case (N claims = N events)
- [x] Decomposition invariant (`roc + cg == amount`) tested
- [x] Docs file created with schema specification
- [x] Existing `tax_roll` event preserved

---

## Example Commit Message

```
feat: emit tax_lot_v1 event for tax-lot reconstruction (#536)

- Add EVENT_TAX_LOT_V1 (tax_lt1) constant with full documentation
- Emit tax_lt1 on every tax-bucket update with holder, return_of_capital,
  capital_gains, amount, period_id, and timestamp
- Update claim() call site to pass period_id and ledger timestamp
- Add 5 comprehensive tests: symbol stability, data shape with
  decomposition invariant, capital gains, zero payout, and burst
- Add docs/tax-lot-export-event.md with schema specification
```

---

## How to Test

```bash
# Run all tests
cargo test --all

# Run only the new tax_lot_v1 fixture tests
cargo test fixture_tax_lot_v1
```

Expected: All 5 new tests pass alongside the existing 480+ test suite.
