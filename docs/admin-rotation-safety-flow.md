# Admin Rotation Safety Flow

**File:** `docs/admin-rotation-safety-flow.md`
**Issues:** [#191 — Admin Rotation Safety Flow](https://github.com/RevoraOrg/Revora-Contracts/issues/191), [#557 — Two-Phase Admin Rotation with Delay](https://github.com/RevoraOrg/Revora-Contracts/issues/557)
**Contract:** `RevoraRevenueShare` (`src/lib.rs`)
**Tests:** `src/test.rs` — multiple `admin_rotation*` modules

---

## Overview

The **Admin Rotation Safety Flow** provides a hardened two-phase mechanism for transferring the global contract admin role to a new address. It is deliberately designed to mirror the existing issuer-transfer pattern (`propose_issuer_transfer` / `accept_issuer_transfer`) so that integrators and auditors only need to understand one mental model for authority handoffs.

The flow prevents four categories of failure that a single-step `set_admin(new_admin)` call is vulnerable to:

| Threat | Single-step risk | Two-step mitigation |
|--------|-----------------|---------------------|
| Typo / wrong address | Admin locked out permanently | Pending; old admin cancels and retries |
| Griefing (attacker proposes to themselves) | Attacker takes control | Only stored admin can propose |
| Race condition | Accept fires before new admin is ready | New admin must explicitly sign acceptance |
| Flash-loan / single-block attack | Admin changes in one tx without community visibility | Mandatory configurable delay between propose and finalize |

---

## Contract Methods

### `propose_admin_rotation(new_admin: Address)`

**Auth:** Current admin must sign.

Records a `PendingAdminRotation { new_admin, proposed_at }` under `DataKey::PendingAdmin`. The `proposed_at` timestamp is the current ledger time and is used to enforce the delay in `finalize_admin_rotation`.

**Preconditions:**
- Current admin matches `DataKey::Admin`. Returns `NotInitialized` otherwise.
- No rotation is already pending. Returns `AdminRotationPending` otherwise.
- `new_admin ≠ current admin`. Returns `AdminRotationSameAddress` otherwise.
- Contract is not frozen. Returns `ContractFrozen` otherwise.

**Events:** `adm_prop(current_admin) → new_admin`

---

### `finalize_admin_rotation(new_admin: Address)`

**Auth:** `new_admin` (the proposed address) must sign.

Completes the rotation after the configured delay has elapsed.

**Flow:**
1. Reads `DataKey::PendingAdmin`; fails with `NoAdminRotationPending` if absent.
2. Verifies `new_admin == pending.new_admin`. Fails with `UnauthorizedRotationAccept` otherwise.
3. Checks configured delay: if `env.ledger().timestamp() - pending.proposed_at < delay`, fails with `AdminRotationDelayNotElapsed`.
4. Writes `new_admin` to `DataKey::Admin`.
5. Removes `DataKey::PendingAdmin`.
6. Persists `AdminRotationEntry` to the append-only log.
7. Emits events `adm_fin` and `adm_log`.

**Events:** `adm_fin(old_admin) → new_admin` and v2 `adm_log → AdminRotationEntry`

---

### `cancel_admin_rotation()`

**Auth:** Current admin must sign.

Removes `DataKey::PendingAdmin` and emits event `adm_canc`. The proposed candidate loses the ability to finalize.

**Preconditions:**
- Current admin matches `DataKey::Admin`.
- A rotation is pending.

**Events:** `adm_canc(current_admin) → cancelled_pending_address`

---

### `set_admin_rotation_delay(delay_secs: u64)`

**Auth:** Current admin must sign.

Sets the mandatory delay in seconds that must elapse between `propose_admin_rotation` and `finalize_admin_rotation`. The delay applies only to proposals made *after* it is configured. Set to 0 to disable (default).

**Events:** `adm_dly(admin) → delay_secs`

---

### `get_admin_rotation_delay() → u64`

Read-only. Returns the configured delay in seconds, or 0 if not set.

---

### `get_pending_admin_rotation() → Option<Address>`

Read-only. Returns the proposed new admin address, or `None` if no rotation is pending.

---

### `get_pending_admin_rotation_details() → Option<PendingAdminRotation>`

Read-only. Returns the full `PendingAdminRotation` struct (`new_admin` + `proposed_at`), or `None` if no rotation is pending.

---

### `get_admin() → Option<Address>`

Read-only. Returns the current admin address, or `None` if the contract has not been initialized.

---

### `get_admin_rotation_history_page(start: u32, limit: u32) → (Vec<AdminRotationEntry>, Option<u32>)`

Read-only. Returns a page of the append-only admin rotation history log. Entries are in chronological order (earliest first).

**Pagination:**
- `start`: zero-based index of the first entry to return.
- `limit`: maximum entries to return (capped at [`MAX_PAGE_LIMIT`] = 20).

**Returns:**
- `entries`: the page of [`AdminRotationEntry`] values, each containing `prior_admin`, `new_admin`, and `rotated_at` (ledger timestamp).
- `next_cursor`: `Some(next_start)` if more entries are available, `None` otherwise.

**Log bounds:**
The log retains at most [`MAX_ADMIN_ROTATION_LOG`] = 100 entries. When the limit is reached, the oldest entry is evicted (FIFO) on each new rotation.

**Auth:** None — read-only.

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
                  │   PendingAdmin = (new_admin,     │
                  │                    proposed_at)  │
                  │   Admin       = old_admin        │
                  └──────────────────────────────────┘
                  │                          │
 finalize_admin_rotation(new_admin)  cancel_admin_rotation(admin)
   (delay must be elapsed)                   │
                  │                          ▼
                  ▼               ┌──────────────────────────┐
      ┌──────────────────┐       │   IDLE (no pending)      │
      │   ROTATED        │       │   Admin = old (unchanged)│
      │   Admin = new    │       └──────────────────────────┘
      │   History logged │
      └──────────────────┘
```

---

## Storage Keys Used

| Key | Type | Description |
|-----|------|-------------|
| `DataKey::Admin` | `Address` | Authoritative admin; controls admin-gated methods |
| `DataKey::PendingAdmin` | `Address` | Proposed new admin during rotation; cleared on accept or cancel |
| `DataKey2::AdminRotationCount` | `u64` | Monotonically increasing counter of completed rotations |
| `DataKey2::AdminRotationLog(u64)` | `AdminRotationEntry` | Append-only log entry keyed by `rotation_id` (sequential) |

All keys use **persistent storage** — state survives ledger close.

---

## Events Emitted

| Event topic | Payload | When |
|-------------|---------|------|
| `adm_prop(current_admin)` | `new_admin: Address` | `propose_admin_rotation` succeeds |
| `adm_fin(old_admin)` | `new_admin: Address` | `finalize_admin_rotation` completes |
| `adm_canc(current_admin)` | `cancelled_pending: Address` | `cancel_admin_rotation` completes |
| `adm_log` (v2) | `AdminRotationEntry` | `accept_admin_rotation` persists the history entry |

---

## Error Codes

| Code | Name | Trigger |
|------|------|---------|
| `20` | `NotInitialized` | Admin not set |
| `35` | `NoAdminRotationPending` | `finalize_admin_rotation` or `cancel_admin_rotation` with nothing pending |
| `36` | `UnauthorizedRotationAccept` | Caller of `finalize_admin_rotation` is not the pending address |
| `40` | `AdminRotationSameAddress` | `propose_admin_rotation` with `new_admin == current admin` |
| `41` | `AdminRotationPending` | `propose_admin_rotation` while one is already pending |
| `58` | `AdminRotationDelayNotElapsed` | `finalize_admin_rotation` called before the configured delay elapsed |

Auth failures (wrong signer) are signaled by host panic, not `RevoraError`. Use `try_propose_admin_rotation`, `try_finalize_admin_rotation`, and `try_cancel_admin_rotation` to receive contract errors as `Result`.

---

## Security Assumptions

**1. Pending admin has zero authority until finalization.**
`DataKey::PendingAdmin` is read only inside `finalize_admin_rotation` and `cancel_admin_rotation`. No other method grants privileges based on this key.

**2. Old admin retains full authority during the pending window.**
`DataKey::Admin` is not modified until `finalize_admin_rotation` commits. The old admin can still freeze the contract, change the delay, and cancel the rotation.

**3. The two-phase flow is not bypassed by `set_admin`.**
`set_admin` (direct single-step update) is disabled while multisig is active and returns `LimitReached`. When multisig is not active, `set_admin` requires admin auth — so it is not a bypass of the rotation flow.

**4. Delay is per-network configurable.**
The admin sets the delay via `set_admin_rotation_delay`. The delay applies only to proposals made *after* configuration, so existing proposals are unaffected.

**5. Delay enforcement is based on ledger timestamp.**
The delay check uses `env.ledger().timestamp()`, which advances monotonically across transactions. An attacker cannot manipulate the clock within a single transaction.

**6. Rotation is blocked when frozen.**
All rotation methods call `require_not_frozen`. A frozen contract cannot rotate its admin, preventing a frozen-state bypass.

**7. Concentration, blacklist, and offering state is not affected by rotation.**
Admin rotation writes only to `DataKey::Admin`, `DataKey::PendingAdmin`, and the history log. All offering-level storage is unaffected.

---

## Threat Model

### Accidental typo in `new_admin`

**Scenario:** The current admin accidentally types the wrong address.

**Mitigation:** The rotation is in `PENDING` state. The old admin calls `cancel_admin_rotation` and starts over with the correct address.

---

### Griefing — attacker proposes rotation to themselves

**Scenario:** An attacker calls `propose_admin_rotation(attacker_addr)` hoping to rotate admin to themselves.

**Mitigation:** `propose_admin_rotation` requires current admin auth. The attacker's address does not match the stored admin, so the call fails.

---

### Replay attack — finalized proposal re-used

**Scenario:** An observer replays a previously successful `finalize_admin_rotation` transaction.

**Mitigation:** `DataKey::PendingAdmin` is removed atomically during finalization. The replayed call finds no pending entry and fails.

---

### Front-running — attacker intercepts a propose and finalizes before the legitimate new admin

**Scenario:** An attacker sees the `adm_prop` event and calls `finalize_admin_rotation` with their own address.

**Mitigation:** `finalize_admin_rotation` checks `new_admin == pending.new_admin`. The pending entry holds the legitimate new admin's address; the attacker's address differs, so the call fails.

---

### Single-block / flash-loan attack

**Scenario:** An attacker with temporary admin access (e.g., via a compromised key) attempts to rotate admin in the same transaction.

**Mitigation:** The mandatory delay requires a minimum time window between proposal and finalization. Even if the attacker proposes a rotation, they must wait for the delay to elapse, giving the community time to detect and respond (e.g., by freezing the contract).

---

### Social engineering — attacker convinces new admin to finalize

**Scenario:** An attacker proposes themselves as admin and convinces a naive address to sign `finalize_admin_rotation`.

**Mitigation (off-chain):** Integrators must verify the `adm_prop` event `current_admin` field matches the legitimately known admin address before finalizing. With the delay enabled, there is additional time for community review.

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

**Checking pending details (including proposal time):**

```typescript
const details = await contract.get_pending_admin_rotation_details();
if (details) {
  console.log(`New admin: ${details.new_admin}`);
  console.log(`Proposed at: ${details.proposed_at}`);
}
```

**Proposing a rotation (current admin):**

```typescript
// Step 1: Current admin signs and submits.
await contract.propose_admin_rotation({
  new_admin: newAdminAddress,
}, { signers: [currentAdminKeypair] });
```

**Finalizing a rotation (new admin, after delay):**

```typescript
// Step 2 (after delay has elapsed): New admin signs and submits.
await contract.finalize_admin_rotation({
  new_admin: newAdminKeypair.publicKey(),
}, { signers: [newAdminKeypair] });
```

**Cancelling a rotation (current admin):**

```typescript
await contract.cancel_admin_rotation({}, { signers: [currentAdminKeypair] });
```

**Configuring the delay:**

```typescript
// Set a 24-hour delay between proposal and finalization.
await contract.set_admin_rotation_delay({ delay_secs: 86400 }, { signers: [adminKeypair] });

// Read the current delay.
const delay = await contract.get_admin_rotation_delay();
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
  case 'adm_fin': {
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
  case 'adm_dly': {
    const admin = event.topic[1];
    const delay_secs = event.data;
    db.record_delay_change(admin, delay_secs, event.ledger);
    break;
  }
}
```

**On-chain history query** (instead of indexing events):

```typescript
// Read the full rotation history in pages
let cursor = 0;
const limit = 20;
let allEntries = [];

while (cursor !== null) {
  const { entries, next_cursor } = await contract.get_admin_rotation_history_page({
    start: cursor,
    limit
  });
  allEntries.push(...entries);
  cursor = next_cursor;
}
```

Each entry contains `prior_admin`, `new_admin`, and `rotated_at` (ledger timestamp).

---

## Interaction with Multisig

When the multisig is initialized via `init_multisig`, `set_admin` (direct single-step update) is disabled. The admin rotation flow (`propose_admin_rotation` / `finalize_admin_rotation`) remains available as an **alternative governance path** for individual key-based admin rotation. The multisig `SetAdmin` proposal action provides the governance-vote path.

Typical production deployment choice:

| Deployment type | Recommended admin rotation method |
|-----------------|-----------------------------------|
| Small team / single operator | `propose_admin_rotation` / `finalize_admin_rotation` with 0 delay |
| DAO / multi-party governance | Multisig `propose_action(SetAdmin)` / `approve_action` / `execute_action` |
| Production with safety window | `propose_admin_rotation` / `finalize_admin_rotation` with 24–72h delay |

---

## Testing Coverage

The following test modules cover the Admin Rotation Safety Flow. Run with:

```bash
cargo test admin_rotation
cargo test admin_rotation_history
cargo test admin_rotation_two_phase
cargo test regression
cargo test -- --nocapture  # Full suite with output
```

| Module | Count | Focus |
|--------|-------|-------|
| `admin_rotation` | 12 | Happy-path: propose, finalize, cancel, events, get_admin, chain rotations |
| `admin_rotation_auth` | 9 | Abuse paths: wrong signer, impostor propose, double-propose, wrong finalize |
| `admin_rotation_edge` | 7 | Invariants: idempotent init, pending cleared, coexistence with other state |
| `admin_rotation_integration` | 6 | End-to-end: new admin exercises authority, five-admin chain, freeze interaction |
| `regression` (rotation) | 5 | Double-accept, stale-cancel, same-address, impostor, frozen-contract |
| `admin_rotation_history` | 14 | History log: persistence, pagination, eviction, reverts, events |

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

# Admin rotation history tests only
cargo test admin_rotation_history

# Regression tests only
cargo test regression

# Coverage report
cargo tarpaulin --out Html --output-dir coverage
```

---

## Commit Reference

```
feat: two-phase admin rotation with delay

- Rename accept_admin_rotation → finalize_admin_rotation
- Add PendingAdminRotation struct (new_admin + proposed_at)
- Add mandatory delay enforcement in finalize_admin_rotation
- Add set_admin_rotation_delay / get_admin_rotation_delay
- Add get_pending_admin_rotation_details read helper
- Add AdminRotationDelayNotElapsed error code (58)
- Add DataKey2::AdminRotationDelay storage key
- Add EVENT_ADMIN_FINALIZE (adm_fin) and EVENT_ADMIN_ROTATION_DELAY_SET (adm_dly)
- Bump STORAGE_LAYOUT_VERSION to 3
- 14 dedicated delay tests (boundary, rejection, chained, auth)
- Update security assumptions and threat model for delay
- Document integration guide for two-phase flow

Closes #557
```
