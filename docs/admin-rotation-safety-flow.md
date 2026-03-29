# Admin Rotation Safety Flow

**File:** `docs/admin-rotation-safety-flow.md`
**Issue:** [#191 — Implement Admin Rotation Safety Flow](https://github.com/RevoraOrg/Revora-Contracts/issues/191)
**Branch:** `feature/contracts-059-admin-rotation-safety-flow`
**Contract:** `RevoraRevenueShare` (`src/lib.rs`)
**Tests:** `src/test.rs` — `mod admin_rotation`, `mod admin_rotation_auth`, `mod admin_rotation_edge`, `mod admin_rotation_integration`, `mod regression`

---

## Overview

The **Admin Rotation Safety Flow** provides a hardened two-step mechanism for transferring the global contract admin role to a new address. It is deliberately designed to mirror the existing issuer-transfer pattern (`propose_issuer_transfer` / `accept_issuer_transfer`) so that integrators and auditors only need to understand one mental model for authority handoffs.

The flow prevents three categories of failure that a single-step `set_admin(new_admin)` call is vulnerable to:

| Threat | Single-step risk | Two-step mitigation |
|--------|-----------------|---------------------|
| Typo / wrong address | Admin locked out permanently | Pending; old admin cancels and retries |
| Griefing (attacker proposes to themselves) | Attacker takes control | Only stored admin can propose |
| Race condition | Accept fires before new admin is ready | New admin must explicitly sign acceptance |

---

## Contract Methods

### `propose_admin_rotation(current_admin: Address, new_admin: Address)`

**Auth:** `current_admin` must sign.

Writes `new_admin` to `DataKey::PendingAdmin`. Emits event `adm_prop`.

**Preconditions:**

- `current_admin` matches `DataKey::Admin` (stored admin). Returns `AdminNotInitialized` otherwise.
- No rotation is already pending. Returns `AdminRotationPending` otherwise.
- `new_admin ≠ current_admin`. Returns `AdminRotationSameAddress` otherwise.
- Contract is not frozen. Panics with `ContractFrozen` otherwise.

---

### `accept_admin_rotation(new_admin: Address)`

**Auth:** `new_admin` (the proposed address) must sign.

Atomically:

1. Reads `DataKey::PendingAdmin`; fails with `NoAdminRotationPending` if absent.
2. Verifies `new_admin == pending`. Fails with `UnauthorizedRotationAccept` otherwise.
3. Writes `new_admin` to `DataKey::Admin`.
4. Removes `DataKey::PendingAdmin`.
5. Emits event `adm_acc`.

After this call, `new_admin` is the fully authoritative admin. The old admin has zero remaining authority.

---

### `cancel_admin_rotation(current_admin: Address)`

**Auth:** `current_admin` (the current stored admin) must sign.

Removes `DataKey::PendingAdmin` and emits event `adm_canc`. The proposed candidate loses the ability to accept.

**Preconditions:**

- `current_admin` matches `DataKey::Admin`. Returns `AdminNotInitialized` otherwise.
- A rotation is pending. Returns `NoAdminRotationPending` otherwise.

---

### `get_pending_admin_rotation() → Option<Address>`

Read-only. Returns the proposed new admin address, or `None` if no rotation is pending.

---

### `get_admin() → Option<Address>`

Read-only. Returns the current admin address, or `None` if the contract has not been initialized.

---

## State Machine

```
                  ┌──────────────────────────────────┐
                  │         IDLE (no pending)         │
                  └──────────────────────────────────┘
                             │
             propose_admin_rotation(admin, new_admin)
                             │
                             ▼
                  ┌──────────────────────────────────┐
                  │   PENDING                        │
                  │   PendingAdmin = new_admin       │
                  │   Admin       = old_admin        │
                  └──────────────────────────────────┘
                  │                          │
 accept_admin_rotation(new_admin)   cancel_admin_rotation(admin)
                  │                          │
                  ▼                          ▼
      ┌──────────────────┐      ┌──────────────────────────┐
      │   ROTATED        │      │   IDLE (no pending)      │
      │   Admin = new    │      │   Admin = old (unchanged)│
      └──────────────────┘      └──────────────────────────┘
```

---

## Storage Keys Used

| Key | Type | Description |
|-----|------|-------------|
| `DataKey::Admin` | `Address` | Authoritative admin; controls admin-gated methods |
| `DataKey::PendingAdmin` | `Address` | Proposed new admin during rotation; cleared on accept or cancel |

Both keys use **persistent storage** — state survives ledger close.

---

## Events Emitted

| Event topic | Payload | When |
|-------------|---------|------|
| `adm_prop(current_admin)` | `new_admin: Address` | `propose_admin_rotation` succeeds |
| `adm_acc(old_admin)` | `new_admin: Address` | `accept_admin_rotation` completes |
| `adm_canc(current_admin)` | `cancelled_pending: Address` | `cancel_admin_rotation` completes |

---

## Error Codes

| Code | Name | Trigger |
|------|------|---------|
| `20` | `AdminRotationPending` | `propose_admin_rotation` called while one is already pending |
| `21` | `NoAdminRotationPending` | `accept_admin_rotation` or `cancel_admin_rotation` called with nothing pending |
| `22` | `UnauthorizedRotationAccept` | Caller of `accept_admin_rotation` is not the pending address |
| `23` | `AdminNotInitialized` | `propose_admin_rotation` / `cancel_admin_rotation` called with wrong `current_admin` parameter |
| `24` | `AdminRotationSameAddress` | `propose_admin_rotation` called with `new_admin == current_admin` |
| `25` | `ContractFrozen` | Any state-changing call while contract is frozen |

Auth failures (wrong signer) are signaled by host panic, not `RevoraError`. Use `try_propose_admin_rotation`, `try_accept_admin_rotation`, and `try_cancel_admin_rotation` to receive contract errors as `Result`.

---

## Security Assumptions

**1. Pending admin has zero authority until acceptance.**
`DataKey::PendingAdmin` is read only inside `accept_admin_rotation`. No other method grants privileges based on this key. An attacker who learns the pending admin address gains nothing until they also control its signing key.

**2. Old admin retains full authority during the pending window.**
`DataKey::Admin` is not modified until `accept_admin_rotation` commits. The old admin can still freeze the contract, toggle testnet mode, and cancel the rotation.

**3. The two-step flow is not bypassed by `set_admin`.**
`set_admin` (direct single-step update) is disabled while multisig is active and returns `LimitReached`. When multisig is not active, `set_admin` requires admin auth — so it is not a bypass of the rotation flow, merely a parallel admin-controlled path for non-critical deployments.

**4. No time-lock or expiry.**
Proposals do not expire. A pending rotation remains until the old admin cancels or the new admin accepts. For production deployments requiring expiry, wrap the contract in an off-chain orchestrator that monitors the `adm_prop` event and calls `cancel_admin_rotation` after a deadline.

**5. Rotation is blocked when frozen.**
All three rotation methods call `require_not_frozen`. A frozen contract cannot rotate its admin, preventing a frozen-state bypass where an attacker rotating admin then unfreezes.

**6. Concentration, blacklist, and offering state is not affected by rotation.**
Admin rotation writes only to `DataKey::Admin` and `DataKey::PendingAdmin`. All offering-level, blacklist, and audit storage is keyed by issuer/token, not by admin, and remains unchanged through a rotation.

---

## Threat Model

### Accidental typo in `new_admin`

**Scenario:** The current admin accidentally types the wrong address.

**Mitigation:** The rotation is in `PENDING` state. The old admin calls `cancel_admin_rotation` and starts over with the correct address. No admin access is lost.

---

### Griefing — attacker proposes rotation to themselves

**Scenario:** An attacker calls `propose_admin_rotation(attacker_addr, attacker_addr)` hoping to rotate admin to themselves.

**Mitigation:** `propose_admin_rotation` verifies that `current_admin == DataKey::Admin`. The attacker's address does not match the stored admin, so the call fails with `AdminNotInitialized` before writing anything.

---

### Replay attack — accepted proposal re-used

**Scenario:** An observer replays a previously successful `accept_admin_rotation` transaction.

**Mitigation:** `DataKey::PendingAdmin` is removed atomically during acceptance. The replayed call finds no pending entry and fails with `NoAdminRotationPending`.

---

### Front-running — attacker intercepts a propose and accepts before the legitimate new admin

**Scenario:** An attacker sees the `adm_prop` event and calls `accept_admin_rotation` with their own address.

**Mitigation:** `accept_admin_rotation` checks `new_admin == DataKey::PendingAdmin`. The pending entry holds the legitimate new admin's address; the attacker's address differs, so the call fails with `UnauthorizedRotationAccept`.

---

### Social engineering — attacker convinces new admin to accept then claims the role

**Scenario:** An attacker proposes themselves as admin and convinces a naive address to sign `accept_admin_rotation`. (Not a contract vulnerability — requires key compromise or social attack on the new admin.)

**Mitigation (off-chain):** Integrators must verify the `adm_prop` event `current_admin` field matches the legitimately known admin address before accepting any rotation request. Treat `adm_prop` events from unexpected senders as suspicious.

---

## Integration Guide

### For issuers and integrators

**Checking if a rotation is pending:**

```typescript
const pending = await contract.get_pending_admin_rotation();
if (pending) {
  console.log(`Rotation pending → ${pending}`);
}
```

**Proposing a rotation (current admin):**

```typescript
// Step 1: Current admin signs and submits.
await contract.propose_admin_rotation({
  current_admin: currentAdminKeypair.publicKey(),
  new_admin: newAdminAddress,
}, { signers: [currentAdminKeypair] });
```

**Accepting a rotation (new admin):**

```typescript
// Step 2: New admin signs and submits.
await contract.accept_admin_rotation({
  new_admin: newAdminKeypair.publicKey(),
}, { signers: [newAdminKeypair] });
```

**Cancelling a rotation (current admin):**

```typescript
await contract.cancel_admin_rotation({
  current_admin: currentAdminKeypair.publicKey(),
}, { signers: [currentAdminKeypair] });
```

---

### For off-chain monitoring / indexers

Listen for these events to build a rotation audit trail:

```typescript
switch (event.topic[0]) {
  case 'adm_prop': {
    const current_admin = event.topic[1];
    const new_admin = event.data;
    db.insert_rotation_proposal(current_admin, new_admin, event.ledger);
    break;
  }
  case 'adm_acc': {
    const old_admin = event.topic[1];
    const new_admin = event.data;
    db.record_rotation_complete(old_admin, new_admin, event.ledger);
    break;
  }
  case 'adm_canc': {
    const current_admin = event.topic[1];
    const cancelled_pending = event.data;
    db.record_rotation_cancelled(current_admin, cancelled_pending, event.ledger);
    break;
  }
}
```

---

## Interaction with Multisig

When the multisig is initialized via `init_multisig`, `set_admin` (direct single-step update) is disabled. The admin rotation flow (`propose_admin_rotation` / `accept_admin_rotation`) remains available as an **alternative governance path** for individual key-based admin rotation. The multisig `SetAdmin` proposal action provides the governance-vote path.

Typical production deployment choice:

| Deployment type | Recommended admin rotation method |
|-----------------|-----------------------------------|
| Small team / single operator | `propose_admin_rotation` / `accept_admin_rotation` |
| DAO / multi-party governance | Multisig `propose_action(SetAdmin)` / `approve_action` / `execute_action` |

---

## Testing Coverage

The following test modules cover the Admin Rotation Safety Flow. Run with:

```bash
cargo test admin_rotation
cargo test regression
cargo test -- --nocapture  # Full suite with output
```

| Module | Count | Focus |
|--------|-------|-------|
| `admin_rotation` | 12 | Happy-path: propose, accept, cancel, events, get_admin, chain rotations |
| `admin_rotation_auth` | 9 | Abuse paths: wrong signer, impostor propose, double-propose, wrong accept |
| `admin_rotation_edge` | 7 | Invariants: idempotent init, pending cleared, coexistence with other state |
| `admin_rotation_integration` | 6 | End-to-end: new admin exercises authority, five-admin chain, freeze interaction |
| `regression` (rotation) | 5 | Double-accept, stale-cancel, same-address, impostor, frozen-contract |

**Minimum required coverage:** 95% (validated via `cargo tarpaulin`).

---

## Build and Test

```bash
# Format
cargo fmt --all -- --check

# Lint
cargo clippy --all-targets -- -D warnings

# Build
cargo build --release

# Full test suite
cargo test

# Admin rotation tests only
cargo test admin_rotation

# Regression tests only
cargo test regression

# Coverage report
cargo tarpaulin --out Html --output-dir coverage
```

---

## Commit Reference

```
feat: implement admin-rotation-safety-flow

- Add propose_admin_rotation / accept_admin_rotation / cancel_admin_rotation
- Add get_pending_admin_rotation and get_admin read helpers
- Add DataKey::PendingAdmin persistent storage key
- Add RevoraError variants: AdminRotationPending, NoAdminRotationPending,
  UnauthorizedRotationAccept, AdminNotInitialized, AdminRotationSameAddress
- Emit adm_prop / adm_acc / adm_canc events
- Block all rotation methods when contract is frozen
- 34 dedicated tests across 5 test modules (happy path, auth, edge,
  integration, regression)
- Document security assumptions, threat model, and integration guide
  in docs/admin-rotation-safety-flow.md

Closes #191
```