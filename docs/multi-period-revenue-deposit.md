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
registered **beneficiaries** may each claim their pro-rata share of that period's
deposited revenue. Deposits are now gated by an admin-managed allowlist of
authorized offering contract addresses so that only approved callers can fund a
period.

```
Admin ──deposit──► Contract ──claim──► Beneficiary₁
                              └──────► Beneficiary₂
                              └──────► Beneficiary₃
```

---

## Key Concepts

### Period

A period is a non-overlapping ledger range `[start_ledger, end_ledger]` with a fixed
`revenue_amount` of tokens deposited at creation time. Multiple periods may co-exist
as long as their ranges do not overlap.

| Field             | Type   | Description                                      |
|-------------------|--------|--------------------------------------------------|
| `id`              | `u32`  | Monotonically-assigned identifier.               |
| `start_ledger`    | `u32`  | First ledger of the period (inclusive).          |
| `end_ledger`      | `u32`  | Last ledger of the period (inclusive).           |
| `revenue_amount`  | `i128` | Total tokens deposited for this period.          |
| `claimed_amount`  | `i128` | Running total of tokens claimed so far.          |

### Beneficiary

An `Address` registered by the admin for a specific period. Beneficiaries receive
`floor(revenue_amount / beneficiary_count)` tokens when they call `claim`. Any
remainder due to integer truncation remains locked in the contract (dust).

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

**Errors:** `Unauthorized`, `PeriodNotFound`

---

### `remove_beneficiary(period_id, beneficiary) → Result<(), ContractError>`

Deregister a beneficiary. Their unclaimed share remains in the contract.

**Requires:** admin auth.

**Errors:** `Unauthorized`, `PeriodNotFound`, `NotBeneficiary`

---

### `claim(period_id, claimant) → Result<i128, ContractError>`

Claim pro-rata share of `period_id` revenue.

**Requires:** claimant auth.

**Returns:** token amount transferred.

**Errors:** `PeriodNotFound`, `PeriodNotEnded`, `NotBeneficiary`, `AlreadyClaimed`,
`NoBeneficiaries`, `Overflow`

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

---

## Security Assumptions & Threat Model

### Trust Model

- **Admin** is fully trusted. Compromise of the admin key allows:
  - Creating arbitrary periods (funds will be drawn from the admin's token balance).
  - Adding/removing beneficiaries.
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

A beneficiary cannot influence the distribution of funds. The share calculation
uses a snapshot of the beneficiary count at claim time. If an admin adds or removes
a beneficiary after the period ends but before all claims are processed, the share
sizes shift. Operators should freeze the beneficiary list before `end_ledger` in
production deployments.

### Griefing / DoS

- A malicious beneficiary cannot block others from claiming.
- An admin cannot prevent a registered beneficiary from claiming after period end
  (short of removing them, which is a privileged admin action).
- Integer dust (from floor division) is permanently locked in the contract in this
  version. A future `withdraw_dust` function callable by admin after all claims are
  finalised would reclaim this.

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

- **`withdraw_dust(period_id)`** – admin reclaims integer remainder after all
  beneficiaries have claimed.
- **Admin rotation** – `set_admin(new_admin)` guarded by current admin auth.
- **Beneficiary freeze** – lock the beneficiary list at `end_ledger` to prevent
  post-period mutations from affecting share calculations.
- **Vesting schedule** – per-beneficiary configurable vesting multipliers.
- **Off-chain event indexing** – emit Soroban contract events on every deposit,
  beneficiary change, and claim for external indexer consumption.