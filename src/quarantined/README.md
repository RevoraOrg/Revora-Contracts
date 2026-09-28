# Quarantined test-support files

These nine files were moved out of the compilation unit on the
`fix/lib-build-recovery` branch. They are **preserved verbatim** for future
restoration, but they no longer compile as part of the crate.

> **Restored (2026-09):**
> - `test_close_period.rs` — moved back to `src/` and ported to the current
>   API (public-client preflight, `test_utils` minting, legacy `DeferredReports`
>   flush suite dropped — the live contract no longer reads that key). See
>   `src/test_close_period.rs` for the porting notes.
> - `test_deferred_priority.rs` — moved back to `src/` and ported to the
>   current API (explicit `None::<T>` generics on `initialize`, index-based
>   event lookup for the pre-`Option`-`Vec::get` pattern). See
>   `src/test_deferred_priority.rs` for the porting notes.
> - `test_faucet_seed.rs` + `test_faucet_metrics.rs` — moved back to `src/`
>   and ported. The metrics pair also surfaced a **contract regression**: the
>   merge chain dropped the `fct_mtr1` emission helper (`fb12481`, PR #676)
>   while its constant, storage counters, docs, and indexer fixture survived —
>   the contract documented an event it never emitted. The emission was
>   restored with per-window counter rollover; see
>   `src/test_faucet_metrics.rs` for details.
>
> **Related casualty (2026-09):** `src/test_indexer_fixtures.rs` was never
> quarantined, but the merge chain dropped its `mod test_indexer_fixtures;`
> declaration from `src/lib.rs` (last present in `0acd0b5`), leaving all 22
> indexer-fixture tests silently uncompiled — CI's `|| true` masked this as
> well. The declaration was restored and the file ported to the current API
> (`emit_v2_event` payload wrapping, `env.as_contract` for cost-basis seeding,
> SDK-21 `Val` comparisons via `try_into_val`).

## Why they were quarantined

The Aug-31 2026 merge chain (PRs #858–#864, in particular #863 which
accidentally replaced `src/lib.rs` with a one-line placeholder and later PRs
re-inflated it from divergent branch points) left these files referencing
contract APIs that do not exist anywhere in the restored snapshot:

| File | Missing API / breakage |
|---|---|
| `test_compute_share_invariants.rs` | `set_class_supply_cap`, per-class supply-cap client methods |
| `test_storage_layout_version.rs` | `create_migration_plan` / `migration_plan` entrypoints |
| `proptest_helpers.rs` | `TestOperation` variants predating the 11-arg `register_offering` |
| `test_quorum_check.rs` | pre-quorum-refactor client signatures |
| `test_event_indexed_v3.rs` | V3 event fixture arity drift |
| `test_snapshot_voting_weight.rs` | `Address::get` (nonexistent) snapshot lookup |
| `test_merkle_proof_depth.rs` | `Symbol`/`Val` comparisons, helper drift |
| `test_merkle_canonical_order.rs` | `Hash<32>` vs `BytesN<32>` comparisons, moved `Address` values |
| `test_accrual_reconciliation_prop.rs` | `proptest::Config::max_local_rng` (removed upstream), 0-arg fn drift |

None of these files compiled since at least commit `8c7b052`
(2026-08-31). The repository's CI workflow marks every Rust job except
`cargo fmt` with `continue-on-error` / `|| true`, so the breakage was never
surfaced by CI; it only became visible when the lib itself stopped compiling
and the whole test target had to be repaired.

## How to restore a file

1. Move it back: `git mv src/quarantined/<file>.rs src/<file>.rs`.
2. Re-declare it in `src/lib.rs`'s `mod` block (see the `NOTE:` comment there).
3. Update the test to the current client API surface (`register_offering` now
   takes 11 arguments, per-class supply caps do not exist, …).
4. Verify with `cargo test --lib <test_name> -- --test-threads=1`.
