# `transfer_with_attestation` — Secondary-Market Handoff

## Overview

`transfer_with_attestation` enables compliant peer-to-peer share transfers between
holders of a revenue-share offering. Both parties must co-sign the transaction, and a
32-byte attestation hash must accompany every transfer for off-chain compliance review.

This makes on-chain share handoffs a first-class operation while preserving the issuer's
ability to enforce jurisdiction restrictions through blacklists and whitelists.

## Signature

```rust
pub fn transfer_with_attestation(
    env: Env,
    issuer: Address,      // Primary issuer of the offering
    namespace: Symbol,    // Offering namespace
    token: Address,       // Offering token
    from: Address,        // Current shareholder; must provide auth
    to: Address,          // Recipient; must provide auth
    shares_bps: u32,      // Basis points to transfer (1–10000, must be > 0)
    category: Symbol,     // Transfer category used for category-cap enforcement
    attest_hash: BytesN<32>, // 32-byte attestation hash for compliance
    network_id: BytesN<32>,  // Ledger network identifier the attestation is bound to
) -> Result<(), RevoraError>
```

## Security Model

Ten guards are applied in strict order before any state is mutated:

| # | Guard | Error on violation |
|---|-------|--------------------|
| 1 | Contract is not globally frozen or paused | `ContractFrozen` / `ContractPaused` |
| 2 | `from` has authorized (`require_auth`) | host panic (non-catchable) |
| 2 | `to` has authorized (`require_auth`) | host panic (non-catchable) |
| 3 | `from != to` | `InvalidTransferParticipants` |
| 10 | `shares_bps > 0` | `InvalidShareBps` |
| 4 | Offering exists with matching primary issuer | `OfferingNotFound` |
| 5 | Offering is not individually frozen | `OfferingFrozen` |
| 6 | Neither `from` nor `to` is blacklisted | `HolderBlacklisted` |
| 7 | If whitelist active: both parties must be listed | `NotAuthorized` |
| 8 | `from` holds ≥ `shares_bps` | `InvalidShareBps` |
| 9 | `to`'s resulting share ≤ 10 000 bps | `InvalidShareBps` |
| 10 | `network_id` matches `env.ledger().network_id()` | `NetworkIdMismatch` |

**Dual-party authorization** (Guard 2) is the primary peer-to-peer security invariant.
Neither the sender nor the recipient can unilaterally move shares.

**Blacklist takes precedence** over the whitelist (Guard 6 fires before Guard 7). A
blacklisted address is always excluded regardless of whitelist membership.

## Storage Invariant

A peer-to-peer transfer is a **pure redistribution**: the total BPS across all holders
for an offering does not change. `HolderShareTotal` is therefore **not** modified.
Only the two `HolderShare` entries (for `from` and `to`) are updated atomically.

This ensures that subsequent calls to `set_holder_share` see the correct running total
and correctly enforce the per-offering 10 000 bps cap.

## Attestation Hash

The 32-byte `attest_hash` is emitted verbatim in the `xfer_att` event. The contract
does not inspect its content — any 32-byte value is accepted (including all-zeros).

The supplied `network_id` is checked against the active ledger network before any state
changes occur. A mismatch returns `NetworkIdMismatch`, which prevents the same attestation
from being replayed on a different chain even if the hash is otherwise valid.

The intended usage is for off-chain compliance tooling to store the hash of an approval
document (KYC confirmation, AML clearance, jurisdiction sign-off, etc.) so that the
on-chain event log can be cross-referenced with the off-chain approval record.

## Event

```
topic:  (xfer_att, issuer, namespace, token)
data:   (from, to, shares_bps, attest_hash)
```

Symbol: `xfer_att` (8 chars, fits in `symbol_short!`).

## Example

```rust
// Alice transfers 2 500 bps (25 %) to Bob, attaching the hash of their
// compliance approval document.
client.transfer_with_attestation(
    &issuer,
    &namespace,
    &token,
    &alice,         // must sign
    &bob,           // must sign
    &2_500u32,
    &category,
    &approval_doc_hash,
    &network_id,
);
```

## Error Reference

| Error | Meaning |
|-------|---------|
| `ContractFrozen` | Global freeze active |
| `ContractPaused` | Contract is paused |
| `InvalidTransferParticipants` | `from == to` |
| `OfferingNotFound` | Offering does not exist or issuer mismatch |
| `OfferingFrozen` | Offering-level freeze active |
| `HolderBlacklisted` | `from` or `to` is blacklisted |
| `NotAuthorized` | Whitelist active and `from` or `to` not listed |
| `InvalidShareBps` | `shares_bps == 0`, `from` has insufficient shares, or `to` would exceed 10 000 bps |
| `NetworkIdMismatch` | The supplied attestation network id does not match the active ledger network |
| `LimitReached` | Arithmetic overflow in `to` share accumulation (edge case) |
