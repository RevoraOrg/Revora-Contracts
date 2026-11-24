#![cfg(test)]

use crate::{test_utils::setup_context, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Ledger as _, Address, Env, Vec};

fn register_offering(
    env: &Env,
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    token: &Address,
    payout_asset: &Address,
) {
    let _ = client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        &symbol_short!("ns"),
        token,
        &5_000,
        payout_asset,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
}

fn give_share(
    env: &Env,
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    token: &Address,
    holder: &Address,
) {
    let _ = client.set_holder_share(
        issuer,
        &symbol_short!("ns"),
        token,
        holder,
        &100,
        &0u64,
        &Vec::from_array(env, [issuer.clone()]),
    );
}

#[test]
fn test_transfer_restrictions_are_offering_scoped() {
    let (env, client, _, issuer, token_a, payout_asset_a) = setup_context();
    let token_b = Address::generate(&env);
    let payout_asset_b = Address::generate(&env);
    register_offering(&env, &client, &issuer, &token_a, &payout_asset_a);
    register_offering(&env, &client, &issuer, &token_b, &payout_asset_b);
    env.ledger().set_network_id([0x01u8; 32]);

    let namespace = symbol_short!("ns");
    let category = symbol_short!("regs");
    let sender_a = Address::generate(&env);
    let sender_b = Address::generate(&env);
    let recipient_a = Address::generate(&env);
    let recipient_b = Address::generate(&env);

    client.set_transfer_restrictions(&issuer, &namespace, &token_a, &category, &1);
    give_share(&env, &client, &issuer, &token_a, &sender_a);
    give_share(&env, &client, &issuer, &token_b, &sender_b);

    assert_eq!(
        client.try_transfer_with_attestation(
            &issuer,
            &namespace,
            &token_a,
            &sender_a,
            &recipient_a,
            &50,
            &category,
        ),
        Ok(Ok(()))
    );
    assert_eq!(
        client.try_transfer_with_attestation(
            &issuer,
            &namespace,
            &token_b,
            &sender_b,
            &recipient_b,
            &50,
            &category,
        ),
        Ok(Ok(()))
    );
}
