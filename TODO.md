Property-Based Invariant Tests - feature/blackboxai/property-tests

Status: 0/6 Complete

## Breakdown from Approved Plan

### 1. Enhance proptest_helpers.rs [ ]
- [ ] Complete TestOperation enum/strategies (register/report/deposit/claim/blacklist/multisig/pause)
- [ ] arb_valid_sequence generator (invariant-preserving sequences)
- [ ] Validate: cargo test proptest_helpers

### 2. Hardened src/test.rs Properties [x]
- [x] check_invariants_enhanced oracle + 7 properties (period/pagination/blacklist/concentration/multisig/pause/random)


- [ ] check_invariants oracle: payout conservation/blacklist/concentration/pause/multisig/pagination
- [ ] prop_period_ordering (strictly increasing)
- [ ] prop_blacklist_enforcement (claims=0)
- [ ] prop_concentration_limits (enforce blocks)
- [ ] prop_pagination_stability (deterministic register→paginate)
- [ ] prop_multisig_threshold (below threshold fails)
- [ ] prop_pause_safety (mutations panic post-pause)
- [ ] prop_random_operations (full sequences, seeds/shrinking)
- [ ] Validate: cargo test prop_

### 3. src/lib.rs Helpers (views only) [ ]
- [ ] total_claimed_for_holder(issuer, holder) → oracle
- [ ] NO mutations

### 4. docs/property-based-invariant-tests.md [ ]
- [ ] Update pass rates + minimal seeds

### 5. Validation [ ]
- [ ] cargo test --lib (100%)
- [ ] cargo clippy --fix
- [ ] cargo test prop_ -- --cases 1000 (stress)
- [ ] Repro: RUST_LOG=proptest=trace cargo test prop_period_ordering --exact 1

### 6. Git + PR [ ]
- [ ] git checkout -b blackboxai/property-tests
- [ ] Commit changes
- [ ] gh pr create

## Next Step
**Step 1: Enhance proptest_helpers.rs → cargo test proptest_helpers**

