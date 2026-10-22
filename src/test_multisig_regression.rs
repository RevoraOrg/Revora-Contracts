#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, IntoVal, Vec,
};

fn setup(quorum_bps: u32) -> (Env, RevoraRevenueShareClient<'static>, Address, Vec<Address>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let owners = Vec::from_array(
        &env,
        [Address::generate(&env), Address::generate(&env), Address::generate(&env)],
    );
    client.init_multisig(&admin, &owners, &2, &86_400, &quorum_bps);
    (env, client, admin, owners)
}

#[test]
fn configured_quorum_is_persisted_and_set_admin_executes() {
    let (env, client, _admin, owners) = setup(6_600);
    let new_admin = Address::generate(&env);
    let proposal_id = client
        .propose_action(&owners.get(0).unwrap(), &ProposalAction::SetAdmin(new_admin.clone()));

    client.approve_action(&owners.get(1).unwrap(), &proposal_id);
    let proposal = client.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal.quorum_bps, 6_600);

    let events_before_execute = env.events().all().len();
    client.execute_action(&owners.get(1).unwrap(), &proposal_id);
    assert_eq!(client.get_admin(), Some(new_admin.clone()));
    let admin_set: soroban_sdk::Val = symbol_short!("admin_set").into_val(&env);
    assert!(env
        .events()
        .all()
        .iter()
        .skip(events_before_execute as usize)
        .any(|(_, topics, _)| topics.contains(admin_set)));
}

#[test]
fn approvals_below_configured_quorum_do_not_apply_set_admin() {
    let (env, client, admin, owners) = setup(7_000);
    let new_admin = Address::generate(&env);
    let proposal_id =
        client.propose_action(&owners.get(0).unwrap(), &ProposalAction::SetAdmin(new_admin));

    client.approve_action(&owners.get(1).unwrap(), &proposal_id);
    let result = client.try_execute_action(&owners.get(1).unwrap(), &proposal_id);
    assert!(matches!(result, Err(Ok(RevoraError::NotAuthorized))));
    assert_eq!(client.get_admin(), Some(admin));
    assert!(!client.get_proposal(&proposal_id).unwrap().executed);
}

#[test]
fn equal_weights_are_deterministic_and_rebalanced_for_owner_changes() {
    let (env, client, _admin, owners) = setup(6_600);

    // 10,000 / 3 is 3,333 with the final 1 bp assigned to the last owner.
    let first = client.propose_action(&owners.get(0).unwrap(), &ProposalAction::Freeze);
    client.approve_action(&owners.get(2).unwrap(), &first);
    client.execute_action(&owners.get(2).unwrap(), &first);
    assert!(client.is_frozen());

    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let owners = Vec::from_array(
        &env,
        [Address::generate(&env), Address::generate(&env), Address::generate(&env)],
    );
    client.init_multisig(&admin, &owners, &2, &86_400, &7_500);
    let new_owner = Address::generate(&env);
    let add = client
        .propose_action(&owners.get(0).unwrap(), &ProposalAction::AddOwner(new_owner.clone()));
    client.approve_action(&owners.get(1).unwrap(), &add);
    client.approve_action(&owners.get(2).unwrap(), &add);
    client.execute_action(&owners.get(1).unwrap(), &add);
    assert_eq!(client.get_multisig_owners().unwrap().len(), 4);

    // Three of four equal 2,500-bp owners meet the 7,500-bp quorum.
    let freeze = client.propose_action(&owners.get(0).unwrap(), &ProposalAction::Freeze);
    client.approve_action(&owners.get(1).unwrap(), &freeze);
    client.approve_action(&owners.get(2).unwrap(), &freeze);
    client.execute_action(&owners.get(2).unwrap(), &freeze);
    assert!(client.is_frozen());
}

#[test]
fn removed_owner_cannot_approve_or_contribute_voting_weight() {
    let (_env, client, admin, owners) = setup(6_600);
    let remove = client.propose_action(
        &owners.get(0).unwrap(),
        &ProposalAction::RemoveOwner(owners.get(2).unwrap()),
    );
    client.approve_action(&owners.get(1).unwrap(), &remove);
    client.execute_action(&owners.get(1).unwrap(), &remove);
    assert_eq!(client.get_multisig_owners().unwrap().len(), 2);

    let proposal = client.propose_action(&owners.get(0).unwrap(), &ProposalAction::SetAdmin(admin));
    let rejected = client.try_approve_action(&owners.get(2).unwrap(), &proposal);
    assert!(matches!(rejected, Err(Ok(RevoraError::NotAuthorized))));
    assert_eq!(client.get_proposal(&proposal).unwrap().approvals.len(), 1);
}
