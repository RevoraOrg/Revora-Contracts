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
        ├─ Write (once): PaymentToken(token) = payment_token (if first deposit)
        └─ Event: rev_dep(issuer, token, (payment_token, amount, period_id))

2. Result: Holders can now claim this period via claim()
```

**Storage Impact:**
- **Persistent writes:** 4-5 (PeriodRevenue + PeriodDepositTime + PeriodEntry + PeriodCount + maybe PaymentToken)
- **Token transfer:** 1 (payment_token: issuer → contract)

**Error conditions:**
- `OfferingNotFound`: No offering exists for (issuer, token)
- `PeriodAlreadyDeposited`: Period already has revenue deposited
- `PaymentTokenMismatch`: Different payment token than previous deposits
- `ContractFrozen`: Contract is frozen

**Integration notes:**
- **Payment token is locked** on first deposit; all subsequent deposits must use the same token
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
    pub total_revenue: i128,    // Sum of all report_revenue() calls
    pub report_count: u64,      // Number of report_revenue() calls
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
   For each period_id in rev_rep events:
     Verify corresponding rev_dep event exists
     Alert if reported amount != deposited amount

2. Completeness check:
   Sum(all rev_dep amounts) should approximate sum(all rev_rep amounts)
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
