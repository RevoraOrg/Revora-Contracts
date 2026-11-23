# Vesting Storage Migration Safety

## Why Cliff and Curve Integrity Are Critical

Vesting schedules represent a **financial commitment** from the issuer to the beneficiary. The three critical timestamps — `cliff_ts` (when tokens begin vesting), `start_ts` (when linear vesting starts accruing), and `end_ts` (when 100% is fully vested) — define the beneficiary's entitlement at any point in time. If even a single second shifts during a storage upgrade, the beneficiary could:

- Lose vested tokens they have already earned (financial loss)
- Gain tokens they should not yet have access to (protocol insolvency)
- Have their cliff reset, extending their lockup period (violation of trust)

The **curve shape** (`VestingCurve`) determines the distribution logic — Linear, Cliff, Graded step, or Step-based vesting. Corrupting this field would change the release profile and potentially give or deny access to funds.

## Storage Layout Versioning Strategy

### Version Stamp

Every contract instance stores a `StorageLayoutVersion` (u32) written during `initialize` and checked during migrations. The current version is `2`.

### Enum Stability

All storage keys (`DataKey`, `DataKey2`, `MigrationDataKey`, `VestingKey`) are `#[contracttype]`-annotated enums with XDR-stable discriminants. Adding new variants is backward-compatible; removing or reordering existing variants is **breaking** and requires a major version bump.

### Registry Enforcement

The build script (`build.rs`) loads `tools/storage_layout_schema.rs` and **validates on every build** that the registered key set matches the source enums. Any drift fails the build. Changes to storage keys must be reflected in both the schema registry and `docs/STORAGE_LAYOUT.json`.

## Migration Guarantees

1. **Deterministic Round-Trip**: Any `VestingSchedule` serialized to XDR must deserialize to an identical struct. This is verified by `test_vesting_xdr_roundtrip_*` tests for all curve variants.

2. **Storage Persistence**: Writing a `VestingSchedule` via the typed storage API and reading it back must yield byte-identical fields. Verified by `test_vesting_storage_roundtrip_*` tests.

3. **Compute Stability**: The vesting math functions `compute_vested` and `compute_claimable` must produce identical results before and after a serialization round-trip. Verified by `test_vesting_compute_functions_preserved_after_roundtrip`.

4. **Byte-Level Determinism**: Two identical `VestingSchedule` values must produce identical XDR byte sequences. Verified by `test_vesting_byte_level_determinism`.

5. **Backward Compatibility**: Schedules written by an older version of the contract (same struct layout) must be readable by the current version with zero field mutation. Verified by `test_vesting_legacy_bytes_migration_*` tests.

## Test Methodology

```
                          ┌──────────────────────────┐
                          │  Build VestingSchedule    │
                          │  fixture with known       │
                          │  cliff_ts, start_ts,      │
                          │  end_ts, curve            │
                          └─────────┬────────────────┘
                                    │
                    ┌───────────────┼───────────────┐
                    ▼               ▼               ▼
            ┌──────────────┐ ┌──────────┐ ┌──────────────┐
            │ Serialize to │ │ Write to │ │ Write to     │
            │ XDR and     │ │ typed    │ │ raw storage  │
            │ deserialize  │ │ storage  │ │ (legacy sim) │
            └──────┬───────┘ └────┬─────┘ └──────┬───────┘
                   │              │               │
                   ▼              ▼               ▼
            ┌──────────────┐ ┌──────────┐ ┌──────────────┐
            │ Compare all  │ │ Read back│ │ Read back    │
            │ fields       │ │ from     │ │ from storage │
            │ byte-for-byte│ │ storage  │ │              │
            └──────────────┘ └──────────┘ └──────────────┘
                   │              │               │
                   └──────────────┼───────────────┘
                                  ▼
                    ┌─────────────────────────┐
                    │ All fields equal?        │
                    │ cliff_ts, start_ts,     │
                    │ end_ts, curve preserved │
                    └─────────────────────────┘
```

## Edge Cases Covered

| Scenario | Test Name | Rationale |
|---|---|---|
| Zero cliff | `test_vesting_xdr_roundtrip_zero_cliff` | Cliff at t=0 is valid; must not truncate |
| Cliff == end_ts | `test_vesting_xdr_roundtrip_cliff_equals_end` | Degenerate case where vesting period is instantaneous |
| Boundary timestamps | `test_vesting_xdr_roundtrip_boundary_timestamps` | `u64::MIN`, `u64::MAX` must not overflow/truncate |
| All curve variants | `test_vesting_legacy_bytes_migration_all_curves` | Each curve type has distinct XDR encoding |
| Accelerated amounts | `test_vesting_compute_with_accelerated_after_roundtrip` | Pre-acceleration must survive serialization |
| Zero total amount | `test_vesting_legacy_bytes_migration_edge_cases` | Edge case for compute functions |
| 90-day real-world fixture | `test_vesting_migration_preserves_all_fields_integration` | Realistic production scenario |

## Security Invariants

- **No silent mutation**: Every field is asserted individually with descriptive failure messages.
- **No timestamp overflow**: Serialization uses Soroban's XDR which preserves `u64` precision.
- **Deterministic and idempotent**: The same `VestingSchedule` always produces the same bytes; reading produces the same struct.
- **No financial inconsistency**: The curve shape is verified by running `compute_vested` / `compute_claimable` at multiple timestamps before and after round-trip.

## Adding New Fields to VestingSchedule

If `VestingSchedule` gains new fields in a future upgrade:

1. Add the field after existing ones (XDR append is backward-compatible for `#[contracttype]` structs).
2. Update `tools/storage_layout_schema.rs` with the new key entry.
3. Regenerate `docs/STORAGE_LAYOUT.json`.
4. Add tests in `test_storage_layout_version.rs` covering the new field's round-trip behavior.
5. Ensure `assert_schedules_eq` includes the new field.