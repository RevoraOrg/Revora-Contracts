# Gas Characterization Automation

## Scope
This implementation is contract-only and lives in:
- `src/lib.rs`
- `src/test.rs`

## What Was Implemented

### 1) Gas characterization model
The contract now defines:
- `GasOperationType`
- `GasMeasurement`
- `GasStats`
- `GasThresholds`
- `GasCharacterizationReport`

Gas storage is isolated under a dedicated `GasDataKey` enum to avoid coupling with business storage keys.

### 2) Admin-controlled configuration
Admin-only functions:
- `set_gas_characterization_enabled`
- `set_gas_max_measurements`
- `set_gas_thresholds`
- `clear_gas_characterization_data`

Read/query functions:
- `get_gas_stats`
- `get_gas_thresholds`
- `get_gas_measurements`
- `get_total_gas_measurements`
- `is_gas_characterization_enabled`
- `check_gas_thresholds`
- `generate_gas_report`

### 3) Automatic instrumentation
Instrumentation is wired into the following operations:
- `register_offering`
- `report_revenue`
- `deposit_revenue`
- `claim`
- `set_holder_share`
- `blacklist_add`
- `blacklist_remove`
- `get_offerings_page`
- `get_pending_periods`
- `simulate_distribution`

### 4) Bounded storage and deterministic behavior
- Measurements per operation are capped (`set_gas_max_measurements`, default = 100).
- Oldest measurements are pruned first.
- Statistics are updated incrementally in O(1) per measurement.
- Report generation iterates a fixed list of operation types.

## Security Assumptions

1. **No privileged config without admin auth**
   All state-changing gas config methods verify stored admin and require auth.

2. **Gas recording can be disabled**
   If characterization is disabled, instrumentation is a no-op.

3. **DoS/storage abuse control**
   Per-operation measurement history is bounded and pruned.

4. **Deterministic measurement source**
   The implementation records deterministic estimated operation cost values (not host-metered runtime gas). This is intentional for repeatable characterization and regression detection.

## Abuse/Failure Paths Covered
- Unauthorized gas-config calls return `NotAuthorized`.
- Invalid measurement limits return `GasCharacterizationError`.
- Invalid threshold configurations (zero values, warning > critical, over max gas) return `GasCharacterizationError`.
- Monitoring-disabled thresholds do not trigger warning/critical flags.
- Clearing gas data removes measurements, stats, thresholds, and counters.

## Tests Added/Updated
`src/test.rs` contains gas characterization tests covering:
- enable/disable controls
- max-measurement bounds and pruning
- thresholds set/get and threshold checking
- threshold invalid-input rejection and auth boundary checks
- measurement recording + stats aggregation
- newest-first measurement retrieval with explicit limit behavior
- report generation
- disabled-mode no-op behavior
- clear-data behavior
- event emission (`gas_meas`, `gas_rep`)

## Notes
- Public report API is `generate_gas_report`.
- The implementation includes developer-focused inline comments around storage bounds, instrumentation, and threshold logic.
