# Tax-Lot Export Event (tax_lt1)

Issue: #536

## Summary

Indexers need a stable per-holder per-bucket event topic to reconstruct tax lots off-chain. This change adds a `tax_lt1` event emitted on each tax-bucket update during `claim`.

## New Event

### `tax_lt1`

**Topic**: `(tax_lt1, issuer, namespace, token)`

**Data tuple** (field order for indexer deserialization):

| Index | Field             | Type      | Description                                      |
|-------|-------------------|-----------|--------------------------------------------------|
| 0     | `holder`          | `Address` | The holder whose tax bucket was updated.         |
| 1     | `return_of_capital` | `i128`  | Amount treated as return of capital (non-taxable). |
| 2     | `capital_gains`   | `i128`    | Amount treated as capital gains (taxable).       |
| 3     | `amount`          | `i128`    | Total payout amount (`return_of_capital + capital_gains`). |
| 4     | `period_id`       | `u64`     | The period associated with this distribution.    |
| 5     | `timestamp`       | `u64`     | Ledger timestamp at the time of the event.       |

**Emitted by**: `rollover_distribution()` in `src/tax_bucket.rs`, called during `claim()`.

**Frequency**: One event per successful `claim()` call that results in a positive payout.

**Version**: 1

## Implementation

### Files changed

- `src/tax_bucket.rs`
  - Added `EVENT_TAX_LOT_V1` constant (`symbol_short!("tax_lt1")`)
  - Modified `rollover_distribution()` to accept `period_id` and `timestamp` params
  - Added `tax_lt1` event emission after every bucket update

- `src/lib.rs`
  - Updated `claim()` call site to pass `previous_period_id` and `now` to `rollover_distribution()`

- `src/test_indexer_fixtures.rs`
  - Added `fixture_tax_lot_v1_topic_symbol_is_stable` — pins the symbol string
  - Added `fixture_tax_lot_v1_data_tuple_shape` — validates the full data tuple
  - Added `fixture_tax_lot_v1_zero_payout_emits_no_event` — empty period edge case
  - Added `fixture_tax_lot_v1_burst_emits_n_events` — burst period edge case

## Security and Correctness

- Event is only emitted when `total_payout > 0`, preventing spurious events on failed claims.
- `return_of_capital` plus `capital_gains` always equals `amount`, ensuring the decomposition is lossless.
- `period_id` and `timestamp` are sourced from the contract's own state and ledger, not from caller input, preventing manipulation.
- The `tax_lt1` symbol (`symbol_short!("tax_lt1")`) is distinct from all other event symbols, checked by the test `v2_event_symbols_are_all_distinct` in `test_indexer_fixtures.rs`.
