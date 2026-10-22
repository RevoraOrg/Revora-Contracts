//! Regression tests for configured issuer quorum enforcement.
//!
//! These tests exercise the public API only. They assert that the stored
//! `quorum` (set at `register_offering` time) is what actually gates
//! issuer-authenticated entrypoints, instead of requiring every co-issuer to
//! sign on every call.
//!
//! Security properties covered
//! ───────────────────────────
//! * A 2-of-3 offering is operable by the primary plus one co-issuer.
//! * The primary alone cannot bypass a quorum of 2+.
//! * Co-issuers alone cannot satisfy quorum (primary is always required).
//! * Repeating the same address never inflates the signer count.
//! * Non-issuer signers are rejected.
//! * Duplicate co-issuers / the primary appearing in `co_issuers` are
//!   rejected at registration, so a single key cannot occupy two slots.

use revora_contracts::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{symbol_short, token, Address, Env, Symbol, Vec};

fn ns() -> Symbol {
    symbol_short!("qns")
}

struct Fx {
    env: Env,
    client: RevoraRevenueShareClient<'static>,
    primary: Address,
    co1: Address,
    co2: Address,
    offering: Address,
    payment: Address,
}

fn setup(quorum: u32) -> Fx {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let primary = Address::generate(&env);
    let co1 = Address::generate(&env);
    let co2 = Address::generate(&env);
    let offering = Address::generate(&env);
    let payment = env.register_stellar_asset_contract_v2(primary.clone()).address();

    client.initialize(&primary, &None::<Address>, &None::<bool>);
    client.register_offering(
        &primary,
        &Vec::from_array(&env, [co1.clone(), co2.clone()]),
        &quorum,
        &ns(),
        &offering,
        &5_000u32,
        &payment,
        &0i128,
        &symbol_short!(""),
        &7u32,
    );
    token::StellarAssetClient::new(&env, &payment).mint(&primary, &1_000_000);

    Fx { env, client, primary, co1, co2, offering, payment }
}

fn signers(f: &Fx, addrs: &[Address]) -> Vec<Address> {
    let mut v = Vec::new(&f.env);
    for a in addrs {
        v.push_back(a.clone());
    }
    v
}

fn deposit(f: &Fx, signed: &[Address]) -> Result<(), RevoraError> {
    match f.client.try_deposit_revenue(
        &f.primary,
        &ns(),
        &f.offering,
        &f.payment,
        &1_000i128,
        &1u64,
        &signers(f, signed),
    ) {
        Ok(Ok(())) => Ok(()),
        Err(Ok(e)) => Err(e),
        _ => panic!("unexpected deposit outcome"),
    }
}

#[test]
fn two_of_three_deposit_with_primary_and_one_co_issuer_succeeds() {
    let f = setup(2);
    assert_eq!(deposit(&f, &[f.primary.clone(), f.co1.clone()]), Ok(()));
}

#[test]
fn primary_alone_cannot_satisfy_quorum_of_two() {
    let f = setup(2);
    assert_eq!(deposit(&f, &[f.primary.clone()]), Err(RevoraError::QuorumNotReached));
}

#[test]
fn co_issuers_alone_cannot_satisfy_quorum() {
    let f = setup(2);
    assert_eq!(deposit(&f, &[f.co1.clone(), f.co2.clone()]), Err(RevoraError::QuorumNotReached));
}

#[test]
fn repeated_signer_is_counted_once_but_still_succeeds_when_quorum_met() {
    let f = setup(2);
    assert_eq!(deposit(&f, &[f.primary.clone(), f.co1.clone(), f.co1.clone()]), Ok(()));
}

#[test]
fn repeated_signers_cannot_inflate_count_past_quorum() {
    let f = setup(3);
    assert_eq!(
        deposit(&f, &[f.primary.clone(), f.co1.clone(), f.co1.clone(), f.primary.clone(),]),
        Err(RevoraError::QuorumNotReached)
    );
}

#[test]
fn non_issuer_signer_is_rejected() {
    let f = setup(2);
    let stranger = Address::generate(&f.env);
    assert_eq!(deposit(&f, &[f.primary.clone(), stranger]), Err(RevoraError::NotAuthorized));
}

#[test]
fn single_issuer_quorum_one_succeeds_and_empty_signers_fail() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let primary = Address::generate(&env);
    let offering = Address::generate(&env);
    let payment = env.register_stellar_asset_contract_v2(primary.clone()).address();
    client.initialize(&primary, &None::<Address>, &None::<bool>);
    client.register_offering(
        &primary,
        &Vec::new(&env),
        &1u32,
        &ns(),
        &offering,
        &5_000u32,
        &payment,
        &0i128,
        &symbol_short!(""),
        &7u32,
    );
    token::StellarAssetClient::new(&env, &payment).mint(&primary, &1_000_000);

    let mut none = Vec::new(&env);
    assert_eq!(
        client.try_deposit_revenue(&primary, &ns(), &offering, &payment, &1_000i128, &1u64, &none,),
        Err(Ok(RevoraError::QuorumNotReached))
    );

    none.push_back(primary.clone());
    assert_eq!(
        client.try_deposit_revenue(&primary, &ns(), &offering, &payment, &1_000i128, &1u64, &none,),
        Ok(Ok(()))
    );
}

#[test]
fn duplicate_co_issuers_are_rejected_at_registration() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let primary = Address::generate(&env);
    let co = Address::generate(&env);
    let offering = Address::generate(&env);
    let payment = env.register_stellar_asset_contract_v2(primary.clone()).address();
    client.initialize(&primary, &None::<Address>, &None::<bool>);

    assert_eq!(
        client.try_register_offering(
            &primary,
            &Vec::from_array(&env, [co.clone(), co.clone()]),
            &2u32,
            &ns(),
            &offering,
            &5_000u32,
            &payment,
            &0i128,
            &symbol_short!(""),
            &7u32,
        ),
        Err(Ok(RevoraError::InvalidCoIssuers))
    );
}

#[test]
fn primary_listed_as_co_issuer_is_rejected_at_registration() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let primary = Address::generate(&env);
    let co = Address::generate(&env);
    let offering = Address::generate(&env);
    let payment = env.register_stellar_asset_contract_v2(primary.clone()).address();
    client.initialize(&primary, &None::<Address>, &None::<bool>);

    assert_eq!(
        client.try_register_offering(
            &primary,
            &Vec::from_array(&env, [primary.clone(), co]),
            &2u32,
            &ns(),
            &offering,
            &5_000u32,
            &payment,
            &0i128,
            &symbol_short!(""),
            &7u32,
        ),
        Err(Ok(RevoraError::InvalidCoIssuers))
    );
}

#[test]
fn config_entrypoint_uses_same_quorum_rule() {
    let f = setup(2);
    assert_eq!(
        f.client.try_set_holder_share(
            &f.primary,
            &ns(),
            &f.offering,
            &Address::generate(&f.env),
            &1_000u32,
            &1u64,
            &signers(&f, &[f.primary.clone()]),
        ),
        Err(Ok(RevoraError::QuorumNotReached))
    );
    assert_eq!(
        f.client.try_set_holder_share(
            &f.primary,
            &ns(),
            &f.offering,
            &Address::generate(&f.env),
            &1_000u32,
            &1u64,
            &signers(&f, &[f.primary.clone(), f.co1.clone()]),
        ),
        Ok(Ok(()))
    );
}
