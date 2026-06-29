# Multisig Proposal Expiry

## Overview

Multisig proposals carry an on-chain `expiry` timestamp.

- Expired proposals cannot be approved.
- Expired proposals cannot be executed.
- Executed proposals cannot run again.

Related: [Duplicate approval guards](./multisig-duplicate-approval-guards.md).

## Expiry Model

`propose_action` computes:

- `expiry = ledger_timestamp + proposal_duration`

`init_multisig` stores `proposal_duration` in `DataKey::MultisigProposalDuration` and rejects `proposal_duration == 0` with `RevoraError::InvalidAmount`.

A proposal is expired when:

- `env.ledger().timestamp() >= proposal.expiry`

The `now == expiry` boundary is intentionally expired.

## Runtime Outcomes

- `approve_action` on expired proposal: `Err(RevoraError::ProposalExpired)`
- `execute_action` on expired proposal: `Err(RevoraError::ProposalExpired)`
- `execute_action` on already executed proposal: `Err(RevoraError::LimitReached)`

Auth failures are host-level panics from `require_auth`, not contract `RevoraError` returns.

## Tests

Implemented adversarial coverage in `src/test.rs`:

- `multisig_approve_fails_after_expiry_boundary`
- `multisig_execute_fails_after_expiry_boundary`
- `multisig_execute_twice_fails`

## Security Notes

- Expiry blocks stale governance execution after long inactivity.
- Expiry does not delete proposal data; historical records remain queryable.
- Changing proposal duration affects only future proposals.
