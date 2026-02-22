# Revora-Contracts

Soroban contract for revenue-share offerings and blacklist management.

## Contract interface summary (for integrators)

**Contract:** `RevoraRevenueShare`

### Public methods

| Method | Parameters | Returns | Auth | Description |
|--------|------------|---------|------|-------------|
| `register_offering` | `issuer: Address`, `token: Address`, `revenue_share_bps: u32` | `Result<(), RevoraError>` | issuer | Register a revenue-share offering. Fails with `InvalidRevenueShareBps` if `revenue_share_bps > 10000`. |
| `get_offering` | `issuer: Address`, `token: Address` | `Option<Offering>` | — | Fetch one offering by issuer and token. |
| `list_offerings` | `issuer: Address` | `Vec<Address>` | — | List offering tokens for issuer (first page only, up to 20). |
| `report_revenue` | `issuer: Address`, `token: Address`, `amount: i128`, `period_id: u64` | — | issuer | Emit a revenue report; event includes current blacklist. |
| `get_offering_count` | `issuer: Address` | `u32` | — | Total offerings registered by issuer. |
| `get_offerings_page` | `issuer: Address`, `start: u32`, `limit: u32` | `(Vec<Offering>, Option<u32>)` | — | Paginated offerings. `limit` capped at 20. `next_cursor` is `Some(next_start)` or `None`. |
| `blacklist_add` | `caller: Address`, `token: Address`, `investor: Address` | — | caller | Add investor to blacklist for token. Idempotent. |
| `blacklist_remove` | `caller: Address`, `token: Address`, `investor: Address` | — | caller | Remove investor from blacklist. Idempotent. |
| `is_blacklisted` | `token: Address`, `investor: Address` | `bool` | — | Whether investor is blacklisted for token. |
| `get_blacklist` | `token: Address` | `Vec<Address>` | — | All blacklisted addresses for token. |

### Types

- **Offering:** `{ issuer: Address, token: Address, revenue_share_bps: u32 }`

### Error codes (RevoraError)

| Code | Name | Meaning |
|------|------|---------|
| 1 | `InvalidRevenueShareBps` | `revenue_share_bps` > 10000. |
| 2 | `LimitReached` | Reserved. |

Auth failures (e.g. wrong signer) are signaled by host/panic, not `RevoraError`. Use `try_register_offering` (and similar `try_*` client methods) to receive contract errors as `Result`.

### Events

| Topic / name | Payload | When |
|--------------|---------|------|
| `offer_reg` | `(issuer), (token, revenue_share_bps)` | After `register_offering`. |
| `rev_rep` | `(issuer, token), (amount, period_id, blacklist_vec)` | After `report_revenue`. |
| `bl_add` | `(token, caller), investor` | After `blacklist_add`. |
| `bl_rem` | `(token, caller), investor` | After `blacklist_remove`. |

### Call patterns and limits

- **Pagination:** Use `get_offerings_page(issuer, start, limit)` with `start = 0` then `start = next_cursor` until `next_cursor` is `None`. Max page size 20.
- **Off-chain:** Prefer small page sizes and bounded blacklist sizes for predictable gas. See storage/gas tests in `src/test.rs` for stress behavior.

### Build and test

```bash
cargo build --release
cargo test
```

### Contributor guidelines (reduce merge conflicts)

- Use feature branches per change (e.g. `feature/structured-error-codes`, `feature/storage-limit-negative-tests`).
- Tests in `src/test.rs` are grouped by area (pagination, blacklist, structured errors, storage stress, gas characterization). Add new tests in the relevant section so parallel PRs touch different regions.
- Keep the contract interface summary above in sync when adding or changing entrypoints or events.
