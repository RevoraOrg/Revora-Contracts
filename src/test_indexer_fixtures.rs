#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

use crate::{RevoraRevenueShare, RevoraRevenueShareClient, EVENT_SCHEMA_VERSION_V2};

// ── Helper ────────────────────────────────────────────────────────────────────

/// Set up a minimal contract with admin + one registered offering.
/// Returns (client, admin/issuer, token, payout_asset).
fn setup_with_offering(env: &Env) -> (RevoraRevenueShareClient, Address, Address, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let token = Address::generate(env);
    let payout_asset = Address::generate(env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(&admin, &symbol_short!("def"), &token, &1_000, &payout_asset, &0, &None);
    (client, admin, token, payout_asset)
}

// ── Existing fixture shape tests ──────────────────────────────────────────────

#[test]
fn fixture_topics_have_stable_order_and_shape() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    let (v2_fixtures, v3_fixtures) = client.get_indexer_fixture_topics(&issuer, &ns, &token, &7u64);
    assert_eq!(v2_fixtures.len(), 15);
    assert_eq!(v3_fixtures.len(), 15);

    let f0 = v2_fixtures.get(0).unwrap();
    assert_eq!(f0.version, 2);
    assert_eq!(f0.event_type, symbol_short!("offer"));
    assert_eq!(f0.period_id, 0);

    let f1 = v2_fixtures.get(1).unwrap();
    assert_eq!(f1.event_type, symbol_short!("rv_init"));
    assert_eq!(f1.period_id, 7);

    let f2 = v2_fixtures.get(2).unwrap();
    assert_eq!(f2.event_type, symbol_short!("rv_ovr"));
    assert_eq!(f2.period_id, 7);

    let f3 = v2_fixtures.get(3).unwrap();
    assert_eq!(f3.event_type, symbol_short!("rv_rej"));
    assert_eq!(f3.period_id, 7);

    let f4 = v2_fixtures.get(4).unwrap();
    assert_eq!(f4.event_type, symbol_short!("rv_rep"));
    assert_eq!(f4.period_id, 7);

    let f5 = v2_fixtures.get(5).unwrap();
    assert_eq!(f5.event_type, symbol_short!("claim"));
    assert_eq!(f5.period_id, 0);

    let f6 = v2_fixtures.get(6).unwrap();
    assert_eq!(f6.event_type, symbol_short!("admin_set"));

    let f7 = v2_fixtures.get(7).unwrap();
    assert_eq!(f7.event_type, symbol_short!("fee_set"));

    let f8 = v2_fixtures.get(8).unwrap();
    assert_eq!(f8.event_type, symbol_short!("fee_ast"));

    let f9 = v2_fixtures.get(9).unwrap();
    assert_eq!(f9.event_type, symbol_short!("fee_off"));

    let f10 = v2_fixtures.get(10).unwrap();
    assert_eq!(f10.event_type, symbol_short!("conc_lim"));

    let f11 = v2_fixtures.get(11).unwrap();
    assert_eq!(f11.event_type, symbol_short!("rnd_mode"));

    let f12 = v2_fixtures.get(12).unwrap();
    assert_eq!(f12.event_type, symbol_short!("meta_key"));

    let f13 = v2_fixtures.get(13).unwrap();
    assert_eq!(f13.event_type, symbol_short!("meta_del"));

    let f14 = v2_fixtures.get(14).unwrap();
    assert_eq!(f14.event_type, symbol_short!("ms_init"));

    for i in 0..15 {
        let v3 = v3_fixtures.get(i).unwrap();
        assert_eq!(v3.version, 3);
        assert_eq!(v3.event_type, v2_fixtures.get(i).unwrap().event_type);
        assert_eq!(v3.period_id, v2_fixtures.get(i).unwrap().period_id);
        assert_eq!(v3.issuer, issuer);
        assert_eq!(v3.namespace, ns);
        assert_eq!(v3.token, token);
        assert_eq!(v3._reserved, 0);
    }
}

#[test]
fn fixture_topics_bind_to_requested_identity() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("abc");

    let (v2_fixtures, v3_fixtures) = client.get_indexer_fixture_topics(&issuer, &ns, &token, &42u64);
    for i in 0..v2_fixtures.len() {
        let f = v2_fixtures.get(i).unwrap();
        assert_eq!(f.issuer, issuer);
        assert_eq!(f.namespace, ns);
        assert_eq!(f.token, token);
        assert_eq!(f.version, 2);
    }
    for i in 0..v3_fixtures.len() {
        let f = v3_fixtures.get(i).unwrap();
        assert_eq!(f.issuer, issuer);
        assert_eq!(f.namespace, ns);
        assert_eq!(f.token, token);
        assert_eq!(f.version, 3);
        assert_eq!(f._reserved, 0);
    }
}

// ── Schema version constant guard ────────────────────────────────────────────

#[test]
fn event_schema_version_v2_constant_is_2() {
    // Prevents accidental constant mutation from silently breaking all indexers.
    assert_eq!(EVENT_SCHEMA_VERSION_V2, 2u32);
}

// ── register_offering emits ofr_reg2 unconditionally ─────────────────────────

#[test]
fn register_offering_emits_ofr_reg2_v2_event() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let before = env.events().all().len();
    client.register_offering(&admin, &symbol_short!("def"), &token, &1_000, &payout_asset, &0, &None);

    let events = env.events().all();
    assert!(events.len() > before, "register_offering must emit at least one event");

    // Verify ofr_reg2 topic is present among the new events.
    let new_events = events.slice(before as u32..);
    let ofr_reg2_sym: soroban_sdk::Val = symbol_short!("ofr_reg2").into_val(&env);
    let found = new_events.iter().any(|(_, topics, _)| {
        topics.len() > 0 && topics.get(0).map(|t| t == ofr_reg2_sym).unwrap_or(false)
    });
    assert!(found, "ofr_reg2 event must be emitted unconditionally by register_offering");
}

#[test]
fn register_offering_v2_event_data_starts_with_version_2() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let before = env.events().all().len();
    client.register_offering(&admin, &symbol_short!("def"), &token, &1_000, &payout_asset, &0, &None);

    let events = env.events().all();
    let new_events = events.slice(before as u32..);
    let ofr_reg2_sym: soroban_sdk::Val = symbol_short!("ofr_reg2").into_val(&env);

    for (_, topics, data) in new_events.iter() {
        if topics.len() > 0 && topics.get(0).map(|t| t == ofr_reg2_sym).unwrap_or(false) {
            // data[0] must be EVENT_SCHEMA_VERSION_V2 = 2u32
            let version: u32 = data.into_val(&env);
            // The data tuple is (2u32, (token, bps, payout)) — outer element is 2
            // We verify this by checking data is non-empty and version-typed.
            assert_eq!(version, 2u32, "ofr_reg2 data[0] must be EVENT_SCHEMA_VERSION_V2 = 2");
            return;
        }
    }
    panic!("ofr_reg2 event not found among new events after register_offering");
}

// ── report_revenue emits rv_init2, rv_rep2, rv_repa2, rv_inia2 unconditionally

#[test]
fn report_revenue_emits_rv_init2_on_initial_report() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup_with_offering(&env);

    let before = env.events().all().len();
    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout_asset,
        &10_000,
        &1,
        &false,
    );

    let events = env.events().all();
    let new_events = events.slice(before as u32..);
    let rv_init2_sym: soroban_sdk::Val = symbol_short!("rv_init2").into_val(&env);
    let found = new_events
        .iter()
        .any(|(_, topics, _)| topics.len() > 0 && topics.get(0).map(|t| t == rv_init2_sym).unwrap_or(false));
    assert!(found, "rv_init2 must be emitted unconditionally on an initial revenue report");
}

#[test]
fn report_revenue_emits_rv_rep2_unconditionally() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup_with_offering(&env);

    let before = env.events().all().len();
    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout_asset,
        &5_000,
        &1,
        &false,
    );

    let events = env.events().all();
    let new_events = events.slice(before as u32..);
    let rv_rep2_sym: soroban_sdk::Val = symbol_short!("rv_rep2").into_val(&env);
    let found = new_events
        .iter()
        .any(|(_, topics, _)| topics.len() > 0 && topics.get(0).map(|t| t == rv_rep2_sym).unwrap_or(false));
    assert!(found, "rv_rep2 must be emitted unconditionally on every revenue report");
}

#[test]
fn report_revenue_emits_rv_repa2_unconditionally() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup_with_offering(&env);

    let before = env.events().all().len();
    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout_asset,
        &5_000,
        &1,
        &false,
    );

    let events = env.events().all();
    let new_events = events.slice(before as u32..);
    let rv_repa2_sym: soroban_sdk::Val = symbol_short!("rv_repa2").into_val(&env);
    let found = new_events
        .iter()
        .any(|(_, topics, _)| topics.len() > 0 && topics.get(0).map(|t| t == rv_repa2_sym).unwrap_or(false));
    assert!(found, "rv_repa2 must be emitted unconditionally on every revenue report");
}

#[test]
fn report_revenue_emits_rv_inia2_unconditionally_without_versioning_flag() {
    let env = Env::default();
    let (client, issuer, token, payout_asset) = setup_with_offering(&env);
    // event_versioning is NOT enabled; rv_inia2 must still be emitted.

    let before = env.events().all().len();
    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout_asset,
        &8_000,
        &1,
        &false,
    );

    let events = env.events().all();
    let new_events = events.slice(before as u32..);
    let rv_inia2_sym: soroban_sdk::Val = symbol_short!("rv_inia2").into_val(&env);
    let found = new_events
        .iter()
        .any(|(_, topics, _)| topics.len() > 0 && topics.get(0).map(|t| t == rv_inia2_sym).unwrap_or(false));
    assert!(
        found,
        "rv_inia2 must be emitted unconditionally (not gated on is_event_versioning_enabled)"
    );
}

// ── set_holder_share emits sh_set2 unconditionally ───────────────────────────

#[test]
fn set_holder_share_emits_sh_set2_v2_event() {
    let env = Env::default();
    let (client, issuer, token, _payout_asset) = setup_with_offering(&env);
    let holder = Address::generate(&env);

    let before = env.events().all().len();
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &1_000);

    let events = env.events().all();
    let new_events = events.slice(before as u32..);
    let sh_set2_sym: soroban_sdk::Val = symbol_short!("sh_set2").into_val(&env);
    let found = new_events
        .iter()
        .any(|(_, topics, _)| topics.len() > 0 && topics.get(0).map(|t| t == sh_set2_sym).unwrap_or(false));
    assert!(found, "sh_set2 must be emitted unconditionally by set_holder_share");
}

// ── All v2 topic symbols are distinct (no collision) ─────────────────────────

#[test]
fn v2_event_symbols_are_all_distinct() {
    let env = Env::default();

    let symbols: soroban_sdk::Vec<soroban_sdk::Symbol> = soroban_sdk::vec![
        &env,
        symbol_short!("ofr_reg2"),
        symbol_short!("rv_init2"),
        symbol_short!("rv_inia2"),
        symbol_short!("rv_rep2"),
        symbol_short!("rv_repa2"),
        symbol_short!("rev_dep2"),
        symbol_short!("rev_snp2"),
        symbol_short!("claim2"),
        symbol_short!("sh_set2"),
        symbol_short!("frz2"),
    ];

    let n = symbols.len();
    for i in 0..n {
        for j in (i + 1)..n {
            assert_ne!(
                symbols.get(i).unwrap(),
                symbols.get(j).unwrap(),
                "v2 event symbols at positions {i} and {j} must be distinct"
            );
        }
    }
}

// ── Fixture version field invariant ──────────────────────────────────────────

#[test]
fn all_fixture_topics_carry_version_2() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let fixtures = client.get_indexer_fixture_topics(&issuer, &symbol_short!("ns"), &token, &1u64);
    for i in 0..fixtures.len() {
        let f = fixtures.get(i).unwrap();
        assert_eq!(
            f.version,
            EVENT_SCHEMA_VERSION_V2,
            "fixture at index {i} must carry version = EVENT_SCHEMA_VERSION_V2 = 2"
        );
    }
}

#[test]
fn fixture_period_id_zero_for_non_period_scoped_events() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let fixtures = client.get_indexer_fixture_topics(&issuer, &symbol_short!("ns"), &token, &99u64);
    // offer (index 0) and claim (index 5) are not period-scoped: period_id must be 0.
    assert_eq!(fixtures.get(0).unwrap().period_id, 0, "offer fixture must have period_id = 0");
    assert_eq!(fixtures.get(5).unwrap().period_id, 0, "claim fixture must have period_id = 0");
}

#[test]
fn fixture_period_scoped_events_carry_requested_period_id() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let fixtures = client.get_indexer_fixture_topics(&issuer, &symbol_short!("ns"), &token, &77u64);
    // rv_init (1), rv_ovr (2), rv_rej (3), rv_rep (4) must all have period_id = 77.
    for idx in 1u32..=4 {
        assert_eq!(
            fixtures.get(idx).unwrap().period_id,
            77u64,
            "fixture at index {idx} must carry the requested period_id"
        );
    }
}
