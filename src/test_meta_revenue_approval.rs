use super::*;
use ed25519_dalek::SigningKey;
use soroban_sdk::{
    testutils::{ed25519::Sign, Address as _, Ledger},
    Address, BytesN, Env, Symbol, Vec,
};

fn setup() -> (Env, Address, RevoraRevenueShareClient<'static>, Address, Symbol, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|ledger| ledger.timestamp = 1_000);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "def");
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    (env, contract_id, client, issuer, namespace, token, payout_asset)
}

fn approve_report(
    env: &Env,
    contract_id: &Address,
    client: &RevoraRevenueShareClient<'static>,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    payout_asset: &Address,
    period_id: u64,
    amount: i128,
    expiry: u64,
) {
    let signer = Address::generate(env);
    let signing_key = SigningKey::from_bytes(&[42; 32]);
    let public_key = BytesN::from_array(env, &signing_key.verifying_key().to_bytes());
    client.register_meta_signer_key(&signer, &public_key);
    client.set_meta_delegate(issuer, namespace, token, &signer);

    let payload = MetaRevenueApprovalPayload {
        issuer: issuer.clone(),
        namespace: namespace.clone(),
        token: token.clone(),
        payout_asset: payout_asset.clone(),
        amount,
        period_id,
        override_existing: false,
    };
    let nonce = 7;
    let authorization = MetaAuthorization {
        version: 1,
        contract: contract_id.clone(),
        signer: signer.clone(),
        nonce,
        expiry,
        action: MetaAction::ApproveRevenueReport(payload.clone()),
    };
    let signature = signing_key.sign(authorization).unwrap();
    let signature = BytesN::from_array(env, &signature);

    client.meta_approve_revenue_report(&signer, &payload, &nonce, &expiry, &signature);
}

#[test]
fn signed_approval_can_be_relayed_once_and_uses_approved_amount() {
    let (env, contract_id, client, issuer, namespace, token, payout_asset) = setup();
    approve_report(
        &env,
        &contract_id,
        &client,
        &issuer,
        &namespace,
        &token,
        &payout_asset,
        1,
        1_250,
        1_100,
    );
    env.set_auths(&[]);

    client.submit_approved_revenue_report(&issuer, &namespace, &token, &1);
    assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 1_250);
    assert_eq!(
        client.try_submit_approved_revenue_report(&issuer, &namespace, &token, &1),
        Err(Ok(RevoraError::NotAuthorized))
    );
}

#[test]
fn approval_cannot_be_submitted_for_another_period() {
    let (env, contract_id, client, issuer, namespace, token, payout_asset) = setup();
    approve_report(
        &env,
        &contract_id,
        &client,
        &issuer,
        &namespace,
        &token,
        &payout_asset,
        2,
        1_250,
        1_100,
    );
    env.set_auths(&[]);

    assert_eq!(
        client.try_submit_approved_revenue_report(&issuer, &namespace, &token, &3),
        Err(Ok(RevoraError::NotAuthorized))
    );
    assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &3), 0);
}

#[test]
fn expired_approval_cannot_be_submitted() {
    let (env, contract_id, client, issuer, namespace, token, payout_asset) = setup();
    approve_report(
        &env,
        &contract_id,
        &client,
        &issuer,
        &namespace,
        &token,
        &payout_asset,
        1,
        1_250,
        1_001,
    );
    env.set_auths(&[]);
    env.ledger().with_mut(|ledger| ledger.timestamp = 1_002);

    assert_eq!(
        client.try_submit_approved_revenue_report(&issuer, &namespace, &token, &1),
        Err(Ok(RevoraError::SignatureExpired))
    );
    assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 0);
}
