# Close-of-Period Preflight Simulation (#563)

This document describes the read-only `preflight_close_period` endpoint that
lets operators preview the outcome of `close_period` / `close_period_dual_sig`
before committing. The preflight is **side-effect-free**: it never writes
storage and never emits events.

---

## Purpose

Operators want to know two things before they seal a period:

1. Whether the atomic close would succeed (no `ContractFrozen`,
   `ContractPaused`, `InvalidPeriodId`, `OfferingNotFound`, or
   `PeriodAlreadyClosed`).
2. What the contract will record at close time — the canonical
   `class_pay_order` and the per-holder distribution that `claim()` will
   later compute for the same period.

`preflight_close_period(offering_id, period_id, holders)` answers both
questions without touching contract state.

---

## Scope deviation from the issue text

The issue text suggests a two-argument signature — `preflight_close_period(
offering_id, period_id)`. We accept an additional `holders: Vec<Address>`
argument. The deviation is forced by Soroban's storage model and the
codebase's idiom:

- Soroban persistent `Map` keys cannot be iterated, so there is no
  existing helper to enumerate every holder with a non-zero share for an
  offering.
- The closest analogue, `simulate_distribution`, already takes a
  caller-supplied `holder_shares: Vec<(Address, u32)>` argument.
- Pass the same holder set the eventual `claim()` flow will iterate.
  Indexers and operator dashboards typically already keep this set
  off-chain.

This deviation is documented in the PR description so reviewers can
re-evaluate it if a future storage redesign adds a per-offering holder
index.

---

## Signature

```rust
pub fn preflight_close_period(
    env: Env,
    offering_id: OfferingId,
    period_id: u64,
    holders: Vec<Address>,
) -> Result<PreflightCloseResult, RevoraError>
```

## Return Type

```rust
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PreflightCloseResult {
    pub period_id: u64,
    /// Revenue currently stored at `DataKey::PeriodRevenue(...)` — 0 if no
    /// `report_revenue` has landed.
    pub period_revenue: i128,
    /// Canonical class pay order that `record_and_emit_pay_order` would
    /// write for this period. Empty when no classes registered.
    pub class_pay_order: Vec<ShareClass>,
    /// Per-holder distribution preview.
    pub payouts: Vec<DistributionEntry>,
    /// Saturating sum of `payouts[i].normalized_payout`. Never exceeds
    /// `period_revenue` because each entry uses
    /// `compute_share(period_revenue, share_bps, mode)` with
    /// `share_bps <= 10_000`.
    pub total_distributed: i128,
}
```

---

## Preconditions (mirrors `close_period`)

| Order | Check                                | Error on failure          |
|-------|--------------------------------------|---------------------------|
| 1     | `require_not_frozen(&env)`           | `ContractFrozen`          |
| 2     | `require_not_paused(&env)`           | `ContractPaused`          |
| 3     | `period_id != 0`                     | `InvalidPeriodId`         |
| 4     | `DataKey2::OfferingRecord` exists    | `OfferingNotFound`        |
| 5     | `DataKey2::ClosedPeriod` absent      | `PeriodAlreadyClosed`     |

Notes:

- The preflight intentionally does **not** call `issuer.require_auth()`.
  It is a read-only view callable by anyone, matching `get_offering`,
  `simulate_distribution`, and `get_class_pay_order`.
- The preflight intentionally does **not** check `DualSigEnabled`. A
  preview must be available even when the atomic close would require
  dual signatures; auth enforcement belongs on the write path.
- The preflight does **not** check `holder.require_auth()` for each
  supplied address — those are caller-controlled inputs the contract
  trusts, just like `simulate_distribution` trusts its caller-supplied
  `(Address, u32)` tuples.

---

## Compute Parity Guarantee

The preflight is designed so that, *given the same storage snapshot* and
the same caller-supplied `holders`:

1. `class_pay_order` is **byte-identical** to what
   `close_period` will write via `record_and_emit_pay_order`. Both call
   the same private helper `resolve_class_pay_order(env, offering_id)`,
   which sorts registered classes by `(priority_index, xdr_bytes)`
   ascending.
2. `payouts[i].normalized_payout` matches `compute_share(revenue,
   share_bps, rounding_mode)` — the same math `simulate_distribution`
   uses for its no-classes path. There is no `normalize_amount` step
   here because neither `simulate_distribution` nor `claim()` for
   classless offerings applies one today; operators can verify parity by
   comparing the preflight `total_distributed` against an on-chain
   `claim()` after `close_period`.
3. Total payout is bounded: `total_distributed <= period_revenue` by
   construction.

### Known deviation vs. `DistributionEntry` docs

The `DistributionEntry` struct's doc comment (src/lib.rs ~L863-865) states
that `normalized_payout` equals `compute_share(normalize_amount(revenue, decimals),
share_bps, mode)`. The preflight currently uses
`compute_share(revenue, share_bps, mode)` instead, matching
`simulate_distribution` exactly. We chose this to keep the preflight
aligned with the closest existing distribution-simulation primitive, and
since neither `simulate_distribution` nor the no-classes claim flow
applies `normalize_amount` today.

If a future PR adopts `normalize_amount` on either path, the preflight
**MUST** be updated in lockstep. A regression test,
`preflight_total_matches_simulate_distribution`, would catch drift if
added — see Future Enhancements.

### Behaviour with non-default token decimals

The default token-decimal precision is `STELLAR_CANONICAL_DECIMALS = 7`.
When an offering has no explicit `set_payment_token_decimals` call, the
preflight's `period_revenue` and `payouts[i].normalized_payout` are
identical to what `close_period + claim()` would compute for the same
storage snapshot. For offerings with non-default token decimals, the
preflight produces raw-token amounts (no normalization); off-chain
indexers normalising display amounts should apply the same factor.

---

## Blacklist precedence

`is_blacklisted(offering_id, holder)` is consulted for every supplied
holder. A blacklisted holder is **silently dropped** from `payouts`,
matching the contract's global rule: blacklist always wins over
whitelist and over any non-zero share. Holders with `share_bps == 0`
are still emitted (with `normalized_payout = 0`) so callers can detect
them rather than silently dropping them.

---

## Idempotency

The preflight performs only reads. Calling it any number of times for the
same `(offering_id, period_id, ...)` tuple returns identical results and
never advances any state machine. The `preflight_does_not_write_period_closed_storage`
test enforces this.

---

## Test Coverage

See `src/test_close_period.rs` for the dedicated preflight test suite
(prefixed `preflight_`). Required by #563:

| Test                                              | Edge Case                       |
|---------------------------------------------------|----------------------------------|
| `preflight_empty_period_returns_zero_payouts`     | Empty period (revenue = 0)       |
| `preflight_single_holder_full_share_matches_actual_close` | Single holder, parity with `claim()` |
| `preflight_multi_holder_split_sums_to_revenue`    | Multi-holder + truncation math   |
| `preflight_zero_holder_input_returns_empty_payouts` | No holders in input            |
| `preflight_handles_no_classes_registered_with_empty_pay_order` | Classless offering |
| `preflight_returns_class_pay_order_matching_close` | Parity with persisted pay order |
| `preflight_rejects_frozen_contract`               | ContractFrozen                   |
| `preflight_rejects_zero_period_id`                | InvalidPeriodId                  |
| `preflight_rejects_unknown_offering`              | OfferingNotFound                 |
| `preflight_rejects_already_closed_period`         | PeriodAlreadyClosed              |
| `preflight_skips_blacklisted_holders`             | Blacklist precedence             |
| `preflight_does_not_write_period_closed_storage`  | Idempotent reads                 |
| `preflight_dual_sig_capable_offering_still_works` | Dual-sig offering, no auth      |

Run with:

```bash
cargo test --lib preflight_
```

---

## Security Assumptions

- **Caller-supplied `holders`**: The preflight treats caller-supplied
  holders as best-effort input. It does not validate each holder
  individually, enumerate them, or guard against address spoofing. Indexers
  building a preview must supply the **same** `holders` Vec they intend
  to claim through; an attacker submitting a tampered Vec cannot move
  funds but can produce a misleading preview.
- **No auth**: Anyone may invoke the preflight. The preview leaks
  `holder_share`, `period_revenue`, and `class_pay_order` snapshots, all
  of which are already publicly readable via `is_blacklisted`,
  `get_holder_share`, and `get_class_pay_order`. Net information
  disclosure is bounded by what the existing public getters already
  expose.
- **Read-only contract**: The preflight does not increment any counters,
  advance any cursor, or write any storage key. It cannot be used as a
  DoS amplifier because it charges the same CPU and storage read budget
  regardless of call frequency.

---

## Migration Notes

- This is a strictly additive entrypoint. Existing callers of
  `close_period` and `close_period_dual_sig` are unaffected.
- No storage layout change. The `PreflightCloseResult` struct is
  indexed via `#[contracttype]` adding a new export — it does not
  modify or rename any existing variant.
- No event symbol is added or changed.

---

## Future Enhancements

Possible follow-ups (out of scope for #563):

1. **Periodic payer-snapshot**: a read-only helper that exposes the same
   preview keyed by period-only, removing the holder enumeration burden
   entirely. Requires a per-offering holder index, which the contract
   intentionally does not maintain today.
2. **`compute_share` with `normalize_amount`**: when the contract
   adopts a token-decimal normalization step on the claim flow, the
   preflight must adopt the same normalization to keep parity.
3. **Snapshot-aware preflight**: a variant that previews the per-class
   amounts using each registered `ClassConfig::bps`.

---

## Related

- `src/lib.rs` — `RevoraRevenueShare::preflight_close_period` and
  `compute_period_close_preview`
- `src/test_close_period.rs` — preflight test suite
- `CLOSE_PERIOD_FEATURE.md` — base `close_period` documentation
- `docs/claim-idempotency-guarantees.md` — claim flow invariants that
  the preflight's `payouts` field is a preview of
