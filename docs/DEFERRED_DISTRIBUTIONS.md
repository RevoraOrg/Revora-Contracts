# Deferred Distributions

Adds a `defer_until_close` flag to revenue reports.

### Lifecycle

1. **Queueing:** Deferred reports are stored in the `DeferredReports` mapping keyed
   by `period_id`.  Each entry holds the deferred amount as an `i128` under
   `DeferredDataKey::DeferredReports(period_id)` in persistent storage.

2. **Security Barrier:** Any `claim` attempt against a period still in the deferred
   mapping will immediately panic with `DistributionDeferred` (error code #456),
   preventing holders from claiming before the issuer has finalised the amounts.

3. **Atomic Flush:** Calling `close_period` removes the block. The internal
   `RevoraRevenueShare::close_period(env, period_id)` performs an O(1) flush:
   - Reads the deferred amount for `period_id`
   - Removes the key from persistent storage
   - Emits a `def_flush` event with the released amount

### Gas Characteristics

Flushing a single deferred entry is **O(1)** — independent of the total number of
entries in the queue.  This property is enforced by dedicated gas-bound tests.

| Scenario | CPU budget |
|----------|------------|
| Single entry flush | ≤ 350,000 instructions |
| 1,000 entry sequential flush | ≤ 350,000,000 instructions |
| No-op flush (absent entry) | ≤ 350,000 instructions |

For the full budget derivation, methodology, and test output see
[`docs/CLOSE_PERIOD_GAS_TEST_SUMMARY.md`](./CLOSE_PERIOD_GAS_TEST_SUMMARY.md).

### Storage Key

```rust
#[soroban_sdk::contracttype]
pub enum DeferredDataKey {
    DeferredReports(u32),  // period_id → deferred amount (i128)
}
```

### Error Code

| Code | Name | Condition |
|------|------|-----------|
| 456 | `DistributionDeferred` | `claim` called while `DeferredReports(period_id)` exists |
