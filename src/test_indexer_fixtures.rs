#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, testutils::Events as _, Address, Env};

use crate::{
    RevoraRevenueShare, RevoraRevenueShareClient, EVENT_SCHEMA_VERSION_V2,
    tax_bucket::EVENT_TAX_LOT_V1,
};

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
    client.register_offering(&admin, &Vec::new(&env), &1u32, &symbol_short!("def"), &token, &1_000, &payout_asset, &0, &symbol_short!(""), &0);
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
    assert_eq!(v2_fixtures.len(), 16);
    assert_eq!(v3_fixtures.len(), 16);

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

    let f15 = v2_fixtures.get(15).unwrap();
    assert_eq!(f15.event_type, symbol_short!("rg_lim_d"));
    assert_eq!(f15.period_id, 0);

    for i in 0..16 {
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
    client.register_offering(&admin, &Vec::new(&env), &1u32, &symbol_short!("def"), &token, &1_000, &payout_asset, &0, &symbol_short!(""), &0);

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
    client.register_offering(&admin, &Vec::new(&env), &1u32, &symbol_short!("def"), &token, &1_000, &payout_asset, &0, &symbol_short!(""), &0);

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
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &1_000, &1);

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
        symbol_short!("frz_rsn"),
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

// ── faucet_metrics_v1 (fct_mtr1) indexer fixture ─────────────────────────────

/// Fixture: `fct_mtr1` topic has the correct symbol.
///
/// Off-chain indexers should subscribe to events where `topics[0] == "fct_mtr1"`.
/// This test pins the symbol string so any accidental rename is caught immediately.
#[test]
fn fixture_fct_mtr1_topic_symbol_is_stable() {
    let env = Env::default();
    let expected: soroban_sdk::Symbol = symbol_short!("fct_mtr1");
    // The contract constant is pub(crate) — verify it via the symbol value
    let actual = crate::EVENT_FAUCET_METRICS;
    assert_eq!(actual, expected, "EVENT_FAUCET_METRICS must be fct_mtr1");
}

/// Fixture: `fct_mtr1` data tuple is `(u32, u32, u32, u64, u64)`.
///
/// Field order (for indexer deserialization):
/// 0. `total_dispensed : u32`
/// 1. `unique_addresses: u32`
/// 2. `cooldown_rejects: u32`
/// 3. `window_start    : u64`
/// 4. `window_end      : u64`
#[test]
fn fixture_fct_mtr1_data_tuple_shape() {
    let env = Env::default();
    env.mock_all_auths();
    let client = crate::RevoraRevenueShareClient::new(
        &env,
        &env.register_contract(None, crate::RevoraRevenueShare),
    );
    let admin = soroban_sdk::Address::generate(&env);
    client.initialize(&admin, &None::<soroban_sdk::Address>, &None::<bool>);
    client.set_testnet_mode(&true);

    let issuer = soroban_sdk::Address::generate(&env);
    let token = soroban_sdk::Address::generate(&env);
    let payout = soroban_sdk::Address::generate(&env);
    let ns = symbol_short!("fix");
    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &10_000, &payout, &0, &symbol_short!(""), &0u32);

    // Set ledger timestamp to a non-zero window (window_id = 1).
    env.ledger().set_timestamp(crate::FAUCET_METRICS_WINDOW_SECS);

    let requester = soroban_sdk::Address::generate(&env);
    client.faucet_seed_holders(&requester, &issuer, &ns, &token, &3);

    // Find the fct_mtr1 event and assert tuple shape.
    let fct_mtr1_val: soroban_sdk::Val = crate::EVENT_FAUCET_METRICS.into_val(&env);
    let mut found = false;
    for (_, topics, data) in env.events().all().iter() {
        if topics.len() >= 2 && topics.get(0).map(|t| t == fct_mtr1_val).unwrap_or(false) {
            let window_id: u64 = topics.get(1).unwrap().into_val(&env);
            let (total_dispensed, unique_addresses, cooldown_rejects, window_start, window_end):
                (u32, u32, u32, u64, u64) = data.into_val(&env);

            // Shape assertions (values are also deterministic here)
            assert_eq!(window_id, 1u64, "window_id = ts / FAUCET_METRICS_WINDOW_SECS");
            assert_eq!(total_dispensed, 3u32);
            assert_eq!(unique_addresses, 1u32);
            assert_eq!(cooldown_rejects, 0u32);
            assert_eq!(window_start, crate::FAUCET_METRICS_WINDOW_SECS);
            assert_eq!(window_end, crate::FAUCET_METRICS_WINDOW_SECS * 2 - 1);
            found = true;
            break;
        }
    }
    assert!(found, "fct_mtr1 event must be emitted and parseable by the indexer fixture");
}

/// Fixture: `fct_mtr1` window_id matches `timestamp / FAUCET_METRICS_WINDOW_SECS`.
///
/// Indexers must use this formula to bucket events into hourly aggregates.
#[test]
fn fixture_fct_mtr1_window_id_formula() {
    let env = Env::default();
    env.mock_all_auths();
    let client = crate::RevoraRevenueShareClient::new(
        &env,
        &env.register_contract(None, crate::RevoraRevenueShare),
    );
    let admin = soroban_sdk::Address::generate(&env);
    client.initialize(&admin, &None::<soroban_sdk::Address>, &None::<bool>);
    client.set_testnet_mode(&true);

    let issuer = soroban_sdk::Address::generate(&env);
    let token = soroban_sdk::Address::generate(&env);
    let payout = soroban_sdk::Address::generate(&env);
    let ns = symbol_short!("fix2");
    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &10_000, &payout, &0, &symbol_short!(""), &0u32);

    // ts = 7 * FAUCET_METRICS_WINDOW_SECS + 999  →  window_id = 7
    let ts = crate::FAUCET_METRICS_WINDOW_SECS * 7 + 999;
    env.ledger().set_timestamp(ts);

    let requester = soroban_sdk::Address::generate(&env);
    client.faucet_seed_holders(&requester, &issuer, &ns, &token, &1);

    let fct_mtr1_val: soroban_sdk::Val = crate::EVENT_FAUCET_METRICS.into_val(&env);
    let mut found_window_id: Option<u64> = None;
    for (_, topics, _) in env.events().all().iter() {
        if topics.len() >= 2 && topics.get(0).map(|t| t == fct_mtr1_val).unwrap_or(false) {
            found_window_id = Some(topics.get(1).unwrap().into_val(&env));
        }
    }

    let window_id = found_window_id.expect("fct_mtr1 must be emitted");
    assert_eq!(
        window_id,
        ts / crate::FAUCET_METRICS_WINDOW_SECS,
        "window_id must equal ts / FAUCET_METRICS_WINDOW_SECS"
    );
}

// ── tax_lot_v1 indexer fixture ───────────────────────────────────────────────

/// Fixture: `tax_lt1` topic has the correct symbol.
///
/// Off-chain indexers should subscribe to events where `topics[0] == "tax_lt1"`.
/// This test pins the symbol string so any accidental rename is caught immediately.
#[test]
fn fixture_tax_lot_v1_topic_symbol_is_stable() {
    let env = Env::default();
    let expected: soroban_sdk::Symbol = symbol_short!("tax_lt1");
    assert_eq!(EVENT_TAX_LOT_V1, expected, "EVENT_TAX_LOT_V1 must be tax_lt1");
}

/// Fixture: `tax_lt1` data tuple shape is validated on a successful claim.
///
/// Field order (for indexer deserialization):
/// 0. `holder`            — Address of the holder.
/// 1. `return_of_capital` — i128, non-taxable portion.
/// 2. `capital_gains`     — i128, taxable portion.
/// 3. `amount`            — i128, total payout (roc + cg).
/// 4. `period_id`         — u64, the period claimed.
/// 5. `timestamp`         — u64, ledger timestamp at claim time.
#[test]
fn fixture_tax_lot_v1_data_tuple_shape() {
    let env = Env::default();
    env.mock_all_auths();

    // Register a Stellar asset for the payout token so claim can transfer.
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = env.register_stellar_asset_contract_v2(admin.clone()).address();
    soroban_sdk::token::StellarAssetClient::new(&env, &payout).mint(&admin, &1_000_000);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = admin.clone();
    let ns = symbol_short!("tx");
    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &10_000, &payout, &0, &symbol_short!(""), &0);

    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &ns, &token, &holder, &5_000, &1); // 50%

    // Track cost basis so we get a return_of_capital component.
    let offering_id = crate::OfferingId {
        issuer: issuer.clone(),
        namespace: ns.clone(),
        token: token.clone(),
    };
    crate::tax_bucket::track_cost_basis(&env, &offering_id, &holder, 100_000);

    client.deposit_revenue(&issuer, &ns, &token, &payout, &100_000, &1);

    let before = env.events().all().len();
    client.claim(&holder, &issuer, &ns, &token, &10);

    // Find the tax_lt1 event among the new events.
    let tax_lt1_val: soroban_sdk::Val = EVENT_TAX_LOT_V1.into_val(&env);
    let mut found = false;
    for (_, topics, data) in env.events().all().slice(before as u32..).iter() {
        if topics.len() >= 4
            && topics.get(0).map(|t| t == tax_lt1_val).unwrap_or(false)
        {
            let (holder_addr, return_of_capital, capital_gains, amount, period_id, timestamp):
                (Address, i128, i128, i128, u64, u64) = data.into_val(&env);

            assert_eq!(holder_addr, holder);
            assert!(return_of_capital > 0, "return_of_capital must be positive");
            assert_eq!(capital_gains, 0i128, "capital_gains must be zero when basis is sufficient");
            assert_eq!(amount, return_of_capital + capital_gains,
                "decomposition invariant: return_of_capital + capital_gains must equal amount");
            assert_eq!(amount, 5_000i128); // 50% of 100_000 = 50_000 normalized
            assert_eq!(period_id, 1u64);
            assert!(timestamp > 0, "timestamp must be positive");
            found = true;
            break;
        }
    }
    assert!(found, "tax_lt1 event must be emitted on successful claim");
}

/// `tax_lt1` event correctly reports capital_gains when remaining basis is exhausted.
#[test]
fn fixture_tax_lot_v1_capital_gains_when_basis_exhausted() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = env.register_stellar_asset_contract_v2(admin.clone()).address();
    soroban_sdk::token::StellarAssetClient::new(&env, &payout).mint(&admin, &1_000_000);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = admin.clone();
    let ns = symbol_short!("txcg");
    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &10_000, &payout, &0, &symbol_short!(""), &0);

    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &ns, &token, &holder, &10_000, &1); // 100%

    let offering_id = crate::OfferingId {
        issuer: issuer.clone(),
        namespace: ns.clone(),
        token: token.clone(),
    };
    // Track small cost basis so payout exceeds it → capital_gains > 0.
    crate::tax_bucket::track_cost_basis(&env, &offering_id, &holder, 1_000);

    client.deposit_revenue(&issuer, &ns, &token, &payout, &100_000, &1);

    let before = env.events().all().len();
    client.claim(&holder, &issuer, &ns, &token, &10);

    let tax_lt1_val: soroban_sdk::Val = EVENT_TAX_LOT_V1.into_val(&env);
    let mut found = false;
    for (_, topics, data) in env.events().all().slice(before as u32..).iter() {
        if topics.len() >= 4
            && topics.get(0).map(|t| t == tax_lt1_val).unwrap_or(false)
        {
            let (holder_addr, return_of_capital, capital_gains, amount, _period_id, _timestamp):
                (Address, i128, i128, i128, u64, u64) = data.into_val(&env);

            assert_eq!(holder_addr, holder);
            assert_eq!(return_of_capital, 1_000i128, "return_of_capital should equal remaining basis");
            assert!(capital_gains > 0, "capital_gains must be positive when basis is exceeded");
            assert_eq!(amount, return_of_capital + capital_gains,
                "decomposition invariant: return_of_capital + capital_gains must equal amount");
            found = true;
            break;
        }
    }
    assert!(found, "tax_lt1 event must be emitted on claim with capital gains");
}

/// No `tax_lt1` event when claim returns zero (share_bps = 0).
#[test]
fn fixture_tax_lot_v1_zero_payout_emits_no_event() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = env.register_stellar_asset_contract_v2(admin.clone()).address();
    soroban_sdk::token::StellarAssetClient::new(&env, &payout).mint(&admin, &1_000_000);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = admin.clone();
    let ns = symbol_short!("tz");
    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &10_000, &payout, &0, &symbol_short!(""), &0);

    let holder = Address::generate(&env);
    // No share set → share_bps = 0 → claim returns NoPendingClaims before any payout.

    client.deposit_revenue(&issuer, &ns, &token, &payout, &100_000, &1);

    let before = env.events().all().len();
    let result = client.try_claim(&holder, &issuer, &ns, &token, &10);
    assert_eq!(
        result,
        Err(Ok(crate::RevoraError::NoPendingClaims)),
        "claim with zero share must fail with NoPendingClaims"
    );

    // Scan for any tax_lt1 event — must be absent.
    let tax_lt1_val: soroban_sdk::Val = EVENT_TAX_LOT_V1.into_val(&env);
    for (_, topics, _) in env.events().all().slice(before as u32..).iter() {
        if topics.len() >= 4
            && topics.get(0).map(|t| t == tax_lt1_val).unwrap_or(false)
        {
            panic!("tax_lt1 must NOT be emitted when claim fails (share_bps = 0)");
        }
    }
}

/// Burst: claiming N separate payout batches emits exactly N `tax_lt1` events.
#[test]
fn fixture_tax_lot_v1_burst_emits_n_events() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = env.register_stellar_asset_contract_v2(admin.clone()).address();
    soroban_sdk::token::StellarAssetClient::new(&env, &payout).mint(&admin, &1_000_000);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = admin.clone();
    let ns = symbol_short!("txb");
    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &10_000, &payout, &0, &symbol_short!(""), &0);

    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &ns, &token, &holder, &5_000, &1); // 50%

    let offering_id = crate::OfferingId {
        issuer: issuer.clone(),
        namespace: ns.clone(),
        token: token.clone(),
    };
    // Track sufficient cost basis so all claims get return_of_capital.
    crate::tax_bucket::track_cost_basis(&env, &offering_id, &holder, 1_000_000);

    // Deposit revenue for 3 periods.
    client.deposit_revenue(&issuer, &ns, &token, &payout, &100_000, &1);
    client.deposit_revenue(&issuer, &ns, &token, &payout, &200_000, &2);
    client.deposit_revenue(&issuer, &ns, &token, &payout, &300_000, &3);

    let before = env.events().all().len();

    // First claim: periods 1 & 2
    client.claim(&holder, &issuer, &ns, &token, &2);

    // Second claim: period 3
    client.claim(&holder, &issuer, &ns, &token, &10);

    let new_events = env.events().all().slice(before as u32..);
    let tax_lt1_val: soroban_sdk::Val = EVENT_TAX_LOT_V1.into_val(&env);
    let mut tax_lot_count = 0u32;
    for (_, topics, _) in new_events.iter() {
        if topics.len() >= 4
            && topics.get(0).map(|t| t == tax_lt1_val).unwrap_or(false)
        {
            tax_lot_count += 1;
        }
    }
    assert_eq!(tax_lot_count, 2, "2 `tax_lt1` events expected for 2 claim calls");
}
