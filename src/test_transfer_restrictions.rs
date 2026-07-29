#![cfg(test)]

use crate::{RevoraError, test_utils::setup_context};
use soroban_sdk::{testutils::Ledger as _, Address, BytesN, Env, Symbol};

#[test]
fn test_transfer_restrictions() {
    let (env, client, _contract_id, issuer, token, payout_asset) = setup_context();
    let namespace = Symbol::new(&env, "public");

    client.initialize(&issuer);
    client.register_offering(&issuer, &namespace, &token, &1000, &payout_asset);

    env.ledger().set_network_id([0x01u8; 32]);
    let category = Symbol::new(&env, "RegD");
    let attestation_hash = BytesN::from_array(&env, &[0xabu8; 32]);
    let network_id = BytesN::from_array(&env, &[0x01u8; 32]);
    client.set_transfer_restrictions(&issuer, &namespace, &token, &category, &1);

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);

    client.set_holder_share(&issuer, &namespace, &token, &holder1, &100);
    
    // Transfer from holder1 to holder2, assigning holder2 to "RegD"
    client.transfer_with_attestation(&issuer, &namespace, &token, &holder1, &holder2, &50, &category, &attestation_hash, &network_id);

    let holder3 = Address::generate(&env);
    let res = client.try_transfer_with_attestation(&issuer, &namespace, &token, &holder1, &holder3, &50, &category, &attestation_hash, &network_id);
    assert_eq!(res.unwrap_err().unwrap(), RevoraError::CategoryCapReached);

    // Drop holder2 to 0
    client.set_holder_share(&issuer, &namespace, &token, &holder2, &0);

    // Now we can transfer to holder3
    client.transfer_with_attestation(&issuer, &namespace, &token, &holder1, &holder3, &50, &category, &attestation_hash, &network_id);
}

#[test]
fn test_oscillating_across_zero() {
    let (env, client, _contract_id, issuer, token, payout_asset) = setup_context();
    let namespace = Symbol::new(&env, "public");

    client.initialize(&issuer);
    client.register_offering(&issuer, &namespace, &token, &1000, &payout_asset);

    env.ledger().set_network_id([0x01u8; 32]);
    let category = Symbol::new(&env, "RegS");
    let attestation_hash = BytesN::from_array(&env, &[0xabu8; 32]);
    let network_id = BytesN::from_array(&env, &[0x01u8; 32]);
    client.set_transfer_restrictions(&issuer, &namespace, &token, &category, &1);

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);
    let holder3 = Address::generate(&env);

    client.set_holder_share(&issuer, &namespace, &token, &holder1, &100);
    
    client.transfer_with_attestation(&issuer, &namespace, &token, &holder1, &holder2, &50, &category, &attestation_hash, &network_id);

    // Holder2 transfers entirely to holder3
    client.transfer_with_attestation(&issuer, &namespace, &token, &holder2, &holder3, &50, &category, &attestation_hash, &network_id);

    // Cap is 1, holder3 is the only one in RegS. Try adding holder4.
    let holder4 = Address::generate(&env);
    let res = client.try_transfer_with_attestation(&issuer, &namespace, &token, &holder1, &holder4, &50, &category, &attestation_hash, &network_id);
    assert_eq!(res.unwrap_err().unwrap(), RevoraError::CategoryCapReached);
}
