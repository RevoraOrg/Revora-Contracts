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
