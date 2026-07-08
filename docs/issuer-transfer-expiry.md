# Issuer Transfer Expiry

Issuer transfer proposals have a configurable expiry window. The default is **7 days**
(604,800 seconds). Issuers can override this per-proposal within the bounds
`[1 hour, 30 days]`.

## Constants

| Constant | Value | Description |
|---|---|---|
| `ISSUER_TRANSFER_EXPIRY_SECS` | 604,800 s (7 days) | Default expiry when none is specified |
| `MIN_ISSUER_TRANSFER_EXPIRY_SECS` | 3,600 s (1 hour) | Minimum allowed custom expiry |
| `MAX_ISSUER_TRANSFER_EXPIRY_SECS` | 2,592,000 s (30 days) | Maximum allowed custom expiry |

## Proposing a Transfer

### Default expiry (7 days)

```
propose_issuer_transfer(issuer, namespace, token, new_issuer)
```

### Custom expiry

```
propose_transfer_with_expiry(issuer, namespace, token, new_issuer, expiry_secs)
```

`expiry_secs` is clamped to `[MIN_ISSUER_TRANSFER_EXPIRY_SECS, MAX_ISSUER_TRANSFER_EXPIRY_SECS]`
before being stored. Passing `0` is treated as "use default" and stores `0` in
`PendingTransfer.expiry_secs`; `accept_issuer_transfer` then applies the 7-day default.

## Accepting a Transfer

`accept_issuer_transfer` reads the stored `expiry_secs` from `PendingTransfer`:

- If `expiry_secs == 0` → effective expiry is `ISSUER_TRANSFER_EXPIRY_SECS` (7 days).
- Otherwise → effective expiry is the stored value.

The check is **inclusive on the boundary**:

```
now <= proposal_timestamp + effective_expiry  →  accepted
now >  proposal_timestamp + effective_expiry  →  IssuerTransferExpired
```

## Replacing a Pending Transfer

`replace_issuer_transfer` atomically cancels the current pending transfer and proposes
a new one to a different `new_issuer`. The **original `expiry_secs` is preserved** so
the replacement inherits the same window as the original proposal.

## Querying Pending Transfer Details

`get_pending_transfer_details(issuer, namespace, token)` returns
`Option<PendingTransfer>` with:

| Field | Type | Description |
|---|---|---|
| `new_issuer` | `Address` | Proposed new issuer |
| `timestamp` | `u64` | Ledger timestamp when the proposal was created |
| `expiry_secs` | `u64` | Stored expiry (0 = default 7 days) |

Use this to display the remaining acceptance window in UIs or off-chain tooling.

## Security Rationale

- **Key compromise protection**: A stale proposal cannot be used to hijack an offering
  after the expiry window closes.
- **Bounded window**: The `[1h, 30d]` clamp prevents both trivially short windows
  (race conditions) and indefinitely long windows (forgotten proposals).
- **Replace preserves expiry**: Replacing a pending transfer does not silently reset
  the expiry to the default, preventing a governance bypass where an attacker replaces
  a short-window proposal with a default-window one.

## Error Codes

| Code | Name | Description |
|---|---|---|
| 12 | `IssuerTransferPending` | A transfer is already pending; cancel or replace it first. |
| 13 | `NoTransferPending` | No pending transfer to accept or cancel. |
| 14 | `UnauthorizedTransferAccept` | Caller is not the proposed new issuer. |
| 43 | `IssuerTransferExpired` | The proposal has passed its expiry window. |

## Test Coverage

| Test | What it verifies |
|---|---|
| `issuer_transfer_default_expiry_used_when_expiry_secs_zero` | Default 7-day window accepted just before expiry |
| `issuer_transfer_default_expiry_rejects_after_seven_days` | Default window rejects after 7 days |
| `issuer_transfer_custom_expiry_accepted_within_window` | Custom 2h window accepts at 1h |
| `issuer_transfer_custom_expiry_rejected_after_window` | Custom 2h window rejects at 2h+1s |
| `issuer_transfer_custom_expiry_accepted_at_exact_boundary` | Inclusive boundary: accepts at exactly 2h |
| `issuer_transfer_expiry_below_min_clamped_to_min` | Below-min input clamped to 1h |
| `issuer_transfer_min_clamp_accept_at_exact_one_hour_boundary` | Min-clamped expiry accepts at exactly 1h |
| `issuer_transfer_expiry_above_max_clamped_to_max` | Above-max input clamped to 30 days |
| `issuer_transfer_max_clamp_accept_within_thirty_day_window` | Max-clamped expiry accepts within 30 days |
| `replace_issuer_transfer_preserves_custom_expiry` | Replace preserves original custom expiry |
| `get_pending_issuer_transfer_details_returns_expiry` | Details query returns correct expiry_secs |
| `get_pending_issuer_transfer_details_returns_none_when_no_pending` | Details query returns None when no pending |