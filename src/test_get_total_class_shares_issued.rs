//! Focused adversarial coverage for `get_total_class_shares_issued` (#1036).
//!
//! Entry point under test:
//! `get_total_class_shares_issued(env, issuer, namespace, token, share_class) -> i128`
//!
//! The per-class aggregate is maintained as a running total by
//! `set_holder_share_class` (`TotalClassSharesIssued`). These tests pin the
//! read-side contract:
//! - an unset class reads back as `0`;
//! - multiple holders in the same class accumulate;
//! - different classes (including `ShareClass::Custom`) are tracked independently;
//! - updating a holder's share replaces their previous contribution instead of
//!   double-counting it;
//! - an unknown offering reads back as `0` (no panic).

#![cfg(test)]

use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol};

fn setup() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token,
        &10_000,
        &payout,
        &0,
        &symbol_short!(""),
        &0,
    );
    (env, contract_id, issuer, token, payout)
}

#[test]
fn unset_class_reads_zero() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    assert_eq!(
        client.get_total_class_shares_issued(&issuer, &ns, &token, &ShareClass::A),
        0,
    );
    assert_eq!(
        client.get_total_class_shares_issued(
            &issuer,
            &ns,
            &token,
            &ShareClass::Custom(symbol_short!("pref")),
        ),
        0,
    );
}

#[test]
fn same_class_shares_accumulate_across_holders() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);

    client.set_holder_share_class(&issuer, &ns, &token, &holder_a, &3_000, &ShareClass::A);
    assert_eq!(
        client.get_total_class_shares_issued(&issuer, &ns, &token, &ShareClass::A),
        3_000,
    );

    client.set_holder_share_class(&issuer, &ns, &token, &holder_b, &2_000, &ShareClass::A);
    assert_eq!(
        client.get_total_class_shares_issued(&issuer, &ns, &token, &ShareClass::A),
        5_000,
    );
}

#[test]
fn distinct_classes_are_tracked_independently() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);
    let holder_c = Address::generate(&env);
    let pref: Symbol = symbol_short!("pref");

    client.set_holder_share_class(&issuer, &ns, &token, &holder_a, &3_000, &ShareClass::A);
    client.set_holder_share_class(&issuer, &ns, &token, &holder_b, &2_000, &ShareClass::A);
    client.set_holder_share_class(&issuer, &ns, &token, &holder_c, &1_000, &ShareClass::B);
    client.set_holder_share_class(
        &issuer,
        &ns,
        &token,
        &holder_c,
        &500,
        &ShareClass::Custom(pref.clone()),
    );

    assert_eq!(
        client.get_total_class_shares_issued(&issuer, &ns, &token, &ShareClass::A),
        5_000,
        "class A must not absorb class B or custom shares",
    );
    assert_eq!(
        client.get_total_class_shares_issued(&issuer, &ns, &token, &ShareClass::B),
        1_000,
    );
    assert_eq!(
        client.get_total_class_shares_issued(&issuer, &ns, &token, &ShareClass::Custom(pref)),
        500,
    );
}

#[test]
fn updating_a_holders_share_replaces_instead_of_double_counting() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);

    client.set_holder_share_class(&issuer, &ns, &token, &holder_a, &3_000, &ShareClass::A);
    client.set_holder_share_class(&issuer, &ns, &token, &holder_b, &2_000, &ShareClass::A);
    assert_eq!(
        client.get_total_class_shares_issued(&issuer, &ns, &token, &ShareClass::A),
        5_000,
    );

    // Reduce holder_a from 3_000 to 1_000 bps: the aggregate must drop by 2_000.
    client.set_holder_share_class(&issuer, &ns, &token, &holder_a, &1_000, &ShareClass::A);
    assert_eq!(
        client.get_total_class_shares_issued(&issuer, &ns, &token, &ShareClass::A),
        3_000,
        "reassigning a holder must replace their previous class contribution",
    );

    // Clearing a holder's share removes it from the class aggregate.
    client.set_holder_share_class(&issuer, &ns, &token, &holder_b, &0, &ShareClass::A);
    assert_eq!(
        client.get_total_class_shares_issued(&issuer, &ns, &token, &ShareClass::A),
        1_000,
    );
}

#[test]
fn unknown_offering_reads_zero_without_panicking() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let stranger = Address::generate(&env);

    // `stranger` never registered an offering: the read must default to 0.
    assert_eq!(
        client.get_total_class_shares_issued(&stranger, &ns, &token, &ShareClass::A),
        0,
    );
    assert_eq!(
        client.get_total_class_shares_issued(&issuer, &symbol_short!("nope"), &token, &ShareClass::A),
        0,
    );
}
