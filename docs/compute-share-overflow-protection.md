# compute_share — Overflow Protection & Invariant Proof [RC26Q2-C02]

## Function Signature

```rust
pub fn compute_share(
    _env: Env,
    amount: i128,
    revenue_share_bps: u32,   // basis points, 0–10_000
    mode: RoundingMode,        // Truncation | RoundHalfUp
) -> i128
```

## Invariants

| # | Invariant | Formal statement |
|---|---|---|
| 1 | **Bounds** | `result ∈ [min(0, amount), max(0, amount)]` |
| 2 | **No overflow** | No panic, no wrap for any valid i128 input |
| 3 | **Zero identity** | `bps = 0 ∨ amount = 0 → result = 0` |
| 4 | **Full share** | `bps = 10_000 → result = amount` |
| 5 | **Over-bps guard** | `bps > 10_000 → result = 0` |
| 6 | **Rounding direction** | `RoundHalfUp ≥ Truncation` for positive amounts |

## Why Overflow Cannot Occur

The implementation decomposes `amount` as `q * 10_000 + r`:

```
q = amount / 10_000        |q| ≤ i128::MAX / 10_000
r = amount % 10_000        |r| < 10_000
bps ≤ 10_000

r * bps  →  |r * bps| < 10_000 × 10_000 = 10^8   (fits i128 trivially)
q * bps  →  checked_mul with saturating fallback   (never wraps)
base + remainder_share  →  checked_add with saturating fallback
final clamp to [min(0,amount), max(0,amount)]      (bounds invariant)
```

The clamp is the last line of defence: even if saturation produced an
out-of-range intermediate, the result is forced back into the valid range.

## Representative Test Ranges

| amount | bps | Truncation | RoundHalfUp | Notes |
|---|---|---|---|---|
| `i128::MAX` | 10_000 | `i128::MAX` | `i128::MAX` | Full share at max |
| `i128::MAX` | 5_000 | `i128::MAX / 2` | `≥ i128::MAX / 2` | 50% at max |
| `i128::MAX` | 1 | `> 0` | `> 0` | 0.01% at max |
| `i128::MIN` | 10_000 | `i128::MIN` | `i128::MIN` | Full share at min |
| `i128::MIN` | 5_000 | `≤ 0` | `≤ 0` | 50% at min |
| `i128::MIN` | 1 | `< 0` | `< 0` | 0.01% at min |
| `1` | 5_000 | `0` | `1` | Rounding boundary |
| `-1` | 5_000 | `0` | `-1` | Negative rounding boundary |
| `3` | 5_000 | `1` | `2` | 1.5 rounds up |
| any | 10_001 | `0` | `0` | Over-bps guard |

## Security Note

`compute_share` is called in every claim payout path. An overflow or
out-of-bounds result would allow a holder to claim more than their entitled
share, potentially draining the contract. The decomposition approach
eliminates the intermediate `amount * bps` product that would overflow for
large i128 values, and the final clamp enforces the bounds invariant
unconditionally.

## Test Coverage

All invariants are verified in `src/test_compute_share_invariants.rs`:

- Table-driven cases for both `Truncation` and `RoundHalfUp`
- i128 extreme values: `i128::MAX`, `i128::MIN`, `i128::MIN + 1`, `i128::MAX / 2`
- Zero identity for all bps values
- Over-bps guard for `bps > 10_000` including `u32::MAX`
- Full share (`bps = 10_000`) for all extreme amounts
- Rounding boundary: exact half-unit cases for positive and negative amounts
- Cross-mode invariant: `RoundHalfUp ≥ Truncation` for a matrix of positive amounts × bps
