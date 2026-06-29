# Share Sum Invariant Checks [RC26Q2-C50]

> **Doc-parity note (PR #299):** A previous version of this document described a
> `TotalShareBps` running-sum counter, a `ShareSumExceeded` error (code 30), a
> `get_total_share_bps` query, and `share_sum` events. **None of those exist in
> the current implementation.** This document has been updated to reflect the
> actual on-chain invariants. See the Security / Risk section at the bottom for
> the implications.

---

## Actual Invariants (as implemented)

### Per-holder cap

```
HolderShare[offering][holder] ∈ [0, 10_000]
```

`set_holder_share` and `set_holder_share_internal` reject any `share_bps > 10_000`
with `RevoraError::InvalidShareBps` (code 8) before writing to storage.

### Per-offering aggregate cap (enforced on-chain)

The contract now persists a per-offering running total of all persisted
`HolderShare` entries and enforces that the aggregate stays within `10_000`
(100%). Attempts to set a holder's share that would make the offering's total
exceed `10_000` bps are rejected with `RevoraError::InvalidShareBps` (code 8).

This guard is performed atomically during `set_holder_share` and when applying
committed snapshot batches via `apply_snapshot_shares`.

### Payout arithmetic

`claim()` computes each period's payout as:

```
payout = normalized_revenue * share_bps / 10_000   (integer truncation)
```

This is direct integer division, not `compute_share`. Because `share_bps ≤ 10_000`
and `normalized_revenue ≥ 0`, the result is always in `[0, normalized_revenue]`
for a single holder. However, if the issuer has allocated more than 10 000 bps
across all holders, the **sum of all payouts can exceed the deposited amount**,
draining the contract's token balance.

---

## Affected Entrypoints

| Entrypoint | What is enforced |
|------------|-----------------|
| `set_holder_share` | `share_bps ≤ 10_000` per holder; aggregate `sum(holder_bps) ≤ 10_000` enforced. |
| `meta_set_holder_share` | Delegates to `set_holder_share_internal`; same per-holder and aggregate caps. |
| `batch_set_holder_shares` | Validates each entry `≤ 10_000` before any write (fail-fast). |
| `claim` | Computes `revenue * share_bps / 10_000`; no cross-holder sum check. |

---

## Error Code

| Code | Name | Meaning |
|------|------|---------|
| 8 | `InvalidShareBps` | `share_bps > 10_000` for a single holder. |

> **Note:** Error code 30 is `ProposalExpired` (multisig), not a share-sum error.

---

## Security Assumptions

1. **Issuer responsibility for sum.** The contract trusts the issuer to allocate
   shares that sum to at most 10 000 bps across all holders. Over-allocation is
   not blocked on-chain; it is an off-chain operational concern.

2. **Per-holder cap is always enforced.** No single holder can be set above
   10 000 bps regardless of testnet mode or any other flag.

3. **Issuer auth required.** `set_holder_share` requires `issuer.require_auth()`.
   An attacker cannot set shares for an offering they do not control.

4. **Scoped per offering.** `HolderShare` is keyed by
   `OfferingId { issuer, namespace, token }`. Shares in one offering cannot
   affect another.

5. **Atomic write.** The validation and storage write happen in the same Soroban
   transaction; there is no TOCTOU window for the per-holder cap check.

---

## Risk Note

Because there is no on-chain aggregate cap, an issuer who sets N holders each
to `10_000 / N + 1` bps will cause the sum of payouts to exceed the deposited
tranche. The contract will attempt to transfer more tokens than it holds, and
the token transfer will fail with `RevoraError::TransferFailed`. No funds are
lost (the transfer reverts), but holders may be unable to claim until the issuer
corrects the share allocations.

**Mitigation (off-chain):** Before calling `set_holder_share`, integrators
should maintain their own running sum and ensure it stays ≤ 10 000 bps:

```
headroom = 10_000 - sum(current_share_bps for all holders in offering)
assert new_share_bps <= headroom
```

When redistributing shares (e.g., after a token transfer), reduce outgoing
holders first, then increase incoming holders, to keep the off-chain sum valid.

---

## Test Coverage

Tests are in `src/test.rs` under the `// ── Share-sum adversarial tests (#299) ──` section:

| Test | What it verifies |
|------|-----------------|
| `share_bps_per_holder_cap_enforced` | `share_bps > 10_000` is rejected with `InvalidShareBps`. |
| `share_bps_exactly_10000_accepted` | Exactly 10 000 bps is accepted per holder. |
| `multi_holder_over_allocation_transfer_fails` | Sum > 10 000 bps causes `TransferFailed` at claim time. |
| `multi_holder_exact_10000_sum_pays_correctly` | Sum = 10 000 bps distributes the full deposited amount. |
| `multi_holder_under_allocation_pays_partial` | Sum < 10 000 bps leaves remainder in contract. |
| `multi_holder_roundhalfup_sum_bounded` | RoundHalfUp across many holders never exceeds deposit. |
| `adversarial_many_holders_max_bps_each` | 10 holders at 10 000 bps each: first claim succeeds, contract drains, subsequent claims fail. |
| `share_bps_zero_holder_gets_no_payout` | Holder with 0 bps cannot claim. |
| `share_bps_update_to_zero_removes_payout` | Updating share to 0 stops future payouts. |
