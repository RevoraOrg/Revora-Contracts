# Revora-Contracts

This repository contains a minimal Soroban contract that implements revenue-share eventing and now includes a global emergency pause mechanism.

## Overview

The emergency pause feature allows a designated `admin` or an optional `safety` role to temporarily disable state-changing operations (such as registering offerings or reporting revenue) in case a bug or security incident is detected.

This README documents the pause semantics, role usage, recovery steps, and test/CI commands.

## Pause semantics

- The contract stores three persistent keys: `admin`, `safety` (optional), and `paused`.
- When `paused` is true, state-modifying entrypoints immediately revert (panic):
  - `register_offering(...)`
  - `report_revenue(...)`
- Read-only methods and event inspection remain available while paused.
- Events are emitted for `initialize`, `pause`, and `unpause` so external monitors can detect toggles.

## APIs 

Use the generated client (`RevoraRevenueShareClient`) from the Soroban SDK to call these contract methods.

- `initialize(admin: Address, safety: Option<Address>)` — set the admin and optional safety role (callable once).
- `pause_admin(admin: Address)` / `unpause_admin(admin: Address)` — admin toggles pause; `admin` must sign the transaction.
- `pause_safety(safety: Address)` / `unpause_safety(safety: Address)` — safety role toggles pause; `safety` must sign.
- `is_paused()` — view that returns current paused state.

Note: These methods require the caller to provide the role address as parameter and that address must call with `require_auth()` (i.e., sign the tx). This makes authorization explicit in tests and enforced on-chain.

## Usage example (tests / SDK client)

Example (pseudo-code using generated client in tests):

```rust
// initialize
client.initialize(&admin_addr, &None::<Address>);

// pause as admin
client.pause_admin(&admin_addr);

// attempts to change state will panic
client.register_offering(&issuer, &token, &1_000); // panics while paused

// unpause
client.unpause_admin(&admin_addr);
```

## Tests & CI

Run these locally to reproduce CI checks:

```bash
cd Revora-Contracts
cargo fmt
cargo clippy -- -D warnings
cargo test
```

Current test coverage includes unit tests validating:
- Event emission for register/report
- Pause blocking `register_offering` and `report_revenue`
- Pause/unpause idempotence and event emission

Aim: maintain >=95% coverage; add more tests as features expand.

## Recovery & Security notes

- Authorized roles: `admin` (required) and `safety` (optional). Both are single addresses persisted in contract instance storage.
- Recovery: Investigate incident; authorized role calls `unpause_*` to resume.
- Key compromise risk: If `admin` or `safety` keys are compromised an attacker could pause/unpause or prevent recovery. Recommended mitigations:
  - Use an on-chain multisig or governance-controlled account as `admin`.
  - Monitor pause/unpause events off-chain and trigger alerts.
  - Consider adding multi-signer unpause, time-locks, or upgradeable governance in a follow-up change.
