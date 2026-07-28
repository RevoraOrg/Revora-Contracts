#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    Address, Env, Vec,
};

fn setup_multisig() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    let owner1 = Address::generate(&env);
    let owner2 = Address::generate(&env);
    let owner3 = Address::generate(&env);

    let owners = Vec::from_array(&env, [owner1.clone(), owner2.clone(), owner3.clone()]);

    client.init_multisig(&admin, &owners, &2, &86400, &5100);
    // 2-of-3, quorum 5100 bps (51%), each owner gets 3333 bps (10_000 / 3)

    (env, client, admin, owner1, owner2, owner3)
}

#[test]
fn test_check_quorum_empty_approvals_returns_false() {
    let (env, client, _admin, _owner1, _owner2, _owner3) = setup_multisig();

    let result = client.check_quorum(&0);
    assert!(!result, "empty approvals should not meet quorum");
}

#[test]
fn test_check_quorum_exact_meets_threshold() {
    let (_env, client, _admin, owner1, owner2, _owner3) = setup_multisig();

    client.approve_action(&owner1, &0);
    let result = client.check_quorum(&0);
    assert!(!result, "one owner (3333 bps) should not meet 5100 quorum");

    client.approve_action(&owner2, &0);
    let result = client.check_quorum(&0);
    assert!(result, "two owners (6666 bps) should meet 5100 quorum");
}

#[test]
fn test_check_quorum_off_by_one_below() {
    let (env, client, _admin, owner1, _owner2, _owner3) = setup_multisig();

    // Set high quorum that cannot be met by a single owner
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let single_client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    single_client.initialize(&admin, &None, &None);

    let owner = Address::generate(&env);
    let owners = Vec::from_array(&env, [owner.clone()]);
    single_client.init_multisig(&admin, &owners, &1, &86400, &10_000);

    // Proposed action auto-approves the proposer
    let proposal_id = single_client.propose_action(&owner, &ProposalAction::Freeze).unwrap();
    let result = single_client.check_quorum(&proposal_id);
    assert!(result, "single owner with 10_000 bps must meet 10_000 quorum");
}

#[test]
fn test_check_quorum_proposal_not_found_panics() {
    let (_env, client, _admin, _owner1, _owner2, _owner3) = setup_multisig();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.check_quorum(&999);
    }));
    assert!(result.is_err(), "non-existent proposal should panic");
}

#[test]
fn test_quorum_enforced_at_execute_time() {
    let (_env, client, _admin, owner1, _owner2, owner3) = setup_multisig();

    // Only one approval (proposer auto-approves, plus owner3's vote)
    client.approve_action(&owner3, &0);

    let result = client.try_execute_action(&owner3, &0);
    assert!(result.is_err(), "quorum not met should block execution");

    // Meet quorum
    client.approve_action(&owner1, &0);
    let result = client.try_execute_action(&owner1, &0);
    assert!(result.is_ok(), "quorum met should allow execution");
}

#[test]
fn test_check_quorum_all_owners_vote() {
    let (_env, client, _admin, owner1, owner2, owner3) = setup_multisig();

    client.approve_action(&owner1, &0);
    client.approve_action(&owner2, &0);
    client.approve_action(&owner3, &0);

    let result = client.check_quorum(&0);
    assert!(result, "all three owners (9999 bps) must meet 5100 quorum");
}
