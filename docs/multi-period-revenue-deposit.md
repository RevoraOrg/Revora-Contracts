# Multi-Period Revenue Deposit

> **Contract:** `RevenueDepositContract`  
> **File:** `Revora-Contracts/src/lib.rs`  
> **Network:** Stellar / Soroban  
> **Feature branch:** `feature/contracts-002-multi-period-revenue-deposit`

---

## Overview

The Multi-Period Revenue Deposit feature allows a privileged **admin** to deposit
token revenue into the smart contract segmented across non-overlapping **periods**.
Each period is defined by a ledger-based time window. After a period closes,
registered **beneficiaries** may each claim their equal share of that period's
deposited revenue. The beneficiary set and revenue are immutable after the
period's end ledger. Deposits are gated by an admin-managed allowlist of
authorized offering contract addresses so that only approved callers can fund a
period while it remains open.

```
Admin ──deposit──► Contract ──claim──► Beneficiary₁
                              └──────► Beneficiary₂
                              └──────► Beneficiary₃
```

---

## Key Concepts

### Period

A period is a non-overlapping ledger range `[start_ledger, end_ledger]` with an
initial `revenue_amount` of tokens deposited at creation time. Additional
authorized deposits are accepted only through `end_ledger`. Multiple periods may
co-exist as long as their ranges do not overlap.

| Field             | Type   | Description                                      |
|-------------------|--------|--------------------------------------------------|
| `id`              | `u32`  | Monotonically-assigned identifier.               |
| `start_ledger`    | `u32`  | First ledger of the period (inclusive).          |
| `end_ledger`      | `u32`  | Last ledger of the period (inclusive).           |
| `revenue_amount`  | `i128` | Total tokens deposited for this period.          |
| `claimed_amount`  | `i128` | Running total of tokens claimed so far.          |

### Beneficiary

An `Address` registered by the admin for a specific period. The first claim fixes
the equal share and remainder. Each beneficiary receives the fixed share, and the
final beneficiary to claim receives the remainder as well.

### Claim

A one-time action per beneficiary per period. Claims are gated behind:

1. The current ledger being **strictly greater** than `end_ledger`.
2. The claimant being a registered beneficiary.
3. The claimant not having claimed before.

---

## Contract API

### `initialize(admin, token) → Result<(), ContractError>`

Must be called exactly once after deployment.

| Argument | Type      | Notes                          |
|----------|-----------|--------------------------------|
| `admin`  | `Address` | Gains admin privileges.        |
| `token`  | `Address` | Stellar asset contract to use. |

**Errors:** `AlreadyInitialized`

---

### `create_period(start_ledger, end_ledger, revenue_amount) → Result<u32, ContractError>`

Create a new period and transfer `revenue_amount` tokens from admin to contract.

**Requires:** admin auth.

| Argument          | Type   | Constraints                  |
|-------------------|--------|------------------------------|
| `start_ledger`    | `u32`  | Must be < `end_ledger`       |
| `end_ledger`      | `u32`  | Must be > `start_ledger`     |
| `revenue_amount`  | `i128` | Must be > 0                  |

**Returns:** assigned `period_id`.

**Errors:** `Unauthorized`, `InvalidInput`, `PeriodOverlap`

---

### `add_beneficiary(period_id, beneficiary) → Result<(), ContractError>`

Register a beneficiary for an existing period. Idempotent.

**Requires:** admin auth.

**Errors:** `Unauthorized`, `PeriodNotFound`, `PeriodClosed`

---

### `remove_beneficiary(period_id, beneficiary) → Result<(), ContractError>`

Deregister a beneficiary before the period closes. Removed beneficiaries cannot
claim.

**Requires:** admin auth.

**Errors:** `Unauthorized`, `PeriodNotFound`, `NotBeneficiary`, `PeriodClosed`

---

### `deposit(caller, period_id, amount) → Result<(), ContractError>`

Add revenue from an authorized offering while the period remains open. Deposits
are rejected once the current ledger is greater than `end_ledger`.

**Errors:** `UnauthorizedDepositor`, `PeriodNotFound`, `InvalidInput`,
`PeriodClosed`, `Overflow`

---

### `claim(period_id, claimant) → Result<i128, ContractError>`

Claim an equal share of `period_id` revenue.

**Requires:** claimant auth.

**Returns:** token amount transferred. The final unclaimed beneficiary receives
any division remainder.

**Errors:** `PeriodNotFound`, `PeriodNotEnded`, `NotBeneficiary`, `AlreadyClaimed`,
`NoBeneficiaries`, `Overflow`, `InvariantViolation`

---

### Read-only helpers

| Function                          | Returns              | Description                            |
|-----------------------------------|----------------------|----------------------------------------|
| `get_period(period_id)`           | `Period`             | Period metadata.                       |
| `get_period_ids()`                | `Vec<u32>`           | All registered period IDs.             |
| `get_beneficiaries(period_id)`    | `Vec<Address>`       | Beneficiary list for a period.         |
| `has_claimed(period_id, address)` | `bool`               | Claim record lookup.                   |
| `get_admin()`                     | `Address`            | Current admin.                         |
| `get_token()`                     | `Address`            | Token contract address.                |
| `unclaimed_summary()`             | `Map<u32, i128>`     | Unclaimed amounts per period.          |

---

## Error Reference

| Code | Name                 | Meaning                                              |
|------|----------------------|------------------------------------------------------|
| 1    | `Unauthorized`       | Caller lacks admin rights.                           |
| 2    | `AlreadyInitialized` | `initialize` called more than once.                  |
| 3    | `PeriodNotFound`     | `period_id` does not exist.                          |
| 4    | `PeriodNotEnded`     | Period still active; claim not yet allowed.          |
| 5    | `NotBeneficiary`     | Caller not registered for this period.               |
| 6    | `AlreadyClaimed`     | Caller already claimed their share.                  |
| 7    | `PeriodOverlap`      | New period ledger range conflicts with existing one. |
| 8    | `InvalidInput`       | Logically invalid parameters.                        |
| 9    | `DepositFailed`      | Token transfer from admin failed.                    |
| 10   | `Overflow`           | Arithmetic overflow (should never occur).            |
| 11   | `NoBeneficiaries`    | No beneficiaries registered for period.              |
| 12   | `UnauthorizedDepositor` | Caller is not an authorized offering contract.    |
| 13   | `PeriodClosed`          | Period ended; beneficiaries and revenue are frozen. |
| 14   | `InvariantViolation`    | Claim would exceed the period's total revenue.      |

---

## Security Assumptions & Threat Model

### Trust Model

- **Admin** is fully trusted. Compromise of the admin key allows:
  - Creating arbitrary periods (funds will be drawn from the admin's token balance).
  - Adding/removing beneficiaries before a period ends.
  - Admin key rotation is **not** implemented in this version; if needed, deploy a
    multisig admin contract as the `admin` address.

- **Beneficiaries** are untrusted beyond their registered entitlement.

- **Token contract** is trusted to behave according to the SEP-0041 standard.

### Reentrancy

Soroban's execution model is synchronous and single-threaded. State changes are
committed atomically after each top-level invocation. Cross-contract re-entrancy
is structurally impossible in Soroban.

### Arithmetic

All arithmetic uses Rust's checked operations (`checked_add`, `checked_div`).
Overflow returns `ContractError::Overflow` rather than silently wrapping.

### Front-Running

Beneficiary membership and revenue cannot change after `end_ledger`. The first
claim snapshots the equal share and remainder, and the final claimant receives
the remainder, independent of claim order.

### Griefing / DoS

- A malicious beneficiary cannot block others from claiming.
- Once the period ends, the beneficiary set cannot be changed by the admin.
- A beneficiary who never claims leaves that beneficiary's fixed share in the
  contract; there is no admin sweep in this version.

---

## Sequence Diagram

```
Admin           Contract              TokenContract
  |                |                       |
  |--initialize--->|                       |
  |                |                       |
  |--create_period(100,200,10000)--------->|
  |                |<--transfer(admin,contract,10000)--|
  |                |                       |
  |--add_beneficiary(0, B1)-------------->|
  |--add_beneficiary(0, B2)-------------->|
  |                |                       |
  ~  [ledger advances past 200]            ~
  |                |                       |
B1|--claim(0, B1)->|                       |
  |                |--transfer(contract,B1,5000)------>|
  |             share=5000                 |
  |                |                       |
B2|--claim(0, B2)->|                       |
  |                |--transfer(contract,B2,5000)------>|
  |             share=5000                 |
```

---

## Running Tests

```bash
# Run only the multi-period revenue deposit tests
cargo test -p revora-contracts -- test

# Full suite
cargo test -p revora-contracts

# With output
cargo test -p revora-contracts -- --nocapture
```

---

## Future Extensions

- **Unclaimed-share recovery** – admin recovery of shares for beneficiaries who
  never claim, with an explicit policy and delay.
- **Admin rotation** – `set_admin(new_admin)` guarded by current admin auth.
- **Vesting schedule** – per-beneficiary configurable vesting multipliers.
- **Off-chain event indexing** – emit Soroban contract events on every deposit,
  beneficiary change, and claim for external indexer consumption.