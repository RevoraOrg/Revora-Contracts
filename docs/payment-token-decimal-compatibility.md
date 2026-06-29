# Payment Token Decimal Compatibility

## Overview

Different payment tokens on the Stellar network use different decimal precisions. For example:

| Token | Decimals | 1 unit (raw) | 1 unit (7-dec canonical) |
|-------|----------|-------------|--------------------------|
| XLM   | 7        | 10_000_000  | 10_000_000               |
| USDC  | 6        | 1_000_000   | 10_000_000               |
| WBTC  | 8        | 100_000_000 | 10_000_000               |

Without normalization, depositing USDC revenue in raw amounts and computing holder shares produces
silent arithmetic errors — holders receive 10× too little or too much depending on the token.

## How It Works

The contract stores a per-offering decimal configuration for the payout asset via
`set_payment_token_decimals`. Before any holder share computation (in `claim` and
`get_claimable` / `get_claimable_chunk`), the raw revenue amount is normalized to Stellar's
canonical 7-decimal precision using `normalize_amount`.

### Normalization Rules

- **`from_decimals == 7`**: no-op, amount returned unchanged.
- **`from_decimals < 7`** (e.g., USDC at 6): scale **up** by `10^(7 - from_decimals)`.
- **`from_decimals > 7`** (e.g., WBTC at 8): scale **down** by `10^(from_decimals - 7)` using
  integer truncation.
- **Overflow protection**: if multiplication overflows `i128`, the function returns `0` to prevent
  fund inflation. This results in a zero payout for that period.

The normalization is applied in:
- `claim()` — before computing each period's payout.
- `compute_claimable_preview()` — used by `get_claimable()` and `get_claimable_chunk()`.

## API

### `set_payment_token_decimals(issuer, namespace, token, decimals: u32) -> Result<(), RevoraError>`

Sets the decimal precision of the payout asset for an offering. Requires issuer authorization.

- **Range**: `0..=18`. Values outside this range return `RevoraError::LimitReached`.
- **Default**: If not set, `7` (canonical Stellar stroops) is assumed.
- **Event**: Emits `dec_set` event with the configured value.

### `get_payment_token_decimals(issuer, namespace, token) -> u32`

Returns the configured decimal precision, or `7` if not set.

## Security Assumptions

1. **Issuer responsibility**: The `issuer` is trusted to supply the correct on-chain token decimal value. An incorrect value directly affects all future claim payouts. Issuers should verify the decimal on-chain before calling this function.
2. **Immutable after set**: There is no restriction on updating decimals after the fact, but changing decimals mid-offering will affect future claims inconsistently with past revenue reports. Issuers should set decimals before the first revenue report.
3. **Overflow is safe**: All multiplications are guarded with `checked_mul`. Overflow returns `0`, preventing fund inflation but potentially causing zero payouts for extremely large amounts with low-decimal tokens.
4. **Scope**: Decimals are per-offering, not per-asset globally. Two offerings with the same payout asset may have different decimal configurations.
5. **Read-Side Pagination**: When computing holder shares iteratively off-chain, be aware that paginated endpoints (e.g., `get_offerings_page`, `get_blacklist_page`) are capped at a `MAX_PAGE_LIMIT` of 20 to prevent unbounded execution. Indexers fetching multiple properties must handle this cursor-based traversal safely.

## Example

```rust
// Register offering with USDC (6 decimals) as payout asset
client.register_offering(&issuer, &ns, &token, &5_000, &usdc_address, &0);

// Configure decimals before first deposit
client.set_payment_token_decimals(&issuer, &ns, &token, &6);

// Deposit 1_000_000 raw USDC units (= 1.0 USDC at 6 decimals)
client.deposit_revenue(&issuer, &ns, &token, &usdc_address, &1_000_000, &1);

// At claim time, normalize_amount scales up:
//   1_000_000 (6-dec) → 10_000_000 (7-dec)
// Holder with 50% share (5_000 bps) receives:
//   10_000_000 * 5_000 / 10_000 = 5_000_000 canonical units
```

## Test Coverage

| Test | What it verifies |
|------|-----------------|
| `get_payment_token_decimals_defaults_to_7` | Default is 7 when not set |
| `set_and_get_payment_token_decimals` | Round-trip set/get |
| `set_payment_token_decimals_rejects_out_of_range` | Values > 18 rejected |
| `set_payment_token_decimals_accepts_max_18` | Boundary value 18 accepted |
| `set_payment_token_decimals_accepts_zero` | Value 0 accepted |
| `claim_normalizes_6_decimal_token_revenue` | 6-dec revenue scaled up in claim |
| `claim_normalizes_8_decimal_token_revenue` | 8-dec revenue scaled down in claim |
| `claim_with_7_decimal_token_is_unchanged` | 7-dec revenue unchanged (no-op) |
| `get_claimable_normalizes_6_decimal_token` | Preview reflects normalization |
