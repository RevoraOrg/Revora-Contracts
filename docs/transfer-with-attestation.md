# `transfer_with_attestation` — Secondary-Market Handoff

## Overview

`transfer_with_attestation` enables compliant peer-to-peer share transfers between
holders of a revenue-share offering. Both parties must co-sign the transaction, and a
32-byte attestation hash must accompany every transfer for off-chain compliance review.

This makes on-chain share handoffs a first-class operation while preserving the issuer's
ability to enforce jurisdiction restrictions through blacklists and whitelists.

---

## Signature

```rust
pub fn transfer_with_attestation(
    env: Env,
    issuer: Address,         // Primary issuer of the offering
    namespace: Symbol,       // Offering namespace
    token: Address,          // Offering token
    from: Address,           // Current shareholder; must provide auth
    to: Address,             // Recipient; must provide auth
    shares_bps: u32,         // Basis points to transfer (1–10000, must be > 0)
    attest_hash: BytesN<32>, // 32-byte attestation hash for compliance
    network_id: BytesN<32>,  // Ledger network identifier the attestation is bound to
) -> Result<(), RevoraError>
```

---

## Security Model

Ten guards are applied in strict order before any state is mutated:

| # | Guard | Error on violation |
|---|-------|--------------------|
| 1 | Contract is not globally frozen or paused | `ContractFrozen` / `ContractPaused` |
| 2 | `from` has authorized (`require_auth`) | host panic (non-catchable) |
| 2 | `issuer` has authorized (`require_auth`) | host panic (non-catchable) |
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
Neither the sender nor the issuer can unilaterally move shares.

**Blacklist takes precedence** over the whitelist (Guard 6 fires before Guard 7). A
blacklisted address is always excluded regardless of whitelist membership.

---

## Storage Invariant

A peer-to-peer transfer is a **pure redistribution**: the total BPS across all holders
for an offering does not change. `HolderShareTotal` is therefore **not** modified.
Only the two `HolderShare` entries (for `from` and `to`) are updated atomically.

This ensures that subsequent calls to `set_holder_share` see the correct running total
and correctly enforce the per-offering 10 000 bps cap.

---

## Attestation Hash

The 32-byte `attest_hash` is emitted verbatim in the `xfer_att` event. The contract
does not inspect its content — any 32-byte value is accepted (including all-zeros).

The supplied `network_id` is checked against the active ledger network before any state
changes occur. A mismatch returns `NetworkIdMismatch`, which prevents the same attestation
from being replayed on a different chain even if the hash is otherwise valid.

The intended usage is for off-chain compliance tooling to store the hash of an approval
document (KYC confirmation, AML clearance, jurisdiction sign-off, etc.) so that the
on-chain event log can be cross-referenced with the off-chain approval record.

---

## Network-Id Domain Separator (closes #578)

### Why network_id matters

An attestation must be **cryptographically bound to one specific Stellar network**.
Without a domain separator an attestation produced and signed on testnet could be
replayed on mainnet by any party who observed it on-chain or in transaction history.

The `network_id` domain separator makes cross-network replay impossible:

```
testnet  network_id = sha256("Test SDF Network ; September 2015")
         = cee0302d59844d32bdca915c8203dd44b33fbb7edc19051ea37abedf28ecd472

mainnet  network_id = sha256("Public Global Stellar Network ; September 2015")
         = e927f128742077640...b17d52d4  (different bytes)
```

Because the `network_id` is included in the signed preimage, a testnet attestation
produces a **different digest** than a mainnet attestation for identical parameters.

### `SignedAttestation` struct

```rust
pub struct SignedAttestation {
    /// sha256 of the Stellar network passphrase — the domain separator.
    pub network_id: BytesN<32>,
    /// Pre-signed digest over (network_id || issuer || namespace || token
    /// || from || to || amount_bps).
    pub digest: BytesN<32>,
}
```

### Digest construction

Off-chain signers compute:

```text
digest = sha256(
    network_id      (32 bytes — env.ledger().network_id())
    || XDR(issuer)
    || XDR(namespace)
    || XDR(token)
    || XDR(from)
    || XDR(to)
    || amount_bps   (4 bytes, big-endian u32)
)
```

Use `compute_attestation_digest` (read-only, no auth required) to obtain the expected
digest for the current chain directly from the contract.

### `compute_attestation_digest`

```rust
pub fn compute_attestation_digest(
    env: Env,
    issuer: Address,
    namespace: Symbol,
    token: Address,
    from: Address,
    to: Address,
    amount_bps: u32,
) -> BytesN<32>
```

Returns the canonical domain-separated digest for the current chain. Call this
read-only before having the parties sign so you always use the correct preimage.

### `verify_attestation_digest`

```rust
pub fn verify_attestation_digest(
    env: Env,
    attestation: SignedAttestation,
    issuer: Address,
    namespace: Symbol,
    token: Address,
    from: Address,
    to: Address,
    amount_bps: u32,
) -> Result<(), RevoraError>
```

Pre-flight validator for a `SignedAttestation`. Two checks are enforced:

1. **Network-id check** — `attestation.network_id` must equal `env.ledger().network_id()`.
   Fails with `NetworkIdMismatch` when the attestation was produced for a different chain.

2. **Digest check** — `attestation.digest` must equal the canonical preimage hash for
   the supplied parameters. Fails with `NetworkIdMismatch` if the digest is wrong.

Both failures return `NetworkIdMismatch` so callers cannot distinguish which check
failed and cannot craft a targeted bypass attempt.

This function is **read-only** — no state is written, no auth is required.

### Off-chain integration example

```rust
// 1. Fetch the expected digest from the contract (read-only).
let digest = client.compute_attestation_digest(
    &issuer, &namespace, &token, &from, &to, &amount_bps,
);

// 2. Have the authorised compliance signer approve the hash.
//    (store the approval record off-chain keyed by `digest`)

// 3. Build the SignedAttestation.
let network_id = client.env.ledger().network_id(); // or from the RPC node
let attestation = SignedAttestation { network_id, digest };

// 4. Optional: verify before submitting (catches env misconfiguration early).
client.verify_attestation_digest(
    &attestation, &issuer, &namespace, &token, &from, &to, &amount_bps,
)?;

// 5. Submit the transfer.
client.transfer_with_attestation(
    &issuer, &namespace, &token, &from, &to, &amount_bps, &digest,
);
```

### Security guarantees

| Property | Guarantee |
|----------|-----------|
| Cross-network replay prevention | `network_id` in preimage binds digest to one chain |
| Parameter binding | All six transfer params are in the signed preimage; changing any one invalidates the digest |
| No-aliasing | Different `amount_bps` (or any other param) produce distinct digests |
| Read-only verification | `verify_attestation_digest` writes no state; safe to call speculatively |
| Fail-closed | Any mismatch (network_id or digest) returns `NetworkIdMismatch`; no partial success |

### Error code

| Code | Name | Meaning |
|------|------|---------|
| 62 | `NetworkIdMismatch` | Attestation's `network_id` does not match the current chain, **or** the digest does not match the expected canonical preimage. |

---

## Event

```
topic:  (xfer_att, issuer, namespace, token)
data:   (from, to, shares_bps, attest_hash)
```

Symbol: `xfer_att` (8 chars, fits in `symbol_short!`).

---

## Example

```rust
// Alice transfers 2 500 bps (25 %) to Bob, attaching the hash of their
// compliance approval document.
client.transfer_with_attestation(
    &issuer,
    &namespace,
    &token,
    &alice,         // must sign
    &bob,           // must sign (issuer also signs)
    &2_500u32,
    &category,
    &approval_doc_hash,
    &network_id,
);
```

---

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
| `NetworkIdMismatch` | Attestation `network_id` or digest does not match the current chain |
| `LimitReached` | Arithmetic overflow in `to` share accumulation (edge case) |

---

## Test Coverage

All guards and the network-id domain separator are covered in
`src/test_transfer_with_attestation.rs`:

| Scenario | Test function |
|----------|---------------|
| Global freeze blocks transfer | `transfer_blocked_when_frozen` |
| Global pause blocks transfer | `transfer_blocked_when_paused` |
| Self-transfer rejected | `self_transfer_rejected` |
| Zero-shares rejected | `zero_shares_rejected` |
| Unknown offering rejected | `unknown_offering_rejected` |
| Wrong issuer rejected | `wrong_issuer_rejected` |
| Offering-level freeze | `transfer_blocked_when_offering_frozen` |
| Blacklisted `from` | `blacklisted_from_rejected` |
| Blacklisted `to` | `blacklisted_to_rejected` |
| Whitelist unlisted `from` | `whitelist_unlisted_from_rejected` |
| Whitelist unlisted `to` | `whitelist_unlisted_to_rejected` |
| Both whitelisted succeeds | `whitelist_both_listed_succeeds` |
| Insufficient shares | `insufficient_shares_rejected` |
| Recipient share cap | `recipient_share_cap_rejected` |
| Happy path full transfer | `happy_path_full_transfer` |
| Happy path partial transfer | `happy_path_partial_transfer` |
| `HolderShareTotal` invariant | `share_total_invariant_after_transfer` |
| Event payload correct | `event_payload_correct` |
| Correct network_id + digest | `verify_attestation_correct_network_id` |
| Mainnet id on testnet rejected | `verify_attestation_mainnet_id_on_testnet_rejected` |
| Testnet id on mainnet rejected | `verify_attestation_testnet_id_on_mainnet_rejected` |
| Unknown network_id rejected | `verify_attestation_unknown_network_id_rejected` |
| Wrong digest rejected | `verify_attestation_wrong_digest_rejected` |
| Compute → verify round-trip | `attestation_compute_verify_round_trip` |
