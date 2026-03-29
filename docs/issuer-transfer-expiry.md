# Issuer Transfer Expiry

The Revora contract implements a 24-hour expiry for issuer transfer proposals to ensure system security and prevent stale transfers from being executed.

## Mechanics

1.  **Proposal Timestamp**: When an issuer proposes a transfer via `propose_issuer_transfer`, the current ledger timestamp is recorded.
2.  **Expiry Window**: Proposals are valid for exactly **24 hours** (86,400 seconds).
3.  **Enforcement**: The `accept_issuer_transfer` function checks the elapsed time. If more than 24 hours have passed since the proposal, the transaction fails with `IssuerTransferExpired` (Error code 30).
4.  **Automatic Overwrite**: If a transfer has expired, the current issuer can simply call `propose_issuer_transfer` again to start a new 24-hour window, overwriting the expired proposal.
5.  **Manual Cleanup**: Anyone can call `cleanup_expired_transfer` to remove an expired proposal from storage, which is useful for storage hygiene.

## Security Rationale

*   **Key Compromise Protection**: If an issuer proposes a transfer and then their keys (or the new issuer's keys) are compromised weeks later, the attacker cannot use the old, forgotten proposal to hijack the offering.
*   **Operational Clarity**: Expiry forces both parties to coordinate and complete the transfer in a timely manner, reducing "pending state" ambiguity.

## Error Codes

| Code | Name | Description |
|---|---|---|
| 30 | `IssuerTransferExpired` | The transfer proposal has passed the 24-hour validity window. |

## Developer Guidance

Developers should ensure that the `accept_issuer_transfer` call is made shortly after the proposal is confirmed on-chain. If the window is missed, the process must be restarted by the current issuer.
