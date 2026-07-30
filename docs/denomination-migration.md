# Denomination Migration

## Overview

Some payment tokens (e.g., USDC, EURC) may change their on-chain decimal precision over time via a protocol upgrade. When this happens, all raw amounts stored in this contract — revenue totals, audit summaries, supply caps — that represent balances in that token must be re-scaled to stay consistent with the token's new decimal representation.

`migrate_denomination` provides a controlled, issuer-authorized migration path that re-scales stored aggregate amounts and updates the `PaymentTokenDecimals` metadata in one atomic call.

## Function Signature

```rust
pub fn migrate_denomination(
    env: Env,
    issuer: Address,
    namespace: Symbol,
    token: Address,
    from_decimals: u32,
    to_decimals: u32,
) -> Result<(), RevoraError>
```

## Parameters

| Parameter       | Type      | Description                                                  |
|-----------------|-----------|--------------------------------------------------------------|
| `issuer`        | `Address` | The offering issuer (must sign the transaction).             |
| `namespace`     | `Symbol`  | The offering namespace.                                      |
| `token`         | `Address` | The offering token address.                                  |
| `from_decimals` | `u32`     | The **current** decimal precision of the payment token.      |
| `to_decimals`   | `u32`     | The **new** decimal precision of the payment token.          |

## Behaviour

### Amounts re-scaled

| Storage Key                              | Type            | Re-scaling behaviour                         |
|------------------------------------------|-----------------|----------------------------------------------|
| `DataKey2::DepositedRevenue(OfferingId)`  | `i128`          | Multiplied / divided by `10^\|to-from\|`      |
| `DataKey::AuditSummary.total_revenue`     | `i128`          | Multiplied / divided by `10^\|to-from\|`      |
| `DataKey2::SupplyCap(OfferingId)`         | `i128`          | Multiplied / divided by `10^\|to-from\|` (if set) |
| `DataKey2::PaymentTokenDecimals`          | `u32`           | Updated to `to_decimals`                     |

### Upscale (`to > from`)

Stored amounts are multiplied by `10^(to - from)`.

**Example:** USDC migrates from 6 decimals → 18 decimals.
A `DepositedRevenue` of `1_000_000` (1.0 USDC) becomes `1_000_000_000_000_000_000`.

### Downscale (`to < from`)

Stored amounts are divided by `10^(from - to)`.

**⚠️ Precision loss warning:** Integer division truncates. If a stored amount is not evenly divisible by the scale factor, the remainder is lost. For example, downscaling from 18 → 6 decimals with a `DepositedRevenue` of `1_999_999_999_999_999_999` (≈ 1.9999... tokens) becomes `1_999_999` (≈ 1.999999 tokens), silently discarding the fractional tail. Re-scaling back (6 → 18) would not recover the lost precision.

Issuers SHOULD ensure that stored amounts are clean multiples of the scale factor before initiating a downscale migration.

### No-op (`from == to`)

Returns `Ok(())` immediately with no state mutation and no event emitted.

## Idempotency

Each distinct `(offering_id, from_decimals, to_decimals)` path is executed **at most once**. A boolean marker is persisted under `DataKey2::DenomMigration(OfferingId, u32, u32)` after the first successful call. Subsequent calls with the same triple return `Ok(())` early with no state changes.

This guarantees that:
- Retries due to ledger failures are safe.
- Multiple issuers cannot accidentally double-migrate the same path.
- Different migration paths (e.g., 6→18 vs 18→6) are independent and can both be executed.

## Authorization

1. `issuer.require_auth()` — the caller must authenticate as the issuer.
2. `Self::require_not_frozen` / `Self::require_not_paused` — the contract must not be frozen or paused.
3. `offering.issuers.primary == issuer` — only the offering's primary issuer can migrate.
4. `Self::require_issuer_quorum_auth` — if co-issuers are configured, the quorum must be met.

## Event

```rust
event: (den_mig, issuer, namespace, token)
data:  (from_decimals: u32, to_decimals: u32, caller: Address)
```

## Error Cases

| Error                          | Condition                                              |
|--------------------------------|--------------------------------------------------------|
| `OfferingNotFound`             | Offering does not exist, or caller is not the issuer.  |
| `LimitReached`                 | `from_decimals > 18` or `to_decimals > 18`.            |
| `ContractFrozen`               | Contract-level freeze is active.                       |
| `ContractPaused`               | Contract is paused.                                    |
| `InvalidAmount`                | Checked arithmetic overflow during re-scaling.         |

## Limitations

### Per-period revenues

`DataKey::PeriodRevenue(OfferingId, u64)` entries are **not** re-scaled by this function. The issuer should close any open periods before calling `migrate_denomination` so that future deposits use the new decimal precision. Past unclaimed periods remain in the old denomination; the issuer may re-deposit corrected amounts if needed.

### Downscale precision

See the **Downscale** section above for the truncation caveat.

## Usage Example

```rust
// USDC migrates from 6 to 18 decimals
contract.migrate_denomination(
    &issuer,
    &symbol_short!("def"),
    &token,
    &6,   // from
    &18,  // to
)?;
```

## Storage Layout

```
DataKey2::DenomMigration(OfferingId, u32, u32)  →  bool    offering+path
```

## Testing

See `src/test_denom_migration.rs` for the test suite covering:

- ✅ Upscale (6→18) — amounts multiplied correctly
- ✅ Downscale (18→6) — amounts divided correctly
- ✅ No-op (same decimals) — no state mutation
- ✅ Idempotency — second call with same (from, to) is a no-op
- ✅ Idempotency — different (from, to) paths execute independently
- ✅ Authorization — non-issuer rejected (returns OfferingNotFound)
- ✅ Non-existent offering — returns OfferingNotFound
- ✅ Decimal bounds — `> 18` rejected
- ✅ SupplyCap re-scaling — supply cap rescaled when present
- ✅ Event emission — `den_mig` event published with correct data
