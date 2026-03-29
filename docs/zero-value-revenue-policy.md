# Zero-Value Revenue Policy

## Overview

The Revora-Contracts smart contract implements a **Zero-Value Revenue Policy** that rejects revenue reports and deposits with zero or negative amounts. This ensures that all recorded revenue represents meaningful positive value, preventing spam reports and maintaining audit trail integrity.

## Security Assumptions

- **Positive Amounts Required:** All revenue operations (reports and deposits) must specify amounts > 0.
- **Invalid Amount Rejection:** Amounts ≤ 0 trigger `RevoraError::InvalidAmount` and prevent the operation.
- **Consistent Validation:** Policy applies uniformly to both `report_revenue` and `deposit_revenue` functions.

## Implementation Details

### Validation Logic

Both `report_revenue` and `deposit_revenue` include validation:

```rust
// Zero-value revenue policy: reject zero or negative amounts
if amount <= 0 {
    return Err(RevoraError::InvalidAmount);
}
```

### Affected Functions
- `report_revenue`: Rejects invalid amounts before processing
- `deposit_revenue`: Rejects invalid amounts before token transfer
- `deposit_revenue_with_snapshot`: Inherits validation from `do_deposit_revenue`

### Error Handling
- **Error Code:** `RevoraError::InvalidAmount`
- **Trigger:** `amount <= 0`
- **Behavior:** Transaction reverts with error, no state changes

## Security Benefits

1. **Prevents Spam:** Eliminates meaningless zero-value reports that could clutter audit trails.
2. **Data Integrity:** Ensures all revenue records represent actual positive value.
3. **Gas Efficiency:** Avoids processing invalid operations that would waste resources.
4. **Audit Clarity:** Maintains clean audit summaries with only meaningful revenue data.

## Usage Examples

### Valid Operations
```rust
// ✅ Valid: positive amount
contract.report_revenue(issuer, namespace, token, payout_asset, 1000, period_id, false);
contract.deposit_revenue(issuer, namespace, token, payment_token, 500, period_id);
```

### Invalid Operations (Rejected)
```rust
// ❌ Invalid: zero amount
contract.report_revenue(issuer, namespace, token, payout_asset, 0, period_id, false);
// Returns: RevoraError::InvalidAmount

// ❌ Invalid: negative amount  
contract.deposit_revenue(issuer, namespace, token, payment_token, -100, period_id);
// Returns: RevoraError::InvalidAmount
```

## Testing

### Test Coverage
- `zero_amount_revenue_report_rejected`: Verifies zero reports are rejected
- `negative_amount_revenue_report_rejected`: Verifies negative reports are rejected
- `deposit_revenue_rejects_zero_amount`: Verifies zero deposits are rejected
- `deposit_revenue_rejects_negative_amount`: Verifies negative deposits are rejected

### Edge Cases Covered
- Amount = 0 (zero)
- Amount < 0 (negative)
- Amount = 1 (minimum valid)
- Large positive amounts (still valid)

## Migration Notes

Existing integrations should ensure all revenue amounts are positive. Previously allowed zero reports will now be rejected. Update client code to validate amounts before submission.

## Related Components

- **Audit Summary:** Only includes valid positive revenue amounts
- **Event Emission:** Only occurs for valid revenue operations
- **Token Transfers:** Only happen for valid positive deposit amounts