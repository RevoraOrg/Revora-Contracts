# Freeze authority

Offering freezes and individual holder reason bits record `FreezeAuthority::Admin`
or `FreezeAuthority::Issuer`. The current admin may clear either; the current
issuer may clear only issuer-imposed holds. If the same caller has both roles,
admin authority wins. An admin re-freeze upgrades issuer authority; an issuer
re-freeze never downgrades admin authority.

## Storage and legacy compatibility

Layout version 6 (contract 1.0.24) adds a dedicated `FreezeDataKey` enum to avoid
exceeding the existing key enums' variant limits:

- `OfferingAuthority(OfferingId)` stores authority beside the existing boolean
  `DataKey2::FrozenOffering` flag.
- `HolderAuthority(OfferingId, Address, FreezeReason)` stores authority per reason
  beside the existing `DataKey2::HolderFreezeMask` bitmask.

A live freeze without metadata is treated as **admin-imposed**. This applies to
legacy booleans, masks, and the old single-reason `EmergencyFreeze` record, and
also fails closed if metadata expires. Existing encodings and read APIs remain
unchanged; no eager migration is needed. Clearing a hold removes its authority
metadata. A subsequent fresh freeze assigns new authority, ignoring stale metadata
when the corresponding flag/bit is inactive.

Issuer-owned reasons can be cleared independently while admin compliance reasons
stay active. Reapplying a holder reason with the same or weaker authority remains
an event-free no-op; an admin upgrade emits an event. Automatic sanctions holds
are admin-owned and upgrade an existing issuer-owned `SanctionsMatch` reason.

## Events

Existing topics remain scoped to `(issuer, namespace, token)`. Data tuples append
authority, so consumers decoding the old tuple lengths must update:

| Event | Data |
| --- | --- |
| `frz_off`, `ufrz_off` | `(caller, state, authority)` |
| `frz_set`, `frz_clr` | `(caller, holder, reason, authority)` |
| `frz_rc` | `(caller, holder, reason, remaining_mask, authority)` |
| `auto_frz` | `(holder, attestation_hash, FreezeAuthority::Admin)` |

Authority describes the hold being imposed/cleared, even when an admin clears an
issuer hold. An already-unfrozen offering retains its no-op behavior; its event
reports the caller's role. Existing global freeze guards, caller authentication,
reason-mismatch errors and money-path enforcement remain unchanged. Attestation
authentication is outside this change.

## Verification

`cargo test -p revora-contracts --test freeze_authority -- --test-threads=1`
exercises the public contract client, including authorization regressions, legacy
state, mixed reasons, upgrades, events and metadata cleanup.
