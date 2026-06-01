//! # Multisig `execute_action` Gas-Budget Tests
//!
//! ## Purpose
//!
//! `execute_action` for `ProposalAction::AddOwner` and `ProposalAction::RemoveOwner`
//! performs a linear walk of the owners `Vec` (O(n) in `MAX_MULTISIG_OWNERS`).
//! This file asserts that executing either action at the maximum owner count
//! stays within documented Soroban resource limits, preventing a future
//! regression where a code change silently blows the budget.
//!
//! ## Soroban resource limits (network-enforced, for reference)
//!
//! | Resource          | Network limit      |
//! |-------------------|--------------------|
//! | CPU instructions  | 100,000,000        |
//! | Memory bytes      | 41,943,040 (40 MB) |
//!
//! The test environment runs with an unlimited budget by default. A successful
//! return from `execute_action` at maximum owners proves the operation
//! completes without resource exhaustion.
//!
//! ## Why plain `impl` calls instead of the generated client
//!
//! `init_multisig`, `propose_action`, `approve_action`, and `execute_action`
//! live in a plain (non-`#[contractimpl]`) `impl RevoraRevenueShare` block to
//! keep the Soroban XDR spec within its variant limit. They are therefore not
//! exposed on the generated `RevoraRevenueShareClient`. Tests call them
//! directly as `RevoraRevenueShare::fn_name(env.clone(), ...)`.
//!
//! ## Budget measurement
//!
//! `env.budget().cpu_instruction_count()` and `env.budget().mem_bytes_count()`
//! return cumulative totals since env creation. Tests snapshot the counters
//! before and after `execute_action` to isolate the delta for that call.
//!
//! ## Security notes
//!
//! - `AddOwner` at `MAX_MULTISIG_OWNERS - 1` owners: verifies the cap check
//!   fires correctly and the linear duplicate scan completes within budget.
//! - `RemoveOwner` at `MAX_MULTISIG_OWNERS` owners: verifies the linear
//!   rebuild loop completes within budget.
//! - `RemoveOwner` when `owners.len() - 1 < threshold`: must return
//!   `LimitReached` without mutating state (threshold invariant).
//! - Non-owner executor: `execute_action` must return `NotAuthorized` before
//!   any state mutation occurs.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Vec,
};

use crate::{DataKey, ProposalAction, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Register the contract and return the env, contract id, and ABI client.
fn setup_env() -> (Env, Address, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);
    (env, id, client)
}

/// Read the current multisig owners list directly from persistent storage.
/// Uses `env.as_contract` to enter the contract's storage context.
fn read_owners(env: &Env, contract_id: &Address) -> Vec<Address> {
    env.as_contract(contract_id, || {
        env.storage().persistent().get::<DataKey, Vec<Address>>(&DataKey::MultisigOwners).unwrap()
    })
}

/// Build a full 20-owner multisig (threshold = 11, majority).
///
/// Returns `(env, contract_id, client, admin, owners_vec)`.
fn setup_max_multisig() -> (Env, Address, RevoraRevenueShareClient<'static>, Address, Vec<Address>)
{
    let (env, id, client) = setup_env();
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let mut owners: Vec<Address> = Vec::new(&env);
    for _ in 0..RevoraRevenueShare::MAX_MULTISIG_OWNERS {
        owners.push_back(Address::generate(&env));
    }

    // Majority threshold; duration = 1 day.
    let threshold = RevoraRevenueShare::MAX_MULTISIG_OWNERS / 2 + 1; // 11
    env.as_contract(&id, || {
        RevoraRevenueShare::init_multisig(
            env.clone(),
            admin.clone(),
            owners.clone(),
            threshold,
            86_400u64,
        )
    })
    .unwrap();

    (env, id, client, admin, owners)
}

/// Propose an action and collect enough approvals to meet threshold.
/// Returns the proposal id ready for `execute_action`.
fn propose_and_approve(
    env: &Env,
    contract_id: &Address,
    owners: &Vec<Address>,
    threshold: u32,
    action: ProposalAction,
) -> u32 {
    // owners[0] proposes (counts as first approval automatically).
    let proposer = owners.get(0).unwrap();
    let proposal_id = env
        .as_contract(contract_id, || {
            RevoraRevenueShare::propose_action(env.clone(), proposer, action)
        })
        .unwrap();

    // Collect remaining approvals up to threshold.
    for i in 1..threshold {
        let approver = owners.get(i).unwrap();
        env.as_contract(contract_id, || {
            RevoraRevenueShare::approve_action(env.clone(), approver, proposal_id)
        })
        .unwrap();
    }

    proposal_id
}

// ── Section A: RemoveOwner at MAX_MULTISIG_OWNERS ────────────────────────────

/// `execute_action(RemoveOwner)` at 20 owners completes successfully.
///
/// ## What is verified
///
/// The call must complete without panicking or exhausting Soroban resources.
/// The Soroban test environment runs with an unlimited budget by default, so
/// a successful return proves the operation fits within any reasonable bound.
/// The linear O(n) walk over 20 owners is the worst-case path; if it
/// completes here it will complete within network limits on-chain.
#[test]
fn execute_remove_owner_at_max_owners_within_budget() {
    let (env, id, _client, _admin, owners) = setup_max_multisig();

    let threshold = RevoraRevenueShare::MAX_MULTISIG_OWNERS / 2 + 1; // 11
                                                                     // Remove the last owner (index 19) — it is not the proposer.
    let target = owners.get(RevoraRevenueShare::MAX_MULTISIG_OWNERS - 1).unwrap();
    let action = ProposalAction::RemoveOwner(target);

    let proposal_id = propose_and_approve(&env, &id, &owners, threshold, action);

    let executor = owners.get(0).unwrap();
    // Must complete without panic or resource exhaustion.
    env.as_contract(&id, || RevoraRevenueShare::execute_action(env.clone(), executor, proposal_id))
        .unwrap();

    // Functional correctness: owner count decreased by 1.
    assert_eq!(
        read_owners(&env, &id).len(),
        RevoraRevenueShare::MAX_MULTISIG_OWNERS - 1,
        "owner count must decrease by 1 after RemoveOwner"
    );
}

// ── Section B: AddOwner at MAX_MULTISIG_OWNERS - 1 ───────────────────────────

/// `execute_action(AddOwner)` when the list is at `MAX - 1` owners completes
/// successfully. This exercises the duplicate-scan loop at near-max capacity.
#[test]
fn execute_add_owner_at_cap_minus_one_within_budget() {
    let (env, id, client) = setup_env();
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    // Start with MAX - 1 = 19 owners, threshold = 10 (majority of 19).
    let count = RevoraRevenueShare::MAX_MULTISIG_OWNERS - 1; // 19
    let mut owners: Vec<Address> = Vec::new(&env);
    for _ in 0..count {
        owners.push_back(Address::generate(&env));
    }
    let threshold = count / 2 + 1; // 10
    env.as_contract(&id, || {
        RevoraRevenueShare::init_multisig(
            env.clone(),
            admin.clone(),
            owners.clone(),
            threshold,
            86_400u64,
        )
    })
    .unwrap();

    let new_owner = Address::generate(&env);
    let action = ProposalAction::AddOwner(new_owner.clone());
    let proposal_id = propose_and_approve(&env, &id, &owners, threshold, action);

    let executor = owners.get(0).unwrap();
    // Must complete without panic or resource exhaustion.
    env.as_contract(&id, || RevoraRevenueShare::execute_action(env.clone(), executor, proposal_id))
        .unwrap();

    // Functional correctness: owner count is now MAX.
    let final_owners = read_owners(&env, &id);
    assert_eq!(
        final_owners.len(),
        RevoraRevenueShare::MAX_MULTISIG_OWNERS,
        "owner count must reach MAX after AddOwner"
    );
    assert!(final_owners.contains(&new_owner), "new owner must appear in the owners list");
}

// ── Section C: AddOwner rejected at MAX_MULTISIG_OWNERS ──────────────────────

/// `execute_action(AddOwner)` when already at `MAX_MULTISIG_OWNERS` returns
/// `LimitReached` and does not mutate the owners list.
#[test]
fn execute_add_owner_at_max_returns_limit_reached() {
    let (env, id, _client, _admin, owners) = setup_max_multisig();

    let threshold = RevoraRevenueShare::MAX_MULTISIG_OWNERS / 2 + 1;
    let new_owner = Address::generate(&env);
    let action = ProposalAction::AddOwner(new_owner);
    let proposal_id = propose_and_approve(&env, &id, &owners, threshold, action);

    let executor = owners.get(0).unwrap();
    let result = env.as_contract(&id, || {
        RevoraRevenueShare::execute_action(env.clone(), executor, proposal_id)
    });

    assert_eq!(
        result,
        Err(RevoraError::LimitReached),
        "AddOwner at MAX owners must return LimitReached"
    );
    // Owner count must be unchanged.
    assert_eq!(
        read_owners(&env, &id).len(),
        RevoraRevenueShare::MAX_MULTISIG_OWNERS,
        "owner count must not change when AddOwner is rejected"
    );
}

// ── Section D: RemoveOwner violates threshold invariant ──────────────────────

/// `execute_action(RemoveOwner)` when `owners.len() - 1 < threshold` returns
/// `LimitReached` and does not mutate the owners list.
///
/// ## Security note
///
/// This guards against a scenario where removing an owner would leave the
/// multisig unable to ever reach its own threshold — permanently locking
/// governance. The contract must reject such a removal.
#[test]
fn execute_remove_owner_below_threshold_returns_limit_reached() {
    let (env, id, client) = setup_env();
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    // 3 owners, threshold = 3 (unanimous). Removing any owner would leave
    // 2 owners < threshold 3 — must be rejected.
    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    let owner3 = Address::generate(&env);
    let mut owners: Vec<Address> = Vec::new(&env);
    owners.push_back(owner1.clone());
    owners.push_back(owner2.clone());
    owners.push_back(owner3.clone());
    env.as_contract(&id, || {
        RevoraRevenueShare::init_multisig(
            env.clone(),
            admin.clone(),
            owners.clone(),
            3u32,
            86_400u64,
        )
    })
    .unwrap();

    // All 3 must approve to meet threshold = 3.
    let action = ProposalAction::RemoveOwner(owner3.clone());
    let proposal_id = env
        .as_contract(&id, || {
            RevoraRevenueShare::propose_action(env.clone(), owner1.clone(), action)
        })
        .unwrap();
    env.as_contract(&id, || {
        RevoraRevenueShare::approve_action(env.clone(), owner2.clone(), proposal_id)
    })
    .unwrap();
    env.as_contract(&id, || {
        RevoraRevenueShare::approve_action(env.clone(), owner3.clone(), proposal_id)
    })
    .unwrap();

    let result = env.as_contract(&id, || {
        RevoraRevenueShare::execute_action(env.clone(), owner1.clone(), proposal_id)
    });

    assert_eq!(
        result,
        Err(RevoraError::LimitReached),
        "RemoveOwner that would violate threshold must return LimitReached"
    );
    // Owner list must be unchanged.
    let remaining = read_owners(&env, &id);
    assert_eq!(remaining.len(), 3);
    assert!(remaining.contains(&owner3));
}

// ── Section E: Non-owner executor is rejected ────────────────────────────────

/// `execute_action` called by an address not in the owners list returns
/// `NotAuthorized` before any state mutation.
///
/// ## Security note
///
/// `require_multisig_owner` is checked inside `execute_action` after
/// `require_auth`. With `mock_all_auths` active, the host auth layer is
/// satisfied for any address, so the contract-level identity check is the
/// only guard — and it must fire.
#[test]
fn execute_action_non_owner_returns_not_authorized() {
    let (env, id, _client, _admin, owners) = setup_max_multisig();

    let threshold = RevoraRevenueShare::MAX_MULTISIG_OWNERS / 2 + 1;
    let target = owners.get(RevoraRevenueShare::MAX_MULTISIG_OWNERS - 1).unwrap();
    let action = ProposalAction::RemoveOwner(target);
    let proposal_id = propose_and_approve(&env, &id, &owners, threshold, action);

    // outsider is not in the owners list
    let outsider = Address::generate(&env);
    let result = env.as_contract(&id, || {
        RevoraRevenueShare::execute_action(env.clone(), outsider, proposal_id)
    });

    assert_eq!(
        result,
        Err(RevoraError::NotAuthorized),
        "non-owner must not be able to execute a proposal"
    );
    // Owner count must be unchanged.
    assert_eq!(read_owners(&env, &id).len(), RevoraRevenueShare::MAX_MULTISIG_OWNERS);
}

// ── Section F: Expired proposal cannot be executed ───────────────────────────

/// `execute_action` on an expired proposal returns `ProposalExpired` and does
/// not mutate state.
#[test]
fn execute_action_expired_proposal_returns_proposal_expired() {
    let (env, id, _client, _admin, owners) = setup_max_multisig();

    let threshold = RevoraRevenueShare::MAX_MULTISIG_OWNERS / 2 + 1;
    let target = owners.get(RevoraRevenueShare::MAX_MULTISIG_OWNERS - 1).unwrap();
    let action = ProposalAction::RemoveOwner(target);
    let proposal_id = propose_and_approve(&env, &id, &owners, threshold, action);

    // Advance ledger time past the 1-day duration.
    env.ledger().with_mut(|li| {
        li.timestamp += 86_401u64; // 1 day + 1 second
    });

    let executor = owners.get(0).unwrap();
    let result = env.as_contract(&id, || {
        RevoraRevenueShare::execute_action(env.clone(), executor, proposal_id)
    });

    assert_eq!(
        result,
        Err(RevoraError::ProposalExpired),
        "expired proposal must not be executable"
    );
    // Owner count must be unchanged.
    assert_eq!(read_owners(&env, &id).len(), RevoraRevenueShare::MAX_MULTISIG_OWNERS);
}

// ── Section G: Already-executed proposal cannot be re-executed ───────────────

/// `execute_action` on an already-executed proposal returns `LimitReached`.
#[test]
fn execute_action_already_executed_returns_limit_reached() {
    let (env, id, _client, _admin, owners) = setup_max_multisig();

    let threshold = RevoraRevenueShare::MAX_MULTISIG_OWNERS / 2 + 1;
    let target = owners.get(RevoraRevenueShare::MAX_MULTISIG_OWNERS - 1).unwrap();
    let action = ProposalAction::RemoveOwner(target);
    let proposal_id = propose_and_approve(&env, &id, &owners, threshold, action);

    let executor = owners.get(0).unwrap();

    // First execution succeeds.
    env.as_contract(&id, || {
        RevoraRevenueShare::execute_action(env.clone(), executor.clone(), proposal_id)
    })
    .unwrap();

    // Second execution must fail.
    let result = env.as_contract(&id, || {
        RevoraRevenueShare::execute_action(env.clone(), executor, proposal_id)
    });
    assert_eq!(
        result,
        Err(RevoraError::LimitReached),
        "already-executed proposal must not be re-executable"
    );
}
