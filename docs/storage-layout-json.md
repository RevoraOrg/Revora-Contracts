# Storage Layout JSON

`docs/STORAGE_LAYOUT.json` is the machine-readable inventory of persisted contract keys.

## Source of truth

The registry lives in `tools/storage_layout_schema.rs`.

- `build.rs` loads that registry on every build.
- The build script validates that the registry still matches the storage-key enums in:
  - `src/lib.rs`
  - `src/revenue_deposit_contract.rs`
  - `src/vesting.rs`
- A targeted CI step runs `cargo test storage_layout_json_matches_checked_in_docs -- --exact --test-threads=1` so checked-in docs drift fails fast.

## Security notes

- The generator is offline-only and reads local source files; it does not execute untrusted input.
- Drift detection blocks undocumented key additions, which reduces migration and indexer blind spots.
- The JSON is deterministic and sorted by key so reviews stay small and tooling can diff safely.

## Regenerating

Update `tools/storage_layout_schema.rs`, then regenerate `docs/STORAGE_LAYOUT.json` from the same registry before committing.
