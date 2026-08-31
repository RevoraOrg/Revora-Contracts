# feat: add per-holder tax-year statement

## Summary

Holders and their accountants can now query year-end tax summaries broken down by tax bucket. The contract accumulates `return_of_capital`, `capital_gains`, and (reserved) `ordinary_income` on every claim, storing them per offering, per holder, per fiscal year. An issuer-configurable fiscal-year start month (default January) determines which fiscal year a payout timestamp belongs to.

---

## Motivation

Revenue-share distributions have tax consequences that vary by bucket:
- **Return of capital** — non-taxable distribution that reduces cost basis
- **Capital gains** — taxable profit realized when distributions exceed cost basis
- **Ordinary income** — reserved for future tax-bucket expansion (dividends, interest)

Prior to this PR, integrators could only observe these buckets via emitted `tax_lot_v1` events and had to reconstruct fiscal-year totals off-chain. This PR makes the accumulated per-fiscal-year totals available as a first-class on-chain query, eliminating the need for event replay.

---

## Architecture

### Storage Layer (`DataKey2`)

Two new persistent storage keys:

| Key | Type | Purpose |
|-----|------|---------|
| `FiscalYearStartMonth(OfferingId)` | `u32` | Configurable fiscal year start month (1–12, default 1 = January) |
| `TaxYearEntry(OfferingId, Address, u64)` | `TaxYearSummary` | Per-holder, per-fiscal-year accumulated totals |

### New Struct — `TaxYearSummary` (`src/tax_bucket.rs`)

```rust
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TaxYearSummary {
    pub ordinary_income: i128,   // reserved; currently always 0
    pub capital_gains: i128,     // taxable profit from distribution
    pub return_of_capital: i128, // non-taxable distribution
}
```

### Timestamp Helpers (`src/tax_bucket.rs`)

Three internal utilities convert Unix timestamps to fiscal years without external date libraries:

| Function | Purpose |
|----------|---------|
| `timestamp_to_year(ts) -> u32` | Gregorian calendar year from Unix timestamp |
| `timestamp_to_month(ts) -> u32` | Calendar month (1-based) from Unix timestamp |
| `fiscal_year_from_ts(ts, start_month) -> u64` | Fiscal year containing `ts` given the configured start month |

The fiscal year algorithm:
- If timestamp month < start month → fiscal year = calendar year - 1
- Otherwise → fiscal year = calendar year

Example: with April start (4), Apr 2024–Mar 2025 → fiscal year 2024.

### Accumulator — `update_tax_year_accumulator` (`src/tax_bucket.rs`)

Called from the `claim` path after `rollover_distribution` returns. Uses `saturating_add` to increment the per-fiscal-year `TaxYearSummary` entry in persistent storage. If no entry exists for the holder/year, it initializes a zero-filled record.

### Public API (`src/lib.rs`)

| Function | Parameters | Returns | Access |
|----------|------------|---------|--------|
| `set_fiscal_year_start` | `env, issuer, namespace, token, month` | `Result<(), RevoraError>` | Issuer quorum auth |
| `get_fiscal_year_start` | `env, issuer, namespace, token` | `u32` | Public read |
| `get_holder_tax_year` | `env, issuer, namespace, token, holder, year` | `TaxYearSummary` | Public read |

#### `set_fiscal_year_start`

- Validates `month` is in range 1–12; returns `InvalidAmount` otherwise
- Looks up the offering (returns `OfferingNotFound` if missing)
- Requires issuer quorum authorization via `require_issuer_quorum_auth`
- Stores the month in `DataKey2::FiscalYearStartMonth`

#### `get_fiscal_year_start`

- Returns the configured month, or `DEFAULT_FISCAL_START_MONTH` (1 / January) if not configured

#### `get_holder_tax_year`

- Returns the accumulated `TaxYearSummary` for a given offering, holder, and fiscal year
- **Never fails** — returns a zero-filled record for holders with no activity in the given year

### Claim Integration

The tax-year accumulator update is inserted in the `claim` method **after** `rollover_distribution` returns the `TaxBucketResult` but **before** the token transfer. The flow:

1. `rollover_distribution` computes `return_of_capital` and `capital_gains` from the remaining cost basis
2. The fiscal year is computed from the current ledger timestamp and the offering's configured start month
3. `update_tax_year_accumulator` is called with `ordinary_income=0` (reserved), `bucket.capital_gains`, and `bucket.return_of_capital`
4. The token transfer proceeds as before

---

## Files Changed

| File | Change | Lines |
|------|--------|-------|
| `src/tax_bucket.rs` | `TaxYearSummary` struct, timestamp helpers, `update_tax_year_accumulator` | +128 |
| `src/lib.rs` | DataKey2 variants, public API methods, claim integration, test module declaration | +108 / -3 |
| `src/test_tax_year.rs` | 7 test cases (new file) | +245 |
| `tools/storage_layout_schema.rs` | 2 new storage entries (`FiscalYearStartMonth`, `TaxYearEntry`) | +2 |

---

## Testing

### Test File: `src/test_tax_year.rs` (245 lines)

| # | Test Case | Coverage |
|---|-----------|----------|
| 1 | `fiscal_year_config_default_and_roundtrip` | Default is January (1); set/get roundtrip for April and December; invalid months 0 and 13 rejected with `InvalidAmount` |
| 2 | `year_boundary_april_start` | Fiscal year correctly splits at the configured April boundary: Mar 2024 → FY2024, Apr 2024 → FY2025 |
| 3 | `multi_year_holder_accumulates_correctly` | Multi-year accumulation: Jan 2024 (100k) + Dec 2024 (75k) → FY2024 = 175k; Mar 2025 (50k) → FY2025 = 50k |
| 4 | `fiscal_year_january_default` | Default January boundary: Jan 2024 → FY2024, Dec 2024 → still FY2024 (100k + 50k = 150k accumulated) |
| 5 | `no_activity_holder_returns_zeros` | Holder with no shares or claims returns zero for all three fields |
| 6 | `test_timestamp_to_year_month` | Timestamp helpers produce correct calendar year/month for epoch (0), Jan 2024, Apr 2024, Dec 2024, Mar 2025 |
| 7 | `test_fiscal_year_from_ts` | Fiscal year computation: Apr start with Mar 2024 → 2024, Apr 2024 → 2025; Jan start with all dates → calendar year match |

### Coverage Summary

- Config roundtrip with valid/invalid months: ✅
- Year boundary at exact fiscal start: ✅
- Multi-year accumulation correctness: ✅
- Default fiscal year (January): ✅
- No-activity holder (zero record): ✅
- Timestamp helper correctness (year + month): ✅
- Fiscal year computation edge cases: ✅

---

## Error Codes

| Discriminant | Variant | Condition |
|---|---|---|
| 21 | `InvalidAmount` | Fiscal year start month < 1 or > 12 |
| 4 | `OfferingNotFound` | Offering doesn't exist when calling `set_fiscal_year_start` |

---

## Security Considerations

1. **Saturating arithmetic** — All accumulator increments use `saturating_add`, preventing overflow for extreme payout amounts
2. **Deterministic fiscal year computation** — `fiscal_year_from_ts` produces the same result for the same timestamp and configuration, regardless of ledger state
3. **No reentrancy surface** — The accumulator is a pure storage read-modify-write with no external calls
4. **Issuer authentication** — `set_fiscal_year_start` requires `require_issuer_quorum_auth`, preventing unauthorized reconfiguration of fiscal year boundaries
5. **Backward compatible** — All new keys are additive; unconfigured offerings default to January fiscal year start; holders with no activity return zero-filled records
6. **Fail-safe reads** — `get_holder_tax_year` never panics: absent entries resolve to `TaxYearSummary { 0, 0, 0 }`
7. **No breaking API changes** — Existing `claim`, `deposit_revenue`, and `register_offering` signatures are unchanged

---

## Migration

No migration required. New `DataKey2` variants are additive with no conflicts. Offerings without `FiscalYearStartMonth` configured default to January (1). Holders without `TaxYearEntry` records return zero-filled summaries.

---

## Example Usage

```rust
// Issuer configures fiscal year starting in April
contract.set_fiscal_year_start(&issuer, &ns, &token, &4);

// After claims are processed, query holder's FY2024 summary
let fy2024 = contract.get_holder_tax_year(&issuer, &ns, &token, &holder, &2024);
// fy2024.return_of_capital == 100_000
// fy2024.capital_gains    == 0
// fy2024.ordinary_income  == 0

// FY2025 summary (starts Apr 2024)
let fy2025 = contract.get_holder_tax_year(&issuer, &ns, &token, &holder, &2025);
// fy2025.return_of_capital == 50_000
```

---

## Commit

```
d538c870aa21eef5e82884dfafdf6633d42d296e
```
