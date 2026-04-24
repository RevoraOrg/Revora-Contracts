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
    assert_eq!(fixtures.len(), 23);

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

    let f6 = fixtures.get(6).unwrap();
    assert_eq!(f6.event_type, symbol_short!("rv_dep"));
    assert_eq!(f6.period_id, 7);

    let f7 = fixtures.get(7).unwrap();
    assert_eq!(f7.event_type, symbol_short!("sh_set"));
    assert_eq!(f7.period_id, 0);

    let f8 = fixtures.get(8).unwrap();
    assert_eq!(f8.event_type, symbol_short!("bl_add"));
    assert_eq!(f8.period_id, 0);

    let f9 = fixtures.get(9).unwrap();
    assert_eq!(f9.event_type, symbol_short!("bl_rem"));
    assert_eq!(f9.period_id, 0);

    let f10 = fixtures.get(10).unwrap();
    assert_eq!(f10.event_type, symbol_short!("sn_com"));
    assert_eq!(f10.period_id, 0);

    let f11 = fixtures.get(11).unwrap();
    assert_eq!(f11.event_type, symbol_short!("sn_shr"));
    assert_eq!(f11.period_id, 0);

    let f12 = fixtures.get(12).unwrap();
    assert_eq!(f12.event_type, symbol_short!("fee_cfg"));
    assert_eq!(f12.period_id, 0);

    let f13 = fixtures.get(13).unwrap();
    assert_eq!(f13.event_type, symbol_short!("min_rev"));
    assert_eq!(f13.period_id, 0);

    let f14 = fixtures.get(14).unwrap();
    assert_eq!(f14.event_type, symbol_short!("round"));
    assert_eq!(f14.period_id, 0);

    let f15 = fixtures.get(15).unwrap();
    assert_eq!(f15.event_type, symbol_short!("conc"));
    assert_eq!(f15.period_id, 0);

    let f16 = fixtures.get(16).unwrap();
    assert_eq!(f16.event_type, symbol_short!("delay"));
    assert_eq!(f16.period_id, 0);

    let f17 = fixtures.get(17).unwrap();
    assert_eq!(f17.event_type, symbol_short!("ms_init"));
    assert_eq!(f17.period_id, 0);

    let f18 = fixtures.get(18).unwrap();
    assert_eq!(f18.event_type, symbol_short!("meta_set"));
    assert_eq!(f18.period_id, 0);

    let f19 = fixtures.get(19).unwrap();
    assert_eq!(f19.event_type, symbol_short!("meta_upd"));
    assert_eq!(f19.period_id, 0);

    let f20 = fixtures.get(20).unwrap();
    assert_eq!(f20.event_type, symbol_short!("inv_con"));
    assert_eq!(f20.period_id, 0);

    let f21 = fixtures.get(21).unwrap();
    assert_eq!(f21.event_type, symbol_short!("adm_set"));
    assert_eq!(f21.period_id, 0);

    let f22 = fixtures.get(22).unwrap();
    assert_eq!(f22.event_type, symbol_short!("plat_fee"));
    assert_eq!(f22.period_id, 0);
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
