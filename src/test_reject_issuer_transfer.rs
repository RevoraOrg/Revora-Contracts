#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::{Address as _, Events}, Address, Env, Vec, IntoVal};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'static> {
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &contract_id)
}

fn setup_offering(env: &Env, client: &RevoraRevenueShareClient) -> (Address, Address) {
    let admin = Address::generate(env);
    client.initialize(&admin, &None, &None);
    env.mock_all_auths();
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    client.register_offering(
        &issuer, 
        &Vec::new(env), 
        &1u32, 
        &symbol_short!("def"), 
        &token, 
        &1_000, 
        &token, 
        &0, 
        &symbol_short!(""), 
        &0
    );
    (issuer, token)
}

#[test]
fn test_reject_issuer_transfer_success() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    let namespace = symbol_short!("def");

    // Propose transfer
    client.propose_issuer_transfer(&issuer, &namespace, &token, &new_issuer);

    // Reject transfer
    let res = client.try_reject_issuer_transfer(&new_issuer, &namespace, &token);
    assert!(res.is_ok());

    // Verify it was rejected by checking we can't accept it anymore
    let accept_res = client.try_accept_issuer_transfer(&new_issuer, &namespace, &token);
    assert_eq!(accept_res, Err(Ok(RevoraError::NoTransferPending)));
    
    // Check events
    let events = env.events().all();
    let mut found_reject = false;
    for (contract_id, topic, data) in events.iter() {
        if contract_id == client.address {
            let topic_vec: Vec<soroban_sdk::Val> = topic.into_val(&env);
            if topic_vec.len() > 0 {
                if let Ok(sym) = soroban_sdk::Symbol::try_from_val(&env, &topic_vec.get(0).unwrap()) {
                    if sym == symbol_short!("iss_rej") {
                        found_reject = true;
                        // check data payload
                    }
                }
            }
        }
    }
    assert!(found_reject);
}

#[test]
fn test_reject_issuer_transfer_no_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    let namespace = symbol_short!("def");

    let res = client.try_reject_issuer_transfer(&new_issuer, &namespace, &token);
    assert_eq!(res, Err(Ok(RevoraError::NoTransferPending)));
}

#[test]
fn test_reject_issuer_transfer_wrong_new_issuer() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    let wrong_new_issuer = Address::generate(&env);
    let namespace = symbol_short!("def");

    client.propose_issuer_transfer(&issuer, &namespace, &token, &new_issuer);

    let res = client.try_reject_issuer_transfer(&wrong_new_issuer, &namespace, &token);
    assert_eq!(res, Err(Ok(RevoraError::NoTransferPending)));
}

#[test]
fn test_reject_issuer_transfer_wrong_namespace() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    let namespace = symbol_short!("def");
    let wrong_namespace = symbol_short!("wrong");

    client.propose_issuer_transfer(&issuer, &namespace, &token, &new_issuer);

    let res = client.try_reject_issuer_transfer(&new_issuer, &wrong_namespace, &token);
    assert_eq!(res, Err(Ok(RevoraError::NoTransferPending)));
}

#[test]
fn test_reject_issuer_transfer_wrong_token() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    let namespace = symbol_short!("def");
    let wrong_token = Address::generate(&env);

    client.propose_issuer_transfer(&issuer, &namespace, &token, &new_issuer);

    let res = client.try_reject_issuer_transfer(&new_issuer, &namespace, &wrong_token);
    assert_eq!(res, Err(Ok(RevoraError::NoTransferPending)));
}
