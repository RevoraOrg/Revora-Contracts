# Claimable Chunk Computation

## Summary

`get_claimable_chunk` is the read-only companion to `claim`. It lets indexers, frontends, and
reviewers page through a holder's currently claimable revenue without mutating contract state.

This implementation is intentionally conservative: previews should never advertise more value than
the holder could actually claim at the current ledger state.

## Production Behavior

- Full previews (`get_claimable`) and chunked previews (`get_claimable_chunk`) now share the same
  internal computation path.
- Caller-provided cursors are clamped to the holder's stored `LastClaimedIdx`.
- The first delayed period stops iteration and becomes the returned `next_cursor`.
- A blacklisted holder receives `0` from both preview methods.
- A closed claim window also yields `0` from both preview methods.
- Chunk size `0` or any size above the contract cap is normalized to `MAX_CHUNK_PERIODS` (200).

## Security Assumptions

- `LastClaimedIdx` is the source of truth for the holder's unclaimed frontier.
- Period entries are consumed in deposit-index order; previews do not skip ahead across a delayed
  period because `claim` cannot do so either.
- Read-only preview methods are public by design. They do not require auth, but they must still
  respect payout blockers such as blacklist state and claim windows.
- Arithmetic uses the same truncating share computation path as claims, preventing preview/claim
  drift from duplicated formulas.

## Abuse and Failure Paths Covered

- Stale or adversarial cursors cannot force previews to recount already claimed history.
- Oversized chunk requests cannot trigger unbounded iteration.
- Delay-gated periods do not leak inflated preview totals for later periods.
- Blacklisted holders cannot use preview endpoints to infer a positive currently claimable payout.
- Closed claim windows do not produce misleading positive claimable amounts.

## Test Coverage

The contract test suite now includes deterministic checks for:

- stale cursor clamping after a partial claim
- delay barrier cursor behavior
- blacklist-gated preview results
- claim-window-gated preview results
- max chunk cap normalization
