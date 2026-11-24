#![cfg(test)]

use crate::{test_utils::setup_context, RevoraError, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Ledger as _, Address, BytesN, Env, Symbol, Vec};

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Symbol) {
    let (env, client, _, issuer, token, payout_asset) = setup_context();
    let namespace = symbol_short!("ns");
    let _ = client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &5_000,
        &payout_asset,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
    env.ledger().set_network_id([0x01u8; 32]);
    (env, client, issuer, token, namespace)
}

fn give_share(
    env: &Env,
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    token: &Address,
    namespace: &Symbol,
    holder: &Address,
    amount: u32,
) {
    let _ = client.set_holder_share(
        issuer,
        namespace,
        token,
        holder,
        &amount,
        &0u64,
        &Vec::from_array(env, [issuer.clone()]),
    );
}

#[test]
fn test_transfer_restriction_blocks_over_cap_recipient() {
    let (env, client, issuer, token, namespace) = setup();
    let category = symbol_short!("regd");
    let network_id = BytesN::from_array(&env, &[0x01u8; 32]);
    let attest_hash = BytesN::from_array(&env, &[0xabu8; 32]);
    let sender = Address::generate(&env);
    let first_recipient = Address::generate(&env);
    let second_recipient = Address::generate(&env);

    client.set_transfer_restrictions(&issuer, &namespace, &token, &category, &1);
    give_share(&env, &client, &issuer, &token, &namespace, &sender, 100);

    assert_eq!(
        client.try_transfer_with_attestation(
            &issuer,
            &namespace,
            &token,
            &sender,
            &first_recipient,
            &50,
            &category,
        ),
        Ok(Ok(()))
    );

    let estimate = client.try_estimate_transfer(
        &issuer,
        &namespace,
        &token,
        &sender,
        &second_recipient,
        &25,
        &category,
        &attest_hash,
        &network_id,
    );
    assert_eq!(estimate, Err(Ok(RevoraError::CategoryCapReached)));

    assert_eq!(
        client.try_transfer_with_attestation(
            &issuer,
            &namespace,
            &token,
            &sender,
            &second_recipient,
            &25,
            &category,
        ),
        Err(Ok(RevoraError::CategoryCapReached))
    );
}
