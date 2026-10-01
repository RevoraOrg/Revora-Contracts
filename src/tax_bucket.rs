use crate::{DataKey3, OfferingId};
use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol};

pub const EVENT_TAX_ROLLOVER: Symbol = symbol_short!("tax_roll");
/// Emitted on each tax-bucket update to enable off-chain tax-lot reconstruction.
///
/// Topic:  `(tax_lot_v1, issuer, namespace, token)`
/// Data:   `(holder: Address, return_of_capital: i128, capital_gains: i128,
///           amount: i128, period_id: u64, timestamp: u64)`
///
/// ### Field order (for indexer deserialization)
/// 0. `holder`          — Address of the holder whose bucket was updated.
/// 1. `return_of_capital` — Amount treated as return of capital (non-taxable).
/// 2. `capital_gains`    — Amount treated as capital gains (taxable).
/// 3. `amount`           — Total payout amount (`return_of_capital + capital_gains`).
/// 4. `period_id`        — The period associated with this distribution.
/// 5. `timestamp`        — Ledger timestamp at the time of the event.
pub const EVENT_TAX_LOT_V1: Symbol = symbol_short!("tax_lt1");

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TaxBucketResult {
    pub return_of_capital: i128,
    pub capital_gains: i128,
}

/// Per-holder, per-fiscal-year accumulated tax summary.
///
/// Returned by `get_holder_tax_year`. Accumulated on every `rollover_distribution`
/// call by incrementing the active fiscal year's entry in persistent storage.
///
/// Fields match the tax-bucket breakdown expected by integrators:
/// - `ordinary_income`:  Ordinary taxable income (dividends, interest, etc.)
/// - `capital_gains`:    Capital gains (profit from sale of securities)
/// - `return_of_capital`: Return of capital (non-taxable distribution)
///
/// Currently the system only populates `return_of_capital` and `capital_gains`.
/// The `ordinary_income` field is reserved for future tax-bucket expansion.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TaxYearSummary {
    /// Ordinary taxable income (dividends, interest, etc.).
    pub ordinary_income: i128,
    /// Total capital gains (taxable) for this fiscal year.
    pub capital_gains: i128,
    /// Total return of capital (non-taxable) for this fiscal year.
    pub return_of_capital: i128,
}

// ── Timestamp helpers ────────────────────────────────────────────────────────
//
// These convert a Unix timestamp (seconds since epoch) into calendar year and
// month, then compute the fiscal year given the offering's configured fiscal
// start month.  The algorithms are adapted from common calendar routines and
// use no external date libraries, keeping the contract `#![no_std]`.

const SECS_PER_DAY: u64 = 86_400;

/// Returns `true` if `year` is a Gregorian leap year.
fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && !year.is_multiple_of(100) || year.is_multiple_of(400)
}

/// Days in each month for a given year (0‑indexed: January = 0).
const MONTH_DAYS_NON_LEAP: [u64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const MONTH_DAYS_LEAP: [u64; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// Convert a Unix timestamp (seconds since epoch) to a Gregorian calendar year.
pub fn timestamp_to_year(ts: u64) -> u32 {
    let days = ts / SECS_PER_DAY;
    let mut year = 1970u32;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    year
}

/// Convert a Unix timestamp (seconds since epoch) to a Gregorian calendar month
/// (1‑based: January = 1, February = 2, …).
pub fn timestamp_to_month(ts: u64) -> u32 {
    let days = ts / SECS_PER_DAY;
    let mut year = 1970u32;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let month_table = if is_leap_year(year) { MONTH_DAYS_LEAP } else { MONTH_DAYS_NON_LEAP };
    let mut month: u32 = 0;
    for &md in month_table.iter() {
        if remaining < md {
            break;
        }
        remaining -= md;
        month += 1;
    }
    // month is 0‑indexed; return 1‑based
    month + 1
}

/// Compute the fiscal year that contains `ts`, given the fiscal year start
/// month (1‑12) configured for the offering.
///
/// For example, if the fiscal year starts in April (`fiscal_start_month = 4`):
/// - Timestamps in Apr 2024 – Mar 2025 → fiscal year 2024.
/// - Timestamps in Apr 2023 – Mar 2024 → fiscal year 2023.
pub fn fiscal_year_from_ts(ts: u64, fiscal_start_month: u32) -> u64 {
    let year = timestamp_to_year(ts);
    let month = timestamp_to_month(ts);
    if month < fiscal_start_month {
        (year - 1) as u64
    } else {
        year as u64
    }
}

/// Default fiscal year start month (January = 1).
pub const DEFAULT_FISCAL_START_MONTH: u32 = 1;

pub fn track_cost_basis(env: &Env, offering_id: &OfferingId, holder: &Address, cost_basis: i128) {
    let key = DataKey3::RemainingBasis(offering_id.clone(), holder.clone());
    env.storage().persistent().set(&key, &cost_basis);
}

/// Update the tax-year accumulator for a holder's distribution.
///
/// Called from `rollover_distribution` (and from `claim`) to increment the
/// per-holder, per-fiscal-year `TaxYearSummary` entry in persistent storage.
pub fn update_tax_year_accumulator(
    env: &Env,
    offering_id: &OfferingId,
    holder: &Address,
    fiscal_year: u64,
    ordinary_income: i128,
    capital_gains: i128,
    return_of_capital: i128,
) {
    let year_key = DataKey3::TaxYearEntry(offering_id.clone(), holder.clone(), fiscal_year);
    let mut summary: TaxYearSummary = env
        .storage()
        .persistent()
        .get(&year_key)
        .unwrap_or(TaxYearSummary { ordinary_income: 0, capital_gains: 0, return_of_capital: 0 });
    summary.ordinary_income = summary.ordinary_income.saturating_add(ordinary_income);
    summary.capital_gains = summary.capital_gains.saturating_add(capital_gains);
    summary.return_of_capital = summary.return_of_capital.saturating_add(return_of_capital);
    env.storage().persistent().set(&year_key, &summary);
}

pub fn rollover_distribution(
    env: &Env,
    offering_id: &OfferingId,
    holder: &Address,
    amount: i128,
    period_id: u64,
    timestamp: u64,
) -> TaxBucketResult {
    let key = DataKey3::RemainingBasis(offering_id.clone(), holder.clone());
    let remaining_basis: i128 = env.storage().persistent().get(&key).unwrap_or(0);

    let (return_of_capital, capital_gains) = if remaining_basis >= amount {
        let new_basis = remaining_basis - amount;
        env.storage().persistent().set(&key, &new_basis);
        (amount, 0i128)
    } else {
        let roc = remaining_basis;
        let cg = amount - remaining_basis;

        env.events().publish(
            (
                EVENT_TAX_ROLLOVER,
                offering_id.issuer.clone(),
                offering_id.namespace.clone(),
                offering_id.token.clone(),
            ),
            (holder.clone(), remaining_basis, 0i128),
        );

        env.storage().persistent().set(&key, &0i128);
        (roc, cg)
    };

    // Emit tax_lot_v1 event for every tax-bucket update
    env.events().publish(
        (
            EVENT_TAX_LOT_V1,
            offering_id.issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
        ),
        (holder.clone(), return_of_capital, capital_gains, amount, period_id, timestamp),
    );

    TaxBucketResult { return_of_capital, capital_gains }
}

/// Taxation bucket for a revenue-share distribution.
///
/// Revenue-share distributions have different tax treatments:
/// - `Ordinary` — ordinary taxable income (dividends, interest, etc.)
/// - `Capital` — capital gains (profit from sale of securities)
/// - `ReturnOfCapital` — return of capital (non-taxable distribution)
/// - `Custom(Symbol)` — a jurisdiction-specific or custom bucket identifier.
///
/// The bucket is tagged on `report_revenue` so downstream indexers and tax
/// engines can categorize each disbursement without out-of-band annotation.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaxBucket {
    Ordinary,
    Capital,
    ReturnOfCapital,
    Custom(Symbol),
}

impl Default for TaxBucket {
    fn default() -> Self {
        TaxBucket::Ordinary
    }
}
