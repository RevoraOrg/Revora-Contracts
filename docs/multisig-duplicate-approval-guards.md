# Multisig Duplicate-Approval Guards

## Overview

Each multisig owner can approve a proposal at most once.

- First approval is recorded.
- Re-approval by the same owner returns `Err(RevoraError::AlreadyApproved)`.
- Approval list length is therefore safe to use for threshold checks.

Related: [Proposal expiry semantics](./multisig-proposal-expiry.md).

## Guard Logic

`approve_action` enforces this order:

1. `approver.require_auth()`
2. owner membership check
3. proposal existence check
4. executed check
5. expiry check
6. duplicate check (`AlreadyApproved`)
7. append approval + persist + emit event

## Error Mapping

- Duplicate approval: `RevoraError::AlreadyApproved` (code `40`)
- Approving executed proposal: `RevoraError::LimitReached`
- Approving expired proposal: `RevoraError::ProposalExpired`
- Approving missing proposal: `RevoraError::OfferingNotFound`

Auth failures remain host panics (`require_auth`) rather than returned `RevoraError` values.

## Tests

Coverage in `src/test.rs` includes:

- `multisig_duplicate_approval_returns_already_approved`
- `multisig_duplicate_second_owner_approval_returns_already_approved`
- `multisig_approve_executed_proposal_fails`
- `multisig_approve_fails_after_expiry_boundary`
- `multisig_threshold_three_requires_third_approval`
