# Merkle Proof Depth Bound (`MAX_PROOF_DEPTH = 32`)

## Summary

The `verify_merkle_proof` entrypoint enforces a hard upper limit on the number of
sibling hashes accepted in a single Merkle membership proof.

| Constant | Value | Location |
|----------|-------|----------|
| `MAX_PROOF_DEPTH` | `32` | `src/merkle_helpers.rs` |

Proofs longer than 32 siblings are rejected **before any hashing** with:

- **Error** `RevoraError::ProofTooDeep` (wire value `77`)
- **Event** `prf_rej_d` — topics `(prf_rej_d, caller)`, data `(proof_len, MAX_PROOF_DEPTH)`

---

## Rationale

### Why a depth bound is necessary

Each step in proof verification requires:

1. One `SHA-256` call (≈ O(1) instructions per 64-byte input)
2. Two `BytesN<32>` allocations for the sorted-pair comparison
3. Two `Bytes` copies for the `min/max` ordering

Without a bound, an adversary submitting `proof.len() = 1_000_000` would force
**one million SHA-256 calls** inside a single contract invocation, exhausting both
the Soroban instruction budget and the per-frame memory budget.

### Why 32 specifically

A standard binary Merkle tree over **N** leaves has depth `ceil(log₂(N))`.

| Leaf count | Max depth |
|-----------|-----------|
| 50 (current `MAX_SNAPSHOT_BATCH`) | 6 |
| 1 000 | 10 |
| 1 000 000 | 20 |
| 4 294 967 296 (2³²) | **32** |

A depth of 32 covers every realistic snapshot while providing ~26 levels of
headroom above the current batch limit.

### Why the check is O(1)

```rust
if proof.len() > MAX_PROOF_DEPTH {
    // emit event, return error
    return Err(MerkleError::ProofTooDeep);
}
```

`Vec::len()` in the Soroban SDK is a single field read — no iteration, no hashing.
An adversary cannot make this check expensive regardless of the proof content.

---

## API

### Helper function (`src/merkle_helpers.rs`)

```rust
pub fn verify_merkle_proof(
    env: &Env,
    leaf_hash: BytesN<32>,
    root: BytesN<32>,
    proof: &Vec<BytesN<32>>,
) -> Result<bool, MerkleError>
```

| Return value | Meaning |
|-------------|---------|
| `Ok(true)` | Leaf is a member of the tree |
| `Ok(false)` | Proof is structurally valid but path does not reach root |
| `Err(MerkleError::ProofTooDeep)` | `proof.len() > MAX_PROOF_DEPTH` |

### Contract entrypoint (`src/lib.rs`)

```rust
pub fn verify_merkle_proof(
    env: Env,
    caller: Address,
    leaf_hash: BytesN<32>,
    root: BytesN<32>,
    proof: Vec<BytesN<32>>,
) -> Result<bool, RevoraError>
```

| Return value | Meaning |
|-------------|---------|
| `Ok(true)` | Leaf is a member of the tree |
| `Ok(false)` | Proof structurally valid but root mismatch |
| `Err(RevoraError::ProofTooDeep)` | `proof.len() > MAX_PROOF_DEPTH` |

No auth required. No storage reads or writes.

---

## Error codes

| Name | Wire value | Module |
|------|-----------|--------|
| `MerkleError::ProofTooDeep` | `1003` | `merkle_helpers` |
| `RevoraError::ProofTooDeep` | `77` | `lib.rs` |

Wire values are **frozen**. Do not renumber.

---

## Event: `prf_rej_d`

Emitted by the contract entrypoint **only** when `proof.len() > MAX_PROOF_DEPTH`.
Never emitted for valid-depth proofs.

```
Topics: (Symbol("prf_rej_d"), caller: Address)
Data:   (proof_len: u32, MAX_PROOF_DEPTH: u32)
```

Off-chain indexers can use this event to:
- Monitor for oversized-proof submission attempts
- Alert on potential denial-of-service probing
- Audit which callers are sending malformed proofs

---

## Proof construction (off-chain)

To build a valid proof for leaf `i` in a tree of `N` leaves:

1. Build all leaf hashes: `h[i] = SHA-256(0x00 || holder_xdr || share_bps_xdr)`
2. Build the tree bottom-up, at each level computing:
   `parent = SHA-256(0x01 || min(left, right) || max(left, right))`
3. The proof for leaf `i` is the sequence of sibling hashes at each level
   from bottom to top (length = `ceil(log₂(N))`)

The maximum valid proof for any current snapshot is **6 siblings** (50-leaf batch,
depth = `ceil(log₂(50)) = 6`), far below the `MAX_PROOF_DEPTH = 32` ceiling.

---

## Changing the bound

| Action | Compatibility |
|--------|--------------|
| Increase `MAX_PROOF_DEPTH` | Backwards-compatible: old valid proofs still accepted |
| Decrease `MAX_PROOF_DEPTH` | **Breaking change**: proofs between new and old bound are rejected |

Any decrease requires a contract version bump and migration notice.

---

## Test coverage

Tests live in `src/test_merkle_proof_depth.rs`:

| Test | What it checks |
|------|---------------|
| `max_proof_depth_is_32` | Constant value |
| `helper_proof_at_max_depth_ok_true` | Exactly `MAX_PROOF_DEPTH` → accepted |
| `helper_proof_one_over_max_depth_err_proof_too_deep` | `MAX_PROOF_DEPTH + 1` → rejected |
| `helper_proof_depth_100_err_proof_too_deep` | Far over bound → rejected |
| `helper_proof_depth_zero_accepted` | Empty proof → accepted |
| `contract_proof_at_max_depth_ok_true` | Contract entrypoint boundary check |
| `contract_proof_one_over_max_depth_err_proof_too_deep` | Contract entrypoint rejects oversized |
| `contract_oversized_proof_emits_proof_reject_depth_event` | `prf_rej_d` event emitted |
| `contract_valid_depth_proof_no_reject_event` | No event for valid proofs |
| `round_trip_two_leaf_tree_both_leaves_verify` | Build + verify self-consistency |
| `round_trip_tampered_sibling_fails_verification` | Tampered proof returns `Ok(false)` |
