#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, Address, BytesN, Env};

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn setup_offering(env: &Env, client: &RevoraRevenueShareClient) -> (Address, Address, Address) {
    env.mock_all_auths();
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let payout = Address::generate(env);
    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000u32, &payout, &0i128, &symbol_short!(""), &0);
    (issuer, token, payout)
}

fn make_meta_hash(env: &Env) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = i as u8;
    }
    BytesN::from_array(env, &bytes)
}

#[test]
fn create_proposal_persists_deterministic_state_and_emits_event() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, _payout) = setup_offering(&env, &client);

    let meta_hash = make_meta_hash(&env);
    let proposal_id = client.create_proposal(&issuer, &symbol_short!("def"), &token, &meta_hash, &2500u32, &86_400u64);

    assert_eq!(proposal_id, 0u32);
    let proposal = client.get_proposal(&issuer, &symbol_short!("def"), &token, &proposal_id).unwrap();
    assert_eq!(proposal.id, 0u32);
    assert_eq!(proposal.meta_hash, meta_hash);
    assert_eq!(proposal.quorum_bps, 2500u32);
    assert_eq!(proposal.ends_at, env.ledger().timestamp() + 86_400u64);
}

#[test]
fn create_proposal_rejects_zero_voting_window() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, _payout) = setup_offering(&env, &client);

    let meta_hash = make_meta_hash(&env);
    let result = client.try_create_proposal(&issuer, &symbol_short!("def"), &token, &meta_hash, &2500u32, &0u64);

    assert!(matches!(result, Err(Ok(RevoraError::InvalidAmount))));
}

#[test]
fn create_proposal_rejects_duplicate_meta_hash_for_same_offering() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, _payout) = setup_offering(&env, &client);

    let meta_hash = make_meta_hash(&env);
    client.create_proposal(&issuer, &symbol_short!("def"), &token, &meta_hash, &2500u32, &86_400u64);
    let result = client.try_create_proposal(&issuer, &symbol_short!("def"), &token, &meta_hash, &2500u32, &86_400u64);

    assert!(matches!(result, Err(Ok(RevoraError::LimitReached))));
}

#[test]
fn create_proposal_requires_offering_to_exist() {
    let env = Env::default();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let meta_hash = make_meta_hash(&env);

    let result = client.try_create_proposal(&issuer, &symbol_short!("def"), &token, &meta_hash, &2500u32, &86_400u64);

    assert!(matches!(result, Err(Ok(RevoraError::OfferingNotFound))));
}
