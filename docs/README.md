# Revora-Contracts documentation index

Top-level overview: [../README.md](../README.md). Each file below is a per-feature design or security note; `src/lib.rs` is the source of truth.

## Reference

- [error-codes.md](error-codes.md)
- [contract-reference-legacy.md](contract-reference-legacy.md) (older long-form reference; partly outdated)

## Admin, pause, freeze and governance

- [admin-rotation-safety-flow.md](admin-rotation-safety-flow.md)
- [freeze-reason-v1-event.md](freeze-reason-v1-event.md)
- [freeze-scope-clarification.md](freeze-scope-clarification.md)
- [issuer-only-blacklist-governance.md](issuer-only-blacklist-governance.md)
- [issuer-transfer-expiry.md](issuer-transfer-expiry.md)
- [multisig-duplicate-approval-guards.md](multisig-duplicate-approval-guards.md)
- [multisig-initialization-validation.md](multisig-initialization-validation.md)
- [multisig-owner-removal-safety.md](multisig-owner-removal-safety.md)
- [multisig-proposal-expiry.md](multisig-proposal-expiry.md)
- [per-offering-emergency-pause.md](per-offering-emergency-pause.md)

## Revenue, claims and distribution

- [CLOSE_PERIOD_GAS_TEST_SUMMARY.md](CLOSE_PERIOD_GAS_TEST_SUMMARY.md)
- [DEFERRED_DISTRIBUTIONS.md](DEFERRED_DISTRIBUTIONS.md)
- [audit-summary-reconciliation.md](audit-summary-reconciliation.md)
- [claim-after-cancel-policy.md](claim-after-cancel-policy.md)
- [claim-idempotency-guarantees.md](claim-idempotency-guarantees.md)
- [claimable-chunk-computation.md](claimable-chunk-computation.md)
- [close-period-preflight.md](close-period-preflight.md)
- [compute-share-decomposition-property.md](compute-share-decomposition-property.md)
- [compute-share-overflow-protection.md](compute-share-overflow-protection.md)
- [dividend-accrual-ledger.md](dividend-accrual-ledger.md)
- [fee-bps.md](fee-bps.md)
- [holder-statement-pagination.md](holder-statement-pagination.md)
- [multi-offering-payment-token-independence.md](multi-offering-payment-token-independence.md)
- [multi-offering-payment-token-security-assertions.md](multi-offering-payment-token-security-assertions.md)
- [multi-period-revenue-deposit.md](multi-period-revenue-deposit.md)
- [negative-amount-validation-matrix.md](negative-amount-validation-matrix.md)
- [payment-token-decimal-compatibility.md](payment-token-decimal-compatibility.md)
- [payment-token-locking.md](payment-token-locking.md)
- [period-amount-fuzz-notes.md](period-amount-fuzz-notes.md)
- [period-id-boundary-tests.md](period-id-boundary-tests.md)
- [period-ordering-invariants.md](period-ordering-invariants.md)
- [reconciliation-event-completeness.md](reconciliation-event-completeness.md)
- [revenue-range-chunk-query.md](revenue-range-chunk-query.md)
- [share-sum-invariant-checks.md](share-sum-invariant-checks.md)
- [snapshot-override-negative-matrix.md](snapshot-override-negative-matrix.md)
- [vesting-partial-claim-tracking.md](vesting-partial-claim-tracking.md)
- [zero-value-revenue-policy.md](zero-value-revenue-policy.md)

## Compliance, holders and offerings

- [class-priority.md](class-priority.md)
- [concentration-reporting-integrity.md](concentration-reporting-integrity.md)
- [investment-constraints.md](investment-constraints.md)
- [jurisdiction-tagging.md](jurisdiction-tagging.md)
- [lockup-schedule-cliff-taper.md](lockup-schedule-cliff-taper.md)
- [namespace-isolation-tests.md](namespace-isolation-tests.md)
- [offering-duplicate-prevention.md](offering-duplicate-prevention.md)
- [offering-pagination-stability.md](offering-pagination-stability.md)
- [tax-lot-export-event.md](tax-lot-export-event.md)
- [transfer-with-attestation.md](transfer-with-attestation.md)

## Vesting

- [token-vesting-core.md](token-vesting-core.md)
- [vesting-amendment-security.md](vesting-amendment-security.md)
- [vesting-core.md](vesting-core.md)
- [vesting-event-schema-versioning.md](vesting-event-schema-versioning.md)
- [vesting-schedule-amendment-flow.md](vesting-schedule-amendment-flow.md)
- [vesting-storage-migration-safety.md](vesting-storage-migration-safety.md)

## Events, storage and indexing

- [core-event-version-field.md](core-event-version-field.md)
- [event-sunset-table.md](event-sunset-table.md)
- [indexer-fixture-coverage.md](indexer-fixture-coverage.md)
- [merkle-proof-depth-bound.md](merkle-proof-depth-bound.md)
- [snapshot-monotonicity-replay-stress.md](snapshot-monotonicity-replay-stress.md)
- [storage-layout-json.md](storage-layout-json.md)
- [storage-stress-automation.md](storage-stress-automation.md)

## Testing, fuzzing and tooling

- [clippy-format-gate-hardening.md](clippy-format-gate-hardening.md)
- [contract-fuzz-harness.md](contract-fuzz-harness.md)
- [contracts-style.md](contracts-style.md)
- [cross-contract-transfer-failure-handling.md](cross-contract-transfer-failure-handling.md)
- [property-based-invariant-tests.md](property-based-invariant-tests.md)
- [security-assertions-module.md](security-assertions-module.md)
- [security-doc-sync.md](security-doc-sync.md)
- [structured-error-coverage-expansion.md](structured-error-coverage-expansion.md)
- [time-window-boundary-matrix.md](time-window-boundary-matrix.md)

## Other

- [contract-reference-legacy.md](contract-reference-legacy.md)

Also: [EVENT_SUNSET.json](EVENT_SUNSET.json), [EVENT_SUNSET.yaml](EVENT_SUNSET.yaml), [STORAGE_LAYOUT.json](STORAGE_LAYOUT.json). Root-level feature notes: [../AUTH_MATRIX.md](../AUTH_MATRIX.md), [../ISSUER_TRANSFER.md](../ISSUER_TRANSFER.md), [../TESTNET_MODE.md](../TESTNET_MODE.md), [../CLOSE_PERIOD_FEATURE.md](../CLOSE_PERIOD_FEATURE.md), [../REVENUE_REPORT_IDEMPOTENCY.md](../REVENUE_REPORT_IDEMPOTENCY.md).
