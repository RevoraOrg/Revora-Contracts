#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};

#[test]
fn fixture_topics_have_stable_order_and_shape() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    let fixtures = client.get_indexer_fixture_topics(&issuer, &ns, &token, &7u64);
    assert_eq!(fixtures.len(), 6);

    let f0 = fixtures.get(0).unwrap();
    assert_eq!(f0.version, 2);
    assert_eq!(f0.event_type, symbol_short!("offer"));
    assert_eq!(f0.period_id, 0);

    let f1 = fixtures.get(1).unwrap();
    assert_eq!(f1.event_type, symbol_short!("rv_init"));
    assert_eq!(f1.period_id, 7);

    let f2 = fixtures.get(2).unwrap();
    assert_eq!(f2.event_type, symbol_short!("rv_ovr"));
    assert_eq!(f2.period_id, 7);

    let f3 = fixtures.get(3).unwrap();
    assert_eq!(f3.event_type, symbol_short!("rv_rej"));
    assert_eq!(f3.period_id, 7);

    let f4 = fixtures.get(4).unwrap();
    assert_eq!(f4.event_type, symbol_short!("rv_rep"));
    assert_eq!(f4.period_id, 7);

    let f5 = fixtures.get(5).unwrap();
    assert_eq!(f5.event_type, symbol_short!("claim"));
    assert_eq!(f5.period_id, 0);
}

#[test]
fn fixture_topics_bind_to_requested_identity() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("abc");

    let fixtures = client.get_indexer_fixture_topics(&issuer, &ns, &token, &42u64);
    for i in 0..fixtures.len() {
        let f = fixtures.get(i).unwrap();
        assert_eq!(f.issuer, issuer);
        assert_eq!(f.namespace, ns);
        assert_eq!(f.token, token);
        assert_eq!(f.version, 2);
    }
}
