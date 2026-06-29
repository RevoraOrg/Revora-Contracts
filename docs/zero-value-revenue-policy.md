# Zero-Value Revenue Policy

## Overview

The Revora-Contracts smart contract treats zero-value deposits and zero-value reports differently:

- `deposit_revenue` rejects zero or negative amounts because token transfers must move positive value.
- `report_revenue` rejects negative amounts but allows zero so issuers can preserve an explicit on-chain audit record for a period.

## Security Assumptions

- **Deposits Require Positive Amounts:** `deposit_revenue` and `deposit_revenue_with_snapshot` require `amount > 0`.
- **Reports Reject Negatives Only:** `report_revenue` accepts `amount == 0` but rejects `amount < 0`.
- **Thresholds Still Apply:** A zero-value new report can still emit `rev_below` and no-op if the configured minimum threshold is above zero.

## Implementation Details

### Validation Logic

Revenue validation is category-specific:

```rust
match category {
    AmountValidationCategory::RevenueDeposit => {
        if amount <= 0 {
            return Err((RevoraError::InvalidAmount, symbol_short!("must_pos")));
        }
    }
    AmountValidationCategory::RevenueReport => {
        if amount < 0 {
            return Err((RevoraError::InvalidAmount, symbol_short!("no_neg")));
        }
    }
    _ => {}
}
```

### Affected Functions
- `report_revenue`: Rejects negative amounts before processing; zero is allowed
- `deposit_revenue`: Rejects invalid amounts before token transfer
- `deposit_revenue_with_snapshot`: Inherits validation from `do_deposit_revenue`

### Error Handling
- **Error Code:** `RevoraError::InvalidAmount`
- **Trigger:** `amount <= 0` for deposits, `amount < 0` for reports
- **Behavior:** Transaction reverts with error, no state changes

## Security Benefits

1. **Transfer Integrity:** Prevents empty or negative-value token transfers.
2. **Audit Completeness:** Allows explicit zero-value report periods to remain visible on-chain.
3. **Gas Efficiency:** Avoids processing invalid transfer operations.
4. **Policy Flexibility:** Lets issuers combine zero-value reports with threshold configuration when they need a no-op audit marker.

## Usage Examples

### Valid Operations
```rust
// ✅ Valid: positive amount
contract.report_revenue(issuer, namespace, token, payout_asset, 1000, period_id, false);
contract.deposit_revenue(issuer, namespace, token, payment_token, 500, period_id);
```

### Invalid Operations (Rejected)
```rust
// ✅ Valid: zero-value audit record
contract.report_revenue(issuer, namespace, token, payout_asset, 0, period_id, false);

// ❌ Invalid: negative amount  
contract.deposit_revenue(issuer, namespace, token, payment_token, -100, period_id);
// Returns: RevoraError::InvalidAmount
```

## Testing

### Test Coverage
- `report_revenue_accepts_zero_amount`
- `report_revenue_rejects_negative_amount`
- `deposit_revenue_rejects_zero_amount`
- `deposit_revenue_rejects_negative_amount`

### Edge Cases Covered
- Amount = 0 (zero)
- Amount < 0 (negative)
- Amount = 1 (minimum valid)
- Large positive amounts (still valid)

## Migration Notes

Existing integrations should distinguish between transfer flows and audit flows. Deposits still require positive amounts; reports may intentionally use zero.

## Related Components

- **Audit Summary:** Includes valid persisted report amounts, including zero
- **Event Emission:** Occurs for valid report outcomes, including zero-value reports
- **Token Transfers:** Only happen for valid positive deposit amounts
