# Revora Contracts

Soroban (Rust) smart contract for **Revora** revenue-share offerings on Stellar. An issuer registers an offering for a token, reports and deposits periodic revenue in a payment asset, assigns holder shares, and holders claim their portion. The contract also provides compliance and safety controls (blacklist/whitelist, jurisdiction gating, freezes, disputes, multisig/governance) and event streams for off-chain indexers.

> **Deployment status:** the standalone revenue-deposit contract (`src/revenue_deposit_contract.rs`) is deployed on **Stellar testnet** (see below). The full `RevoraRevenueShare` contract in `src/lib.rs` is **not** deployed: it compiles to about 412 KB, above Stellar's 128 KiB contract-size limit. Nothing is deployed on mainnet.

- Backend: [RevoraOrg/Revora-Backend](https://github.com/RevoraOrg/Revora-Backend)
- Frontend: [RevoraOrg/Revora-Frontend](https://github.com/RevoraOrg/Revora-Frontend)

## Testnet deployment

| | |
| --- | --- |
| Network | Stellar **testnet** (not mainnet, no real funds) |
| Contract | Revenue-deposit contract (`src/revenue_deposit_contract.rs`) |
| Contract ID | `CABIJLQV4TKBHMETNIISV4OT76EDR5WXCZVJJLXHA6AEIII2JVYAHP2I` |
| Explorer | [Stellar Lab](https://lab.stellar.org/r/testnet/contract/CABIJLQV4TKBHMETNIISV4OT76EDR5WXCZVJJLXHA6AEIII2JVYAHP2I) · [stellar.expert](https://stellar.expert/explorer/testnet/contract/CABIJLQV4TKBHMETNIISV4OT76EDR5WXCZVJJLXHA6AEIII2JVYAHP2I) |
| Payment token | Native XLM Stellar Asset Contract on testnet (`CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC`) |
| Admin | `GBFYPMZJKOOAQ7UBBBRIJHYKZ4DQSS7EW7EYCZDU767E7SFE3RFF3JJJ` (throwaway testnet key) |
| State | Initialised once; a second `initialize` is rejected |

Build notes: the file in this repo does not compile as shipped. It needs `#![no_std]`, event topics passed as tuples (`publish((symbol_short!("allow_add"),), ..)`), and event symbols of at most 9 characters, so the deployed build used `allow_add` and `allow_rm`. Build with `cargo build --release --target wasm32-unknown-unknown` using `soroban-sdk 21.7.7`, then `stellar contract optimize` (about 18 KB).

Live app: see the [Revora-Frontend](https://github.com/RevoraOrg/Revora-Frontend) repo for the website link.

## Security notice: testnet mode

The contract has an admin-toggled testnet mode (`set_testnet_mode`, `is_testnet_mode`) that relaxes validations (for example revenue-share bps above 10000 and concentration enforcement). It must never be enabled on a production contract; always check `is_testnet_mode()` returns `false`. Details: [TESTNET_MODE.md](TESTNET_MODE.md).

## Repository layout

| Path | Contents |
| --- | --- |
| `src/lib.rs` | The `RevoraRevenueShare` contract (types, `RevoraError`, ~230 public entrypoints, `CONTRACT_VERSION` currently `(1, 0, 23)`) |
| `src/vesting.rs` | Separate `VestingContract` (schedules, curves, claims) |
| `src/security_assertions.rs`, `safe_math.rs`, `merkle_helpers.rs`, `tax_bucket.rs`, `milestone_signals.rs`, `revenue_deposit_contract.rs` | Supporting modules |
| `src/test_*.rs`, `src/*_tests.rs`, `tests/` | Unit, property (proptest) and JSON/layout tests; `test_snapshots/` holds Soroban snapshots |
| `src/kani_harness/` | Bounded-verification harnesses (`kani` feature, not in default CI) |
| `build.rs`, `tools/` | Verifies the storage-layout registry against source enums and generates `STORAGE_LAYOUT.json` |
| `indexer/` | Separate workspace crate `revora-indexer` (Rust, axum + sqlx/Postgres): HTTP service with `/events`, `/analytics`, `/export/csv`, `/export/json`; needs `DATABASE_URL` (see `indexer/.env.example`) |
| `scripts/` | Event-sunset table generation/checks |
| `docs/` | Per-feature design and security notes; index in [docs/README.md](docs/README.md) |

Dependencies: `soroban-sdk` 21 (with `alloc`). Toolchain: stable Rust with `rustfmt`, `clippy` and the `wasm32-unknown-unknown` target (`rust-toolchain.toml`).

## Contract surface (high level)

Entrypoints live in several `#[contractimpl]` blocks in `src/lib.rs`. Offerings are keyed by issuer, namespace (`Symbol`) and token. Authorization is per-call `require_auth` on the relevant address (issuer, holder, admin or safety role); contract errors use `RevoraError`, and `try_*` client methods return them as values. Signatures below are summarized; read `src/lib.rs` for exact parameters.

- **Setup and admin:** `initialize(admin, safety, event_only)`, `set_admin`, `get_admin`, admin rotation (`propose_admin_rotation`, `finalize_admin_rotation`, `cancel_admin_rotation`, `revoke_admin_rotation`, history getters), `get_version`, `migrate_storage` and migration hooks, `storage_layout_version`.
- **Pause and freeze:** `pause_admin` / `unpause_admin`, `hard_pause_admin`, `pause_safety` / `unpause_safety`, `is_paused`, `freeze`, `freeze_offering` / `unfreeze_offering`, `emergency_freeze_holder` / `emergency_unfreeze_holder`, freeze-reason getters.
- **Offerings:** `register_offering` (primary issuer, co-issuers and quorum, namespace, token, revenue-share bps, payout asset, supply cap, denomination metadata), `get_offering`, `list_offerings`, `get_offerings_page`, `get_offering_count`, `get_payment_token`, supply-cap and max-shares getters/setters, issuer transfer (`propose_issuer_transfer`, `propose_transfer_with_expiry`, `accept_`, `cancel_`, `reject_`, `replace_`, pending getters).
- **Revenue reporting and deposits:** `report_revenue` (with `override_existing` corrections), `report_revenue_with_attestation`, `deposit_revenue`, `deposit_revenue_with_snapshot`, period getters (`get_revenue_by_period`, `get_revenue_range`, `get_revenue_range_chunk`), `get_audit_summary`, `reconcile_audit_summary`, `repair_audit_summary`, minimum-revenue threshold, `close_period` / `close_period_dual_sig` / `preflight_close_period`, deferred distribution queue.
- **Holder shares and claims:** `set_holder_share` (nonce-protected), share classes and `convert_class`, `claim` (bounded by `max_periods`), `get_claimable`, `get_claimable_chunk`, `get_holder_accrued_unclaimed`, `get_holder_statement_page`, claim delay and report/claim windows, `simulate_distribution`, `compute_share` with `RoundingMode`.
- **Snapshots and proofs:** snapshot config, `commit_snapshot`, `apply_snapshot_shares`, `finalize_snapshot`, `verify_snapshot_proof`, `verify_merkle_proof`, `compute_attestation_digest`.
- **Compliance and transfers:** blacklist and whitelist management (single, batch, paged, attested), jurisdiction tags/allowlists with grace periods, concentration limits and reporting, investment constraints, transfer restrictions/cooldowns/lockup schedules, `estimate_transfer`, `transfer_with_attestation`, `process_ofac_attestation`, redemption requests and fulfilment with fee config.
- **Fees and markets:** platform fee (global, per asset, per offering), offering fee bps, secondary-market royalty (`pay_secondary_market_royalty`), `atomic_swap`, FX oracle and oracle chain configuration.
- **Disputes:** `open_dispute`, `resolve_dispute`, `get_dispute`, `is_dispute_freeze_active` (critical disputes halt claims until resolved).
- **Multisig and governance:** `init_multisig`, `create_proposal` / `propose_action` / `approve_action` / `execute_action`, `create_gov_proposal`, `cast_vote`, `close_gov_proposal`, `check_quorum`, meta-delegation and meta-signed actions.
- **Reporting and indexing:** `get_disclosure` / `update_disclosure`, fiscal-year and tax-year helpers, `get_indexer_fixture_topics`, `supported_event_versions`, `set_emit_v2_compat`. State changes emit indexed events (`ev_idx2`, `ev_idx3`); see [docs/event-sunset-table.md](docs/event-sunset-table.md) and `docs/EVENT_SUNSET.json`.
- **Test/faucet helpers:** `faucet_seed_holders`, `faucet_reset` and `test_*` entrypoints exist for test environments; see TESTNET_MODE.md before relying on them.

`VestingContract` (`src/vesting.rs`) exposes `vesting_register`, `accelerate_vesting`, `vesting_claim`, and getters (`get_claimed_amount`, `get_vested_amount`, `get_claimable_amount`, `get_vesting_schedules`, `vested_progress_at`); see [docs/vesting-core.md](docs/vesting-core.md).

Error codes: [docs/error-codes.md](docs/error-codes.md) (generated from the enum; source comments that point to a README error table refer to this and the legacy reference).

The previous, much longer reference (method tables, events, error codes) is kept at [docs/contract-reference-legacy.md](docs/contract-reference-legacy.md); parts of it are out of date.

## Build and test

```bash
rustup target add wasm32-unknown-unknown
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test -- --test-threads=1
cargo build --release --target wasm32-unknown-unknown   # contract WASM
```

The `npm` scripts in `package.json` are thin wrappers around `cargo build/test`. The workspace includes `indexer/`; use `-p revora-contracts` to restrict commands to the contract. Style and lint policy: [docs/contracts-style.md](docs/contracts-style.md), [docs/clippy-format-gate-hardening.md](docs/clippy-format-gate-hardening.md).

## CI

`.github/workflows/ci.yml` (push/PR to `master`) runs `cargo fmt --all -- --check` and `cargo check --lib` on stable Rust. Clippy and the test suite are not part of the current workflow.

## Deployment notes

No deployment scripts or contract IDs are tracked here. A deployer would build the WASM, deploy with the Stellar CLI, call `initialize`, and then verify `is_testnet_mode()` is `false` for any production use. Treat this as guidance, not a tested runbook. Review [AUTH_MATRIX.md](AUTH_MATRIX.md) and [docs/admin-rotation-safety-flow.md](docs/admin-rotation-safety-flow.md) first.
