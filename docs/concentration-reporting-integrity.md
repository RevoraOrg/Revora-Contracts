# Concentration Reporting Integrity

This document outlines the architecture, assumptions, and validation rules for the
Concentration Reporting Integrity feature in the Revora smart contracts.

## Overview

The concentration reporting feature provides a verifiable and tamper-resistant mechanism
for issuers to define, report, and optionally enforce maximum holder concentration limits
(in basis points) on their offerings.

## Key Integrity Guarantees

### 1. Verification and Audit Trail

Every successful call to `report_concentration` unconditionally emits an
`EVENT_CONCENTRATION_REPORTED` (`"conc_rep"`) event. This creates an immutable, on-chain
timeline of concentration metrics that indexers and off-chain clients can use to
independently verify historical concentration levels.

### 2. Bounds Validation

All concentration-related values are strictly enforced against a maximum logical bound of
`10,000` basis points (100%).

- `set_concentration_limit`: The `max_bps` parameter must be `<= 10,000`.
- `report_concentration`: The `concentration_bps` parameter must be `<= 10,000`.

Any value exceeding this limit results in a `RevoraError::InvalidShareBps`.

### 3. Emergency Controls (Security & Pausing)

Both `set_concentration_limit` and `report_concentration` incorporate the global pause
check (`require_not_paused()`). This ensures that in the event of a security emergency,
administrators can freeze all state-mutating actions related to offering concentration,
preventing malicious issuers from altering definitions or bypassing automated enforcement.

### 4. Authorization Boundaries

Only the current authenticated issuer of an offering can set limits or report
concentrations. This is enforced via `issuer.require_auth()` and validated against the
`OfferingIssuer` storage key.

## Milestone Gate Integration (#289)

The concentration enforcement path is a critical input for `hardenedMilestoneValidation`
consumers. The following invariants are guaranteed by the contract and verified by the
`milestone_signals` test module:

| Invariant | Contract behaviour | Test |
|---|---|---|
| Enforcement blocks `report_revenue` | `ConcentrationLimitExceeded` returned; audit summary unchanged | `milestone_concentration_enforcement_blocks_revenue_report` |
| At-limit allows `report_revenue` | Succeeds when `concentration_bps == max_bps` | `milestone_concentration_at_limit_allows_revenue_report` |
| Warning-only does not block | `conc_wrn` emitted; `report_revenue` proceeds | `milestone_concentration_warning_does_not_block_report` |
| Warning event emitted | `conc_wrn` present in event log | `milestone_concentration_warning_event_emitted` |

### Off-chain integration pattern

```
1. Indexer computes top-holder concentration from token balances.
2. Issuer calls report_concentration(issuer, namespace, token, concentration_bps).
3. Contract stores value; emits conc_rep (and conc_wrn if over limit).
4. Issuer calls report_revenue(...).
   - If enforce=true and stored concentration > max_bps → rejected.
   - Backend milestone gate reads the audit summary and event log to confirm
     the revenue report was accepted before advancing the milestone.
```

## Failure Modes and Handling

1. **Unregistered Offering / Unauthorized Caller:** `set_concentration_limit` and
   `report_concentration` will fail (`LimitReached` / `OfferingNotFound`) if the token is
   not recognized as an offering or if the caller does not match the securely stored
   `current_issuer`.
2. **Invalid Metrics:** Exceeding `10,000` bps safely panics the transaction via
   `RevoraError::InvalidShareBps` before state modification.
3. **Globally Paused State:** Any attempt to manipulate concentration limits while the
   contract is globally paused will abort the transaction.
4. **Limit Breaches:** If enforcement is active (`enforce = true`), reporting revenue will
   fail automatically if the stored concentration exceeds `max_bps`. An explicitly captured
   warning event (`"conc_warn"`) is emitted immediately when a limit is exceeded via
   `report_concentration`.

## Security Risk Note

Concentration values are **issuer-reported** — the contract trusts the value passed to
`report_concentration` without on-chain verification against token balances. The
enforcement guardrail is therefore best-effort: a malicious or negligent issuer who
reports a low concentration value can bypass it. Backend milestone gates should treat
concentration enforcement as a defence-in-depth layer, not a sole control.
