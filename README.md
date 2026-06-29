# Revora-Contracts

Soroban contract for revenue-share offerings and blacklist management.

## 🚨 CRITICAL SECURITY WARNING

**TESTNET MODE DANGER:** This contract includes a "testnet mode" that relaxes critical validations for development/testing. **NEVER enable testnet mode on production/mainnet deployments.** 

- **Check before use:** Always call `is_testnet_mode()` and verify it returns `false` for production contracts.
- **Admin responsibility:** Only enable testnet mode during development/testing phases.
- **Relaxed validations:** When enabled, allows `revenue_share_bps > 10000` and bypasses concentration enforcement, potentially leading to fund loss.
- **Audit requirement:** Production deployments must be verified to have testnet mode disabled.

**If `is_testnet_mode()` returns `true` on a production contract, DO NOT USE IT - funds may be at risk.**

## Contract interface summary (for integrators)

*- **Issuer authority:** Only the offering issuer can register offerings, report revenue, set concentration limits, set rounding mode, and report concentration for that offering. The contract does not implement a separate "platform admin" role; all offering-level actions are issuer-authorized.
- **Issuer transferability:** Issuer control can be securely transferred via a two-step propose/accept flow. The old issuer proposes, the new issuer accepts. Either party can abort before acceptance (old issuer cancels, or new issuer simply doesn't accept). This prevents accidental loss of control and griefing attacks.
- **Blacklist authority:** Only the current issuer of the offering can add/remove blacklist entries for that offering's token. This ensures issuers have full control over compliance and investor management.

### Public methods

| Method | Parameters | Returns | Auth | Description |
|--------|------------|---------|------|-------------|
| `register_offering` | `issuer: Address`, `token: Address`, `revenue_share_bps: u32` | `Result<(), RevoraError>` | issuer | Register a revenue-share offering. Fails with `InvalidRevenueShareBps` if `revenue_share_bps > 10000`. |
| `get_offering` | `issuer: Address`, `token: Address` | `Option<Offering>` | — | Fetch one offering by issuer and token. |
| `get_payment_token` | `issuer: Address`, `namespace: Symbol`, `token: Address` | `Option<Address>` | — | Return the payment token locked by the first successful deposit. Returns `None` if the offering is unknown or if the offering exists but has not yet received a successful deposit. |
| `list_offerings` | `issuer: Address` | `Vec<Address>` | — | List offering tokens for issuer (first page only, up to 20). |
| `report_revenue` | `issuer: Address`, `token: Address`, `amount: i128`, `period_id: u64` | `Result<(), RevoraError>` | issuer | Emit or correct a revenue report. New periods update `AuditSummary`; existing periods may be corrected with `override_existing=true`, which emits explicit override events and applies the net delta to `total_revenue` without incrementing `report_count`. |
| `get_offering_count` | `issuer: Address` | `u32` | — | Total offerings registered by issuer. |
| `get_offerings_page` | `issuer: Address`, `start: u32`, `limit: u32` | `(Vec<Offering>, Option<u32>)` | — | Paginated offerings. `limit` capped at 20. `next_cursor` is `Some(next_start)` or `None`. |
| `blacklist_add` | `caller: Address`, `token: Address`, `investor: Address` | — | issuer | Add investor to blacklist for token. Only the current issuer can perform this action. Idempotent. |
| `blacklist_remove` | `caller: Address`, `token: Address`, `investor: Address` | — | issuer | Remove investor from blacklist. Only the current issuer can perform this action. Idempotent. |
| `is_blacklisted` | `token: Address`, `investor: Address` | `bool` | — | Whether investor is blacklisted for token. |
| `get_blacklist` | `token: Address` | `Vec<Address>` | — | All blacklisted addresses for token. |
| `set_concentration_limit` | `issuer: Address`, `token: Address`, `max_bps: u32`, `enforce: bool`, `max_staleness_secs: u64` | `Result<(), RevoraError>` | issuer | Set per-offering max single-holder concentration (bps). 0 = disabled. If `enforce` is true, `report_revenue` fails when reported concentration > `max_bps`. When `max_staleness_secs > 0` and `enforce` is true, `report_revenue` also fails if no concentration has been reported or the last report is older than `max_staleness_secs` seconds. Offering must exist. |
| `report_concentration` | `issuer: Address`, `token: Address`, `concentration_bps: u32` | `Result<(), RevoraError>` | issuer | Report current top-holder concentration (bps). Emits `conc_warn` if over configured limit. |
| `get_concentration_limit` | `issuer: Address`, `token: Address` | `Option<ConcentrationLimitConfig>` | — | Get concentration limit config for offering. |
| `get_current_concentration` | `issuer: Address`, `token: Address` | `Option<u32>` | — | Last reported concentration (bps) for offering. |
| `get_audit_summary` | `issuer: Address`, `token: Address` | `Option<AuditSummary>` | — | Per-offering audit summary cache (`total_revenue`, `report_count`) derived from persisted revenue reports. |
| `reconcile_audit_summary` | `issuer: Address`, `token: Address` | `AuditReconciliationResult` | — | Recompute the audit summary from persisted reports and compare it to the stored cache. Read-only. |
| `set_rounding_mode` | `issuer: Address`, `token: Address`, `mode: RoundingMode` | `Result<(), RevoraError>` | issuer | Set rounding mode for share calculations. Offering must exist. |
| `get_rounding_mode` | `issuer: Address`, `token: Address` | `RoundingMode` | — | Get rounding mode (default Truncation if not set). |
| `set_min_revenue_threshold` | `issuer: Address`, `token: Address`, `min_amount: i128` | `Result<(), RevoraError>` | issuer | Per-offering minimum revenue for new periods. When a new `report_revenue` call is below the threshold, the contract emits `rev_below` and skips report/audit state updates. Stored periods can still be corrected explicitly with `override_existing=true`. |
| `get_min_revenue_threshold` | `issuer: Address`, `token: Address` | `i128` | — | Minimum revenue threshold for offering (0 = none). |
| `compute_share` | `amount: i128`, `revenue_share_bps: u32`, `mode: RoundingMode` | `i128` | — | Compute share of amount at given bps with given rounding. Bounds: 0 ≤ result ≤ amount. |
| `prove_distribution_for_period` | `issuer: Address`, `namespace: Symbol`, `token: Address`, `period_id: u64`, `holders: Vec<Address>` | `(Vec<DistributionEntry>, BytesN<32>)` | — | Return a deterministic per-holder distribution proof for a single period. See [Distribution Proofs](#distribution-proofs) below. |
| `propose_issuer_transfer` | `token: Address`, `new_issuer: Address` | `Result<(), RevoraError>` | current issuer | Propose transferring issuer control to a new address. First step of two-step transfer. |
| `accept_issuer_transfer` | `token: Address` | `Result<(), RevoraError>` | proposed new issuer | Accept a pending issuer transfer. Completes the transfer and grants full control to new issuer. |
| `cancel_issuer_transfer` | `token: Address` | `Result<(), RevoraError>` | current issuer | Cancel a pending issuer transfer before it's accepted. |
| `get_pending_issuer_transfer` | `token: Address` | `Option<Address>` | — | Get the proposed new issuer for a pending transfer, if any. |
| `set_testnet_mode` | `enabled: bool` | `Result<(), RevoraError>` | admin | Enable or disable testnet mode. When enabled, certain validations are relaxed for testnet deployments. |
| `is_testnet_mode` | — | `bool` | — | Return true if testnet mode is enabled. |
| `get_version` | — | `u32` | — | Return the current contract version (#23). Used for upgrade compatibility. |

### Types

- **Offering:** `{ issuer: Address, token: Address, revenue_share_bps: u32 }`
- **ConcentrationLimitConfig:** `{ max_bps: u32, enforce: bool, max_staleness_secs: u64 }` — per-offering concentration guardrail. When `enforce` is true and `max_staleness_secs > 0`, `report_revenue` rejects with `StaleConcentrationData` if no concentration has been reported or the last report is older than `max_staleness_secs` seconds.
- **AuditSummary:** `{ total_revenue: i128, report_count: u64 }` — per-offering audit log summary.
- **RoundingMode:** `Truncation` (0) or `RoundHalfUp` (1) — used by `compute_share` and per-offering default.

### Error codes (RevoraError)

| Code | Name | Meaning |
|------|------|---------|
| 1 | `InvalidRevenueShareBps` | `revenue_share_bps` > 10000. |
| 2 | `LimitReached` | Reserved / offering not found (e.g. for set_concentration_limit, set_rounding_mode). Also returned when `set_report_window` / `set_claim_window` is called with `start > end`. |
| 3 | `ConcentrationLimitExceeded` | Holder concentration exceeds configured limit and enforcement is on; `report_revenue` rejected. |
| 11 | `ClaimDelayNotElapsed` | Revenue for this period is not yet claimable; the per-offering delay has not elapsed since deposit. |
| 12 | `IssuerTransferPending` | A transfer is already pending for this offering. |
| 13 | `NoTransferPending` | No transfer is pending for this offering (accept/cancel failed). |
| 14 | `UnauthorizedTransferAccept` | Caller is not authorized to accept this transfer. |
| 17 | `InvalidAmount` | Amount is invalid (e.g. negative, or zero for deposit) (#35). |
| 18 | `InvalidPeriodId` | period_id is 0 where a positive value is required (#35). |
| 25 | `ReportingWindowClosed` | Current ledger timestamp is outside the configured reporting window; `report_revenue` rejected. |
| 26 | `ClaimWindowClosed` | Current ledger timestamp is outside the configured claiming window; `claim` rejected. |
| 49 | `StaleConcentrationData` | `report_concentration` has never been called, or the last call is older than `max_staleness_secs`; `report_revenue` rejected when enforcement is on. |

Auth failures (e.g. wrong signer) are signaled by host/panic, not `RevoraError`. Use `try_register_offering`, `try_report_revenue`, and similar `try_*` client methods to receive contract errors as `Result`.

### Events

| Topic / name | Payload | When |
|--------------|---------|------|
| `offer_reg` | `(issuer), (token, revenue_share_bps)` | After `register_offering`. |
| `rev_init` | `(issuer, token), (amount, period_id, blacklist_vec)` | First persisted report for a period. |
| `rev_ovrd` | `(issuer, token), (new_amount, period_id, old_amount, blacklist_vec)` | Accepted correction of an existing persisted period (`override_existing=true`). |
| `rev_rej` | `(issuer, token), (attempted_amount, period_id, existing_amount, blacklist_vec)` | Duplicate report attempt for an existing period when `override_existing=false`; no state change. |
| `rev_omiss` | (`issuer, token`), (`attempted_amount, period_id`) | Rejected override attempt when `override_existing=true` is used for a period with no prior report. |
| `rev_rep` | `(issuer, token), (amount, period_id, blacklist_vec)` | Receipt for an accepted persisted report call (initial or override). Use `rev_init` plus `rev_ovrd` to reconstruct audit totals. |
| `bl_add` | `(token, caller), investor` | After `blacklist_add`. |
| `bl_rem` | `(token, caller), investor` | After `blacklist_remove`. |
| `min_rev` | `(issuer, token), (previous_amount, new_amount)` | When `set_min_revenue_threshold` is set or changed. |
| `rev_below` | `(issuer, token), (amount, period_id, threshold)` | When a new `report_revenue` call is below the offering's minimum threshold; no report/audit update and the period remains available for a later accepted report. |
| `conc_warn` | `(issuer, token), (concentration_bps, limit_bps)` | When `report_concentration` is called and reported concentration exceeds configured limit (warning only; enforce blocks at `report_revenue`). |
| `rep_win` | `(issuer, namespace, token), (start_timestamp, end_timestamp)` | When `set_report_window` is called. |
| `clm_win` | `(issuer, namespace, token), (start_timestamp, end_timestamp)` | When `set_claim_window` is called. |
| `iss_prop` | `(token), (current_issuer, proposed_new_issuer)` | When `propose_issuer_transfer` is called. |
| `iss_acc` | `(token), (old_issuer, new_issuer)` | When `accept_issuer_transfer` completes the transfer. |
| `iss_canc` | `(token), (current_issuer, proposed_new_issuer)` | When `cancel_issuer_transfer` revokes a pending transfer. |
| `test_mode` | `(admin), enabled` | When `set_testnet_mode` is called to toggle testnet mode. |

### Call patterns and limits

- **Pagination:** Use `get_offerings_page(issuer, start, limit)` with `start = 0` then `start = next_cursor` until `next_cursor` is `None`. Max page size 20. Ordering: by registration index (creation order), deterministic.
 - **Chunked read-only queries:** For long numeric ranges or unbounded per-holder lists, prefer the chunked helpers to avoid long-running loops:
     - `get_revenue_range_chunk(env, issuer, namespace, token, from_period, to_period, max_periods)` — sums up to `max_periods` numeric period ids in [from_period, to_period], returns `(sum, next_start)` to continue.
     - `get_pending_periods_page(env, issuer, namespace, token, holder, start, limit)` — returns a page of pending period IDs and a `next_cursor` if more remain.
     - `get_claimable_chunk(env, issuer, namespace, token, holder, start_idx, count)` — computes claimable amount over a bounded index window and returns a `next_cursor` when further eligible periods exist.
     These helpers enforce reasonable caps (`MAX_PAGE_LIMIT`, `MAX_CHUNK_PERIODS`) so off-chain orchestrators should iterate using the returned cursors until exhaustion.
- **Ordering:** `get_offerings_page` returns offerings by registration index. `get_blacklist` returns addresses in insertion order. `get_pending_periods` returns period IDs by deposit index. All query results are deterministic.
- **Minimum revenue threshold:** Issuers can set `set_min_revenue_threshold(issuer, token, min_amount)`. When a new `report_revenue` call is made with `amount < min_amount`, the contract emits `rev_below` and does not update revenue reports, `AuditSummary`, or the report-period cursor. Set to 0 to disable. Thresholds do not block explicit corrections of already persisted periods.
- **Off-chain:** Prefer small page sizes and bounded blacklist sizes for predictable gas. See storage/gas tests in `src/test.rs` for stress behavior.
- **Holder concentration:** Concentration is not computed on-chain (no token balance reads). Issuer or indexer calls `report_concentration(issuer, token, bps)` with the current top-holder share in bps; the contract stores it and enforces or warns based on `set_concentration_limit`. Use `try_report_revenue` when enforcement may be enabled.
- **Rounding:** Use `compute_share(amount, revenue_share_bps, mode)` for consistent distribution math. Per-offering default is `get_rounding_mode(issuer, token)` (Truncation if unset). Sum of shares must not exceed total; both modes keep result in [0, amount].
- **Distribution proofs:** Use `prove_distribution_for_period(issuer, namespace, token, period_id, holders)` to obtain a contract-computed, verifiable per-holder payout vector and a SHA-256 digest. Off-chain indexers can call this endpoint and compare the returned digest against their own reconstruction to detect drift from contract math. See [Distribution Proofs](#distribution-proofs) below.
- **Issuer Transfer:** See [ISSUER_TRANSFER.md](./ISSUER_TRANSFER.md) for comprehensive documentation on securely transferring issuer control via the two-step propose/accept flow.
- **Payment token locking:** Once an offering's payout asset is set at registration, all deposits must use that same token. See [docs/payment-token-locking.md](./docs/payment-token-locking.md) for invariants and test coverage.
- **Payment token decimals:** Different Stellar assets use different decimal precisions (e.g., USDC=6, XLM=7, WBTC=8). Use `set_payment_token_decimals` to configure the offering's asset precision; the contract normalizes raw amounts to 7-decimal canonical units before computing holder shares. See [docs/payment-token-decimal-compatibility.md](./docs/payment-token-decimal-compatibility.md) for details and examples.
- **Testnet mode:** Admin can enable testnet mode via `set_testnet_mode(true)` to relax certain validations for non-production deployments. When enabled: (1) `register_offering` allows `revenue_share_bps > 10000`, (2) `report_revenue` skips concentration enforcement. Use only for testnet/development environments. Check mode with `is_testnet_mode()`.
- **Reporting and claiming windows:** Issuers can optionally restrict when `report_revenue` and `claim` are permitted using time-based access windows. See [Time Windows](#time-based-access-windows-reporting--claiming) below.

### Distribution Proofs

`prove_distribution_for_period(issuer, namespace, token, period_id, holders)` is a **read-only** endpoint that lets off-chain indexers verify their payout reconstruction against contract truth.

#### What it returns

`(Vec<DistributionEntry>, BytesN<32>)` — a per-holder vector and a SHA-256 digest.

Each `DistributionEntry` contains:
- `holder: Address` — the holder's address (same order as the input `holders` slice)
- `share_bps: u32` — the holder's on-chain share in basis points
- `normalized_payout: i128` — `compute_share(normalize_amount(period_revenue, decimals), share_bps, rounding_mode)`

#### Digest construction

```
digest = SHA-256(
    XDR(issuer) || XDR(namespace) || XDR(token) || XDR(period_id) || XDR(entries)
)
```

The digest covers the full output vector in input order. Off-chain indexers reproduce it by:
1. Calling `prove_distribution_for_period` with the same ordered `holders` slice.
2. Computing the same SHA-256 over the XDR-serialised fields.
3. Comparing — any mismatch indicates drift from contract math.

#### Deterministic ordering

The contract preserves the **caller-supplied order** of `holders` exactly. There is no on-chain sorting. Off-chain tools must use a stable, agreed-upon ordering (e.g. lexicographic by address bytes) and pass the same order on every call to get a reproducible digest.

#### Edge cases

| Condition | Behaviour |
|-----------|-----------|
| Unknown `period_id` (no deposit) | `normalized_payout = 0` for all holders; digest still valid |
| `share_bps == 0` for a holder | `normalized_payout = 0` |
| Empty `holders` vec | Returns empty `entries`; digest is SHA-256 of the header-only payload |
| `holders.len() > 200` | Silently capped at `MAX_CHUNK_PERIODS` (200); paginate off-chain |
| Decimals ≠ 7 | `normalize_amount` scales the raw revenue before `compute_share` |

#### Security

- **Read-only**: no storage writes, no auth required.
- **Tamper-evident**: the digest covers contract-computed values only. It cannot be forged without changing on-chain `HolderShare` or `PeriodRevenue` state.
- **No double-counting risk**: the function does not transfer tokens or advance any index.

### Time-Based Access Windows (Reporting & Claiming)

Issuers can configure per-offering time windows that gate `report_revenue` and `claim`.
If no window is set, the operation is always permitted.

#### Soroban Time Source

All window checks use `env.ledger().timestamp()` — the Unix timestamp (seconds since
epoch) of the current ledger's close time. This value is set by Stellar network
consensus and is monotonically non-decreasing. It is **not** manipulable per-transaction.

#### Window Methods

| Method | Auth | Description |
|--------|------|-------------|
| `set_report_window(issuer, namespace, token, start_timestamp, end_timestamp)` | issuer | Configure when `report_revenue` is permitted. If unset, always open. |
| `set_claim_window(issuer, namespace, token, start_timestamp, end_timestamp)` | issuer | Configure when `claim` is permitted. If unset, always open. |
| `get_report_window(issuer, namespace, token)` | — | Read current report window (`None` if unset). |
| `get_claim_window(issuer, namespace, token)` | — | Read current claim window (`None` if unset). |

#### Boundary Semantics

Windows are **inclusive on both ends**: a transaction whose ledger closes at exactly
`start_timestamp` or `end_timestamp` is permitted.

```
is_open = now >= start_timestamp && now <= end_timestamp
```

| `now` vs `[start, end]` | Result |
|------------------------|--------|
| `now < start` | Closed (`ReportingWindowClosed` / `ClaimWindowClosed`) |
| `now == start` | **Open** (inclusive) |
| `start < now < end` | Open |
| `now == end` | **Open** (inclusive) |
| `now > end` | Closed |

#### Zero-Width Windows

Setting `start_timestamp == end_timestamp` is valid and creates a single-second
eligibility slot. This is intentional but operationally fragile in production — prefer
windows with meaningful duration (≥ 3600 seconds).

#### Which Operations Are Gated

| Operation | Report Window | Claim Window |
|-----------|:---:|:---:|
| `report_revenue` | ✅ gated | — |
| `deposit_revenue` | — | — (never gated) |
| `claim` | — | ✅ gated |

`deposit_revenue` is **never** time-window gated. Issuers can always deposit revenue
regardless of any configured window.

#### Reconfiguration Mid-Flight

Windows can be changed at any time via `set_report_window` / `set_claim_window`. The
contract applies the window active at the **ledger that closes the transaction** — not
at submission time. Use sufficiently wide windows to reduce reconfiguration races.

#### Claim Delay vs Claim Window

The per-offering `ClaimDelaySecs` (set via `set_claim_delay`) and the claim window are
independent. The claim window is checked first; if open, the per-period delay is then
checked inside the claim loop. Both must pass for a period to be claimable.

For the full boundary matrix, zero-width window notes, and security/risk analysis see
[docs/time-window-boundary-matrix.md](./docs/time-window-boundary-matrix.md).

### Claim Delay and Index Monotonicity

The contract enforces strict security invariants for multi-period claims to prevent double-payment, period skipping, and ensure blacklist/whitelist decisiveness.

#### Claim Delay Semantics

Issuers can configure a per-offering claim delay via `set_claim_delay(issuer, namespace, token, delay_secs)`. This delay is enforced **per-period** based on the deposit timestamp:

- A period becomes claimable only after `deposit_time + delay_secs` has elapsed
- The delay check occurs **inside the claim iteration loop** for each period
- If a period's delay has not elapsed, the claim loop stops immediately
- Periods that haven't elapsed their delay are **not** claimed, even if later periods in the batch have elapsed their delay
- `ClaimDelayNotElapsed` is returned if **no periods** in the batch have elapsed their delay

Example: With delay=100s, periods deposited at T=1000, T=1050, T=1100:
- At T=1150: Only periods 1 and 2 are claimable (1000+100≤1150, 1050+100≤1150)
- At T=1200: All three periods are claimable (1100+100≤1200)

#### Index Monotonicity and Period Ordering

The contract maintains a strict invariant that `LastClaimedIdx` advances only in ways that match the deposited `PeriodEntry` order:

- Periods are stored in `PeriodEntry(offering_id, sequential_index) -> period_id`
- The `claim` function validates that period IDs are strictly increasing as retrieved from `PeriodEntry`
- This prevents any possibility of skipping periods or claiming out of order
- If the period order is violated (defensive check), the claim fails with `NoPendingClaims`

#### Blacklist/Whitelist Decisiveness During Partial Sequences

The blacklist check is performed **inside the period iteration loop**, not just at the start:

- If a holder becomes blacklisted mid-sequence during a multi-period claim, the loop breaks immediately
- No subsequent periods in the batch are claimed after the blacklist takes effect
- `LastClaimedIdx` is only advanced for periods successfully processed **before** the blacklist took effect
- This ensures blacklist/whitelist decisions remain decisive even during partial claim sequences

Example: A holder claims periods 1-2, then gets blacklisted. A subsequent claim attempt for periods 3-5 will fail with `HolderBlacklisted` and no periods will be claimed.

#### Security Invariants

The `claim` function provides the following hard guarantees:

1. **No double-pay**: `LastClaimedIdx` is written to storage only after the token transfer succeeds. If the transfer fails, the index is not advanced.
2. **Index advances only on processed periods**: The index reflects only periods that passed the delay check and blacklist check.
3. **Zero-payout periods advance the index**: A period with zero revenue still advances `LastClaimedIdx` to prevent permanently stuck indices.
4. **Exhausted state returns `NoPendingClaims`**: Once all periods are claimed, subsequent calls return this error without touching storage.
5. **Per-holder isolation**: Each holder's `LastClaimedIdx` is keyed by `(offering_id, holder)`.
6. **Auth checked first**: `holder.require_auth()` is the first operation.
7. **Blacklist/whitelist decisiveness during partial sequences**: The blacklist check is performed inside the loop, ensuring mid-sequence blacklists stop claiming immediately.
8. **Index monotonicity enforced**: Period IDs must be strictly increasing as retrieved from `PeriodEntry`, preventing out-of-order claims.

#### Test Coverage

Comprehensive tests verify these invariants:
- `claim_before_delay_returns_claim_delay_not_elapsed`: Pre-delay claim fails
- `claim_after_delay_succeeds`: Post-delay claim succeeds
- `claim_delay_partial_periods_only_claimable_after_delay`: Partial claim respects delay per period
- `claim_blacklist_mid_sequence_stops_claiming`: Blacklist enforced during partial sequences
- `claim_multi_index_respects_delay_per_period`: Multi-index claims respect delay per period
- `claim_index_monotonicity_enforced`: Index monotonicity and period ordering
- `claim_v2_event_payload_verification`: V2 event payloads include correct period information
- `claim_partial_sequence_with_delay_advances_index_correctly`: Partial sequences advance index correctly


- **Version:** Call `get_version()` to read the current contract version (a constant, e.g., `23`). This value is bumped when storage layout or semantics change in a way that affects compatibility.
- **Upgrade strategy:** This codebase deploys a single WASM contract; Soroban has no EVM-style proxy upgrade, so upgrades require deploying a new contract instance. Future upgrades follow this process:
  1. Deploy a new contract (new WASM) with a higher `CONTRACT_VERSION`.
  2. Optionally run a one-time migration (e.g., admin or migration script) that reads state from the old contract and writes into the new one, or that emits migration-milestone events for indexers.
  3. **Re-point consumers:** Update all frontend, backend, and indexer configurations to use the new contract address. Indexers and custodial backends must:
     - Update their contract address references.
     - Check `get_version()` on the new contract to confirm the upgrade.
     - Update event parsing and API handling logic if the new version introduces changes to event schemas or method signatures.
     - Treat the first successful transaction on the new contract as the migration cutover point.
  4. The old contract remains deployed but should be considered inactive; consumers should not interact with it post-migration.
- **Migration milestones:** When a new version is deployed, integrators can treat the first transaction that succeeds on the new contract as a migration milestone; the contract does not currently emit a dedicated "migration" event, but event schemas may include a version field (e.g., v1 events) for consumers.

### Input parameter validation (#35)

Accepted ranges and rejection semantics:

| Parameter | Entrypoint(s) | Accepted range | Error if invalid |
|-----------|----------------|----------------|------------------|
| `supply_cap` | `register_offering` | >= 0 | `InvalidAmount` |
| `revenue_share_bps` | `register_offering` | 0–10000 (testnet: any) | `InvalidRevenueShareBps` |
| `share_bps` | `set_holder_share` | 0–10000 | `InvalidShareBps` |
| `amount` | `report_revenue` | ≥ 0 | `InvalidAmount` |
| `amount` | `deposit_revenue` | > 0 | `InvalidAmount` |
| `amount` | `deposit_revenue_with_snapshot` | > 0 | `InvalidAmount` |
| `snapshot_reference` | `deposit_revenue_with_snapshot` | > 0 | `InvalidAmount` |
| `period_id` | `deposit_revenue` | `> 0`, `last + 1`, and non-duplicate | `InvalidPeriodId` (0/gap), `PeriodAlreadyDeposited` (dup) |
| `min_stake` | `set_investment_constraints` | >= 0 | `InvalidAmount` |
| `max_stake` | `set_investment_constraints` | >= 0 and `>= min_stake` when set | `InvalidAmount` |
| `period_id` | `report_revenue` | `> 0` and `last + 1` (for new reports) | `InvalidPeriodId` |
| `min_amount` | `set_min_revenue_threshold` | ≥ 0 | `InvalidAmount` |

- `snapshot content_hash`: SHA-256 digest over the on-chain snapshot rows in ascending slot order. Each row is encoded in XDR as the tuple `(slot_index: u32, holder: Address, share_bps: u32)`.

Use `try_*` client methods to receive these errors as `Result`.
Consolidated invalid-amount regression coverage lives in `src/invalid_amount_matrix_tests.rs`; the checklist is in `docs/negative-amount-validation-matrix.md`.
This branch's public fee-related amount helper is `calculate_fee_for_asset`; it is a pure quote helper and is documented separately from the `InvalidAmount` rejection matrix.

---

## Local Development & Quality Gates

These are the **exact** commands CI runs. Run them locally before every push.

```bash
# 1. Format check — must produce no diff
cargo fmt --all -- --check

# 2. Clippy — every warning is a hard error
cargo clippy --all-targets --all-features -- -D warnings

# 3. Build
cargo build --release

# 4. Tests — single-threaded for deterministic Soroban output
cargo test -- --test-threads=1
```

All four checks must pass before a PR can be merged. The CI pipeline runs them as
three sequential jobs (`fmt → clippy → test`) so failures are fast and readable.

For the full rationale behind each lint gate, suppression policy, and security
assumptions see [docs/clippy-format-gate-hardening.md](./docs/clippy-format-gate-hardening.md).

---

## Architecture Deep Dive

This section provides detailed explanations of the on-chain data model, core flows, and integration patterns for developers building on or integrating with Revora-Contracts.

### Contract Purpose and Design Philosophy

**Revora-Contracts** is a Soroban smart contract designed to facilitate **revenue-sharing offerings** on the Stellar blockchain. It enables issuers to:

1. Register revenue-share offerings tied to specific tokens
2. Deposit revenue for token holders across multiple periods
3. Allow holders to claim their accumulated revenue shares
4. Maintain compliance through blacklist management
5. Monitor holder concentration for regulatory guardrails
6. Maintain transparent audit trails of all revenue activities

**Key Design Principles:**

- **Off-chain computation, on-chain verification**: The contract doesn't compute token balances or distributions; it stores issuer-provided data and enforces rules.
- **Gas efficiency**: All operations are bounded (max 20 items per page, max 50 periods per claim) to ensure predictable costs.
- **Immutable offerings**: Once registered, offering parameters (issuer, token, revenue_share_bps) cannot be changed. New configurations require new offerings.
- **Progressive disclosure**: Holders claim revenue progressively as periods are deposited; no need to claim all at once.
- **Auditability first**: Every state change emits events; audit summaries provide aggregated views of revenue flow.

---

### On-Chain Data Model

The contract uses **persistent storage** exclusively (no temporary or instance storage) with the following key structures:

#### Storage Keys (`DataKey` enum)

```rust
pub enum DataKey {
    // ── Offering Management ──
    OfferCount(Address),              // Per-issuer: total offerings registered
    OfferItem(Address, u32),          // Per-issuer: offering at index N
    
    // ── Blacklist Management ──
    Blacklist(Address),               // Per-token: map of blacklisted addresses
    
    // ── Concentration Monitoring ──
    ConcentrationLimit(Address, Address),   // Per-offering: {max_bps, enforce}
    CurrentConcentration(Address, Address), // Per-offering: last reported bps
    
    // ── Audit & Rounding ──
    AuditSummary(Address, Address),   // Per-offering: {total_revenue, report_count}
    RoundingMode(Address, Address),   // Per-offering: Truncation | RoundHalfUp
    
    // ── Multi-Period Claims ──
    PeriodRevenue(Address, u64),      // Per (offering_token, period_id): revenue amount
    PeriodEntry(Address, u32),        // Per (offering_token, index): period_id mapping
    PeriodCount(Address),             // Per offering_token: total periods deposited
    HolderShare(Address, Address),    // Per (offering_token, holder): share_bps
    LastClaimedIdx(Address, Address), // Per (offering_token, holder): next index to claim
    PaymentToken(Address),            // Per offering_token: locked payment token address
    ClaimDelaySecs(Address),          // Per offering_token: delay in seconds (#27)
    PeriodDepositTime(Address, u64),  // Per (offering_token, period_id): deposit timestamp
    
    // ── Admin & Freeze ──
    Admin,                            // Global: admin address
    Frozen,                           // Global: contract freeze flag
}
```

#### Core Data Structures

**Offering:**
```rust
pub struct Offering {
    pub issuer: Address,           // Address authorized to manage this offering
    pub token: Address,            // Token representing this offering
    pub revenue_share_bps: u32,    // Revenue share in basis points (0-10000)
}
```
*Stored in:* `DataKey::OfferItem(issuer, index)`

**ConcentrationLimitConfig:**
```rust
pub struct ConcentrationLimitConfig {
    pub max_bps: u32,    // Maximum single-holder concentration (0 = disabled)
    pub enforce: bool,   // If true, report_revenue fails when exceeded
}
```
*Stored in:* `DataKey::ConcentrationLimit(issuer, token)`

**AuditSummary:**
```rust
pub struct AuditSummary {
    pub total_revenue: i128,   // Cumulative revenue reported (not deposited)
    pub report_count: u64,     // Total number of report_revenue calls
}
```
*Stored in:* `DataKey::AuditSummary(issuer, token)`

**RoundingMode:**
```rust
pub enum RoundingMode {
    Truncation = 0,     // floor(amount * bps / 10000)
    RoundHalfUp = 1,    // round((amount * bps) / 10000)
}
```
*Stored in:* `DataKey::RoundingMode(issuer, token)` *(defaults to Truncation)*

#### Storage Relationships

```
Issuer (Address)
  ├─ OfferCount: u32
  └─ OfferItem[0..N]: Offering
       ├─ token: Address
       ├─ revenue_share_bps: u32
       └─ (issuer, token) composite key used for:
            ├─ ConcentrationLimit
            ├─ CurrentConcentration
            ├─ AuditSummary
            └─ RoundingMode

Offering Token (Address)
  ├─ Blacklist: Map<Address, ()>
  ├─ PaymentToken: Address (locked on first deposit)
  ├─ ClaimDelaySecs: u64
  ├─ PeriodCount: u32
  └─ PeriodEntry[0..N]: period_id
       └─ PeriodRevenue(token, period_id): i128
       └─ PeriodDepositTime(token, period_id): u64

(Offering Token, Holder) tuple
  ├─ HolderShare: u32 (basis points)
  └─ LastClaimedIdx: u32 (next period index to claim)
```

---

### Core Flows & Sequences

#### 1. Offering Registration Flow

**Purpose:** Register a new revenue-share offering on-chain.

**Sequence:**
```
1. Issuer calls: register_offering(issuer, token, revenue_share_bps)
   ├─ Auth: issuer.require_auth() ✓
   ├─ Validate: revenue_share_bps ≤ 10000
   └─ State changes:
        ├─ Read: OfferCount(issuer) → count
        ├─ Write: OfferItem(issuer, count) = Offering {issuer, token, revenue_share_bps}
        ├─ Write: OfferCount(issuer) = count + 1
        └─ Event: offer_reg(issuer, (token, revenue_share_bps))

2. Result: Offering is now queryable via get_offering(issuer, token)
```

**Storage Impact:**
- **Persistent writes:** 2 (OfferItem + OfferCount)
- **Gas cost:** Low (< 2KB write)

**Error conditions:**
- `InvalidRevenueShareBps`: revenue_share_bps > 10000
- `ContractFrozen`: Contract is frozen
- Auth panic: Wrong signer

**Integration notes:**
- Offerings are **immutable** after registration
- **Duplicate prevention:** Registration is idempotent; re-registering the same `(issuer, namespace, token)` is a no-op that returns `Ok(())`, preserving the original registration's parameters.
- Off-chain systems should track registration events to build offering directories

---

#### 2. Revenue Deposit Flow (Multi-Period Claims)

**Purpose:** Deposit actual revenue for a specific period, enabling holder claims.

**Sequence:**
```
1. Issuer calls: deposit_revenue(issuer, token, payment_token, amount, period_id)
   ├─ Auth: issuer.require_auth() ✓
   ├─ Validate:
   │    ├─ Offering exists (get_offering)
   │    ├─ Period not already deposited (PeriodRevenue not set)
   │    └─ Payment token matches the stored lock (if a previous successful deposit exists)
   ├─ Token transfer: payment_token.transfer(issuer → contract, amount)
   └─ State changes:
        ├─ Write: PeriodRevenue(token, period_id) = amount
        ├─ Write: PeriodDepositTime(token, period_id) = now
        ├─ Read: PeriodCount(token) → count
        ├─ Write: PeriodEntry(token, count) = period_id
        ├─ Write: PeriodCount(token) = count + 1
        ├─ Write (once): PaymentToken(token) = payment_token (after the first successful deposit)
        └─ Event: rev_dep(issuer, token, (payment_token, amount, period_id))

2. Result: Holders can now claim this period via claim()
```

**Storage Impact:**
- **Persistent writes:** 4-5 (PeriodRevenue + PeriodDepositTime + PeriodEntry + PeriodCount + maybe PaymentToken)
- **Token transfer:** 1 (payment_token: issuer → contract)

**Error conditions:**
- `OfferingNotFound`: No offering exists for (issuer, token)
- `PeriodAlreadyDeposited`: Period already has revenue deposited
- `PaymentTokenMismatch`: Different payment token than the token locked by the first successful deposit
- `ContractFrozen`: Contract is frozen

**Integration notes:**
- **Payment token is locked only after a successful first deposit**; failed deposits do not set `PaymentToken`
- **Duplicate period IDs fail as `PeriodAlreadyDeposited`** before any sequencing state is updated
- **Period IDs are arbitrary** (u64); issuers can use timestamps, sequential numbers, or any scheme
- **Period order matters**: Claims are processed in deposit order (via PeriodEntry index), not period_id order

---

#### 3. Revenue Reporting Flow (Event-Based Audit)

**Purpose:** Emit an audit event for off-chain tracking; doesn't transfer funds.

**Sequence:**
```
1. Issuer calls: report_revenue(issuer, token, amount, period_id)
   ├─ Auth: issuer.require_auth() ✓
   ├─ Concentration check:
   │    ├─ Read: ConcentrationLimit(issuer, token)
   │    ├─ Read: CurrentConcentration(issuer, token)
   │    └─ If enforce && current > max_bps → Err(ConcentrationLimitExceeded)
   ├─ Read: Blacklist(token) → blacklist_vec
   ├─ If existing period and !override_existing:
   │    └─ Emit: rev_rej(...), no state change
   ├─ If existing period and override_existing:
   │    ├─ Emit: rev_ovrd(...)
   │    └─ Update: summary.total_revenue += (new_amount - old_amount)
   └─ If new period:
        ├─ If amount < min_threshold → emit rev_below(...), no state change
        ├─ Emit: rev_init(...) then rev_rep(...)
        ├─ Update: summary.total_revenue += amount
        ├─ Update: summary.report_count += 1
        └─ Write: AuditSummary(issuer, token) = summary

2. Result: Off-chain indexers see revenue report event with current blacklist snapshot
```

**Storage Impact:**
- **Persistent writes:** 1 (AuditSummary update)
- **Event payload:** ~100 bytes + blacklist size

**Error conditions:**
- `ConcentrationLimitExceeded`: Current concentration > limit and enforcement enabled
- `ContractFrozen`: Contract is frozen

**Key difference from deposit_revenue:**
- **No token transfer**: This is audit-only
- **Includes blacklist snapshot**: Event payload contains current blacklisted addresses
- **Updates audit summary**: Tracks cumulative reported revenue (may differ from deposited)

---

#### 4. Holder Claims Flow

**Purpose:** Holders claim accumulated revenue across unclaimed periods.

**Sequence:**
```
1. Holder calls: claim(holder, token, max_periods)
   ├─ Auth: holder.require_auth() ✓
   ├─ Validate:
   │    ├─ Not blacklisted: !is_blacklisted(token, holder)
   │    ├─ Has share: HolderShare(token, holder) > 0
   │    └─ Has unclaimed periods: LastClaimedIdx < PeriodCount
   ├─ Iterate periods [LastClaimedIdx .. min(LastClaimedIdx + max_periods, PeriodCount)]:
   │    ├─ Read: PeriodEntry(token, i) → period_id
   │    ├─ Check delay: PeriodDepositTime(token, period_id) + ClaimDelaySecs ≤ now
   │    │    └─ If not elapsed: break loop
   │    ├─ Read: PeriodRevenue(token, period_id) → revenue
   │    ├─ Compute: payout = revenue * share_bps / 10000
   │    └─ Accumulate: total_payout += payout
   ├─ Token transfer: payment_token.transfer(contract → holder, total_payout)
   ├─ Write: LastClaimedIdx(token, holder) = new_idx (advanced by claimed periods)
   └─ Event: claim(holder, token, (total_payout, claimed_periods_vec))

2. Result: Holder receives aggregated payout; claim index advances
```

**Storage Impact:**
- **Persistent reads:** 2N + 5 (N = periods claimed, typically ≤ 50)
- **Persistent writes:** 1 (LastClaimedIdx update)
- **Token transfer:** 1 (payment_token: contract → holder)

**Max periods per transaction:**
- **MAX_CLAIM_PERIODS = 50**: Gas safety limit
- Holders with > 50 unclaimed periods must call claim() multiple times

**Error conditions:**
- `HolderBlacklisted`: Holder is on offering's blacklist
- `NoPendingClaims`: No share set or all periods claimed
- `ClaimDelayNotElapsed`: Next claimable period hasn't passed delay threshold

**Integration notes:**
- **Zero-value periods advance index**: Even if payout is 0, LastClaimedIdx increments
- **Claim delay enforced per-period**: If delay not elapsed, loop breaks early
- **Idempotent**: Calling claim() with no new periods simply returns 0

---

#### 5. Blacklist Management Flow

**Purpose:** Manage per-token investor blacklists for compliance.

**Add to Blacklist:**
```
1. Caller calls: blacklist_add(caller, token, investor)
   ├─ Auth: caller.require_auth() ✓
   ├─ State changes:
   │    ├─ Read: Blacklist(token) → map
   │    ├─ Insert: map[investor] = ()
   │    └─ Write: Blacklist(token) = map
   └─ Event: bl_add((token, caller), investor)

2. Result: investor cannot claim revenue for this token
```

**Remove from Blacklist:**
```
1. Caller calls: blacklist_remove(caller, token, investor)
   ├─ Auth: caller.require_auth() ✓
   ├─ State changes:
   │    ├─ Read: Blacklist(token) → map
   │    ├─ Remove: map.remove(investor)
   │    └─ Write: Blacklist(token) = map
   └─ Event: bl_rem((token, caller), investor)

2. Result: investor can claim revenue again
```

**Storage Impact:**
- **Persistent writes:** 1 per operation (Blacklist map update)
- **Idempotent**: Adding an already-blacklisted address is safe (no error)

**Security notes:**
- **No issuer restriction**: Any address can manage blacklists (see Security section)
- **Affects claims only**: Blacklisted holders retain their share_bps, but cannot call claim()
- **Snapshot in report_revenue**: Current blacklist is included in rev_rep event payload

---

#### 6. Concentration Monitoring Flow

**Purpose:** Track and enforce single-holder concentration limits for regulatory compliance.

**Set Concentration Limit:**
```
1. Issuer calls: set_concentration_limit(issuer, token, max_bps, enforce)
   ├─ Auth: issuer.require_auth() ✓
   ├─ Validate: Offering exists
   ├─ State changes:
   │    └─ Write: ConcentrationLimit(issuer, token) = {max_bps, enforce}
   └─ No event (configuration change)

2. Result: Enforcement rules updated for this offering
```

**Report Current Concentration:**
```
1. Issuer/Indexer calls: report_concentration(issuer, token, concentration_bps)
   ├─ Auth: issuer.require_auth() ✓
   ├─ State changes:
   │    └─ Write: CurrentConcentration(issuer, token) = concentration_bps
   ├─ Check limit:
   │    ├─ Read: ConcentrationLimit(issuer, token)
   │    └─ If concentration_bps > max_bps → Event: conc_warn((issuer, token), (concentration_bps, limit_bps))
   └─ No error (warning only)

2. Result: Current concentration stored; warning event if exceeded
```

**Enforcement at report_revenue:**
```
When issuer calls report_revenue():
   ├─ Read: ConcentrationLimit(issuer, token)
   ├─ Read: CurrentConcentration(issuer, token)
   └─ If enforce && current > max_bps:
        └─ Err(ConcentrationLimitExceeded) → Transaction reverts
```

**Integration pattern:**
```
Off-chain indexer:
1. Monitor token holder balances
2. Compute: top_holder_balance / total_supply * 10000 = concentration_bps
3. Call: report_concentration(issuer, token, concentration_bps)
4. Contract stores value for next report_revenue() call
```

**Security notes:**
- **Trust model**: Contract trusts reported concentration values (no on-chain verification)
- **Warning vs. enforcement**: `conc_warn` event is informational; `enforce=true` blocks revenue reports
- **No automatic updates**: Concentration must be reported manually before each revenue report

---

### Integration Patterns

#### Pattern 1: Off-Chain Indexer for Revenue Distribution

**Problem:** Contract doesn't compute holder shares; issuers need to know who gets paid and how much.

**Solution:** Build an off-chain indexer that:

1. **Monitors offering registrations:**
   ```
   Listen for: offer_reg events
   Store: (issuer, token, revenue_share_bps) mappings
   ```

2. **Tracks token holder balances:**
   ```
   Query: Token contract balance changes
   Compute: holder_balance / total_supply = holder_share_pct
   ```

3. **Calculates revenue shares:**
   ```
   For each holder:
     share_bps = floor(holder_share_pct * 10000)
     Call: set_holder_share(issuer, token, holder, share_bps)
   ```

4. **Deposits revenue:**
   ```
   For each revenue period:
     Compute: total_revenue_for_holders = total_revenue * revenue_share_bps / 10000
     Call: deposit_revenue(issuer, token, payment_token, amount, period_id)
   ```

5. **Monitors concentration:**
   ```
   Compute: top_holder_bps = max(holder_share_pct) * 10000
   Call: report_concentration(issuer, token, top_holder_bps)
   ```

**Example pseudo-code:**
```rust
// Off-chain worker (runs periodically)
async fn distribute_revenue(issuer: Address, token: Address, period_id: u64) {
    // 1. Query token holders from Stellar network
    let holders = query_token_holders(&token).await;
    let total_supply = query_total_supply(&token).await;
    
    // 2. Set holder shares on-chain
    for holder in holders {
        let balance = holder.balance;
        let share_bps = (balance * 10_000) / total_supply;
        contract.set_holder_share(issuer, token, holder.address, share_bps).await;
    }
    
    // 3. Report concentration
    let max_holder = holders.iter().max_by_key(|h| h.balance).unwrap();
    let concentration_bps = (max_holder.balance * 10_000) / total_supply;
    contract.report_concentration(issuer, token, concentration_bps).await;
    
    // 4. Deposit revenue
    let total_revenue = compute_period_revenue(period_id);
    contract.deposit_revenue(issuer, token, payment_token, total_revenue, period_id).await;
    
    // 5. Emit audit event
    contract.report_revenue(issuer, token, total_revenue, period_id).await;
}
```

---

#### Pattern 2: Event Monitoring for Audit Trails

**Problem:** Need real-time visibility into contract activity for compliance and analytics.

**Solution:** Subscribe to contract events and build audit database.

**Event stream processing:**
```rust
match event.topic {
    "offer_reg" => {
        let (issuer, (token, revenue_share_bps)) = event.payload;
        db.insert_offering(issuer, token, revenue_share_bps, event.ledger);
    },
    "rev_dep" => {
        let (issuer, token, (payment_token, amount, period_id)) = event.payload;
        db.insert_deposit(token, period_id, amount, payment_token, event.ledger);
    },
    "rev_rep" => {
        let ((issuer, token), (amount, period_id, blacklist)) = event.payload;
        db.insert_report(issuer, token, amount, period_id, blacklist, event.ledger);
    },
    "claim" => {
        let (holder, token, (payout, periods)) = event.payload;
        db.insert_claim(holder, token, payout, periods, event.ledger);
    },
    "bl_add" | "bl_rem" => {
        let ((token, caller), investor) = event.payload;
        db.update_blacklist(token, investor, event.topic == "bl_add", event.ledger);
    },
    "conc_warn" => {
        let ((issuer, token), (concentration_bps, limit_bps)) = event.payload;
        db.insert_concentration_warning(issuer, token, concentration_bps, limit_bps, event.ledger);
    },
}
```

**Query patterns:**
- Offering history: `SELECT * FROM offerings WHERE issuer = ?`
- Holder claims: `SELECT * FROM claims WHERE holder = ? AND token = ?`
- Revenue timeline: `SELECT * FROM deposits WHERE token = ? ORDER BY period_id`
- Compliance violations: `SELECT * FROM concentration_warnings WHERE concentration_bps > limit_bps`

---

#### Pattern 3: Batched Claims for Large Holder Bases

**Problem:** Gas costs for individual holder claims can be high; want to optimize for large distributions.

**Solution:** Off-chain aggregation with periodic claim notifications.

**Approach:**
```
1. Indexer monitors deposit_revenue events
2. For each new deposit:
   a. Query all holders with share_bps > 0
   b. Compute each holder's payout: revenue * share_bps / 10000
   c. Store in off-chain DB: (holder, token, estimated_payout, period_id)
   d. Send notification: "You have $X available to claim"
   
3. Holders claim at their convenience:
   - High-value holders: claim frequently (every period)
   - Low-value holders: claim in batches (every N periods)
   - Gas optimization: max_periods parameter controls batch size
   
4. Unclaimed revenue stays in contract (no forced distribution)
```

**Claim optimization:**
```rust
// Holder decides when to claim based on gas vs. revenue
let estimated_gas_cost = estimate_claim_gas(num_unclaimed_periods);
let estimated_payout = query_unclaimed_payout(holder, token);

if estimated_payout > estimated_gas_cost * MIN_PROFIT_RATIO {
    contract.claim(holder, token, num_unclaimed_periods).await;
} else {
    // Wait for more periods to accumulate
    log("Skipping claim; gas cost too high for current payout");
}
```

---

#### Pattern 4: Rounding Mode Selection

**Problem:** Different jurisdictions/contracts may require different rounding for fairness.

**Solution:** Configure per-offering rounding mode based on legal requirements.

**Rounding modes:**
```rust
// Truncation (default): Always rounds down
// Benefit: Conservative; prevents over-distribution
// Drawback: Small holders lose fractional amounts
compute_share(100, 3333, Truncation)  // = 33  (33.33 truncated)

// RoundHalfUp: Standard rounding (>= 0.5 rounds up)
// Benefit: More accurate; fairer to small holders
// Drawback: May over-distribute if not careful with total
compute_share(100, 3333, RoundHalfUp)  // = 33  (33.33 rounds to 33)
compute_share(100, 6667, RoundHalfUp)  // = 67  (66.67 rounds to 67)
```

**Selection guidance:**
```
Use Truncation when:
- Conservative accounting required
- Preventing over-distribution is critical
- Small fractional losses are acceptable

Use RoundHalfUp when:
- Fairness to small holders is priority
- Total distribution carefully controlled off-chain
- Regulatory requirement for "fair rounding"
```

**Integration:**
```rust
// Set once per offering during setup
contract.set_rounding_mode(issuer, token, RoundingMode::RoundHalfUp).await;

// Verify before distributions
let mode = contract.get_rounding_mode(issuer, token).await;
assert_eq!(mode, RoundingMode::RoundHalfUp);

// Use consistently off-chain
for holder in holders {
    let share = compute_share(revenue, holder.share_bps, mode);
    estimated_distributions.push((holder.address, share));
}
```

---

### Advanced Topics

#### Pagination Strategies for Large Datasets

**Problem:** Issuers with hundreds of offerings need efficient querying.

**Contract pagination API:**
```rust
pub fn get_offerings_page(
    env: Env,
    issuer: Address,
    start: u32,      // Starting index
    limit: u32,      // Max items (capped at 20)
) -> (Vec<Offering>, Option<u32>)  // (results, next_cursor)
```

**Pagination pattern:**
```rust
let mut all_offerings = Vec::new();
let mut cursor = Some(0);

while let Some(start) = cursor {
    let (page, next) = contract.get_offerings_page(issuer, start, 20).await;
    all_offerings.extend(page);
    cursor = next;  // None when no more pages
}
```

**Performance notes:**
- Each page costs ~O(20) storage reads
- For 100 offerings: 5 RPC calls (100 / 20)
- Alternative: Cache offerings off-chain after monitoring `offer_reg` events

---

#### Claim Delay Mechanics

**Purpose:** Time-lock revenue claims for dispute windows or regulatory hold periods.

**Configuration:**
```rust
// Set delay once per offering
contract.set_claim_delay(issuer, token, 86400).await;  // 24-hour delay
```

**Behavior:**
```
Deposit at t=0:  deposit_revenue(..., period_id=1)
Delay window:    [t=0 ... t=86400]
Claimable at:    t=86401+

If holder calls claim() at t=43200 (12 hours):
  → Err(ClaimDelayNotElapsed)  // Too early

If holder calls claim() at t=90000:
  → Success, payout transferred
```

**Use cases:**
- **Dispute windows**: Allow time to challenge revenue calculations
- **Regulatory holds**: Comply with holding period requirements
- **Batch optimization**: Encourage holders to claim less frequently

---

#### Gas Optimization Tips

**For issuers:**
1. **Batch holder share updates**: Set shares for multiple holders in quick succession to amortize RPC overhead
2. **Minimize blacklist size**: Each blacklist entry adds storage cost and increases `rev_rep` event payload
3. **Use sequential period IDs**: Simplifies off-chain tracking (e.g., Unix timestamps)

**For holders:**
1. **Claim in batches**: Waiting for N periods (max 50) reduces transactions by N×
2. **Monitor gas prices**: Claim during low-fee periods on Stellar network
3. **Check unclaimed balance**: Query `LastClaimedIdx` vs `PeriodCount` before claiming

**For integrators:**
1. **Cache read-only data**: `get_offering`, `get_concentration_limit`, etc. change rarely
2. **Use event streams**: More efficient than polling `get_offerings_page` repeatedly
3. **Parallel RPCs**: Query multiple offerings simultaneously (Stellar supports concurrent reads)

---

#### Audit Summary Usage

**Purpose:** On-chain aggregated view of revenue reporting activity.

**Structure:**
```rust
pub struct AuditSummary {
    pub total_revenue: i128,    // Sum of persisted reports after override deltas
    pub report_count: u64,      // Number of persisted periods
}
```

**Key insights:**
```rust
let summary = contract.get_audit_summary(issuer, token).await;

// Average revenue per report
let avg_revenue = summary.total_revenue / (summary.report_count as i128);

// Compare reported vs. deposited
let total_deposited = query_period_revenues(token).sum();
let discrepancy = summary.total_revenue - total_deposited;
// Note: These may differ! report_revenue is informational; deposit_revenue is actual.
```

**Audit patterns:**
```
1. Consistency check:
   For each period_id in rev_init / rev_ovrd events:
     Verify the latest reported amount matches your expected deposited or accounted value
     Alert if an override changes a period without a corresponding off-chain explanation

2. Completeness check:
   Start from all rev_init amounts, then apply each rev_ovrd delta `(new - old)`
   Compare the reconstructed total to `get_audit_summary` / `reconcile_audit_summary`
   Investigate significant discrepancies

3. Compliance reporting:
   Generate quarterly reports using audit_summary data
   Cross-reference with off-chain payment records
```

---

### Code Examples

#### Example 1: Complete Offering Lifecycle (Pseudo-Code)

```rust
use soroban_sdk::{Address, Env};

// ── Step 1: Register Offering ──
async fn register_new_offering(
    env: &Env,
    issuer: &Address,
    token: &Address,
) -> Result<()> {
    let revenue_share_bps = 2500;  // 25% to holders
    
    contract.register_offering(
        issuer.clone(),
        token.clone(),
        revenue_share_bps,
    ).await?;
    
    println!("Offering registered: {}", token);
    Ok(())
}

// ── Step 2: Set Holder Shares (Off-Chain Indexer) ──
async fn update_holder_shares(
    env: &Env,
    issuer: &Address,
    token: &Address,
) -> Result<()> {
    // Query token balances from Stellar
    let holders = stellar.query_token_holders(token).await?;
    let total_supply = stellar.query_total_supply(token).await?;
    
    for holder in holders {
        let share_bps = (holder.balance * 10_000) / total_supply;
        
        contract.set_holder_share(
            issuer.clone(),
            token.clone(),
            holder.address.clone(),
            share_bps as u32,
        ).await?;
        
        println!("Set share for {}: {} bps", holder.address, share_bps);
    }
    
    Ok(())
}

// ── Step 3: Deposit Revenue ──
async fn deposit_quarterly_revenue(
    env: &Env,
    issuer: &Address,
    token: &Address,
    quarter: u64,
) -> Result<()> {
    let payment_token = usdc_token_address();
    let revenue_amount = 1_000_000_000;  // 1,000 USDC (7 decimals)
    let period_id = quarter;  // e.g., 20241 for Q1 2024
    
    // First, approve contract to spend tokens
    payment_token_client.approve(
        issuer,
        contract_address,
        revenue_amount,
        expiration_ledger,
    ).await?;
    
    // Then deposit
    contract.deposit_revenue(
        issuer.clone(),
        token.clone(),
        payment_token.clone(),
        revenue_amount,
        period_id,
    ).await?;
    
    println!("Deposited {} for period {}", revenue_amount, period_id);
    Ok(())
}

// ── Step 4: Report Revenue (Audit Event) ──
async fn report_quarterly_revenue(
    env: &Env,
    issuer: &Address,
    token: &Address,
    quarter: u64,
) -> Result<()> {
    let total_revenue = 4_000_000_000;  // Total revenue (not just holder share)
    let period_id = quarter;
    
    contract.report_revenue(
        issuer.clone(),
        token.clone(),
        total_revenue,
        period_id,
    ).await?;
    
    println!("Reported {} for audit", total_revenue);
    Ok(())
}

// ── Step 5: Holder Claims ──
async fn holder_claim_revenue(
    env: &Env,
    holder: &Address,
    token: &Address,
) -> Result<i128> {
    let max_periods = 10;  // Claim up to 10 periods at once
    
    let payout = contract.claim(
        holder.clone(),
        token.clone(),
        max_periods,
    ).await?;
    
    println!("Holder {} claimed {}", holder, payout);
    Ok(payout)
}
```

---

#### Example 2: Event Handling for Monitoring

```rust
use stellar_sdk::{EventFilter, EventType};

async fn monitor_contract_events(contract_id: &str) -> Result<()> {
    let filter = EventFilter::new()
        .contract(contract_id)
        .event_types(vec![EventType::Contract]);
    
    let mut stream = stellar.subscribe_events(filter).await?;
    
    while let Some(event) = stream.next().await {
        match event.topic.as_str() {
            "offer_reg" => {
                let issuer = event.data[0].as_address()?;
                let token = event.data[1].as_address()?;
                let revenue_share_bps = event.data[2].as_u32()?;
                
                database.insert_offering(OfferingRecord {
                    issuer,
                    token,
                    revenue_share_bps,
                    registered_at: event.ledger_timestamp,
                }).await?;
                
                println!("New offering: {} by {}", token, issuer);
            },
            
            "rev_dep" => {
                let issuer = event.data[0].as_address()?;
                let token = event.data[1].as_address()?;
                let payment_token = event.data[2].as_address()?;
                let amount = event.data[3].as_i128()?;
                let period_id = event.data[4].as_u64()?;
                
                database.insert_deposit(DepositRecord {
                    issuer,
                    token,
                    payment_token,
                    amount,
                    period_id,
                    deposited_at: event.ledger_timestamp,
                }).await?;
                
                // Notify holders
                let holders = database.get_holders(token).await?;
                for holder in holders {
                    let payout = compute_share(amount, holder.share_bps, RoundingMode::Truncation);
                    notification_service.notify_holder(holder.address, payout).await?;
                }
            },
            
            "claim" => {
                let holder = event.data[0].as_address()?;
                let token = event.data[1].as_address()?;
                let payout = event.data[2].as_i128()?;
                let periods = event.data[3].as_vec()?;
                
                database.insert_claim(ClaimRecord {
                    holder,
                    token,
                    payout,
                    periods_claimed: periods.len(),
                    claimed_at: event.ledger_timestamp,
                }).await?;
                
                println!("Claim: {} received {} for {} periods", holder, payout, periods.len());
            },
            
            "conc_warn" => {
                let issuer = event.data[0].as_address()?;
                let token = event.data[1].as_address()?;
                let concentration_bps = event.data[2].as_u32()?;
                let limit_bps = event.data[3].as_u32()?;
                
                alert_service.send_concentration_alert(
                    issuer,
                    token,
                    concentration_bps,
                    limit_bps,
                ).await?;
                
                println!("⚠️  Concentration warning: {} bps (limit: {} bps)", 
                         concentration_bps, limit_bps);
            },
            
            _ => {
                println!("Unknown event: {}", event.topic);
            }
        }
    }
    
    Ok(())
}
```

---

#### Example 3: Error Handling Patterns

```rust
use revora_contracts::{RevoraError, RevoraRevenueShareClient};

async fn safe_deposit_with_retry(
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
    payment_token: &Address,
    amount: i128,
    period_id: u64,
) -> Result<()> {
    const MAX_RETRIES: u32 = 3;
    let mut attempt = 0;
    
    loop {
        match client.try_deposit_revenue(
            issuer,
            token,
            payment_token,
            amount,
            period_id,
        ).await {
            Ok(_) => {
                println!("✓ Revenue deposited successfully");
                return Ok(());
            },
            
            Err(RevoraError::OfferingNotFound) => {
                eprintln!("✗ Offering not found; cannot deposit");
                return Err("Offering must be registered first".into());
            },
            
            Err(RevoraError::PeriodAlreadyDeposited) => {
                println!("⚠ Period already deposited; skipping");
                return Ok(());  // Idempotent behavior
            },
            
            Err(RevoraError::PaymentTokenMismatch) => {
                eprintln!("✗ Payment token mismatch; locked to different token");
                return Err("Cannot change payment token after first deposit".into());
            },
            
            Err(RevoraError::ContractFrozen) => {
                eprintln!("✗ Contract is frozen; waiting for admin action");
                return Err("Contract operations suspended".into());
            },
            
            Err(e) => {
                attempt += 1;
                if attempt >= MAX_RETRIES {
                    eprintln!("✗ Max retries exceeded: {:?}", e);
                    return Err(format!("Failed after {} attempts", MAX_RETRIES).into());
                }
                
                eprintln!("⚠ Retrying deposit (attempt {}/{}): {:?}", attempt, MAX_RETRIES, e);
                tokio::time::sleep(Duration::from_secs(2_u64.pow(attempt))).await;
            }
        }
    }
}

async fn safe_claim_with_validation(
    client: &RevoraRevenueShareClient,
    holder: &Address,
    token: &Address,
) -> Result<i128> {
    // Pre-flight checks
    if client.is_blacklisted(token, holder).await? {
        return Err("Holder is blacklisted; cannot claim".into());
    }
    
    let share_bps = client.get_holder_share(token, holder).await?;
    if share_bps == 0 {
        return Err("No share allocated; nothing to claim".into());
    }
    
    // Attempt claim
    match client.try_claim(holder, token, 50).await {
        Ok(payout) => {
            println!("✓ Claimed {} tokens", payout);
            Ok(payout)
        },
        
        Err(RevoraError::NoPendingClaims) => {
            println!("⚠ No unclaimed periods available");
            Ok(0)  // Not an error; just nothing to claim
        },
        
        Err(RevoraError::ClaimDelayNotElapsed) => {
            println!("⚠ Claim delay not elapsed; try again later");
            Ok(0)
        },
        
        Err(RevoraError::HolderBlacklisted) => {
            // Shouldn't happen due to pre-flight check, but handle anyway
            Err("Holder was blacklisted after validation".into())
        },
        
        Err(e) => {
            eprintln!("✗ Claim failed: {:?}", e);
            Err(format!("Claim error: {:?}", e).into())
        }
    }
}
```

---

## Architecture Deep Dive

This section provides detailed explanations of the on-chain data model, core flows, and integration patterns for developers building on or integrating with Revora-Contracts.

### Contract Purpose and Design Philosophy

**Revora-Contracts** is a Soroban smart contract designed to facilitate **revenue-sharing offerings** on the Stellar blockchain. It enables issuers to:

1. Register revenue-share offerings tied to specific tokens
2. Deposit revenue for token holders across multiple periods
3. Allow holders to claim their accumulated revenue shares
4. Maintain compliance through blacklist management
5. Monitor holder concentration for regulatory guardrails
6. Maintain transparent audit trails of all revenue activities

**Key Design Principles:**

- **Off-chain computation, on-chain verification**: The contract doesn't compute token balances or distributions; it stores issuer-provided data and enforces rules.
- **Gas efficiency**: All operations are bounded (max 20 items per page, max 50 periods per claim) to ensure predictable costs.
- **Immutable offerings**: Once registered, offering parameters (issuer, token, revenue_share_bps) cannot be changed. New configurations require new offerings.
- **Progressive disclosure**: Holders claim revenue progressively as periods are deposited; no need to claim all at once.
- **Auditability first**: Every state change emits events; audit summaries provide aggregated views of revenue flow.

---

### On-Chain Data Model

The contract uses **persistent storage** exclusively (no temporary or instance storage) with the following key structures:

#### Storage Keys (`DataKey` enum)

```rust
pub enum DataKey {
    // ── Offering Management ──
    OfferCount(Address),              // Per-issuer: total offerings registered
    OfferItem(Address, u32),          // Per-issuer: offering at index N
    
    // ── Blacklist Management ──
    Blacklist(Address),               // Per-token: map of blacklisted addresses
    
    // ── Concentration Monitoring ──
    ConcentrationLimit(Address, Address),   // Per-offering: {max_bps, enforce}
    CurrentConcentration(Address, Address), // Per-offering: last reported bps
    
    // ── Audit & Rounding ──
    AuditSummary(Address, Address),   // Per-offering: {total_revenue, report_count}
    RoundingMode(Address, Address),   // Per-offering: Truncation | RoundHalfUp
    
    // ── Multi-Period Claims ──
    PeriodRevenue(Address, u64),      // Per (offering_token, period_id): revenue amount
    PeriodEntry(Address, u32),        // Per (offering_token, index): period_id mapping
    PeriodCount(Address),             // Per offering_token: total periods deposited
    HolderShare(Address, Address),    // Per (offering_token, holder): share_bps
    LastClaimedIdx(Address, Address), // Per (offering_token, holder): next index to claim
    PaymentToken(Address),            // Per offering_token: locked payment token address
    ClaimDelaySecs(Address),          // Per offering_token: delay in seconds (#27)
    PeriodDepositTime(Address, u64),  // Per (offering_token, period_id): deposit timestamp
    
    // ── Admin & Freeze ──
    Admin,                            // Global: admin address
    Frozen,                           // Global: contract freeze flag
}
```

#### Core Data Structures

**Offering:**
```rust
pub struct Offering {
    pub issuer: Address,           // Address authorized to manage this offering
    pub token: Address,            // Token representing this offering
    pub revenue_share_bps: u32,    // Revenue share in basis points (0-10000)
}
```
*Stored in:* `DataKey::OfferItem(issuer, index)`

**ConcentrationLimitConfig:**
```rust
pub struct ConcentrationLimitConfig {
    pub max_bps: u32,    // Maximum single-holder concentration (0 = disabled)
    pub enforce: bool,   // If true, report_revenue fails when exceeded
}
```
*Stored in:* `DataKey::ConcentrationLimit(issuer, token)`

**AuditSummary:**
```rust
pub struct AuditSummary {
    pub total_revenue: i128,   // Cumulative revenue reported (not deposited)
    pub report_count: u64,     // Total number of report_revenue calls
}
```
*Stored in:* `DataKey::AuditSummary(issuer, token)`

**RoundingMode:**
```rust
pub enum RoundingMode {
    Truncation = 0,     // floor(amount * bps / 10000)
    RoundHalfUp = 1,    // round((amount * bps) / 10000)
}
```
*Stored in:* `DataKey::RoundingMode(issuer, token)` *(defaults to Truncation)*

#### Storage Relationships

```
Issuer (Address)
  ├─ OfferCount: u32
  └─ OfferItem[0..N]: Offering
       ├─ token: Address
       ├─ revenue_share_bps: u32
       └─ (issuer, token) composite key used for:
            ├─ ConcentrationLimit
            ├─ CurrentConcentration
            ├─ AuditSummary
            └─ RoundingMode

Offering Token (Address)
  ├─ Blacklist: Map<Address, ()>
  ├─ PaymentToken: Address (locked on first deposit)
  ├─ ClaimDelaySecs: u64
  ├─ PeriodCount: u32
  └─ PeriodEntry[0..N]: period_id
       └─ PeriodRevenue(token, period_id): i128
       └─ PeriodDepositTime(token, period_id): u64

(Offering Token, Holder) tuple
  ├─ HolderShare: u32 (basis points)
  └─ LastClaimedIdx: u32 (next period index to claim)
```

---

### Core Flows & Sequences

#### 1. Offering Registration Flow

**Purpose:** Register a new revenue-share offering on-chain.

**Sequence:**
```
1. Issuer calls: register_offering(issuer, token, revenue_share_bps)
   ├─ Auth: issuer.require_auth() ✓
   ├─ Validate: revenue_share_bps ≤ 10000
   └─ State changes:
        ├─ Read: OfferCount(issuer) → count
        ├─ Write: OfferItem(issuer, count) = Offering {issuer, token, revenue_share_bps}
        ├─ Write: OfferCount(issuer) = count + 1
        └─ Event: offer_reg(issuer, (token, revenue_share_bps))

2. Result: Offering is now queryable via get_offering(issuer, token)
```

**Storage Impact:**
- **Persistent writes:** 2 (OfferItem + OfferCount)
- **Gas cost:** Low (< 2KB write)

**Error conditions:**
- `InvalidRevenueShareBps`: revenue_share_bps > 10000
- `ContractFrozen`: Contract is frozen
- Auth panic: Wrong signer

**Integration notes:**
- Offerings are **immutable** after registration
- No duplicate prevention; same (issuer, token) can be registered multiple times with different indices
- Off-chain systems should track registration events to build offering directories

---

#### 2. Revenue Deposit Flow (Multi-Period Claims)

**Purpose:** Deposit actual revenue for a specific period, enabling holder claims.

**Sequence:**
```
1. Issuer calls: deposit_revenue(issuer, token, payment_token, amount, period_id)
   ├─ Auth: issuer.require_auth() ✓
   ├─ Validate:
   │    ├─ Offering exists (get_offering)
   │    ├─ Period not already deposited (PeriodRevenue not set)
   │    └─ Payment token matches previous deposits (if any)
   ├─ Token transfer: payment_token.transfer(issuer → contract, amount)
   └─ State changes:
        ├─ Write: PeriodRevenue(token, period_id) = amount
        ├─ Write: PeriodDepositTime(token, period_id) = now
        ├─ Read: PeriodCount(token) → count
        ├─ Write: PeriodEntry(token, count) = period_id
        ├─ Write: PeriodCount(token) = count + 1
        ├─ Write (once): PaymentToken(token) = payment_token (after the first successful deposit)
        └─ Event: rev_dep(issuer, token, (payment_token, amount, period_id))

2. Result: Holders can now claim this period via claim()
```

**Storage Impact:**
- **Persistent writes:** 4-5 (PeriodRevenue + PeriodDepositTime + PeriodEntry + PeriodCount + maybe PaymentToken)
- **Token transfer:** 1 (payment_token: issuer → contract)

**Error conditions:**
- `OfferingNotFound`: No offering exists for (issuer, token)
- `PeriodAlreadyDeposited`: Period already has revenue deposited
- `PaymentTokenMismatch`: Different payment token than the token locked by the first successful deposit
- `ContractFrozen`: Contract is frozen

**Integration notes:**
- **Payment token is locked only after a successful first deposit**; failed deposits do not set `PaymentToken`
- **Duplicate period IDs fail as `PeriodAlreadyDeposited`** before any sequencing state is updated
- **Period IDs are arbitrary** (u64); issuers can use timestamps, sequential numbers, or any scheme
- **Period order matters**: Claims are processed in deposit order (via PeriodEntry index), not period_id order

---

#### 3. Revenue Reporting Flow (Event-Based Audit)

**Purpose:** Emit an audit event for off-chain tracking; doesn't transfer funds.

**Sequence:**
```
1. Issuer calls: report_revenue(issuer, token, amount, period_id)
   ├─ Auth: issuer.require_auth() ✓
   ├─ Concentration check:
   │    ├─ Read: ConcentrationLimit(issuer, token)
   │    ├─ Read: CurrentConcentration(issuer, token)
   │    └─ If enforce && current > max_bps → Err(ConcentrationLimitExceeded)
   ├─ Read: Blacklist(token) → blacklist_vec
   ├─ Event: rev_rep((issuer, token), (amount, period_id, blacklist_vec))
   └─ State changes:
        ├─ Read: AuditSummary(issuer, token) → summary
        ├─ Update: summary.total_revenue += amount
        ├─ Update: summary.report_count += 1
        └─ Write: AuditSummary(issuer, token) = summary

2. Result: Off-chain indexers see revenue report event with current blacklist snapshot
```

**Storage Impact:**
- **Persistent writes:** 1 (AuditSummary update)
- **Event payload:** ~100 bytes + blacklist size

**Error conditions:**
- `ConcentrationLimitExceeded`: Current concentration > limit and enforcement enabled
- `ContractFrozen`: Contract is frozen

**Key difference from deposit_revenue:**
- **No token transfer**: This is audit-only
- **Includes blacklist snapshot**: Event payload contains current blacklisted addresses
- **Updates audit summary**: Tracks cumulative reported revenue (may differ from deposited)

---

#### 4. Holder Claims Flow

**Purpose:** Holders claim accumulated revenue across unclaimed periods.

**Sequence:**
```
1. Holder calls: claim(holder, token, max_periods)
   ├─ Auth: holder.require_auth() ✓
   ├─ Validate:
   │    ├─ Not blacklisted: !is_blacklisted(token, holder)
   │    ├─ Has share: HolderShare(token, holder) > 0
   │    └─ Has unclaimed periods: LastClaimedIdx < PeriodCount
   ├─ Iterate periods [LastClaimedIdx .. min(LastClaimedIdx + max_periods, PeriodCount)]:
   │    ├─ Read: PeriodEntry(token, i) → period_id
   │    ├─ Check delay: PeriodDepositTime(token, period_id) + ClaimDelaySecs ≤ now
   │    │    └─ If not elapsed: break loop
   │    ├─ Read: PeriodRevenue(token, period_id) → revenue
   │    ├─ Compute: payout = revenue * share_bps / 10000
   │    └─ Accumulate: total_payout += payout
   ├─ Token transfer: payment_token.transfer(contract → holder, total_payout)
   ├─ Write: LastClaimedIdx(token, holder) = new_idx (advanced by claimed periods)
   └─ Event: claim(holder, token, (total_payout, claimed_periods_vec))

2. Result: Holder receives aggregated payout; claim index advances
```

**Storage Impact:**
- **Persistent reads:** 2N + 5 (N = periods claimed, typically ≤ 50)
- **Persistent writes:** 1 (LastClaimedIdx update)
- **Token transfer:** 1 (payment_token: contract → holder)

**Max periods per transaction:**
- **MAX_CLAIM_PERIODS = 50**: Gas safety limit
- Holders with > 50 unclaimed periods must call claim() multiple times

**Error conditions:**
- `HolderBlacklisted`: Holder is on offering's blacklist
- `NoPendingClaims`: No share set or all periods claimed
- `ClaimDelayNotElapsed`: Next claimable period hasn't passed delay threshold

**Integration notes:**
- **Zero-value periods advance index**: Even if payout is 0, LastClaimedIdx increments
- **Claim delay enforced per-period**: If delay not elapsed, loop breaks early
- **Idempotent**: Calling claim() with no new periods simply returns 0

---

#### 5. Blacklist Management Flow

**Purpose:** Manage per-token investor blacklists for compliance.

**Add to Blacklist:**
```
1. Caller calls: blacklist_add(caller, token, investor)
   ├─ Auth: caller.require_auth() ✓
   ├─ State changes:
   │    ├─ Read: Blacklist(token) → map
   │    ├─ Insert: map[investor] = ()
   │    └─ Write: Blacklist(token) = map
   └─ Event: bl_add((token, caller), investor)

2. Result: investor cannot claim revenue for this token
```

**Remove from Blacklist:**
```
1. Caller calls: blacklist_remove(caller, token, investor)
   ├─ Auth: caller.require_auth() ✓
   ├─ State changes:
   │    ├─ Read: Blacklist(token) → map
   │    ├─ Remove: map.remove(investor)
   │    └─ Write: Blacklist(token) = map
   └─ Event: bl_rem((token, caller), investor)

2. Result: investor can claim revenue again
```

**Storage Impact:**
- **Persistent writes:** 1 per operation (Blacklist map update)
- **Idempotent**: Adding an already-blacklisted address is safe (no error)

**Security notes:**
- **No issuer restriction**: Any address can manage blacklists (see Security section)
- **Affects claims only**: Blacklisted holders retain their share_bps, but cannot call claim()
- **Snapshot in report_revenue**: Current blacklist is included in rev_rep event payload

---

#### 6. Concentration Monitoring Flow

**Purpose:** Track and enforce single-holder concentration limits for regulatory compliance.

**Set Concentration Limit:**
```
1. Issuer calls: set_concentration_limit(issuer, token, max_bps, enforce)
   ├─ Auth: issuer.require_auth() ✓
   ├─ Validate: Offering exists
   ├─ State changes:
   │    └─ Write: ConcentrationLimit(issuer, token) = {max_bps, enforce}
   └─ No event (configuration change)

2. Result: Enforcement rules updated for this offering
```

**Report Current Concentration:**
```
1. Issuer/Indexer calls: report_concentration(issuer, token, concentration_bps)
   ├─ Auth: issuer.require_auth() ✓
   ├─ State changes:
   │    └─ Write: CurrentConcentration(issuer, token) = concentration_bps
   ├─ Check limit:
   │    ├─ Read: ConcentrationLimit(issuer, token)
   │    └─ If concentration_bps > max_bps → Event: conc_warn((issuer, token), (concentration_bps, limit_bps))
   └─ No error (warning only)

2. Result: Current concentration stored; warning event if exceeded
```

**Enforcement at report_revenue:**
```
When issuer calls report_revenue():
   ├─ Read: ConcentrationLimit(issuer, token)
   ├─ Read: CurrentConcentration(issuer, token)
   └─ If enforce && current > max_bps:
        └─ Err(ConcentrationLimitExceeded) → Transaction reverts
```

**Integration pattern:**
```
Off-chain indexer:
1. Monitor token holder balances
2. Compute: top_holder_balance / total_supply * 10000 = concentration_bps
3. Call: report_concentration(issuer, token, concentration_bps)
4. Contract stores value for next report_revenue() call
```

**Security notes:**
- **Trust model**: Contract trusts reported concentration values (no on-chain verification)
- **Warning vs. enforcement**: `conc_warn` event is informational; `enforce=true` blocks revenue reports
- **No automatic updates**: Concentration must be reported manually before each revenue report

---

### Integration Patterns

#### Pattern 1: Off-Chain Indexer for Revenue Distribution

**Problem:** Contract doesn't compute holder shares; issuers need to know who gets paid and how much.

**Solution:** Build an off-chain indexer that:

1. **Monitors offering registrations:**
   ```
   Listen for: offer_reg events
   Store: (issuer, token, revenue_share_bps) mappings
   ```

2. **Tracks token holder balances:**
   ```
   Query: Token contract balance changes
   Compute: holder_balance / total_supply = holder_share_pct
   ```

3. **Calculates revenue shares:**
   ```
   For each holder:
     share_bps = floor(holder_share_pct * 10000)
     Call: set_holder_share(issuer, token, holder, share_bps)
   ```

4. **Deposits revenue:**
   ```
   For each revenue period:
     Compute: total_revenue_for_holders = total_revenue * revenue_share_bps / 10000
     Call: deposit_revenue(issuer, token, payment_token, amount, period_id)
   ```

5. **Monitors concentration:**
   ```
   Compute: top_holder_bps = max(holder_share_pct) * 10000
   Call: report_concentration(issuer, token, top_holder_bps)
   ```

**Example pseudo-code:**
```rust
// Off-chain worker (runs periodically)
async fn distribute_revenue(issuer: Address, token: Address, period_id: u64) {
    // 1. Query token holders from Stellar network
    let holders = query_token_holders(&token).await;
    let total_supply = query_total_supply(&token).await;
    
    // 2. Set holder shares on-chain
    for holder in holders {
        let balance = holder.balance;
        let share_bps = (balance * 10_000) / total_supply;
        contract.set_holder_share(issuer, token, holder.address, share_bps).await;
    }
    
    // 3. Report concentration
    let max_holder = holders.iter().max_by_key(|h| h.balance).unwrap();
    let concentration_bps = (max_holder.balance * 10_000) / total_supply;
    contract.report_concentration(issuer, token, concentration_bps).await;
    
    // 4. Deposit revenue
    let total_revenue = compute_period_revenue(period_id);
    contract.deposit_revenue(issuer, token, payment_token, total_revenue, period_id).await;
    
    // 5. Emit audit event
    contract.report_revenue(issuer, token, total_revenue, period_id).await;
}
```

---

#### Pattern 2: Event Monitoring for Audit Trails

**Problem:** Need real-time visibility into contract activity for compliance and analytics.

**Solution:** Subscribe to contract events and build audit database.

**Event stream processing:**
```rust
match event.topic {
    "offer_reg" => {
        let (issuer, (token, revenue_share_bps)) = event.payload;
        db.insert_offering(issuer, token, revenue_share_bps, event.ledger);
    },
    "rev_dep" => {
        let (issuer, token, (payment_token, amount, period_id)) = event.payload;
        db.insert_deposit(token, period_id, amount, payment_token, event.ledger);
    },
    "rev_rep" => {
        let ((issuer, token), (amount, period_id, blacklist)) = event.payload;
        db.insert_report(issuer, token, amount, period_id, blacklist, event.ledger);
    },
    "claim" => {
        let (holder, token, (payout, periods)) = event.payload;
        db.insert_claim(holder, token, payout, periods, event.ledger);
    },
    "bl_add" | "bl_rem" => {
        let ((token, caller), investor) = event.payload;
        db.update_blacklist(token, investor, event.topic == "bl_add", event.ledger);
    },
    "conc_warn" => {
        let ((issuer, token), (concentration_bps, limit_bps)) = event.payload;
        db.insert_concentration_warning(issuer, token, concentration_bps, limit_bps, event.ledger);
    },
}
```

**Query patterns:**
- Offering history: `SELECT * FROM offerings WHERE issuer = ?`
- Holder claims: `SELECT * FROM claims WHERE holder = ? AND token = ?`
- Revenue timeline: `SELECT * FROM deposits WHERE token = ? ORDER BY period_id`
- Compliance violations: `SELECT * FROM concentration_warnings WHERE concentration_bps > limit_bps`

---

#### Pattern 3: Batched Claims for Large Holder Bases

**Problem:** Gas costs for individual holder claims can be high; want to optimize for large distributions.

**Solution:** Off-chain aggregation with periodic claim notifications.

**Approach:**
```
1. Indexer monitors deposit_revenue events
2. For each new deposit:
   a. Query all holders with share_bps > 0
   b. Compute each holder's payout: revenue * share_bps / 10000
   c. Store in off-chain DB: (holder, token, estimated_payout, period_id)
   d. Send notification: "You have $X available to claim"
   
3. Holders claim at their convenience:
   - High-value holders: claim frequently (every period)
   - Low-value holders: claim in batches (every N periods)
   - Gas optimization: max_periods parameter controls batch size
   
4. Unclaimed revenue stays in contract (no forced distribution)
```

**Claim optimization:**
```rust
// Holder decides when to claim based on gas vs. revenue
let estimated_gas_cost = estimate_claim_gas(num_unclaimed_periods);
let estimated_payout = query_unclaimed_payout(holder, token);

if estimated_payout > estimated_gas_cost * MIN_PROFIT_RATIO {
    contract.claim(holder, token, num_unclaimed_periods).await;
} else {
    // Wait for more periods to accumulate
    log("Skipping claim; gas cost too high for current payout");
}
```

---

#### Pattern 4: Rounding Mode Selection

**Problem:** Different jurisdictions/contracts may require different rounding for fairness.

**Solution:** Configure per-offering rounding mode based on legal requirements.

**Rounding modes:**
```rust
// Truncation (default): Always rounds down
// Benefit: Conservative; prevents over-distribution
// Drawback: Small holders lose fractional amounts
compute_share(100, 3333, Truncation)  // = 33  (33.33 truncated)

// RoundHalfUp: Standard rounding (>= 0.5 rounds up)
// Benefit: More accurate; fairer to small holders
// Drawback: May over-distribute if not careful with total
compute_share(100, 3333, RoundHalfUp)  // = 33  (33.33 rounds to 33)
compute_share(100, 6667, RoundHalfUp)  // = 67  (66.67 rounds to 67)
```

**Selection guidance:**
```
Use Truncation when:
- Conservative accounting required
- Preventing over-distribution is critical
- Small fractional losses are acceptable

Use RoundHalfUp when:
- Fairness to small holders is priority
- Total distribution carefully controlled off-chain
- Regulatory requirement for "fair rounding"
```

**Integration:**
```rust
// Set once per offering during setup
contract.set_rounding_mode(issuer, token, RoundingMode::RoundHalfUp).await;

// Verify before distributions
let mode = contract.get_rounding_mode(issuer, token).await;
assert_eq!(mode, RoundingMode::RoundHalfUp);

// Use consistently off-chain
for holder in holders {
    let share = compute_share(revenue, holder.share_bps, mode);
    estimated_distributions.push((holder.address, share));
}
```

---

### Advanced Topics

#### Pagination Strategies for Large Datasets

**Problem:** Issuers with hundreds of offerings need efficient querying.

**Contract pagination API:**
```rust
pub fn get_offerings_page(
    env: Env,
    issuer: Address,
    start: u32,      // Starting index
    limit: u32,      // Max items (capped at 20)
) -> (Vec<Offering>, Option<u32>)  // (results, next_cursor)
```

**Pagination pattern:**
```rust
let mut all_offerings = Vec::new();
let mut cursor = Some(0);

while let Some(start) = cursor {
    let (page, next) = contract.get_offerings_page(issuer, start, 20).await;
    all_offerings.extend(page);
    cursor = next;  // None when no more pages
}
```

**Performance notes:**
- Each page costs ~O(20) storage reads
- For 100 offerings: 5 RPC calls (100 / 20)
- Alternative: Cache offerings off-chain after monitoring `offer_reg` events

---

#### Claim Delay Mechanics

**Purpose:** Time-lock revenue claims for dispute windows or regulatory hold periods.

**Configuration:**
```rust
// Set delay once per offering
contract.set_claim_delay(issuer, token, 86400).await;  // 24-hour delay
```

**Behavior:**
```
Deposit at t=0:  deposit_revenue(..., period_id=1)
Delay window:    [t=0 ... t=86400]
Claimable at:    t=86401+

If holder calls claim() at t=43200 (12 hours):
  → Err(ClaimDelayNotElapsed)  // Too early

If holder calls claim() at t=90000:
  → Success, payout transferred
```

**Use cases:**
- **Dispute windows**: Allow time to challenge revenue calculations
- **Regulatory holds**: Comply with holding period requirements
- **Batch optimization**: Encourage holders to claim less frequently

---

#### Gas Optimization Tips

**For issuers:**
1. **Batch holder share updates**: Set shares for multiple holders in quick succession to amortize RPC overhead
2. **Minimize blacklist size**: Each blacklist entry adds storage cost and increases `rev_rep` event payload
3. **Use sequential period IDs**: Simplifies off-chain tracking (e.g., Unix timestamps)

**For holders:**
1. **Claim in batches**: Waiting for N periods (max 50) reduces transactions by N×
2. **Monitor gas prices**: Claim during low-fee periods on Stellar network
3. **Check unclaimed balance**: Query `LastClaimedIdx` vs `PeriodCount` before claiming

**For integrators:**
1. **Cache read-only data**: `get_offering`, `get_concentration_limit`, etc. change rarely
2. **Use event streams**: More efficient than polling `get_offerings_page` repeatedly
3. **Parallel RPCs**: Query multiple offerings simultaneously (Stellar supports concurrent reads)

---

#### Audit Summary Usage

**Purpose:** On-chain aggregated view of revenue reporting activity.

**Structure:**
```rust
pub struct AuditSummary {
    pub total_revenue: i128,    // Sum of persisted reports after override deltas
    pub report_count: u64,      // Number of persisted periods
}
```

**Key insights:**
```rust
let summary = contract.get_audit_summary(issuer, token).await;

// Average revenue per report
let avg_revenue = summary.total_revenue / (summary.report_count as i128);

// Compare reported vs. deposited
let total_deposited = query_period_revenues(token).sum();
let discrepancy = summary.total_revenue - total_deposited;
// Note: These may differ! report_revenue is informational; deposit_revenue is actual.
```

**Audit patterns:**
```
1. Consistency check:
   For each period_id in rev_init / rev_ovrd events:
     Verify the latest accepted reported amount matches expected accounting
     Alert if a correction changes a period without the matching off-chain justification

2. Completeness check:
   Reconstruct totals as sum(rev_init) + sum(rev_ovrd.new_amount - rev_ovrd.old_amount)
   Compare the result to get_audit_summary / reconcile_audit_summary
   Investigate significant discrepancies

3. Compliance reporting:
   Generate quarterly reports using audit_summary data
   Cross-reference with off-chain payment records
```

---

### Code Examples

#### Example 1: Complete Offering Lifecycle (Pseudo-Code)

```rust
use soroban_sdk::{Address, Env};

// ── Step 1: Register Offering ──
async fn register_new_offering(
    env: &Env,
    issuer: &Address,
    token: &Address,
) -> Result<()> {
    let revenue_share_bps = 2500;  // 25% to holders
    
    contract.register_offering(
        issuer.clone(),
        token.clone(),
        revenue_share_bps,
    ).await?;
    
    println!("Offering registered: {}", token);
    Ok(())
}

// ── Step 2: Set Holder Shares (Off-Chain Indexer) ──
async fn update_holder_shares(
    env: &Env,
    issuer: &Address,
    token: &Address,
) -> Result<()> {
    // Query token balances from Stellar
    let holders = stellar.query_token_holders(token).await?;
    let total_supply = stellar.query_total_supply(token).await?;
    
    for holder in holders {
        let share_bps = (holder.balance * 10_000) / total_supply;
        
        contract.set_holder_share(
            issuer.clone(),
            token.clone(),
            holder.address.clone(),
            share_bps as u32,
        ).await?;
        
        println!("Set share for {}: {} bps", holder.address, share_bps);
    }
    
    Ok(())
}

// ── Step 3: Deposit Revenue ──
async fn deposit_quarterly_revenue(
    env: &Env,
    issuer: &Address,
    token: &Address,
    quarter: u64,
) -> Result<()> {
    let payment_token = usdc_token_address();
    let revenue_amount = 1_000_000_000;  // 1,000 USDC (7 decimals)
    let period_id = quarter;  // e.g., 20241 for Q1 2024
    
    // First, approve contract to spend tokens
    payment_token_client.approve(
        issuer,
        contract_address,
        revenue_amount,
        expiration_ledger,
    ).await?;
    
    // Then deposit
    contract.deposit_revenue(
        issuer.clone(),
        token.clone(),
        payment_token.clone(),
        revenue_amount,
        period_id,
    ).await?;
    
    println!("Deposited {} for period {}", revenue_amount, period_id);
    Ok(())
}

// ── Step 4: Report Revenue (Audit Event) ──
async fn report_quarterly_revenue(
    env: &Env,
    issuer: &Address,
    token: &Address,
    quarter: u64,
) -> Result<()> {
    let total_revenue = 4_000_000_000;  // Total revenue (not just holder share)
    let period_id = quarter;
    
    contract.report_revenue(
        issuer.clone(),
        token.clone(),
        total_revenue,
        period_id,
    ).await?;
    
    println!("Reported {} for audit", total_revenue);
    Ok(())
}

// ── Step 5: Holder Claims ──
async fn holder_claim_revenue(
    env: &Env,
    holder: &Address,
    token: &Address,
) -> Result<i128> {
    let max_periods = 10;  // Claim up to 10 periods at once
    
    let payout = contract.claim(
        holder.clone(),
        token.clone(),
        max_periods,
    ).await?;
    
    println!("Holder {} claimed {}", holder, payout);
    Ok(payout)
}
```

---

#### Example 2: Event Handling for Monitoring

```rust
use stellar_sdk::{EventFilter, EventType};

async fn monitor_contract_events(contract_id: &str) -> Result<()> {
    let filter = EventFilter::new()
        .contract(contract_id)
        .event_types(vec![EventType::Contract]);
    
    let mut stream = stellar.subscribe_events(filter).await?;
    
    while let Some(event) = stream.next().await {
        match event.topic.as_str() {
            "offer_reg" => {
                let issuer = event.data[0].as_address()?;
                let token = event.data[1].as_address()?;
                let revenue_share_bps = event.data[2].as_u32()?;
                
                database.insert_offering(OfferingRecord {
                    issuer,
                    token,
                    revenue_share_bps,
                    registered_at: event.ledger_timestamp,
                }).await?;
                
                println!("New offering: {} by {}", token, issuer);
            },
            
            "rev_dep" => {
                let issuer = event.data[0].as_address()?;
                let token = event.data[1].as_address()?;
                let payment_token = event.data[2].as_address()?;
                let amount = event.data[3].as_i128()?;
                let period_id = event.data[4].as_u64()?;
                
                database.insert_deposit(DepositRecord {
                    issuer,
                    token,
                    payment_token,
                    amount,
                    period_id,
                    deposited_at: event.ledger_timestamp,
                }).await?;
                
                // Notify holders
                let holders = database.get_holders(token).await?;
                for holder in holders {
                    let payout = compute_share(amount, holder.share_bps, RoundingMode::Truncation);
                    notification_service.notify_holder(holder.address, payout).await?;
                }
            },
            
            "claim" => {
                let holder = event.data[0].as_address()?;
                let token = event.data[1].as_address()?;
                let payout = event.data[2].as_i128()?;
                let periods = event.data[3].as_vec()?;
                
                database.insert_claim(ClaimRecord {
                    holder,
                    token,
                    payout,
                    periods_claimed: periods.len(),
                    claimed_at: event.ledger_timestamp,
                }).await?;
                
                println!("Claim: {} received {} for {} periods", holder, payout, periods.len());
            },
            
            "conc_warn" => {
                let issuer = event.data[0].as_address()?;
                let token = event.data[1].as_address()?;
                let concentration_bps = event.data[2].as_u32()?;
                let limit_bps = event.data[3].as_u32()?;
                
                alert_service.send_concentration_alert(
                    issuer,
                    token,
                    concentration_bps,
                    limit_bps,
                ).await?;
                
                println!("⚠️  Concentration warning: {} bps (limit: {} bps)", 
                         concentration_bps, limit_bps);
            },
            
            _ => {
                println!("Unknown event: {}", event.topic);
            }
        }
    }
    
    Ok(())
}
```

---

#### Example 3: Error Handling Patterns

```rust
use revora_contracts::{RevoraError, RevoraRevenueShareClient};

async fn safe_deposit_with_retry(
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
    payment_token: &Address,
    amount: i128,
    period_id: u64,
) -> Result<()> {
    const MAX_RETRIES: u32 = 3;
    let mut attempt = 0;
    
    loop {
        match client.try_deposit_revenue(
            issuer,
            token,
            payment_token,
            amount,
            period_id,
        ).await {
            Ok(_) => {
                println!("✓ Revenue deposited successfully");
                return Ok(());
            },
            
            Err(RevoraError::OfferingNotFound) => {
                eprintln!("✗ Offering not found; cannot deposit");
                return Err("Offering must be registered first".into());
            },
            
            Err(RevoraError::PeriodAlreadyDeposited) => {
                println!("⚠ Period already deposited; skipping");
                return Ok(());  // Idempotent behavior
            },
            
            Err(RevoraError::PaymentTokenMismatch) => {
                eprintln!("✗ Payment token mismatch; locked to different token");
                return Err("Cannot change payment token after first deposit".into());
            },
            
            Err(RevoraError::ContractFrozen) => {
                eprintln!("✗ Contract is frozen; waiting for admin action");
                return Err("Contract operations suspended".into());
            },
            
            Err(e) => {
                attempt += 1;
                if attempt >= MAX_RETRIES {
                    eprintln!("✗ Max retries exceeded: {:?}", e);
                    return Err(format!("Failed after {} attempts", MAX_RETRIES).into());
                }
                
                eprintln!("⚠ Retrying deposit (attempt {}/{}): {:?}", attempt, MAX_RETRIES, e);
                tokio::time::sleep(Duration::from_secs(2_u64.pow(attempt))).await;
            }
        }
    }
}

async fn safe_claim_with_validation(
    client: &RevoraRevenueShareClient,
    holder: &Address,
    token: &Address,
) -> Result<i128> {
    // Pre-flight checks
    if client.is_blacklisted(token, holder).await? {
        return Err("Holder is blacklisted; cannot claim".into());
    }
    
    let share_bps = client.get_holder_share(token, holder).await?;
    if share_bps == 0 {
        return Err("No share allocated; nothing to claim".into());
    }
    
    // Attempt claim
    match client.try_claim(holder, token, 50).await {
        Ok(payout) => {
            println!("✓ Claimed {} tokens", payout);
            Ok(payout)
        },
        
        Err(RevoraError::NoPendingClaims) => {
            println!("⚠ No unclaimed periods available");
            Ok(0)  // Not an error; just nothing to claim
        },
        
        Err(RevoraError::ClaimDelayNotElapsed) => {
            println!("⚠ Claim delay not elapsed; try again later");
            Ok(0)
        },
        
        Err(RevoraError::HolderBlacklisted) => {
            // Shouldn't happen due to pre-flight check, but handle anyway
            Err("Holder was blacklisted after validation".into())
        },
        
        Err(e) => {
            eprintln!("✗ Claim failed: {:?}", e);
            Err(format!("Claim error: {:?}", e).into())
        }
    }
}
```

---

## Security review checklist (contracts)

This section enumerates key security assumptions, trust boundaries, and mitigations for the Revora contracts. It is kept in sync with the implementation; see `src/lib.rs` and `src/test.rs` for the code that enforces these behaviors.

### Assumptions and trust boundaries

- **Issuer authority:** Only the offering issuer can register offerings, report revenue, set concentration limits, set rounding mode, and report concentration for that offering. The contract does not implement a separate “platform admin” role; all offering-level actions are issuer-authorized.
- **Blacklist authority:** Only the current issuer of the offering can add/remove blacklist entries for that offering's token. This ensures issuers have full control over compliance and investor management.
- **Concentration data:** Holder concentration is not derived on-chain. The contract trusts the value passed to `report_concentration`. Enforcing or warning is based on this reported value; manipulation of the reported value can bypass the guardrail.
- **Revenue reports:** The contract does not verify that reported revenue amounts are correct or consistent with any external source. It only records and aggregates them for the audit summary and emits events.
- **Zero-value revenue policy:** `deposit_revenue` requires a positive amount, but `report_revenue` allows zero so issuers can preserve an explicit on-chain audit record for a period even when the final reported amount is zero.

### Threat model and mitigations

| Risk | Mitigation |
|------|------------|
| **Auth misuse / wrong signer** | All state-changing entrypoints call `require_auth` on the appropriate address. Auth failures cause host panic; use `try_*` client methods to handle errors. Issuer-only enforcement for blacklist operations. Tests: `blacklist_add_requires_auth`, `blacklist_remove_requires_auth`, `blacklist_add_requires_issuer_auth`, `blacklist_remove_requires_issuer_auth`. |
| **Issuer transfer security** | Two-step propose/accept flow prevents accidental loss of control. Old issuer must propose, new issuer must explicitly accept. Either can abort (old cancels, new doesn't accept). Current issuer verified via reverse lookup on all auth checks. Tests: `issuer_transfer_*` (35 tests covering happy path, abuse attempts, edge cases, and integration). |
| **Incorrect math (overflow, rounding)** | Revenue share bps is capped at 10000. `compute_share` uses checked arithmetic where applicable and clamps output to [0, amount]. Rounding modes (Truncation, RoundHalfUp) are documented and tested. Tests: `compute_share_*`, `register_offering_rejects_bps_over_10000`. |
| **Invalid revenue amounts** | Deposits reject amounts ≤ 0; reports reject negatives but allow zero-value audit entries. This preserves explicit audit history without letting transfers carry empty or negative amounts. |
| **Concentration guardrail bypass** | Enforcement is applied in `report_revenue` using the last value set by `report_concentration`. If concentration is not reported or is reported low, enforcement cannot block. Design: guardrail is advisory or best-effort unless the issuer reliably reports concentration before each report. Tests: concentration_enforce_blocks_report_revenue_when_over_limit, concentration_near_threshold_boundary. |
| **Audit summary consistency** | `AuditSummary` is derived from persisted report state. Initial reports add `(amount, +1)`, overrides add the net delta `(new - old, +0)`, rejected duplicates and `rev_below` no-ops do not mutate the summary, and `reconcile_audit_summary` / `repair_audit_summary` are available if drift is detected. |
| **Storage / gas exhaustion** | Large blacklists and many offerings increase read/write cost. Pagination (max 20 per page) and stress tests document behavior. No unbounded loops over user-controlled collections except the blacklist map (bounded by who is added). Tests: storage_stress_*, gas_characterization_*. |
| **Upgradeability** | The contract is not upgradeable in this codebase; deployment is a single WASM with no proxy pattern. Any upgrade would require a new deployment and migration of off-chain indexing. |

### Limitations of on-chain checks

- **Holder concentration:** Token balances are held in the token contract. This contract does not call the token contract to compute concentration; it only stores and compares a reported value. Full concentration checks require off-chain indexing of balances and optional submission via `report_concentration`.
- **Revenue authenticity:** There is no on-chain verification that reported revenue matches actual payments or external systems. Auditability is via events and the on-chain audit summary; integrity of the source data is an off-chain concern.

### Build and test

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo build --release
cargo test
```


## Multisig Admin Pattern

### Overview

The contract includes an optional multi-signature (multisig) pattern for critical administrative operations. When initialized, it replaces the single-admin model for sensitive actions such as freezing the contract and changing the admin address.

### Multisig Methods

| Method | Parameters | Returns | Auth | Description |
|--------|------------|---------|------|-------------|
| `init_multisig` | `caller: Address`, `owners: Vec<Address>`, `threshold: u32` | `Result<(), RevoraError>` | caller | Initialize multisig. Can only be called once. Disables `set_admin` and `freeze`. |
| `propose_action` | `proposer: Address`, `action: ProposalAction` | `Result<u32, RevoraError>` | proposer (must be owner) | Create a new proposal. Proposer's vote is automatically counted. Returns proposal ID. |
| `approve_action` | `approver: Address`, `proposal_id: u32` | `Result<(), RevoraError>` | approver (must be owner) | Approve an existing proposal. Duplicate approvals are silently ignored. |
| `execute_action` | `proposal_id: u32` | `Result<(), RevoraError>` | — | Execute a proposal if threshold is met. Fails if already executed or threshold not met. |
| `get_proposal` | `proposal_id: u32` | `Option<Proposal>` | — | Fetch a proposal by ID. |
| `get_multisig_owners` | — | `Vec<Address>` | — | Get current owner list. |
| `get_multisig_threshold` | — | `Option<u32>` | — | Get current approval threshold. |

### Proposal Actions

| Action | Effect |
|--------|--------|
| `SetAdmin(Address)` | Updates the contract admin address. |
| `Freeze` | Freezes the contract (disables state-changing operations). |
| `SetThreshold(u32)` | Updates the approval threshold. Must be ≤ current owner count. |
| `AddOwner(Address)` | Adds a new owner to the multisig. |
| `RemoveOwner(Address)` | Removes an owner. Fails if remaining owners < threshold. |

### Events

| Topic / name | Payload | When |
|--------------|---------|------|
| `prop_new` | `(proposer), proposal_id` | After `propose_action`. |
| `prop_app` | `(approver), proposal_id` | After `approve_action` (and auto-approval on propose). |
| `prop_exe` | `(proposal_id), true` | After `execute_action`. |

### Soroban Compatibility and Limitations

**Soroban does not support multi-party authorization in a single transaction.** Each owner must call `approve_action` in a separate transaction. This is a fundamental constraint of the Soroban execution model.

Key design decisions and limitations:

1. **Single-transaction init**: `init_multisig` only requires the caller (deployer) to authorize. Owners are registered without requiring their individual signatures at init time.

2. **Auto-approval on propose**: The proposer's address is automatically counted as the first approval when `propose_action` is called. This reduces the number of separate transactions needed.

3. **No time-lock**: Proposals can be executed immediately once the threshold is met. For production use, consider adding a time-lock delay between threshold-met and execution.

4. **No proposal expiry**: Proposals do not expire. A stale proposal can be executed at any time once it reaches threshold. For production use, add an expiry timestamp to proposals.

5. **No replay protection beyond executed flag**: Once executed, a proposal cannot be re-executed. However, a new identical proposal can be created.

6. **Owner management via proposals**: Adding/removing owners and changing the threshold all require multisig approval, preventing unilateral changes.

7. **Mutual exclusion with direct admin**: Once `init_multisig` is called, `set_admin` and `freeze` are disabled and return `LimitReached`. All admin operations must go through the proposal flow.

### Production Recommendation

This multisig pattern is **suitable for low-frequency admin operations** in a controlled environment. For high-security production deployments, consider:

- Adding time-locks (e.g. 24–72 hour delay between threshold met and execution)
- Adding proposal expiry (e.g. proposals expire after 7 days)
- Off-chain coordination tooling (e.g. a multisig UI that tracks pending proposals)
- A formal security audit of the threshold/owner management flows
- Using a dedicated multisig contract (e.g. a Soroban port of Gnosis Safe) for maximum security

---
### Regression Testing Policy

The contract includes a dedicated regression test suite to capture and prevent recurrence of critical bugs discovered in production, audits, or security reviews. All regression tests are located in `src/test.rs` under the `mod regression` section.

#### When to Add a Regression Test

Add a regression test when:
- A critical bug is discovered in production or testnet deployments
- An audit or security review identifies a vulnerability
- A bug fix addresses incorrect behavior that could recur
- An edge case causes unexpected contract behavior or panic
- A fix prevents data corruption or loss of funds

#### Naming Convention

Use descriptive names that reference the issue:
- Format: `regression_issue_N_brief_description`
- Example: `regression_issue_48_overflow_in_share_calculation`
- For audit findings: `regression_audit_2024_q1_section_3_2`

#### Required Documentation Format

Each regression test MUST include:

```rust
/// Regression Test: [Brief Title]
///
/// **Related Issue:** #N or [Audit Report Reference]
///
/// **Original Bug:**
/// [Detailed description of what went wrong, including:
///  - Conditions that triggered the bug
///  - Incorrect behavior observed
///  - Impact (panic, wrong calculation, security issue)]
///
/// **Expected Behavior:**
/// [What should happen instead]
///
/// **Fix Applied:**
/// [Brief description of the code change that resolved it]
#[test]
fn regression_issue_N_description() {
    // Test implementation
}
```

#### Determinism Requirements

All regression tests MUST be deterministic and CI-safe:
- Use `Env::default()` with `mock_all_auths()` for predictable auth
- Use `Address::generate(&env)` for test addresses (deterministic within test)
- Avoid `env.ledger().timestamp()` without explicit mocking
- Use fixed seeds for any pseudo-random test data
- No external network calls or file system dependencies

#### Performance Expectations

- Individual tests should complete in <100ms
- Avoid unnecessary setup; use helper functions (`make_client()`, `setup()`)
- Keep test scope focused on the specific bug being prevented
- Use minimal data sets that reproduce the issue

#### Coverage Requirement

The overall test suite (including regression tests) MUST maintain minimum 95% code coverage. Run coverage checks with:

```bash
cargo tarpaulin --out Html --output-dir coverage
```

#### CI Integration

Regression tests run automatically as part of `cargo test`:
- No special flags or environment variables required
- Tests must pass on all supported platforms (Linux, macOS, Windows)
- Snapshot tests in `test_snapshots/` are validated automatically

#### Example Regression Test

See `src/test.rs::regression::regression_template_example` for a complete template demonstrating the required structure and documentation format.

### Contributor guidelines (reduce merge conflicts)

- Use feature branches per change (e.g. `feature/structured-error-codes`, `feature/storage-limit-negative-tests`).
- Tests in `src/test.rs` are grouped by area (pagination, blacklist, structured errors, storage stress, gas characterization). Add new tests in the relevant section so parallel PRs touch different regions.
- Keep the contract interface summary above in sync when adding or changing entrypoints or events.
- Follow the contract lint/style policy in [`docs/contracts-style.md`](./docs/contracts-style.md).
