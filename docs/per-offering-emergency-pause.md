# Per-Offering Emergency Controls (Freeze)

## Overview
The Per-Offering Emergency control mechanism allows authorized roles (Admin, Issuer) to halt most state-mutating operations for a specific offering without affecting the rest of the contract. This granular control is essential for managing individual offering risks or responding to suspicious activities localized to a single issuance.

**Note on Terminology**: This feature is implemented in the codebase as an "offering freeze" (`freeze_offering`) rather than a "pause".

## Security Roles and Authorizations
The following roles are authorized to freeze or unfreeze an offering:
- **Global Admin**: Full control to freeze/unfreeze ANY offering.
- **Current Issuer**: The current authorized issuer of the specific offering may freeze it at any time.

## Protected Entrypoints
When an offering is frozen, all offering-scoped mutators will return `RevoraError::OfferingFrozen` (code `30`). This blocks:
- `deposit_revenue` & `deposit_revenue_with_snapshot`
- `report_revenue`
- `blacklist_add` / `blacklist_remove`
- `whitelist_add` / `whitelist_remove`
- `set_concentration_limit`
- `set_rounding_mode`
- `set_investment_constraints`
- `set_min_revenue_threshold`
- `set_snapshot_config`
- `set_holder_share`
- `set_meta_delegate`
- `meta_set_holder_share`
- `meta_approve_revenue_report`
- `set_report_window` / `set_claim_window` / `set_claim_delay`
- `set_offering_metadata`

## Claim Continuity & Security Notes
1. **Claims are NEVER blocked by an offering freeze**: The `claim` entrypoint intentionally does **not** check the offering freeze state. This prevents issuer-side freeze abuse from trapping already deposited funds.
2. **Flash-Loan Resistance**: Freeze checks are performed at the beginning of each state-mutating call.
3. **Read-Only Access**: View functions remain operational during an offering freeze to allow users to verify their state.
4. **State Persistence**: The offering freeze state is stored under `DataKey::FrozenOffering(OfferingId)`.

## Error Codes
- `RevoraError::OfferingFrozen` (30): Returned when an operation is attempted on a frozen offering.
