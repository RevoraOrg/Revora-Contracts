# Revenue Threshold Filtering

The Revenue Threshold Filtering feature ensures that revenue reports below a specified minimum amount do not trigger on-chain distribution logic. This protects the protocol from "dust" reports that are computationally expensive to process relative to the value distributed.

## Objective

Prevent inefficient distributions by enforcing a per-offering minimum revenue requirement.

## Implementation Details

### Core Logic

The filtering occurs in both the `report_revenue` and `do_deposit_revenue` functions in `src/lib.rs`. 

#### Reporting Filtering
In `report_revenue`, if the `amount` is strictly less than the `MinRevenueThreshold`:
- An `EVENT_REV_BELOW_THRESHOLD` (`rev_below`) is emitted for off-chain tracking.
- The function returns early with `Ok(())`.
- No state changes occur (no update to `RevenueReports`, `AuditSummary`, or `RevenueIndex`).

#### Funding Filtering
In `do_deposit_revenue`, if the `amount` is strictly less than the `MinRevenueThreshold`:
- The transaction fails with `RevoraError::InvalidAmount`.
- This ensures that issuers cannot fund dust periods, protecting holders from inefficient claim operations.

### Configuration

Issuers can set their threshold using:
- `set_min_revenue_threshold(issuer, namespace, token, min_amount)`
- Setting `min_amount` to `0` effectively disables filtering.

## Security Assumptions

- **Issuer & Admin Control**: The offering issuer and the platform admin are authorized to set or change the threshold. This is enforced via `caller.require_auth()` and an internal check against the current issuer and platform admin addresses.
- **Integrity**: Boundary conditions (exactly matching threshold) are permitted to pass, ensuring no legitimate "at-limit" reports are blocked.

## Failure & Abuse Scenarios

- **Unauthorized Modification**: Attempting to set a threshold by a non-issuer will result in a panic or host error.
- **Dust Flooding**: While filtering prevents on-chain distribution, multiple "below-threshold" reports still consume some network resources to emit events. Systemic protection against event-only spam should be handled by charging Soroban's standard resource fees.

## Boundary Conditions

- `amount == threshold`: Allowed.
- `amount < threshold`: Filtered.
- `threshold == 0`: All non-negative reports allowed.
- `amount < 0`: Rejected by initial validation before threshold check.
