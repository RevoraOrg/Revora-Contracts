# Revenue Range Chunk Query

The Revenue Range Chunk Query capability provides a production-grade, bounded mechanism for querying revenue data over a range of periods. This is designed for DApps and indexers to safely paginate through large datasets without exceeding execution limits.

## Capability

### `get_revenue_range_chunk`
Returns the sum of revenue for a numeric period range, bounded by a maximum number of periods per call.

**Signature:**
```rust
pub fn get_revenue_range_chunk(
    env: Env,
    issuer: Address,
    namespace: Symbol,
    token: Address,
    from_period: u64,
    to_period: u64,
    max_periods: u32,
) -> (i128, Option<u64>)
```

**Returns:**
- `(sum, next_start)`:
    - `sum`: Total revenue for the processed periods.
    - `next_start`: `Some(period)` if more periods remain in the requested range; `None` if the range is fully processed.

## Features & Hardening

### 1. Deterministic Execution
To prevent CPU/Gas exhaustion in the Soroban environment, the query enforces a hard cap on the number of periods processed per call (`MAX_CHUNK_PERIODS = 200`). If `max_periods` is requested as `0` or exceeds this limit, it is automatically capped.

### 2. Robust Input Validation
- **Invalid Ranges**: If `from_period > to_period`, the function returns `(0, None)` immediately.
- **Empty Offerings**: If the offering or specific periods have no reported revenue, the function returns a sum of `0` for those segments, ensuring consistent behavior across all queries.

### 3. Gas Efficiency
The function performs indexed storage reads for each period. By batching these reads into chunks, users can optimize their data retrieval costs while staying within ledger read limits.

## Usage Pattern

To query a full range from `start` to `end`:

```rust
let mut cursor = start;
let mut total = 0;
loop {
    let (chunk_sum, next) = client.get_revenue_range_chunk(&issuer, &ns, &token, &cursor, &end, &50);
    total += chunk_sum;
    if let Some(next_p) = next {
        cursor = next_p;
    } else {
        break;
    }
}
```

## Security Assumptions
- **Read-Only**: This function does not modify state and is safe to call from any context.
- **No Auth Required**: Revenue data is public to all participants in the Revora ecosystem; therefore, no `require_auth` is enforced on this query.

## Tests & Notes

Automated tests were added to validate deterministic, bounded cursor iteration and edge cases:

- `get_revenue_range_chunk_matches_full_sum` — sums a full range by iterating in chunks and compares to the unbounded `get_revenue_range` result.
- `get_revenue_range_chunk_inverted_range_returns_zero` — validates that `from > to` returns `(0, None)`.
- `get_revenue_range_chunk_cap_clamps_and_returns_next_start` — ensures `max_periods=0` normalizes to `MAX_CHUNK_PERIODS` (200) and next cursor points to the remaining period.
- `get_revenue_range_chunk_chunked_iteration_off_by_one_sequence` — verifies cursor progression and off-by-one behavior for small ranges.

Running `cargo test` and `cargo clippy` in the Soroban/Rust environment should produce green tests for these additions. Security considerations:

- The function is read-only and deterministic; attackers cannot manipulate returned cursors because the function uses only input ranges and stored per-period revenue values.
- Capping `max_periods` prevents denial-of-service via unbounded reads. Ensure indexers respect returned `next_start` and loop until `None`.
