# Cross-Contract Transfer Failure Handling

## Overview
This documentation details the newly implemented cross-contract transfer failure handling. We have enhanced the robustness of token transfers by switching from `.transfer()` to `.try_transfer()`.

## Security Assumptions
- Core token contracts conforming to Soroban token standards could potentially panic or return failure if liquidity or balances are insufficient/frozen.
- By using `.try_transfer()`, we ensure that our smart contract catches exceptions and returns the defined `TransferFailed` error inside of the application logic instead of relying on the host engine to panic.
- As a consequence, we protect the contract execution stack from halting abruptly and allow callers to gracefully respond to token transfer failures.

## Implementation Details
1. `RevoraError::TransferFailed (30)` code has been added to our error definitions.
2. In `report_revenue`, the deposit logic now executes `try_transfer` which intercepts failed token pulls and returns `Err(RevoraError::TransferFailed)`.
3. In `claim_revenue`, pushing payouts via `try_transfer` allows safe catch logic returning `Err(RevoraError::TransferFailed)`.
