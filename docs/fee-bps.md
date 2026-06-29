# Fee BPS Configuration (RC26Q2-C20, Issue #269)

## Overview

The Revora contract supports a three-tier fee resolution hierarchy for platform
and per-asset basis-point (BPS) fees.  All fee values are unsigned 32-bit
integers representing hundredths of a percent (1 BPS = 0.01 %).

### Upper Bound

```
MAX_PLATFORM_FEE_BPS = 5 000   // 50 %
BPS_DENOMINATOR      = 10 000
```

Any setter that receives a value > `MAX_PLATFORM_FEE_BPS` returns
`RevoraError::InvalidRevenueShareBps` and makes no state change.

---

## Fee Resolution Hierarchy

When the contract needs the effective fee for a given *(offering, asset)* pair
it walks the following chain and returns at the first match (**O(1)** per level):

| Priority | DataKey                                | Description                               |
|----------|----------------------------------------|-------------------------------------------|
| 1 (highest) | `OfferingFeeBps(offering_id, asset)` | Per-offering per-asset override           |
| 2        | `PlatformFeePerAsset(asset)`           | Platform-wide per-asset fee               |
| 3 (lowest) | `PlatformFeeBps`                     | Global platform fee (default 0)           |

---

## Public API — `RevoraRevenueShare`

### Global Platform Fee

| Function | Auth | Complexity | Event |
|---|---|---|---|
| `set_platform_fee(fee_bps: u32)` | Admin | O(1) | `EVENT_PLATFORM_FEE_SET` |
| `get_platform_fee() -> u32` | None | O(1) | — |
| `calculate_platform_fee(amount: i128) -> i128` | None | O(1) | — |

`calculate_platform_fee` computes `amount × fee_bps / 10 000` using the stored
global platform fee BPS.

### Per-Offering Per-Asset Fee Override

| Function | Auth | Complexity | Event |
|---|---|---|---|
| `set_offering_fee_bps(issuer, namespace, token, asset, fee_bps: u32)` | Issuer | O(1) | `EVENT_FEE_CONFIG` |
| `get_offering_fee_bps(issuer, namespace, token, asset) -> u32` | None | O(1) | — |

The issuer must be the current owner of the offering.  The call returns
`OfferingNotFound` for unregistered offerings.

### Platform-Level Per-Asset Fee

| Function | Auth | Complexity | Event |
|---|---|---|---|
| `set_platform_fee_per_asset(asset, fee_bps: u32)` | Admin | O(1) | `EVENT_FEE_CONFIG` |
| `get_platform_fee_per_asset(asset) -> u32` | None | O(1) | — |

Different assets carry fully independent fees; setting one does not affect
another.

---

## Event Schema

### `EVENT_PLATFORM_FEE_SET` (`"fee_set"`)

Emitted by `set_platform_fee`.

| Field | Type | Value |
|-------|------|-------|
| topic[0] | `Symbol` | `"fee_set"` |
| data | `u32` | new `fee_bps` |

### `EVENT_FEE_CONFIG` (`"fee_cfg"`)

Emitted by `set_offering_fee_bps` and `set_platform_fee_per_asset`.

**Per-offering per-asset** (`set_offering_fee_bps`):

| Field | Type | Value |
|-------|------|-------|
| topic[0] | `Symbol` | `"fee_cfg"` |
| topic[1] | `Address` | issuer |
| topic[2] | `Symbol` | namespace |
| topic[3] | `Address` | token |
| topic[4] | `Address` | asset |
| data | `u32` | new `fee_bps` |

**Platform-level per-asset** (`set_platform_fee_per_asset`):

| Field | Type | Value |
|-------|------|-------|
| topic[0] | `Symbol` | `"fee_cfg"` |
| topic[1] | `Address` | asset |
| data | `u32` | new `fee_bps` |

---

## Test Coverage (Issue #269)

Tests live in `src/test.rs` inside `mod regression`.

### Platform Fee (existing + new)

| Test | Description |
|---|---|
| `default_platform_fee_is_zero` | Unset fee defaults to 0 |
| `set_and_get_platform_fee` | Basic set/get round-trip |
| `set_platform_fee_to_zero` | Fee can be reset to 0 |
| `set_platform_fee_to_maximum` | 5 000 BPS is accepted |
| `set_platform_fee_above_maximum_fails` | 5 001 BPS is rejected |
| `update_platform_fee_multiple_times` | Repeated updates store latest value |
| `calculate_platform_fee_basic` | 2.5 % of 10 000 = 250 |
| `calculate_platform_fee_with_zero_amount` | Zero amount yields zero fee |
| `calculate_platform_fee_with_zero_fee` | Zero fee yields zero fee |
| `calculate_platform_fee_at_maximum_rate` | 50 % of 10 000 = 5 000 |
| `calculate_platform_fee_precision` | 0.01 % of 1 000 000 = 100 |
| `platform_fee_large_amount` | 1 % of 1 trillion = 10 billion |
| `platform_fee_integration_with_revenue` | Fee subtracted from revenue correctly |
| `platform_fee_set_emits_event` | `EVENT_PLATFORM_FEE_SET` emitted |
| `platform_fee_reconfigure_emits_event_each_time` | Each set call emits one event |
| `platform_fee_boundary_one_bps_accepted` | 1 BPS accepted |
| `platform_fee_boundary_4999_bps_accepted` | 4 999 BPS accepted |

### Per-Offering Per-Asset Fee Override

| Test | Description |
|---|---|
| `offering_fee_bps_default_is_zero` | Unset override defaults to 0 |
| `set_offering_fee_bps_stores_and_retrieves` | Basic set/get round-trip |
| `set_offering_fee_bps_emits_fee_config_event` | `EVENT_FEE_CONFIG` emitted |
| `set_offering_fee_bps_at_maximum_boundary_succeeds` | 5 000 BPS accepted |
| `set_offering_fee_bps_above_maximum_fails` | 5 001 BPS rejected |
| `set_offering_fee_bps_fails_for_nonexistent_offering` | `OfferingNotFound` for unknown offering |
| `set_offering_fee_bps_reconfigure_replaces_previous` | Second set overwrites first |

### Platform-Level Per-Asset Fee

| Test | Description |
|---|---|
| `platform_fee_per_asset_default_is_zero` | Unset per-asset fee defaults to 0 |
| `set_platform_fee_per_asset_stores_and_retrieves` | Basic set/get round-trip |
| `set_platform_fee_per_asset_emits_fee_config_event` | `EVENT_FEE_CONFIG` emitted |
| `set_platform_fee_per_asset_above_maximum_fails` | 5 001 BPS rejected |
| `set_platform_fee_per_asset_zero_and_max_boundary` | 5 000 then 0 both accepted |
| `per_asset_fees_are_independent_across_different_assets` | Asset A fee ≠ Asset B fee |

---

## Security Assumptions

- Only the **admin** can set the global platform fee or platform-level per-asset
  fees.  Unauthorized calls panic via `Address::require_auth`.
- Only the **issuer** of a registered offering can set per-offering overrides.
  Non-issuer callers are rejected before any state mutation.
- All setters validate `fee_bps ≤ MAX_PLATFORM_FEE_BPS` before writing to
  storage, preventing fees that exceed the protocol maximum.
- Fee reads are non-authorised and O(1); they cannot mutate state.
