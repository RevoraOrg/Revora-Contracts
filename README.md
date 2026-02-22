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
| `report_revenue` | `issuer: Address`, `token: Address`, `amount: i128`, `period_id: u64` | `Result<(), RevoraError>` | issuer | Emit a revenue report; event includes current blacklist. Updates audit summary. Fails with `ConcentrationLimitExceeded` if holder concentration enforcement is on and reported concentration exceeds limit. |
| `get_offering_count` | `issuer: Address` | `u32` | — | Total offerings registered by issuer. |
| `get_offerings_page` | `issuer: Address`, `start: u32`, `limit: u32` | `(Vec<Offering>, Option<u32>)` | — | Paginated offerings. `limit` capped at 20. `next_cursor` is `Some(next_start)` or `None`. |
| `blacklist_add` | `caller: Address`, `token: Address`, `investor: Address` | — | caller | Add investor to blacklist for token. Idempotent. |
| `blacklist_remove` | `caller: Address`, `token: Address`, `investor: Address` | — | caller | Remove investor from blacklist. Idempotent. |
| `is_blacklisted` | `token: Address`, `investor: Address` | `bool` | — | Whether investor is blacklisted for token. |
| `get_blacklist` | `token: Address` | `Vec<Address>` | — | All blacklisted addresses for token. |
| `set_concentration_limit` | `issuer: Address`, `token: Address`, `max_bps: u32`, `enforce: bool` | `Result<(), RevoraError>` | issuer | Set per-offering max single-holder concentration (bps). 0 = disabled. If `enforce` is true, `report_revenue` fails when reported concentration > `max_bps`. Offering must exist. |
| `report_concentration` | `issuer: Address`, `token: Address`, `concentration_bps: u32` | `Result<(), RevoraError>` | issuer | Report current top-holder concentration (bps). Emits `conc_warn` if over configured limit. |
| `get_concentration_limit` | `issuer: Address`, `token: Address` | `Option<ConcentrationLimitConfig>` | — | Get concentration limit config for offering. |
| `get_current_concentration` | `issuer: Address`, `token: Address` | `Option<u32>` | — | Last reported concentration (bps) for offering. |
| `get_audit_summary` | `issuer: Address`, `token: Address` | `Option<AuditSummary>` | — | Per-offering audit summary (total_revenue, report_count). |
| `set_rounding_mode` | `issuer: Address`, `token: Address`, `mode: RoundingMode` | `Result<(), RevoraError>` | issuer | Set rounding mode for share calculations. Offering must exist. |
| `get_rounding_mode` | `issuer: Address`, `token: Address` | `RoundingMode` | — | Get rounding mode (default Truncation if not set). |
| `compute_share` | `amount: i128`, `revenue_share_bps: u32`, `mode: RoundingMode` | `i128` | — | Compute share of amount at given bps with given rounding. Bounds: 0 ≤ result ≤ amount. |

### Types

- **Offering:** `{ issuer: Address, token: Address, revenue_share_bps: u32 }`
- **ConcentrationLimitConfig:** `{ max_bps: u32, enforce: bool }` — per-offering concentration guardrail.
- **AuditSummary:** `{ total_revenue: i128, report_count: u64 }` — per-offering audit log summary.
- **RoundingMode:** `Truncation` (0) or `RoundHalfUp` (1) — used by `compute_share` and per-offering default.

### Error codes (RevoraError)

| Code | Name | Meaning |
|------|------|---------|
| 1 | `InvalidRevenueShareBps` | `revenue_share_bps` > 10000. |
| 2 | `LimitReached` | Reserved / offering not found (e.g. for set_concentration_limit, set_rounding_mode). |
| 3 | `ConcentrationLimitExceeded` | Holder concentration exceeds configured limit and enforcement is on; `report_revenue` rejected. |

Auth failures (e.g. wrong signer) are signaled by host/panic, not `RevoraError`. Use `try_register_offering`, `try_report_revenue`, and similar `try_*` client methods to receive contract errors as `Result`.

### Events

| Topic / name | Payload | When |
|--------------|---------|------|
| `offer_reg` | `(issuer), (token, revenue_share_bps)` | After `register_offering`. |
| `rev_rep` | `(issuer, token), (amount, period_id, blacklist_vec)` | After `report_revenue`. |
| `bl_add` | `(token, caller), investor` | After `blacklist_add`. |
| `bl_rem` | `(token, caller), investor` | After `blacklist_remove`. |
| `conc_warn` | `(issuer, token), (concentration_bps, limit_bps)` | When `report_concentration` is called and reported concentration exceeds configured limit (warning only; enforce blocks at `report_revenue`). |

### Call patterns and limits

- **Pagination:** Use `get_offerings_page(issuer, start, limit)` with `start = 0` then `start = next_cursor` until `next_cursor` is `None`. Max page size 20.
- **Off-chain:** Prefer small page sizes and bounded blacklist sizes for predictable gas. See storage/gas tests in `src/test.rs` for stress behavior.
- **Holder concentration:** Concentration is not computed on-chain (no token balance reads). Issuer or indexer calls `report_concentration(issuer, token, bps)` with the current top-holder share in bps; the contract stores it and enforces or warns based on `set_concentration_limit`. Use `try_report_revenue` when enforcement may be enabled.
- **Rounding:** Use `compute_share(amount, revenue_share_bps, mode)` for consistent distribution math. Per-offering default is `get_rounding_mode(issuer, token)` (Truncation if unset). Sum of shares must not exceed total; both modes keep result in [0, amount].

---

## Security review checklist (contracts)

This section enumerates key security assumptions, trust boundaries, and mitigations for the Revora contracts. It is kept in sync with the implementation; see `src/lib.rs` and `src/test.rs` for the code that enforces these behaviors.

### Assumptions and trust boundaries

- **Issuer authority:** Only the offering issuer can register offerings, report revenue, set concentration limits, set rounding mode, and report concentration for that offering. The contract does not implement a separate “platform admin” role; all offering-level actions are issuer-authorized.
- **Blacklist authority:** Any address that passes `require_auth` can add/remove blacklist entries for any token. The contract does not restrict blacklist edits to the issuer. Integrators must enforce policy off-chain or via a wrapper if only the issuer should manage the blacklist.
- **Concentration data:** Holder concentration is not derived on-chain. The contract trusts the value passed to `report_concentration`. Enforcing or warning is based on this reported value; manipulation of the reported value can bypass the guardrail.
- **Revenue reports:** The contract does not verify that reported revenue amounts are correct or consistent with any external source. It only records and aggregates them for the audit summary and emits events.

### Threat model and mitigations

| Risk | Mitigation |
|------|------------|
| **Auth misuse / wrong signer** | All state-changing entrypoints call `require_auth` on the appropriate address. Auth failures cause host panic; use `try_*` client methods to handle errors. Tests: `blacklist_add_requires_auth`, `blacklist_remove_requires_auth`. |
| **Incorrect math (overflow, rounding)** | Revenue share bps is capped at 10000. `compute_share` uses checked arithmetic where applicable and clamps output to [0, amount]. Rounding modes (Truncation, RoundHalfUp) are documented and tested. Tests: `compute_share_*`, `register_offering_rejects_bps_over_10000`. |
| **Concentration guardrail bypass** | Enforcement is applied in `report_revenue` using the last value set by `report_concentration`. If concentration is not reported or is reported low, enforcement cannot block. Design: guardrail is advisory or best-effort unless the issuer reliably reports concentration before each report. Tests: concentration_enforce_blocks_report_revenue_when_over_limit, concentration_near_threshold_boundary. |
| **Audit summary consistency** | Summary is updated atomically in `report_revenue` (total_revenue += amount, report_count += 1). No corrections or overrides are supported; each report is additive. Tests: audit_summary_aggregates_revenue_and_count, audit_summary_per_offering_isolation. |
| **Storage / gas exhaustion** | Large blacklists and many offerings increase read/write cost. Pagination (max 20 per page) and stress tests document behavior. No unbounded loops over user-controlled collections except the blacklist map (bounded by who is added). Tests: storage_stress_*, gas_characterization_*. |
| **Upgradeability** | The contract is not upgradeable in this codebase; deployment is a single WASM with no proxy pattern. Any upgrade would require a new deployment and migration of off-chain indexing. |

### Limitations of on-chain checks

- **Holder concentration:** Token balances are held in the token contract. This contract does not call the token contract to compute concentration; it only stores and compares a reported value. Full concentration checks require off-chain indexing of balances and optional submission via `report_concentration`.
- **Revenue authenticity:** There is no on-chain verification that reported revenue matches actual payments or external systems. Auditability is via events and the on-chain audit summary; integrity of the source data is an off-chain concern.

### Build and test

```bash
cargo build --release
cargo test
```

### Contributor guidelines (reduce merge conflicts)

- Use feature branches per change (e.g. `feature/structured-error-codes`, `feature/storage-limit-negative-tests`).
- Tests in `src/test.rs` are grouped by area (pagination, blacklist, structured errors, storage stress, gas characterization). Add new tests in the relevant section so parallel PRs touch different regions.
- Keep the contract interface summary above in sync when adding or changing entrypoints or events.
