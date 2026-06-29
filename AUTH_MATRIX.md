# Revora Revenue Share Contract - Auth Matrix

This document outlines the authentication requirements for all externally callable methods in the `RevoraRevenueShare` contract.

## Role Definitions

- **Admin**: The contract administrator, capable of pausing/unpausing and managing critical parameters. Set during initialization.
- **Safety**: An optional safety guardian, capable of soft-pausing the contract in emergencies. Cannot escalate to HardPaused.
- **Issuer**: The entity creating and managing an offering (e.g., reporting revenue). Identified by address.
- **Holder**: An investor holding the offering token, capable of claiming revenue.
- **Any**: Any caller (public access), though logic may still restrict actions based on state.

## Two-Tier Pause State

The contract uses a three-value `PauseState` enum instead of a binary flag:

| State | Storage value | reports / deposits | `claim` |
| :--- | :--- | :--- | :--- |
| `NotPaused` | 0 | ✓ allowed | ✓ allowed |
| `SoftPaused` | 1 | ✗ `ContractPaused` | ✓ allowed |
| `HardPaused` | 2 | ✗ `ContractPaused` | ✗ `ContractPaused` |

**Design rationale:** During incident response operators need to halt new deposits and reports while still allowing investors to withdraw their already-claimable funds. `SoftPaused` provides this window. `HardPaused` is reserved for critical exploits where all state changes must be frozen.

### Pause Escalation Matrix

```
NotPaused ──pause_admin──────────────► SoftPaused
NotPaused ──hard_pause_admin─────────► HardPaused
SoftPaused ──hard_pause_admin────────► HardPaused   (escalation, admin only)
HardPaused ──pause_admin─────────────► SoftPaused   (de-escalation, admin only)
SoftPaused ──unpause_admin/safety────► NotPaused
HardPaused ──unpause_admin───────────► NotPaused
```

**Security constraint:** The safety role is capped at `SoftPaused`. It cannot call `hard_pause_admin` and therefore cannot strand holder funds. Only the admin can reach `HardPaused`.

### Events

Every pause/unpause call emits two events:
- Legacy `paused` / `unpaused` — for backward-compatible consumers.
- Versioned `paused2` — carries the `PauseState` tier as payload, enabling indexers to distinguish tiers.

## Method Authorization Table

| Method | Required Auth | Logic Check | Notes |
| :--- | :--- | :--- | :--- |
| `initialize` | None (public) | Checks `!has_admin` | Can only be called once to set admin. |
| `pause_admin` | `caller` | `caller == admin` | Sets **SoftPaused**. Admin only. |
| `unpause_admin` | `caller` | `caller == admin` | Sets **NotPaused**. Admin only. Works from any tier. |
| `hard_pause_admin` | `caller` | `caller == admin` | Sets **HardPaused**. Admin only. Blocks `claim`. |
| `pause_safety` | `caller` | `caller == safety` | Sets **SoftPaused**. Safety guardian only. Cannot reach HardPaused. |
| `unpause_safety` | `caller` | `caller == safety` | Sets **NotPaused**. Safety guardian only. |
| `register_offering` | `issuer` | None | Registers a new offering. Issuer must sign. |
| `report_revenue` | `issuer` | `current_issuer == issuer` | Only the registered issuer can report revenue. |
| `blacklist_add` | `issuer` or `admin` | `current_issuer == issuer` when issuer path | Adds investor to blacklist. |
| `blacklist_remove` | `issuer` or `admin` | `current_issuer == issuer` when issuer path | Removes investor from blacklist. |
| `deposit_revenue` | `issuer` | `current_issuer == issuer` | Only issuer can deposit. |
| `claim` | `holder` | None | Holder claims their share. |
| `propose_issuer_transfer` | `current_issuer` | None | Current issuer proposes transfer. |
| `cancel_issuer_transfer` | `current_issuer` | None | Current issuer cancels transfer. |
| `accept_issuer_transfer` | `new_issuer` | None | New issuer accepts transfer. |
| `freeze` | `admin` | None | Admin freezes contract permanently. |
| `update_admin` | `admin` | None | Admin updates admin address. |
| `set_safety` | `admin` | None | Admin sets safety address. |
| `set_concentration_limit` | `issuer` | `current_issuer == issuer` | Issuer sets concentration limit. |
| `set_rounding_mode` | `issuer` | `current_issuer == issuer` | Issuer sets rounding mode. |
| `set_min_revenue_threshold` | `issuer` | `current_issuer == issuer` | Issuer sets min revenue threshold. |
| `set_holder_share` | `issuer` | `current_issuer == issuer` | Issuer sets holder share. |
| `set_claim_delay` | `issuer` | `current_issuer == issuer` | Issuer sets claim delay. |
| `set_offering_metadata` | `issuer` | `current_issuer == issuer` | Issuer sets metadata. |

## Identified Issues

- No outstanding auth vulnerabilities identified in blacklist operations; they now require issuer or admin.

## Additional Public Methods (Read-Only)

- `is_paused` – no auth; returns `true` for both `SoftPaused` and `HardPaused` (backward-compatible binary signal)
- `get_pause_state` – no auth; returns the exact `PauseState` tier (`NotPaused` / `SoftPaused` / `HardPaused`)
- `get_offering`, `list_offerings`, `get_offering_count`, `get_offerings_page` – no auth
- `get_concentration_limit`, `get_current_concentration` – no auth
- `get_rounding_mode` – no auth
- `get_min_revenue_threshold` – no auth
- `get_holder_share` – no auth
- `get_pending_periods`, `get_claimable` – no auth
- `get_period_count` – no auth
- `get_pending_issuer_transfer` – no auth
- `is_frozen` – no auth
- `get_offering_metadata` – no auth
- `is_testnet_mode` – no auth
- `get_platform_fee`, `calculate_platform_fee` – no auth

## Test Coverage

The `test_auth` module contains negative tests for:
-   Admin/Safety pause functions (unauthorized access)
-   Issuer resource management (reporting revenue, settings)
-   Issuer transfer workflow
-   Blacklist operations

All tests use `mock_all_auths` where appropriate to isolate logic checks, or explicit auth failures where necessary.
