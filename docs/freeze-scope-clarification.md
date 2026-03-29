# Freeze Scope Clarification

## Summary

This change introduces **offering-scoped freeze controls** in addition to the existing global contract freeze.

- Global freeze (`freeze`) remains a contract-wide emergency stop for mutable operations.
- Offering freeze (`freeze_offering`) allows pausing mutable operations for one offering only.
- Claims remain intentionally available during both global and offering freezes.

## New API

- `freeze_offering(caller, issuer, namespace, token) -> Result<(), RevoraError>`
- `unfreeze_offering(caller, issuer, namespace, token) -> Result<(), RevoraError>`
- `is_offering_frozen(issuer, namespace, token) -> bool`

## Authorization Model

For `freeze_offering` and `unfreeze_offering`, caller must be one of:

- current offering issuer, or
- contract admin

If caller is neither, the call returns `RevoraError::NotAuthorized`.

If the target offering does not exist, calls return `RevoraError::OfferingNotFound`.

## Security Assumptions and Abuse Resistance

1. Fail-closed under global freeze:
- `freeze_offering` and `unfreeze_offering` both check global freeze first.
- If contract is globally frozen, both return `RevoraError::ContractFrozen`.

2. Claim continuity:
- `claim` intentionally does **not** enforce offering freeze.
- This prevents issuer-side freeze abuse from trapping already deposited funds.

3. Scope isolation:
- Offering freeze state is keyed by full `OfferingId` (`issuer`, `namespace`, `token`).
- Freezing one offering does not affect other offerings.

4. Deterministic behavior:
- Offering freeze defaults to `false` when unset.
- Freeze/unfreeze set explicit boolean values and emit dedicated events.

## Storage and Events

### Storage

- New key: `DataKey::FrozenOffering(OfferingId)` with value `bool`.

### Events

- `frz_off`: emitted on offering freeze.
- `ufrz_off`: emitted on offering unfreeze.

Both events publish `(caller, state)` in data and are topic-scoped by `(issuer, namespace, token)`.

## Mutating Entry Points Protected by Offering Freeze

The following offering-scoped mutators now enforce `require_not_offering_frozen`:

- `report_revenue`
- `deposit_revenue`
- `deposit_revenue_with_snapshot`
- `set_snapshot_config`
- `set_holder_share`
- `blacklist_add`
- `blacklist_remove`
- `whitelist_add`
- `whitelist_remove`
- `set_concentration_limit`
- `set_rounding_mode`
- `set_investment_constraints`
- `set_min_revenue_threshold`
- `set_claim_delay`
- `set_report_window`
- `set_claim_window`
- `set_meta_delegate`
- `meta_set_holder_share`
- `meta_approve_revenue_report`
- `set_offering_metadata`

## Error Surface

New explicit error:

- `RevoraError::OfferingFrozen = 30`

Returned when offering-scoped mutators are invoked for a frozen offering.

## Notes for Reviewers

- Contract version bumped from `2` to `3` due to storage and semantics expansion.
- Existing global freeze semantics are unchanged.
- Existing claim-on-freeze behavior remains unchanged by design.
