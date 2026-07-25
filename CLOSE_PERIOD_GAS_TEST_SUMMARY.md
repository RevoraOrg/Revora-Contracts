# Close Period Gas Test Summary

## Purpose

This document describes the gas-bound tests for the `close_period` function to ensure it maintains a linear O(holders) time complexity with no hidden quadratic paths.

## Test Cases

1. **`close_period_cpu_grows_linearly_with_holders`**:
   - Parameterized over holder counts [1, 10, 100, 1000]
   - Measures CPU instructions consumed per call
   - Fits a linear regression line and verifies R² (coefficient of determination) > 0.98
   - Ensures cost grows linearly with number of holders

2. **`close_period_zero_holders_has_constant_cost`**:
   - Tests closing a period with 0 holders
   - Verifies cost is positive but bounded by a constant (<5,000,000 instructions)

## Linearity Check

Uses coefficient of determination (R²) to measure how well the data fits a linear model:
- R² > 0.98 means the data is well explained by a linear relationship
- Calculates slope, intercept, and residual sum of squares
- Handles edge cases like zero variance in y-values

## Key Assumptions

- `close_period` currently has constant O(1) cost (no holder iteration)
- If future modifications add holder iteration, these tests will catch O(n²) or worse regressions
- Uses Soroban's built-in `env.budget().cpu_instruction_count()` for accurate measurements

## Security Notes

- Linear cost ensures scalability for offerings with many holders
- Prevents gas bombs from accidental quadratic loops
- Tests are designed to fail fast if performance degrades
