# Period and Amount Fuzz Test Notes

## Scope
- Entry point tested: `report_revenue(issuer, token, amount, period_id)`
- Input axes fuzzed: `amount: i128`, `period_id: u64`

## Tested ranges
- `amount` boundaries: `i128::MIN`, `i128::MIN + 1`, `-1`, `0`, `1`, `i128::MAX - 1`, `i128::MAX`
- `period_id` boundaries: `0`, `1`, `2`, `10_000`, `u64::MAX - 1`, `u64::MAX`
- Deterministic pseudo-random sweep: 512 generated (`amount`, `period_id`) pairs, with periodic forced boundary injections.

## Observed behavior
- No panics occurred across boundary matrix or deterministic fuzz sweep.
- No assertion failures occurred in the tests.
- `report_revenue` currently accepts full `i128` and `u64` domains without validation beyond `issuer.require_auth()`.

## Input validation implications
- Negative `amount` values are accepted and emitted in events.
- `period_id == 0` and `period_id == u64::MAX` are accepted and emitted in events.
- If business rules require stricter semantics (for example positive amount or non-zero period), those checks must be added in contract logic.
