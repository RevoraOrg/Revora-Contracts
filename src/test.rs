#![cfg(test)]
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    token, vec, Address, Env, IntoVal, Vec,
};

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, RoundingMode};

// ── helper ────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

const BOUNDARY_AMOUNTS: [i128; 7] = [i128::MIN, i128::MIN + 1, -1, 0, 1, i128::MAX - 1, i128::MAX];
const BOUNDARY_PERIODS: [u64; 6] = [0, 1, 2, 10_000, u64::MAX - 1, u64::MAX];
const FUZZ_ITERATIONS: usize = 128;

fn next_u64(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed
}

fn next_amount(seed: &mut u64) -> i128 {
    let hi = next_u64(seed) as u128;
    let lo = next_u64(seed) as u128;
    ((hi << 64) | lo) as i128
}

fn next_period(seed: &mut u64) -> u64 {
    next_u64(seed)
}

// ─── Event-to-flow mapping ───────────────────────────────────────────────────
//
//  Flow: Offering Registration  (register_offering)
//    topic[0] = Symbol("offer_reg")
//    topic[1] = Address  (issuer)
//    data     = (Address (token), u32 (revenue_share_bps))
//
//  Flow: Revenue Report  (report_revenue)
//    topic[0] = Symbol("rev_rep")
//    topic[1] = Address  (issuer)
//    topic[2] = Address  (token)
//    data     = (i128 (amount), u64 (period_id), Vec<Address> (blacklist))
//
// ─────────────────────────────────────────────────────────────────────────────

// ── Single-event structure tests ─────────────────────────────────────────────

#[test]
fn register_offering_emits_exact_event() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let bps: u32 = 1_500;

    client.register_offering(&issuer, &token, &bps, &token);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token.clone(), bps, token.clone()).into_val(&env),
            ),
        ]
    );
}

#[test]
fn report_revenue_emits_exact_event() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let amount: i128 = 5_000_000;
    let period_id: u64 = 42;

    client.register_offering(&issuer, &token, &1000, &token);
    client.report_revenue(&issuer, &token, &token, &amount, &period_id, &false);

    let empty_bl = Vec::<Address>::new(&env);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token.clone(), 1000_u32, token.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer.clone(), token.clone()).into_val(&env),
                (amount, period_id, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (amount, period_id, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer.clone(), token.clone()).into_val(&env),
                (amount, period_id, empty_bl).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (amount, period_id).into_val(&env),
            ),
        ]
    );
}

// ── Ordering tests ───────────────────────────────────────────────────────────

#[test]
fn combined_flow_preserves_event_order() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let bps: u32 = 1_000;
    let amount: i128 = 1_000_000;
    let period_id: u64 = 1;

    client.register_offering(&issuer, &token, &bps, &token);
    client.report_revenue(&issuer, &token, &token, &amount, &period_id, &false);

    let events = env.events().all();
    assert_eq!(events.len(), 5);

    let empty_bl = Vec::<Address>::new(&env);
    assert_eq!(
        events,
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token.clone(), bps, token.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer.clone(), token.clone()).into_val(&env),
                (amount, period_id, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (amount, period_id, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer.clone(), token.clone()).into_val(&env),
                (amount, period_id, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (amount, period_id).into_val(&env),
            ),
        ]
    );
}

#[test]
fn complex_mixed_flow_events_in_order() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token_x = Address::generate(&env);
    let token_y = Address::generate(&env);

    // Interleave: register A, register B, report A, report B
    client.register_offering(&issuer_a, &token_x, &500, &token_x);
    client.register_offering(&issuer_b, &token_y, &750, &token_y);
    client.report_revenue(&issuer_a, &token_x, &token_x, &100_000, &1, &false);
    client.report_revenue(&issuer_b, &token_y, &token_y, &200_000, &1, &false);

    let events = env.events().all();
    assert_eq!(events.len(), 10);

    let empty_bl = Vec::<Address>::new(&env);
    assert_eq!(
        events,
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer_a.clone()).into_val(&env),
                (token_x.clone(), 500u32, token_x.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer_b.clone()).into_val(&env),
                (token_y.clone(), 750u32, token_y.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer_a.clone(), token_x.clone()).into_val(&env),
                (100_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer_a.clone(),
                    token_x.clone(),
                    token_x.clone(),
                )
                    .into_val(&env),
                (100_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer_a.clone(), token_x.clone()).into_val(&env),
                (100_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer_a.clone(),
                    token_x.clone(),
                    token_x.clone(),
                )
                    .into_val(&env),
                (100_000i128, 1u64).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer_b.clone(), token_y.clone()).into_val(&env),
                (200_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer_b.clone(),
                    token_y.clone(),
                    token_y.clone(),
                )
                    .into_val(&env),
                (200_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer_b.clone(), token_y.clone()).into_val(&env),
                (200_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer_b.clone(),
                    token_y.clone(),
                    token_y.clone(),
                )
                    .into_val(&env),
                (200_000i128, 1u64).into_val(&env),
            ),
        ]
    );
}

// ── Multi-entity tests ───────────────────────────────────────────────────────

#[test]
fn multiple_offerings_emit_distinct_events() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let token_c = Address::generate(&env);

    client.register_offering(&issuer, &token_a, &100, &token_a);
    client.register_offering(&issuer, &token_b, &200, &token_b);
    client.register_offering(&issuer, &token_c, &300, &token_c);

    let events = env.events().all();
    assert_eq!(events.len(), 3);

    assert_eq!(
        events,
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token_a.clone(), 100u32, token_a.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token_b.clone(), 200u32, token_b.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token_c.clone(), 300u32, token_c.clone()).into_val(&env),
            ),
        ]
    );
}

#[test]
fn multiple_revenue_reports_same_offering() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &1000, &token);
    client.report_revenue(&issuer, &token, &token, &10_000, &1, &false);
    client.report_revenue(&issuer, &token, &token, &20_000, &2, &false);
    client.report_revenue(&issuer, &token, &token, &30_000, &3, &false);

    let events = env.events().all();
    assert_eq!(events.len(), 13);

    let empty_bl = Vec::<Address>::new(&env);
    assert_eq!(
        events,
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token.clone(), 1000_u32, token.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer.clone(), token.clone()).into_val(&env),
                (10_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (10_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer.clone(), token.clone()).into_val(&env),
                (10_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (10_000i128, 1u64).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer.clone(), token.clone()).into_val(&env),
                (20_000i128, 2u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (20_000i128, 2u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer.clone(), token.clone()).into_val(&env),
                (20_000i128, 2u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (20_000i128, 2u64).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer.clone(), token.clone()).into_val(&env),
                (30_000i128, 3u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (30_000i128, 3u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer.clone(), token.clone()).into_val(&env),
                (30_000i128, 3u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (30_000i128, 3u64).into_val(&env),
            ),
        ]
    );
}

#[test]
fn same_issuer_different_tokens() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token_x = Address::generate(&env);
    let token_y = Address::generate(&env);

    client.register_offering(&issuer, &token_x, &1_000, &token_x);
    client.register_offering(&issuer, &token_y, &2_000, &token_y);
    client.report_revenue(&issuer, &token_x, &token_x, &500_000, &1, &false);
    client.report_revenue(&issuer, &token_y, &token_y, &750_000, &1, &false);

    let events = env.events().all();
    assert_eq!(events.len(), 10);

    let empty_bl = Vec::<Address>::new(&env);
    assert_eq!(
        events,
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token_x.clone(), 1_000u32, token_x.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token_y.clone(), 2_000u32, token_y.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer.clone(), token_x.clone()).into_val(&env),
                (500_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer.clone(),
                    token_x.clone(),
                    token_x.clone()
                )
                    .into_val(&env),
                (500_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer.clone(), token_x.clone()).into_val(&env),
                (500_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer.clone(),
                    token_x.clone(),
                    token_x.clone()
                )
                    .into_val(&env),
                (500_000i128, 1u64).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer.clone(), token_y.clone()).into_val(&env),
                (750_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer.clone(),
                    token_y.clone(),
                    token_y.clone()
                )
                    .into_val(&env),
                (750_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer.clone(), token_y.clone()).into_val(&env),
                (750_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer.clone(),
                    token_y.clone(),
                    token_y.clone()
                )
                    .into_val(&env),
                (750_000i128, 1u64).into_val(&env),
            ),
        ]
    );
}

// ── Topic / symbol inspection tests ──────────────────────────────────────────

#[test]
fn topic_symbols_are_distinct() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &1_000, &token);
    client.report_revenue(&issuer, &token, &token, &1_000_000, &1, &false);

    let empty_bl = Vec::<Address>::new(&env);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token.clone(), 1_000u32, token.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer.clone(), token.clone()).into_val(&env),
                (1_000_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (1_000_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer.clone(), token.clone()).into_val(&env),
                (1_000_000i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (1_000_000i128, 1u64).into_val(&env),
            ),
        ]
    );
}

#[test]
fn rev_rep_topics_include_token_address() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &1000, &token);
    client.report_revenue(&issuer, &token, &token, &999, &7, &false);

    let empty_bl = Vec::<Address>::new(&env);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token.clone(), 1000_u32, token.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer.clone(), token.clone()).into_val(&env),
                (999i128, 7u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (999i128, 7u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer.clone(), token.clone()).into_val(&env),
                (999i128, 7u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (999i128, 7u64).into_val(&env),
            ),
        ]
    );
}

// ── Boundary / edge-case tests ───────────────────────────────────────────────

#[test]
fn zero_bps_offering() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &0, &token);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token.clone(), 0u32, token.clone()).into_val(&env),
            ),
        ]
    );
}

#[test]
fn max_bps_offering() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // 10_000 bps == 100%
    client.register_offering(&issuer, &token, &10_000, &token);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token.clone(), 10_000u32, token.clone()).into_val(&env),
            ),
        ]
    );
}

#[test]
fn zero_amount_revenue_report() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &1000, &token);
    client.report_revenue(&issuer, &token, &token, &0, &1, &false);

    let empty_bl = Vec::<Address>::new(&env);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token.clone(), 1000_u32, token.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer.clone(), token.clone()).into_val(&env),
                (0i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (0i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer.clone(), token.clone()).into_val(&env),
                (0i128, 1u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (0i128, 1u64).into_val(&env),
            ),
        ]
    );
}

#[test]
fn large_revenue_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let large_amount: i128 = i128::MAX;
    client.register_offering(&issuer, &token, &1000, &token);
    client.report_revenue(&issuer, &token, &token, &large_amount, &u64::MAX, &false);

    let empty_bl = Vec::<Address>::new(&env);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token.clone(), 1000_u32, token.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer.clone(), token.clone()).into_val(&env),
                (large_amount, u64::MAX, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (large_amount, u64::MAX, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer.clone(), token.clone()).into_val(&env),
                (large_amount, u64::MAX, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (large_amount, u64::MAX).into_val(&env),
            ),
        ]
    );
}

#[test]
fn negative_revenue_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // Negative revenue (e.g. clawback / adjustment)
    let negative: i128 = -500_000;
    client.register_offering(&issuer, &token, &1000, &token);
    client.report_revenue(&issuer, &token, &token, &negative, &99, &false);

    let empty_bl = Vec::<Address>::new(&env);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("offer_reg"), issuer.clone()).into_val(&env),
                (token.clone(), 1000_u32, token.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_init"), issuer.clone(), token.clone()).into_val(&env),
                (negative, 99u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_inia"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (negative, 99u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("rev_rep"), issuer.clone(), token.clone()).into_val(&env),
                (negative, 99u64, empty_bl.clone()).into_val(&env),
            ),
            (
                contract_id.clone(),
                (
                    symbol_short!("rev_repa"),
                    issuer.clone(),
                    token.clone(),
                    token.clone()
                )
                    .into_val(&env),
                (negative, 99u64).into_val(&env),
            ),
        ]
    );
}

// ── original smoke test ───────────────────────────────────────

#[test]
fn it_emits_events_on_register_and_report() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    client.report_revenue(&issuer, &token, &payout_asset, &1_000_000, &1, &false);

    assert!(env.events().all().len() >= 2);
}

#[test]
fn it_emits_versioned_events() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    let bps: u32 = 1_000;
    let amount: i128 = 1_000_000;
    let period_id: u64 = 1;

    // enable versioned events for this test
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&crate::DataKey::EventVersioningEnabled, &true);
    });

    client.register_offering(&issuer, &token, &bps, &payout);
    client.report_revenue(&issuer, &token, &payout, &amount, &period_id, &false);

    let events = env.events().all();

    let expected = (
        contract_id.clone(),
        (symbol_short!("ofr_reg1"), issuer.clone()).into_val(&env),
        (
            crate::EVENT_SCHEMA_VERSION,
            token.clone(),
            bps,
            payout.clone(),
        )
            .into_val(&env),
    );

    assert!(events.contains(&expected));
}

// ── period/amount fuzz coverage ───────────────────────────────

#[test]
fn fuzz_period_and_amount_boundaries_do_not_panic() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);

    let mut calls = 0usize;
    for amount in BOUNDARY_AMOUNTS {
        for period in BOUNDARY_PERIODS {
            client.report_revenue(&issuer, &token, &payout_asset, &amount, &period, &false);
            calls += 1;
        }
    }

    assert_eq!(env.events().all().len(), calls as u32 * 4 + 1);
}

#[test]
fn fuzz_period_and_amount_repeatable_sweep_do_not_panic() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);

    // Same seed must produce the exact same sequence.
    let mut seed_a = 0x00A1_1CE5_ED19_u64;
    let mut seed_b = 0x00A1_1CE5_ED19_u64;
    for _ in 0..64 {
        assert_eq!(next_amount(&mut seed_a), next_amount(&mut seed_b));
        assert_eq!(next_period(&mut seed_a), next_period(&mut seed_b));
    }

    // Reset and run deterministic fuzz-style inputs through contract entrypoint.
    let mut seed = 0x00A1_1CE5_ED19_u64;
    for i in 0..FUZZ_ITERATIONS {
        let mut amount = next_amount(&mut seed);
        let mut period = next_period(&mut seed);

        // Periodically force hard boundaries into the sweep.
        if i % 64 == 0 {
            amount = i128::MAX;
        } else if i % 64 == 1 {
            amount = i128::MIN;
        }
        if i % 97 == 0 {
            period = u64::MAX;
        } else if i % 97 == 1 {
            period = 0;
        }

        client.report_revenue(&issuer, &token, &payout_asset, &amount, &period, &false);
    }

    assert_eq!(env.events().all().len(), FUZZ_ITERATIONS as u32 * 4 + 1);
}

// ---------------------------------------------------------------------------
// Pagination tests
// ---------------------------------------------------------------------------

/// Helper: set up env + client, return (env, client, issuer).
fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    (env, client, issuer)
}

/// Register `n` offerings for `issuer`, each with a unique token.
fn register_n(env: &Env, client: &RevoraRevenueShareClient, issuer: &Address, n: u32) {
    for i in 0..n {
        let token = Address::generate(env);
        let payout_asset = Address::generate(env);
        client.register_offering(issuer, &token, &(100 + i), &payout_asset);
    }
}

#[test]
fn empty_issuer_returns_empty_page() {
    let (_env, client, issuer) = setup();

    let (page, cursor) = client.get_offerings_page(&issuer, &0, &10);
    assert_eq!(page.len(), 0);
    assert_eq!(cursor, None);
}

#[test]
fn empty_issuer_count_is_zero() {
    let (_env, client, issuer) = setup();
    assert_eq!(client.get_offering_count(&issuer), 0);
}

#[test]
fn register_persists_and_count_increments() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 3);
    assert_eq!(client.get_offering_count(&issuer), 3);
}

#[test]
fn single_page_returns_all_no_cursor() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 5);

    let (page, cursor) = client.get_offerings_page(&issuer, &0, &10);
    assert_eq!(page.len(), 5);
    assert_eq!(cursor, None);
}

#[test]
fn multi_page_cursor_progression() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 7);

    // First page: items 0..3
    let (page1, cursor1) = client.get_offerings_page(&issuer, &0, &3);
    assert_eq!(page1.len(), 3);
    assert_eq!(cursor1, Some(3));

    // Second page: items 3..6
    let (page2, cursor2) = client.get_offerings_page(&issuer, &cursor1.unwrap(), &3);
    assert_eq!(page2.len(), 3);
    assert_eq!(cursor2, Some(6));

    // Third (final) page: items 6..7
    let (page3, cursor3) = client.get_offerings_page(&issuer, &cursor2.unwrap(), &3);
    assert_eq!(page3.len(), 1);
    assert_eq!(cursor3, None);
}

#[test]
fn final_page_has_no_cursor() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 4);

    let (page, cursor) = client.get_offerings_page(&issuer, &2, &10);
    assert_eq!(page.len(), 2);
    assert_eq!(cursor, None);
}

#[test]
fn out_of_bounds_cursor_returns_empty() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 3);

    let (page, cursor) = client.get_offerings_page(&issuer, &100, &5);
    assert_eq!(page.len(), 0);
    assert_eq!(cursor, None);
}

#[test]
fn limit_zero_uses_max_page_limit() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 5);

    // limit=0 should behave like MAX_PAGE_LIMIT (20), returning all 5.
    let (page, cursor) = client.get_offerings_page(&issuer, &0, &0);
    assert_eq!(page.len(), 5);
    assert_eq!(cursor, None);
}

#[test]
fn limit_one_iterates_one_at_a_time() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 3);

    let (p1, c1) = client.get_offerings_page(&issuer, &0, &1);
    assert_eq!(p1.len(), 1);
    assert_eq!(c1, Some(1));

    let (p2, c2) = client.get_offerings_page(&issuer, &c1.unwrap(), &1);
    assert_eq!(p2.len(), 1);
    assert_eq!(c2, Some(2));

    let (p3, c3) = client.get_offerings_page(&issuer, &c2.unwrap(), &1);
    assert_eq!(p3.len(), 1);
    assert_eq!(c3, None);
}

#[test]
fn limit_exceeding_max_is_capped() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 25);

    // limit=50 should be capped to 20.
    let (page, cursor) = client.get_offerings_page(&issuer, &0, &50);
    assert_eq!(page.len(), 20);
    assert_eq!(cursor, Some(20));
}

#[test]
fn offerings_preserve_correct_data() {
    let (env, client, issuer) = setup();
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &500, &payout_asset);

    let (page, _) = client.get_offerings_page(&issuer, &0, &10);
    let offering = page.get(0).unwrap();
    assert_eq!(offering.issuer, issuer);
    assert_eq!(offering.token, token);
    assert_eq!(offering.revenue_share_bps, 500);
    assert_eq!(offering.payout_asset, payout_asset);
}

#[test]
fn separate_issuers_have_independent_pages() {
    let (env, client, issuer_a) = setup();
    let issuer_b = Address::generate(&env);

    register_n(&env, &client, &issuer_a, 3);
    register_n(&env, &client, &issuer_b, 5);

    assert_eq!(client.get_offering_count(&issuer_a), 3);
    assert_eq!(client.get_offering_count(&issuer_b), 5);

    let (page_a, _) = client.get_offerings_page(&issuer_a, &0, &20);
    let (page_b, _) = client.get_offerings_page(&issuer_b, &0, &20);
    assert_eq!(page_a.len(), 3);
    assert_eq!(page_b.len(), 5);
}

#[test]
fn exact_page_boundary_no_cursor() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 6);

    // Exactly 2 pages of 3
    let (p1, c1) = client.get_offerings_page(&issuer, &0, &3);
    assert_eq!(p1.len(), 3);
    assert_eq!(c1, Some(3));

    let (p2, c2) = client.get_offerings_page(&issuer, &c1.unwrap(), &3);
    assert_eq!(p2.len(), 3);
    assert_eq!(c2, None);
}

// ── blacklist CRUD ────────────────────────────────────────────

#[test]
fn add_marks_investor_as_blacklisted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    assert!(!client.is_blacklisted(&token, &investor));
    client.blacklist_add(&admin, &token, &investor);
    assert!(client.is_blacklisted(&token, &investor));
}

#[test]
fn remove_unmarks_investor() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);
    client.blacklist_remove(&admin, &token, &investor);
    assert!(!client.is_blacklisted(&token, &investor));
}

#[test]
fn get_blacklist_returns_all_blocked_investors() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let inv_a = Address::generate(&env);
    let inv_b = Address::generate(&env);
    let inv_c = Address::generate(&env);

    client.blacklist_add(&admin, &token, &inv_a);
    client.blacklist_add(&admin, &token, &inv_b);
    client.blacklist_add(&admin, &token, &inv_c);

    let list = client.get_blacklist(&token);
    assert_eq!(list.len(), 3);
    assert!(list.contains(&inv_a));
    assert!(list.contains(&inv_b));
    assert!(list.contains(&inv_c));
}

#[test]
fn get_blacklist_empty_before_any_add() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let token = Address::generate(&env);

    assert_eq!(client.get_blacklist(&token).len(), 0);
}

// ── idempotency ───────────────────────────────────────────────

#[test]
fn double_add_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);
    client.blacklist_add(&admin, &token, &investor);

    assert_eq!(client.get_blacklist(&token).len(), 1);
}

#[test]
fn remove_nonexistent_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_remove(&admin, &token, &investor); // must not panic
    assert!(!client.is_blacklisted(&token, &investor));
}

// ── per-offering isolation ────────────────────────────────────

#[test]
fn blacklist_is_scoped_per_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token_a, &investor);

    assert!(client.is_blacklisted(&token_a, &investor));
    assert!(!client.is_blacklisted(&token_b, &investor));
}

#[test]
fn removing_from_one_offering_does_not_affect_another() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token_a, &investor);
    client.blacklist_add(&admin, &token_b, &investor);
    client.blacklist_remove(&admin, &token_a, &investor);

    assert!(!client.is_blacklisted(&token_a, &investor));
    assert!(client.is_blacklisted(&token_b, &investor));
}

// ── event emission ────────────────────────────────────────────

#[test]
fn blacklist_add_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    let before = env.events().all().len();
    client.blacklist_add(&admin, &token, &investor);
    assert!(env.events().all().len() > before);
}

#[test]
fn blacklist_remove_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);
    let before = env.events().all().len();
    client.blacklist_remove(&admin, &token, &investor);
    assert!(env.events().all().len() > before);
}

// ── distribution enforcement ──────────────────────────────────

#[test]
fn blacklisted_investor_excluded_from_distribution_filter() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let allowed = Address::generate(&env);
    let blocked = Address::generate(&env);

    client.blacklist_add(&admin, &token, &blocked);

    let investors = [allowed.clone(), blocked.clone()];
    let eligible = investors
        .iter()
        .filter(|inv| !client.is_blacklisted(&token, inv))
        .count();

    assert_eq!(eligible, 1);
}

#[test]
fn blacklist_takes_precedence_over_whitelist() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);

    // Even if investor were on a whitelist, blacklist must win
    assert!(client.is_blacklisted(&token, &investor));
}

// ── auth enforcement ──────────────────────────────────────────

#[test]
#[should_panic]
fn blacklist_add_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let client = make_client(&env);
    let bad_actor = Address::generate(&env);
    let token = Address::generate(&env);
    let victim = Address::generate(&env);

    client.blacklist_add(&bad_actor, &token, &victim);
}

#[test]
#[should_panic]
fn blacklist_remove_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let client = make_client(&env);
    let bad_actor = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_remove(&bad_actor, &token, &investor);
}

// ── structured error codes (#41) ──────────────────────────────

#[test]
fn register_offering_rejects_bps_over_10000() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(&issuer, &token, &10_001, &payout_asset);
    assert!(
        result.is_err(),
        "contract must return Err(RevoraError::InvalidRevenueShareBps) for bps > 10000"
    );
    assert_eq!(
        RevoraError::InvalidRevenueShareBps as u32,
        1,
        "error code for integrators"
    );
}

#[test]
fn register_offering_accepts_bps_exactly_10000() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(&issuer, &token, &10_000, &payout_asset);
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// Storage limit negative tests (#31): many offerings/reports, no panics
// ---------------------------------------------------------------------------

/// Maximum reasonable offering count used in tests to probe storage growth.
/// Reduced to 50 to stay within Soroban computational budget limits.
const STORAGE_STRESS_OFFERING_COUNT: u32 = 50;

#[test]
fn storage_stress_many_offerings_no_panic() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, STORAGE_STRESS_OFFERING_COUNT);
    let count = client.get_offering_count(&issuer);
    assert_eq!(count, STORAGE_STRESS_OFFERING_COUNT);
    let (page, cursor) =
        client.get_offerings_page(&issuer, &(STORAGE_STRESS_OFFERING_COUNT - 5), &10);
    assert_eq!(page.len(), 5);
    assert_eq!(cursor, None);
}

#[test]
fn storage_stress_many_reports_no_panic() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);

    for period_id in 1..=100_u64 {
        client.report_revenue(
            &issuer,
            &token,
            &payout_asset,
            &(period_id as i128 * 10_000),
            &period_id,
            &false,
        );
    }
    assert!(env.events().all().len() >= 100);
}

#[test]
fn storage_stress_large_blacklist_no_panic() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);

    for _ in 0..80 {
        let investor = Address::generate(&env);
        client.blacklist_add(&admin, &token, &investor);
    }
    let list = client.get_blacklist(&token);
    assert_eq!(list.len(), 80);
}

// ---------------------------------------------------------------------------
// Gas / compute usage characterization (#36): large scenarios, document behavior
// ---------------------------------------------------------------------------

#[test]
fn gas_characterization_many_offerings_single_issuer() {
    let (env, client, issuer) = setup();
    let n = 50_u32;
    register_n(&env, &client, &issuer, n);

    let (page, _) = client.get_offerings_page(&issuer, &0, &20);
    assert_eq!(page.len(), 20);
}

#[test]
fn gas_characterization_report_revenue_with_large_blacklist() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &500, &payout_asset);

    for _ in 0..30 {
        client.blacklist_add(&Address::generate(&env), &token, &Address::generate(&env));
    }
    let admin = Address::generate(&env);
    env.mock_all_auths();
    client.blacklist_add(&admin, &token, &Address::generate(&env));

    client.report_revenue(&issuer, &token, &payout_asset, &1_000_000, &1, &false);
    assert!(!env.events().all().is_empty());
}

// ---------------------------------------------------------------------------
// Holder concentration guardrail (#26)
// ---------------------------------------------------------------------------

#[test]
fn concentration_limit_not_set_allows_report_revenue() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    client.report_revenue(&issuer, &token, &payout_asset, &1_000, &1, &false);
}

#[test]
fn set_concentration_limit_requires_offering_to_exist() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    // No offering registered
    let r = client.try_set_concentration_limit(&issuer, &token, &5000, &false);
    assert!(r.is_err());
}

#[test]
fn set_concentration_limit_stores_config() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    client.set_concentration_limit(&issuer, &token, &5000, &false);
    let config = client.get_concentration_limit(&issuer, &token).unwrap();
    assert_eq!(config.max_bps, 5000);
    assert!(!config.enforce);
}

#[test]
fn report_concentration_emits_warning_when_over_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    client.set_concentration_limit(&issuer, &token, &5000, &false);
    let before = env.events().all().len();
    client.report_concentration(&issuer, &token, &6000);
    assert!(env.events().all().len() > before);
    assert_eq!(
        client.get_current_concentration(&issuer, &token),
        Some(6000)
    );
}

#[test]
fn report_concentration_no_warning_when_below_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    client.set_concentration_limit(&issuer, &token, &5000, &false);
    client.report_concentration(&issuer, &token, &4000);
    assert_eq!(
        client.get_current_concentration(&issuer, &token),
        Some(4000)
    );
}

#[test]
fn concentration_enforce_blocks_report_revenue_when_over_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    client.set_concentration_limit(&issuer, &token, &5000, &true);
    client.report_concentration(&issuer, &token, &6000);
    let r = client.try_report_revenue(&issuer, &token, &payout_asset, &1_000, &1, &false);
    assert!(
        r.is_err(),
        "report_revenue must fail when concentration exceeds limit with enforce=true"
    );
}

#[test]
fn concentration_enforce_allows_report_revenue_when_at_or_below_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    client.set_concentration_limit(&issuer, &token, &5000, &true);
    client.report_concentration(&issuer, &token, &5000);
    client.report_revenue(&issuer, &token, &payout_asset, &1_000, &1, &false);
    client.report_concentration(&issuer, &token, &4999);
    client.report_revenue(&issuer, &token, &payout_asset, &1_000, &2, &false);
}

#[test]
fn concentration_near_threshold_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    client.set_concentration_limit(&issuer, &token, &5000, &true);
    client.report_concentration(&issuer, &token, &5001);
    assert!(client
        .try_report_revenue(&issuer, &token, &payout_asset, &1_000, &1, &false)
        .is_err());
}

// ---------------------------------------------------------------------------
// On-chain audit log summary (#34)
// ---------------------------------------------------------------------------

#[test]
fn audit_summary_empty_before_any_report() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    let summary = client.get_audit_summary(&issuer, &token);
    assert!(summary.is_none());
}

#[test]
fn audit_summary_aggregates_revenue_and_count() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    client.report_revenue(&issuer, &token, &payout_asset, &100, &1, &false);
    client.report_revenue(&issuer, &token, &payout_asset, &200, &2, &false);
    client.report_revenue(&issuer, &token, &payout_asset, &300, &3, &false);
    let summary = client.get_audit_summary(&issuer, &token).unwrap();
    assert_eq!(summary.total_revenue, 600);
    assert_eq!(summary.report_count, 3);
}

#[test]
fn audit_summary_per_offering_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout_asset_a = Address::generate(&env);
    let payout_asset_b = Address::generate(&env);
    client.register_offering(&issuer, &token_a, &1_000, &payout_asset_a);
    client.register_offering(&issuer, &token_b, &1_000, &payout_asset_b);
    client.report_revenue(&issuer, &token_a, &payout_asset_a, &1000, &1, &false);
    client.report_revenue(&issuer, &token_b, &payout_asset_b, &2000, &1, &false);
    let sum_a = client.get_audit_summary(&issuer, &token_a).unwrap();
    let sum_b = client.get_audit_summary(&issuer, &token_b).unwrap();
    assert_eq!(sum_a.total_revenue, 1000);
    assert_eq!(sum_a.report_count, 1);
    assert_eq!(sum_b.total_revenue, 2000);
    assert_eq!(sum_b.report_count, 1);
}

// ---------------------------------------------------------------------------
// Configurable rounding modes (#44)
// ---------------------------------------------------------------------------

#[test]
fn compute_share_truncation() {
    let env = Env::default();
    let client = make_client(&env);
    // 1000 * 2500 / 10000 = 250
    let share = client.compute_share(&1000, &2500, &RoundingMode::Truncation);
    assert_eq!(share, 250);
}

#[test]
fn compute_share_round_half_up() {
    let env = Env::default();
    let client = make_client(&env);
    // 1000 * 2500 = 2_500_000; half-up: (2_500_000 + 5000) / 10000 = 250
    let share = client.compute_share(&1000, &2500, &RoundingMode::RoundHalfUp);
    assert_eq!(share, 250);
}

#[test]
fn compute_share_round_half_up_rounds_up_at_half() {
    let env = Env::default();
    let client = make_client(&env);
    // 1 * 2500 = 2500; 2500/10000 trunc = 0; half-up (2500+5000)/10000 = 0.75 -> 0? No: (2500+5000)/10000 = 7500/10000 = 0. So 1 bps would be 1*100/10000 = 0.01 -> 0 trunc, round half up (100+5000)/10000 = 0.51 -> 1. So 1 * 100 = 100, (100+5000)/10000 = 0.
    // 3 * 3333 = 9999; 9999/10000 = 0 trunc. (9999+5000)/10000 = 14999/10000 = 1 round half up.
    let share_trunc = client.compute_share(&3, &3333, &RoundingMode::Truncation);
    let share_half = client.compute_share(&3, &3333, &RoundingMode::RoundHalfUp);
    assert_eq!(share_trunc, 0);
    assert_eq!(share_half, 1);
}

#[test]
fn compute_share_bps_over_10000_returns_zero() {
    let env = Env::default();
    let client = make_client(&env);
    let share = client.compute_share(&1000, &10_001, &RoundingMode::Truncation);
    assert_eq!(share, 0);
}

#[test]
fn set_and_get_rounding_mode() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    assert_eq!(
        client.get_rounding_mode(&issuer, &token),
        RoundingMode::Truncation
    );
    client.set_rounding_mode(&issuer, &token, &RoundingMode::RoundHalfUp);
    assert_eq!(
        client.get_rounding_mode(&issuer, &token),
        RoundingMode::RoundHalfUp
    );
}

#[test]
fn set_rounding_mode_requires_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let r = client.try_set_rounding_mode(&issuer, &token, &RoundingMode::RoundHalfUp);
    assert!(r.is_err());
}

#[test]
fn compute_share_tiny_payout_truncation() {
    let env = Env::default();
    let client = make_client(&env);
    let share = client.compute_share(&1, &1, &RoundingMode::Truncation);
    assert_eq!(share, 0);
}

#[test]
fn compute_share_no_overflow_bounds() {
    let env = Env::default();
    let client = make_client(&env);
    let amount = 1_000_000_i128;
    let share = client.compute_share(&amount, &10_000, &RoundingMode::Truncation);
    assert_eq!(share, amount);
    let share2 = client.compute_share(&amount, &10_000, &RoundingMode::RoundHalfUp);
    assert_eq!(share2, amount);
}

// ===========================================================================
// Multi-period aggregated claim tests
// ===========================================================================

/// Helper: create a Stellar Asset Contract for testing token transfers.
/// Returns (token_contract_address, admin_address).
fn create_payment_token(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract_v2(admin.clone());
    (token_id.address().clone(), admin)
}

/// Mint `amount` of payment token to `recipient`.
fn mint_tokens(
    env: &Env,
    payment_token: &Address,
    admin: &Address,
    recipient: &Address,
    amount: &i128,
) {
    let _ = admin;
    token::StellarAssetClient::new(env, payment_token).mint(recipient, amount);
}

/// Check balance of `who` for `payment_token`.
fn balance(env: &Env, payment_token: &Address, who: &Address) -> i128 {
    token::Client::new(env, payment_token).balance(who)
}

/// Full setup for claim tests: env, client, issuer, offering token, payment token, contract addr.
fn claim_setup() -> (
    Env,
    RevoraRevenueShareClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let (payment_token, pt_admin) = create_payment_token(&env);

    // Register offering
    client.register_offering(&issuer, &token, &5_000, &payment_token); // 50% revenue share

    // Mint payment tokens to the issuer so they can deposit
    mint_tokens(&env, &payment_token, &pt_admin, &issuer, &10_000_000);

    (env, client, issuer, token, payment_token, contract_id)
}

// ── deposit_revenue tests ─────────────────────────────────────

#[test]
fn deposit_revenue_stores_period_data() {
    let (env, client, issuer, token, payment_token, contract_id) = claim_setup();

    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);

    assert_eq!(client.get_period_count(&token), 1);
    // Contract should hold the deposited tokens
    assert_eq!(balance(&env, &payment_token, &contract_id), 100_000);
}

#[test]
fn deposit_revenue_multiple_periods() {
    let (_env, client, issuer, token, payment_token, _contract_id) = claim_setup();

    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    client.deposit_revenue(&issuer, &token, &payment_token, &200_000, &2);
    client.deposit_revenue(&issuer, &token, &payment_token, &300_000, &3);

    assert_eq!(client.get_period_count(&token), 3);
}

#[test]
fn deposit_revenue_fails_for_nonexistent_offering() {
    let (env, client, issuer, _token, payment_token, _contract_id) = claim_setup();
    let unknown_token = Address::generate(&env);

    let result = client.try_deposit_revenue(&issuer, &unknown_token, &payment_token, &100_000, &1);
    assert!(result.is_err());
}

#[test]
fn deposit_revenue_fails_for_duplicate_period() {
    let (_env, client, issuer, token, payment_token, _contract_id) = claim_setup();

    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    let result = client.try_deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    assert!(result.is_err());
}

#[test]
fn deposit_revenue_fails_for_payment_token_mismatch() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();

    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);

    // Try to deposit with a different payment token
    let (other_pt, other_admin) = create_payment_token(&env);
    mint_tokens(&env, &other_pt, &other_admin, &issuer, &1_000_000);
    let result = client.try_deposit_revenue(&issuer, &token, &other_pt, &100_000, &2);
    assert!(result.is_err());
}

#[test]
fn report_revenue_rejects_mismatched_payout_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let wrong_asset = Address::generate(&env);

    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    let r = client.try_report_revenue(&issuer, &token, &wrong_asset, &1_000, &1, &false);
    assert!(r.is_err());
}

#[test]
fn deposit_revenue_rejects_mismatched_payout_asset_on_first_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let (configured_asset, configured_admin) = create_payment_token(&env);
    let (wrong_asset, wrong_admin) = create_payment_token(&env);

    client.register_offering(&issuer, &offering_token, &5_000, &configured_asset);
    mint_tokens(&env, &wrong_asset, &wrong_admin, &issuer, &1_000_000);
    mint_tokens(
        &env,
        &configured_asset,
        &configured_admin,
        &issuer,
        &1_000_000,
    );

    let r = client.try_deposit_revenue(&issuer, &offering_token, &wrong_asset, &100_000, &1);
    assert!(r.is_err());
}

#[test]
fn deposit_revenue_emits_event() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();

    let before = env.events().all().len();
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    assert!(env.events().all().len() > before);
}

#[test]
fn deposit_revenue_transfers_tokens() {
    let (env, client, issuer, token, payment_token, contract_id) = claim_setup();

    let issuer_balance_before = balance(&env, &payment_token, &issuer);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);

    assert_eq!(
        balance(&env, &payment_token, &issuer),
        issuer_balance_before - 100_000
    );
    assert_eq!(balance(&env, &payment_token, &contract_id), 100_000);
}

#[test]
fn deposit_revenue_sparse_period_ids() {
    let (_env, client, issuer, token, payment_token, _contract_id) = claim_setup();

    // Deposit with non-sequential period IDs
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &10);
    client.deposit_revenue(&issuer, &token, &payment_token, &200_000, &50);
    client.deposit_revenue(&issuer, &token, &payment_token, &300_000, &100);

    assert_eq!(client.get_period_count(&token), 3);
}

#[test]
#[should_panic]
fn deposit_revenue_requires_auth() {
    let env = Env::default();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let tok = Address::generate(&env);
    // No mock_all_auths — should panic on require_auth
    client.deposit_revenue(&issuer, &tok, &Address::generate(&env), &100, &1);
}

// ── set_holder_share tests ────────────────────────────────────

#[test]
fn set_holder_share_stores_share() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &2_500); // 25%
    assert_eq!(client.get_holder_share(&token, &holder), 2_500);
}

#[test]
fn set_holder_share_updates_existing() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &2_500);
    client.set_holder_share(&issuer, &token, &holder, &5_000);
    assert_eq!(client.get_holder_share(&token, &holder), 5_000);
}

#[test]
fn set_holder_share_fails_for_nonexistent_offering() {
    let (env, client, issuer, _token, _payment_token, _contract_id) = claim_setup();
    let unknown_token = Address::generate(&env);
    let holder = Address::generate(&env);

    let result = client.try_set_holder_share(&issuer, &unknown_token, &holder, &2_500);
    assert!(result.is_err());
}

#[test]
fn set_holder_share_fails_for_bps_over_10000() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    let result = client.try_set_holder_share(&issuer, &token, &holder, &10_001);
    assert!(result.is_err());
}

#[test]
fn set_holder_share_accepts_bps_exactly_10000() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    let result = client.try_set_holder_share(&issuer, &token, &holder, &10_000);
    assert!(result.is_ok());
    assert_eq!(client.get_holder_share(&token, &holder), 10_000);
}

#[test]
fn set_holder_share_emits_event() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    let before = env.events().all().len();
    client.set_holder_share(&issuer, &token, &holder, &2_500);
    assert!(env.events().all().len() > before);
}

#[test]
fn get_holder_share_returns_zero_for_unknown() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let unknown = Address::generate(&env);
    assert_eq!(client.get_holder_share(&token, &unknown), 0);
}

// ── claim tests (core multi-period aggregation) ───────────────

#[test]
fn claim_single_period() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &5_000); // 50%
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);

    let payout = client.claim(&holder, &token, &1);
    assert_eq!(payout, 50_000); // 50% of 100_000
    assert_eq!(balance(&env, &payment_token, &holder), 50_000);
}

#[test]
fn claim_multiple_periods_aggregated() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &2_000); // 20%
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    client.deposit_revenue(&issuer, &token, &payment_token, &200_000, &2);
    client.deposit_revenue(&issuer, &token, &payment_token, &300_000, &3);

    // Claim all 3 periods in one transaction
    // 20% of (100k + 200k + 300k) = 20% of 600k = 120k
    let payout = client.claim(&holder, &token, &0);
    assert_eq!(payout, 120_000);
    assert_eq!(balance(&env, &payment_token, &holder), 120_000);
}

#[test]
fn claim_max_periods_zero_claims_all() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &10_000); // 100%
    for i in 1..=5_u64 {
        client.deposit_revenue(&issuer, &token, &payment_token, &10_000, &i);
    }

    let payout = client.claim(&holder, &token, &0);
    assert_eq!(payout, 50_000); // 100% of 5 * 10k
}

#[test]
fn claim_partial_then_rest() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &10_000); // 100%
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    client.deposit_revenue(&issuer, &token, &payment_token, &200_000, &2);
    client.deposit_revenue(&issuer, &token, &payment_token, &300_000, &3);

    // Claim first 2 periods
    let payout1 = client.claim(&holder, &token, &2);
    assert_eq!(payout1, 300_000); // 100k + 200k

    // Claim remaining period
    let payout2 = client.claim(&holder, &token, &0);
    assert_eq!(payout2, 300_000); // 300k

    assert_eq!(balance(&env, &payment_token, &holder), 600_000);
}

#[test]
fn claim_no_double_counting() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &10_000); // 100%
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);

    let payout1 = client.claim(&holder, &token, &0);
    assert_eq!(payout1, 100_000);

    // Second claim should fail - nothing pending
    let result = client.try_claim(&holder, &token, &0);
    assert!(result.is_err());
}

#[test]
fn claim_advances_index_correctly() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &5_000); // 50%
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    client.deposit_revenue(&issuer, &token, &payment_token, &200_000, &2);

    // Claim period 1 only
    client.claim(&holder, &token, &1);

    // Deposit another period
    client.deposit_revenue(&issuer, &token, &payment_token, &400_000, &3);

    // Claim remaining - should get periods 2 and 3 only
    let payout = client.claim(&holder, &token, &0);
    assert_eq!(payout, 300_000); // 50% of (200k + 400k)
}

#[test]
fn claim_emits_event() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);

    let before = env.events().all().len();
    client.claim(&holder, &token, &0);
    assert!(env.events().all().len() > before);
}

#[test]
fn claim_fails_for_blacklisted_holder() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);

    // Blacklist the holder
    client.blacklist_add(&issuer, &token, &holder);

    let result = client.try_claim(&holder, &token, &0);
    assert!(result.is_err());
}

#[test]
fn claim_fails_when_no_pending_periods() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &5_000);
    // No deposits made
    let result = client.try_claim(&holder, &token, &0);
    assert!(result.is_err());
}

#[test]
fn claim_fails_for_zero_share_holder() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    // Don't set any share
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);

    let result = client.try_claim(&holder, &token, &0);
    assert!(result.is_err());
}

#[test]
fn claim_sparse_period_ids() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &10_000); // 100%

    // Non-sequential period IDs
    client.deposit_revenue(&issuer, &token, &payment_token, &50_000, &10);
    client.deposit_revenue(&issuer, &token, &payment_token, &75_000, &50);
    client.deposit_revenue(&issuer, &token, &payment_token, &125_000, &100);

    let payout = client.claim(&holder, &token, &0);
    assert_eq!(payout, 250_000); // 50k + 75k + 125k
}

#[test]
fn claim_multiple_holders_same_periods() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder_a, &3_000); // 30%
    client.set_holder_share(&issuer, &token, &holder_b, &2_000); // 20%

    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    client.deposit_revenue(&issuer, &token, &payment_token, &200_000, &2);

    let payout_a = client.claim(&holder_a, &token, &0);
    let payout_b = client.claim(&holder_b, &token, &0);

    // A: 30% of 300k = 90k; B: 20% of 300k = 60k
    assert_eq!(payout_a, 90_000);
    assert_eq!(payout_b, 60_000);
    assert_eq!(balance(&env, &payment_token, &holder_a), 90_000);
    assert_eq!(balance(&env, &payment_token, &holder_b), 60_000);
}

#[test]
fn claim_with_max_periods_cap() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &10_000); // 100%

    // Deposit 5 periods
    for i in 1..=5_u64 {
        client.deposit_revenue(&issuer, &token, &payment_token, &10_000, &i);
    }

    // Claim only 3 at a time
    let payout1 = client.claim(&holder, &token, &3);
    assert_eq!(payout1, 30_000);

    let payout2 = client.claim(&holder, &token, &3);
    assert_eq!(payout2, 20_000); // only 2 remaining

    // No more pending
    let result = client.try_claim(&holder, &token, &0);
    assert!(result.is_err());
}

#[test]
fn claim_zero_revenue_periods_still_advance() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &10_000); // 100%

    // Deposit zero-value periods followed by a real one
    client.deposit_revenue(&issuer, &token, &payment_token, &0, &1);
    client.deposit_revenue(&issuer, &token, &payment_token, &0, &2);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &3);

    // Claim first 2 (zero-value) - payout is 0 but index advances
    let payout1 = client.claim(&holder, &token, &2);
    assert_eq!(payout1, 0);

    // Now claim the real period
    let payout2 = client.claim(&holder, &token, &0);
    assert_eq!(payout2, 100_000);
}

#[test]
#[should_panic]
fn claim_requires_auth() {
    let env = Env::default();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let holder = Address::generate(&env);
    // No mock_all_auths — should panic on require_auth
    client.claim(&holder, &Address::generate(&env), &0);
}

// ── view function tests ───────────────────────────────────────

#[test]
fn get_pending_periods_returns_unclaimed() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &10);
    client.deposit_revenue(&issuer, &token, &payment_token, &200_000, &20);
    client.deposit_revenue(&issuer, &token, &payment_token, &300_000, &30);

    let pending = client.get_pending_periods(&token, &holder);
    assert_eq!(pending.len(), 3);
    assert_eq!(pending.get(0).unwrap(), 10);
    assert_eq!(pending.get(1).unwrap(), 20);
    assert_eq!(pending.get(2).unwrap(), 30);
}

#[test]
fn get_pending_periods_after_partial_claim() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    client.deposit_revenue(&issuer, &token, &payment_token, &200_000, &2);
    client.deposit_revenue(&issuer, &token, &payment_token, &300_000, &3);

    // Claim first 2
    client.claim(&holder, &token, &2);

    let pending = client.get_pending_periods(&token, &holder);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending.get(0).unwrap(), 3);
}

#[test]
fn get_pending_periods_empty_after_full_claim() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &5_000);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);

    client.claim(&holder, &token, &0);

    let pending = client.get_pending_periods(&token, &holder);
    assert_eq!(pending.len(), 0);
}

#[test]
fn get_pending_periods_empty_for_new_holder() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let unknown = Address::generate(&env);

    let pending = client.get_pending_periods(&token, &unknown);
    assert_eq!(pending.len(), 0);
}

#[test]
fn get_claimable_returns_correct_amount() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &2_500); // 25%
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    client.deposit_revenue(&issuer, &token, &payment_token, &200_000, &2);

    let claimable = client.get_claimable(&token, &holder);
    assert_eq!(claimable, 75_000); // 25% of 300k
}

#[test]
fn get_claimable_after_partial_claim() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &10_000); // 100%
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    client.deposit_revenue(&issuer, &token, &payment_token, &200_000, &2);

    client.claim(&holder, &token, &1); // claim period 1

    let claimable = client.get_claimable(&token, &holder);
    assert_eq!(claimable, 200_000); // only period 2 remains
}

#[test]
fn get_claimable_returns_zero_for_unknown_holder() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();

    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);

    let unknown = Address::generate(&env);
    assert_eq!(client.get_claimable(&token, &unknown), 0);
}

#[test]
fn get_claimable_returns_zero_after_full_claim() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &10_000);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);

    client.claim(&holder, &token, &0);
    assert_eq!(client.get_claimable(&token, &holder), 0);
}

#[test]
fn get_period_count_default_zero() {
    let (env, client, _issuer, _token, _payment_token, _contract_id) = claim_setup();
    let random_token = Address::generate(&env);
    assert_eq!(client.get_period_count(&random_token), 0);
}

// ── multi-holder correctness ──────────────────────────────────

#[test]
fn multiple_holders_independent_claim_indices() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder_a, &5_000); // 50%
    client.set_holder_share(&issuer, &token, &holder_b, &3_000); // 30%

    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    client.deposit_revenue(&issuer, &token, &payment_token, &200_000, &2);

    // A claims period 1 only
    client.claim(&holder_a, &token, &1);

    // B still has both periods pending
    let pending_b = client.get_pending_periods(&token, &holder_b);
    assert_eq!(pending_b.len(), 2);

    // B claims all
    let payout_b = client.claim(&holder_b, &token, &0);
    assert_eq!(payout_b, 90_000); // 30% of 300k

    // A claims remaining period 2
    let payout_a = client.claim(&holder_a, &token, &0);
    assert_eq!(payout_a, 100_000); // 50% of 200k

    assert_eq!(balance(&env, &payment_token, &holder_a), 150_000); // 50k + 100k
    assert_eq!(balance(&env, &payment_token, &holder_b), 90_000);
}

#[test]
fn claim_after_holder_share_change() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &5_000); // 50%
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);

    // Claim at 50%
    let payout1 = client.claim(&holder, &token, &0);
    assert_eq!(payout1, 50_000);

    // Change share to 25% and deposit new period
    client.set_holder_share(&issuer, &token, &holder, &2_500);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &2);

    // Claim at new 25% rate
    let payout2 = client.claim(&holder, &token, &0);
    assert_eq!(payout2, 25_000);
}

// ── stress / gas characterization for claims ──────────────────

#[test]
fn claim_many_periods_stress() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &1_000); // 10%

    // Deposit 50 periods (MAX_CLAIM_PERIODS)
    for i in 1..=50_u64 {
        client.deposit_revenue(&issuer, &token, &payment_token, &10_000, &i);
    }

    // Claim all 50 in one transaction
    let payout = client.claim(&holder, &token, &0);
    assert_eq!(payout, 50_000); // 10% of 50 * 10k

    let pending = client.get_pending_periods(&token, &holder);
    assert_eq!(pending.len(), 0);
    // Gas note: claim iterates over 50 periods, each requiring 2 storage reads
    // (PeriodEntry + PeriodRevenue). Total: ~100 persistent reads + 1 write
    // for LastClaimedIdx + 1 token transfer. Well within Soroban compute limits.
}

#[test]
fn claim_exceeding_max_is_capped() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &10_000); // 100%

    // Deposit 55 periods (more than MAX_CLAIM_PERIODS of 50)
    for i in 1..=55_u64 {
        client.deposit_revenue(&issuer, &token, &payment_token, &1_000, &i);
    }

    // Request 100 periods - should be capped at 50
    let payout1 = client.claim(&holder, &token, &100);
    assert_eq!(payout1, 50_000); // 50 * 1k

    // 5 remaining
    let pending = client.get_pending_periods(&token, &holder);
    assert_eq!(pending.len(), 5);

    let payout2 = client.claim(&holder, &token, &0);
    assert_eq!(payout2, 5_000);
}

#[test]
fn get_claimable_stress_many_periods() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &5_000); // 50%

    let period_count = 40_u64;
    let amount_per_period: i128 = 10_000;
    for i in 1..=period_count {
        client.deposit_revenue(&issuer, &token, &payment_token, &amount_per_period, &i);
    }

    let claimable = client.get_claimable(&token, &holder);
    assert_eq!(claimable, (period_count as i128) * amount_per_period / 2);
    // Gas note: get_claimable is a read-only view that iterates all unclaimed periods.
    // Cost: O(n) persistent reads. For 40 periods: ~80 reads. Acceptable for views.
}

// ── edge cases ────────────────────────────────────────────────

#[test]
fn claim_with_rounding() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &3_333); // 33.33%

    client.deposit_revenue(&issuer, &token, &payment_token, &100, &1);

    // 100 * 3333 / 10000 = 33 (integer division, rounds down)
    let payout = client.claim(&holder, &token, &0);
    assert_eq!(payout, 33);
}

#[test]
fn claim_single_unit_revenue() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &10_000); // 100%
    client.deposit_revenue(&issuer, &token, &payment_token, &1, &1);

    let payout = client.claim(&holder, &token, &0);
    assert_eq!(payout, 1);
}

#[test]
fn deposit_then_claim_then_deposit_then_claim() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &token, &holder, &10_000); // 100%

    // Round 1
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    let p1 = client.claim(&holder, &token, &0);
    assert_eq!(p1, 100_000);

    // Round 2
    client.deposit_revenue(&issuer, &token, &payment_token, &200_000, &2);
    client.deposit_revenue(&issuer, &token, &payment_token, &300_000, &3);
    let p2 = client.claim(&holder, &token, &0);
    assert_eq!(p2, 500_000);

    assert_eq!(balance(&env, &payment_token, &holder), 600_000);
}

#[test]
fn offering_isolation_claims_independent() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();

    // Register a second offering
    let token_b = Address::generate(&env);
    let (pt_b, pt_b_admin) = create_payment_token(&env);
    client.register_offering(&issuer, &token_b, &3_000, &pt_b);

    // Create a second payment token for offering B
    mint_tokens(&env, &pt_b, &pt_b_admin, &issuer, &5_000_000);

    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &5_000); // 50% of offering A
    client.set_holder_share(&issuer, &token_b, &holder, &10_000); // 100% of offering B

    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    client.deposit_revenue(&issuer, &token_b, &pt_b, &50_000, &1);

    let payout_a = client.claim(&holder, &token, &0);
    let payout_b = client.claim(&holder, &token_b, &0);

    assert_eq!(payout_a, 50_000); // 50% of 100k
    assert_eq!(payout_b, 50_000); // 100% of 50k

    // Verify token A claim doesn't affect token B pending
    assert_eq!(client.get_pending_periods(&token, &holder).len(), 0);
    assert_eq!(client.get_pending_periods(&token_b, &holder).len(), 0);
}

// ===========================================================================
// Time-delayed revenue claim (#27)
// ===========================================================================

#[test]
fn set_claim_delay_stores_and_returns_delay() {
    let (_env, client, issuer, token, _payment_token, _contract_id) = claim_setup();

    assert_eq!(client.get_claim_delay(&token), 0);
    client.set_claim_delay(&issuer, &token, &3600);
    assert_eq!(client.get_claim_delay(&token), 3600);
}

#[test]
fn set_claim_delay_requires_offering() {
    let (env, client, issuer, _token, _payment_token, _contract_id) = claim_setup();
    let unknown_token = Address::generate(&env);

    let r = client.try_set_claim_delay(&issuer, &unknown_token, &3600);
    assert!(r.is_err());
}

#[test]
fn claim_before_delay_returns_claim_delay_not_elapsed() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    client.set_holder_share(&issuer, &token, &holder, &10_000);
    client.set_claim_delay(&issuer, &token, &100);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    // Still at 1000, delay 100 -> claimable at 1100
    let r = client.try_claim(&holder, &token, &0);
    assert!(r.is_err());
}

#[test]
fn claim_after_delay_succeeds() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    client.set_holder_share(&issuer, &token, &holder, &10_000);
    client.set_claim_delay(&issuer, &token, &100);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    env.ledger().set_timestamp(1100);
    let payout = client.claim(&holder, &token, &0);
    assert_eq!(payout, 100_000);
    assert_eq!(balance(&env, &payment_token, &holder), 100_000);
}

#[test]
fn get_claimable_respects_delay() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    env.ledger().set_timestamp(2000);
    client.set_holder_share(&issuer, &token, &holder, &5_000);
    client.set_claim_delay(&issuer, &token, &500);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    // At 2000, deposit at 2000, claimable at 2500
    assert_eq!(client.get_claimable(&token, &holder), 0);
    env.ledger().set_timestamp(2500);
    assert_eq!(client.get_claimable(&token, &holder), 50_000);
}

#[test]
fn claim_delay_partial_periods_only_claimable_after_delay() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    env.ledger().set_timestamp(1000);
    client.set_holder_share(&issuer, &token, &holder, &10_000);
    client.set_claim_delay(&issuer, &token, &100);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    env.ledger().set_timestamp(1050);
    client.deposit_revenue(&issuer, &token, &payment_token, &200_000, &2);
    // At 1100: period 1 claimable (1000+100<=1100), period 2 not (1050+100>1100)
    env.ledger().set_timestamp(1100);
    let payout = client.claim(&holder, &token, &0);
    assert_eq!(payout, 100_000);
    // At 1160: period 2 claimable (1050+100<=1160)
    env.ledger().set_timestamp(1160);
    let payout2 = client.claim(&holder, &token, &0);
    assert_eq!(payout2, 200_000);
}

#[test]
fn set_claim_delay_emits_event() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();

    let before = env.events().all().len();
    client.set_claim_delay(&issuer, &token, &3600);
    assert!(env.events().all().len() > before);
}

// ===========================================================================
// On-chain distribution simulation (#29)
// ===========================================================================

#[test]
fn simulate_distribution_returns_correct_payouts() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);

    let mut shares = Vec::new(&env);
    shares.push_back((holder_a.clone(), 3_000u32));
    shares.push_back((holder_b.clone(), 2_000u32));

    let result = client.simulate_distribution(&issuer, &token, &100_000, &shares);
    assert_eq!(result.total_distributed, 50_000); // 30% + 20% of 100k
    assert_eq!(result.payouts.len(), 2);
    assert_eq!(result.payouts.get(0).unwrap(), (holder_a, 30_000));
    assert_eq!(result.payouts.get(1).unwrap(), (holder_b, 20_000));
}

#[test]
fn simulate_distribution_zero_holders() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();

    let shares = Vec::new(&env);
    let result = client.simulate_distribution(&issuer, &token, &100_000, &shares);
    assert_eq!(result.total_distributed, 0);
    assert_eq!(result.payouts.len(), 0);
}

#[test]
fn simulate_distribution_zero_revenue() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    let mut shares = Vec::new(&env);
    shares.push_back((holder.clone(), 5_000u32));
    let result = client.simulate_distribution(&issuer, &token, &0, &shares);
    assert_eq!(result.total_distributed, 0);
    assert_eq!(result.payouts.get(0).unwrap().1, 0);
}

#[test]
fn simulate_distribution_read_only_no_state_change() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);

    let mut shares = Vec::new(&env);
    shares.push_back((holder.clone(), 10_000u32));
    let _ = client.simulate_distribution(&issuer, &token, &1_000_000, &shares);
    let count_before = client.get_period_count(&token);
    let _ = client.simulate_distribution(&issuer, &token, &999_999, &shares);
    assert_eq!(client.get_period_count(&token), count_before);
}

#[test]
fn simulate_distribution_uses_rounding_mode() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    client.set_rounding_mode(&issuer, &token, &RoundingMode::RoundHalfUp);
    let holder = Address::generate(&env);

    let mut shares = Vec::new(&env);
    shares.push_back((holder.clone(), 3_333u32));
    let result = client.simulate_distribution(&issuer, &token, &100, &shares);
    assert_eq!(result.total_distributed, 33);
    assert_eq!(result.payouts.get(0).unwrap().1, 33);
}

// ===========================================================================
// Upgradeability guard and freeze (#32)
// ===========================================================================

#[test]
fn set_admin_once_succeeds() {
    let (env, client, _issuer, _token, _payment_token, _contract_id) = claim_setup();
    let admin = Address::generate(&env);

    client.set_admin(&admin);
    assert_eq!(client.get_admin(), Some(admin));
}

#[test]
fn set_admin_twice_fails() {
    let (env, client, _issuer, _token, _payment_token, _contract_id) = claim_setup();
    let admin = Address::generate(&env);

    client.set_admin(&admin);
    let other = Address::generate(&env);
    let r = client.try_set_admin(&other);
    assert!(r.is_err());
}

#[test]
fn freeze_sets_flag_and_emits_event() {
    let (env, client, _issuer, _token, _payment_token, _contract_id) = claim_setup();
    let admin = Address::generate(&env);

    client.set_admin(&admin);
    assert!(!client.is_frozen());
    let before = env.events().all().len();
    client.freeze();
    assert!(client.is_frozen());
    assert!(env.events().all().len() > before);
}

#[test]
fn frozen_blocks_register_offering() {
    let (env, client, issuer, _token, _payment_token, _contract_id) = claim_setup();
    let admin = Address::generate(&env);
    let new_token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.set_admin(&admin);
    client.freeze();
    let r = client.try_register_offering(&issuer, &new_token, &1_000, &payout_asset);
    assert!(r.is_err());
}

#[test]
fn frozen_blocks_deposit_revenue() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let admin = Address::generate(&env);

    client.set_admin(&admin);
    client.freeze();
    let r = client.try_deposit_revenue(&issuer, &token, &payment_token, &100_000, &99);
    assert!(r.is_err());
}

#[test]
fn frozen_blocks_set_holder_share() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let admin = Address::generate(&env);
    let holder = Address::generate(&env);

    client.set_admin(&admin);
    client.freeze();
    let r = client.try_set_holder_share(&issuer, &token, &holder, &2_500);
    assert!(r.is_err());
}

#[test]
fn frozen_allows_claim() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);
    let admin = Address::generate(&env);

    client.set_holder_share(&issuer, &token, &holder, &10_000);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    client.set_admin(&admin);
    client.freeze();

    let payout = client.claim(&holder, &token, &0);
    assert_eq!(payout, 100_000);
    assert_eq!(balance(&env, &payment_token, &holder), 100_000);
}

#[test]
fn freeze_succeeds_when_called_by_admin() {
    let (env, client, _issuer, _token, _payment_token, _contract_id) = claim_setup();
    let admin = Address::generate(&env);

    client.set_admin(&admin);
    env.mock_all_auths();
    let r = client.try_freeze();
    assert!(r.is_ok());
    assert!(client.is_frozen());
}

// ===========================================================================
// Testnet mode tests (#24)
// ===========================================================================

#[test]
fn testnet_mode_disabled_by_default() {
    let env = Env::default();
    let client = make_client(&env);
    assert!(!client.is_testnet_mode());
}

#[test]
fn set_testnet_mode_requires_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);

    // Set admin first
    client.set_admin(&admin);

    // Now admin can toggle testnet mode
    client.set_testnet_mode(&true);
    assert!(client.is_testnet_mode());
}

#[test]
fn set_testnet_mode_fails_without_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    // No admin set - should fail
    let result = client.try_set_testnet_mode(&true);
    assert!(result.is_err());
}

#[test]
fn set_testnet_mode_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);

    client.set_admin(&admin);
    let before = env.events().all().len();
    client.set_testnet_mode(&true);
    assert!(env.events().all().len() > before);
}

#[test]
fn issuer_transfer_accept_completes_transfer() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // Verify no pending transfer after acceptance
    assert_eq!(client.get_pending_issuer_transfer(&token), None);

    // Verify offering issuer is updated - offering is now stored under new_issuer
    let offering = client.get_offering(&new_issuer, &token);
    assert!(offering.is_some());
    assert_eq!(offering.unwrap().issuer, new_issuer);
}

#[test]
fn issuer_transfer_accept_emits_event() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    let before = env.events().all().len();
    client.accept_issuer_transfer(&token);
    assert!(env.events().all().len() > before);
}

#[test]
fn issuer_transfer_new_issuer_can_deposit_revenue() {
    let (env, client, _issuer, token, payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    // Mint tokens to new issuer
    let (_, pt_admin) = create_payment_token(&env);
    mint_tokens(&env, &payment_token, &pt_admin, &new_issuer, &5_000_000);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // New issuer should be able to deposit revenue
    let result = client.try_deposit_revenue(&new_issuer, &token, &payment_token, &100_000, &1);
    assert!(result.is_ok());
}

#[test]
fn testnet_mode_can_be_toggled() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);

    client.set_admin(&admin);

    // Enable
    client.set_testnet_mode(&true);
    assert!(client.is_testnet_mode());

    // Disable
    client.set_testnet_mode(&false);
    assert!(!client.is_testnet_mode());

    // Enable again
    client.set_testnet_mode(&true);
    assert!(client.is_testnet_mode());
}

#[test]
fn testnet_mode_allows_bps_over_10000() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    // Set admin and enable testnet mode
    client.set_admin(&admin);
    client.set_testnet_mode(&true);

    // Should allow bps > 10000 in testnet mode
    let result = client.try_register_offering(&issuer, &token, &15_000, &payout_asset);
    assert!(result.is_ok());

    // Verify offering was registered
    let offering = client.get_offering(&issuer, &token).unwrap();
    assert_eq!(offering.revenue_share_bps, 15_000);
}

#[test]
fn testnet_mode_disabled_rejects_bps_over_10000() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    // Testnet mode is disabled by default
    let result = client.try_register_offering(&issuer, &token, &15_000, &payout_asset);
    assert!(result.is_err());
}

#[test]
fn testnet_mode_skips_concentration_enforcement() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    // Set admin and enable testnet mode
    client.set_admin(&admin);
    client.set_testnet_mode(&true);

    // Register offering and set concentration limit with enforcement
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    client.set_concentration_limit(&issuer, &token, &5000, &true);
    client.report_concentration(&issuer, &token, &8000); // Over limit

    // In testnet mode, report_revenue should succeed despite concentration being over limit
    let result = client.try_report_revenue(&issuer, &token, &payout_asset, &1_000, &1, &false);
    assert!(result.is_ok());
}

#[test]
fn issuer_transfer_new_issuer_can_set_holder_share() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);
    let holder = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // New issuer should be able to set holder shares
    let result = client.try_set_holder_share(&new_issuer, &token, &holder, &5_000);
    assert!(result.is_ok());
    assert_eq!(client.get_holder_share(&token, &holder), 5_000);
}

#[test]
fn issuer_transfer_old_issuer_loses_access() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // Old issuer should not be able to deposit revenue
    let result = client.try_deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);
    assert!(result.is_err());
}

#[test]
fn issuer_transfer_old_issuer_cannot_set_holder_share() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);
    let holder = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // Old issuer should not be able to set holder shares
    let result = client.try_set_holder_share(&issuer, &token, &holder, &5_000);
    assert!(result.is_err());
}

#[test]
fn issuer_transfer_cancel_clears_pending() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.cancel_issuer_transfer(&token);

    assert_eq!(client.get_pending_issuer_transfer(&token), None);
}

#[test]
fn issuer_transfer_cancel_emits_event() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    let before = env.events().all().len();
    client.cancel_issuer_transfer(&token);
    let after = env.events().all().len();
    assert_eq!(after, before + 1);
}

#[test]
fn testnet_mode_disabled_enforces_concentration() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    // Testnet mode disabled (default)
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    client.set_concentration_limit(&issuer, &token, &5000, &true);
    client.report_concentration(&issuer, &token, &8000); // Over limit

    // Should fail with concentration enforcement
    let result = client.try_report_revenue(&issuer, &token, &payout_asset, &1_000, &1, &false);
    assert!(result.is_err());
}

#[test]
fn testnet_mode_toggle_after_offerings_exist() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token1 = Address::generate(&env);
    let token2 = Address::generate(&env);
    let payout_asset1 = Address::generate(&env);
    let payout_asset2 = Address::generate(&env);

    // Register offering in normal mode
    client.register_offering(&issuer, &token1, &5_000, &payout_asset1);

    // Set admin and enable testnet mode
    client.set_admin(&admin);
    client.set_testnet_mode(&true);

    // Register offering with high bps in testnet mode
    let result = client.try_register_offering(&issuer, &token2, &20_000, &payout_asset2);
    assert!(result.is_ok());

    // Verify both offerings exist
    assert_eq!(client.get_offering_count(&issuer), 2);
}

#[test]
fn testnet_mode_affects_only_validation_not_storage() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    // Enable testnet mode
    client.set_admin(&admin);
    client.set_testnet_mode(&true);

    // Register with high bps
    client.register_offering(&issuer, &token, &25_000, &payout_asset);

    // Disable testnet mode
    client.set_testnet_mode(&false);

    // Offering should still exist with high bps value
    let offering = client.get_offering(&issuer, &token).unwrap();
    assert_eq!(offering.revenue_share_bps, 25_000);
}

#[test]
fn testnet_mode_multiple_offerings_with_varied_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);

    client.set_admin(&admin);
    client.set_testnet_mode(&true);

    // Register multiple offerings with various bps values
    for i in 1..=5 {
        let token = Address::generate(&env);
        let bps = 10_000 + (i * 1_000);
        let payout_asset = Address::generate(&env);
        client.register_offering(&issuer, &token, &bps, &payout_asset);
    }

    assert_eq!(client.get_offering_count(&issuer), 5);
}

#[test]
fn testnet_mode_concentration_warning_still_emitted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.set_admin(&admin);
    client.set_testnet_mode(&true);

    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    client.set_concentration_limit(&issuer, &token, &5000, &false);

    // Warning should still be emitted in testnet mode
    let before = env.events().all().len();
    client.report_concentration(&issuer, &token, &7000);
    assert!(env.events().all().len() > before);
}

#[test]
fn issuer_transfer_cancel_then_can_propose_again() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer_1 = Address::generate(&env);
    let new_issuer_2 = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer_1);
    client.cancel_issuer_transfer(&token);

    // Should be able to propose to different address
    let result = client.try_propose_issuer_transfer(&token, &new_issuer_2);
    assert!(result.is_ok());
    assert_eq!(
        client.get_pending_issuer_transfer(&token),
        Some(new_issuer_2)
    );
}

// ── Security and abuse prevention tests ──────────────────────

#[test]
fn issuer_transfer_cannot_propose_for_nonexistent_offering() {
    let (env, client, _issuer, _token, _payment_token, _contract_id) = claim_setup();
    let unknown_token = Address::generate(&env);
    let new_issuer = Address::generate(&env);

    let result = client.try_propose_issuer_transfer(&unknown_token, &new_issuer);
    assert!(result.is_err());
}

#[test]
fn issuer_transfer_cannot_propose_when_already_pending() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer_1 = Address::generate(&env);
    let new_issuer_2 = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer_1);

    // Second proposal should fail
    let result = client.try_propose_issuer_transfer(&token, &new_issuer_2);
    assert!(result.is_err());
}

#[test]
fn issuer_transfer_cannot_accept_when_no_pending() {
    let (_env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();

    let result = client.try_accept_issuer_transfer(&token);
    assert!(result.is_err());
}

#[test]
fn issuer_transfer_cannot_cancel_when_no_pending() {
    let (_env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();

    let result = client.try_cancel_issuer_transfer(&token);
    assert!(result.is_err());
}

#[test]
#[should_panic]
fn issuer_transfer_propose_requires_auth() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let _issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let new_issuer = Address::generate(&env);

    // No mock_all_auths - should panic
    client.propose_issuer_transfer(&token, &new_issuer);
}

#[test]
#[should_panic]
fn issuer_transfer_accept_requires_auth() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);

    // No mock_all_auths - should panic
    client.accept_issuer_transfer(&token);
}

#[test]
#[should_panic]
fn issuer_transfer_cancel_requires_auth() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);

    // No mock_all_auths - should panic
    client.cancel_issuer_transfer(&token);
}

#[test]
fn issuer_transfer_double_accept_fails() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // Second accept should fail (no pending transfer)
    let result = client.try_accept_issuer_transfer(&token);
    assert!(result.is_err());
}

// ── Edge case tests ───────────────────────────────────────────

#[test]
fn issuer_transfer_to_same_address() {
    let (_env, client, issuer, token, _payment_token, _contract_id) = claim_setup();

    // Transfer to self (issuer is used here)
    let result = client.try_propose_issuer_transfer(&token, &issuer);
    assert!(result.is_ok());

    let result = client.try_accept_issuer_transfer(&token);
    assert!(result.is_ok());
}

#[test]
fn issuer_transfer_multiple_offerings_isolation() {
    let (env, client, issuer, token_a, _payment_token, _contract_id) = claim_setup();
    let token_b = Address::generate(&env);
    let new_issuer_a = Address::generate(&env);
    let new_issuer_b = Address::generate(&env);

    // Register second offering
    client.register_offering(&issuer, &token_b, &3_000, &token_b);

    // Propose transfers for both
    client.propose_issuer_transfer(&token_a, &new_issuer_a);
    client.propose_issuer_transfer(&token_b, &new_issuer_b);

    // Accept only token_a transfer
    client.accept_issuer_transfer(&token_a);

    // Verify token_a transferred but token_b still pending
    assert_eq!(client.get_pending_issuer_transfer(&token_a), None);
    assert_eq!(
        client.get_pending_issuer_transfer(&token_b),
        Some(new_issuer_b)
    );
}

#[test]
fn issuer_transfer_blocked_when_frozen() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);
    let admin = Address::generate(&env);

    client.set_admin(&admin);
    client.freeze();

    let result = client.try_propose_issuer_transfer(&token, &new_issuer);
    assert!(result.is_err());
}

#[test]
fn issuer_transfer_accept_blocked_when_frozen() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);
    let admin = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);

    client.set_admin(&admin);
    client.freeze();

    let result = client.try_accept_issuer_transfer(&token);
    assert!(result.is_err());
}

#[test]
fn issuer_transfer_cancel_blocked_when_frozen() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);
    let admin = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);

    client.set_admin(&admin);
    client.freeze();

    let result = client.try_cancel_issuer_transfer(&token);
    assert!(result.is_err());
}

// ── Integration tests with other features ─────────────────────

#[test]
fn issuer_transfer_preserves_audit_summary() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    // Report revenue before transfer
    client.report_revenue(&issuer, &token, &payment_token, &100_000, &1, &false);
    let summary_before = client.get_audit_summary(&issuer, &token).unwrap();

    // Transfer issuer
    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // Audit summary should still be accessible
    let summary_after = client.get_audit_summary(&issuer, &token).unwrap();
    assert_eq!(summary_before.total_revenue, summary_after.total_revenue);
    assert_eq!(summary_before.report_count, summary_after.report_count);
}

#[test]
fn issuer_transfer_new_issuer_can_report_revenue() {
    let (env, client, _issuer, token, payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // New issuer can report revenue
    let result =
        client.try_report_revenue(&new_issuer, &token, &payment_token, &200_000, &2, &false);
    assert!(result.is_ok());
}

#[test]
fn issuer_transfer_new_issuer_can_set_concentration_limit() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // New issuer can set concentration limit
    let result = client.try_set_concentration_limit(&new_issuer, &token, &5_000, &true);
    assert!(result.is_ok());
}

#[test]
fn issuer_transfer_new_issuer_can_set_rounding_mode() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // New issuer can set rounding mode
    let result = client.try_set_rounding_mode(&new_issuer, &token, &RoundingMode::RoundHalfUp);
    assert!(result.is_ok());
}

#[test]
fn issuer_transfer_new_issuer_can_set_claim_delay() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // New issuer can set claim delay
    let result = client.try_set_claim_delay(&new_issuer, &token, &3600);
    assert!(result.is_ok());
}

#[test]
fn issuer_transfer_holders_can_still_claim() {
    let (env, client, issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);
    let new_issuer = Address::generate(&env);

    // Setup: deposit and set share before transfer
    client.set_holder_share(&issuer, &token, &holder, &10_000);
    client.deposit_revenue(&issuer, &token, &payment_token, &100_000, &1);

    // Transfer issuer
    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // Holder should still be able to claim
    let payout = client.claim(&holder, &token, &0);
    assert_eq!(payout, 100_000);
}

#[test]
fn issuer_transfer_then_new_deposits_and_claims_work() {
    let (env, client, _issuer, token, payment_token, _contract_id) = claim_setup();
    let holder = Address::generate(&env);
    let new_issuer = Address::generate(&env);

    // Mint tokens to new issuer
    let (_, pt_admin) = create_payment_token(&env);
    mint_tokens(&env, &payment_token, &pt_admin, &new_issuer, &5_000_000);

    // Transfer issuer
    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // New issuer sets share and deposits
    client.set_holder_share(&new_issuer, &token, &holder, &5_000);
    client.deposit_revenue(&new_issuer, &token, &payment_token, &200_000, &1);

    // Holder claims
    let payout = client.claim(&holder, &token, &0);
    assert_eq!(payout, 100_000); // 50% of 200k
}

#[test]
fn issuer_transfer_get_offering_still_works() {
    let (env, client, _issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // get_offering should find the offering under new issuer now
    let offering = client.get_offering(&new_issuer, &token);
    assert!(offering.is_some());
    assert_eq!(offering.unwrap().issuer, new_issuer);
}

#[test]
fn issuer_transfer_preserves_revenue_share_bps() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    let offering_before = client.get_offering(&issuer, &token).unwrap();

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    let offering_after = client.get_offering(&new_issuer, &token).unwrap();
    assert_eq!(
        offering_before.revenue_share_bps,
        offering_after.revenue_share_bps
    );
}

#[test]
fn issuer_transfer_old_issuer_cannot_report_concentration() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // Old issuer should not be able to report concentration
    let result = client.try_report_concentration(&issuer, &token, &5_000);
    assert!(result.is_err());
}

#[test]
fn issuer_transfer_new_issuer_can_report_concentration() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let new_issuer = Address::generate(&env);

    client.set_concentration_limit(&issuer, &token, &6_000, &false);

    client.propose_issuer_transfer(&token, &new_issuer);
    client.accept_issuer_transfer(&token);

    // New issuer can report concentration
    let result = client.try_report_concentration(&new_issuer, &token, &5_000);
    assert!(result.is_ok());
}

#[test]
fn testnet_mode_normal_operations_unaffected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.set_admin(&admin);
    client.set_testnet_mode(&true);

    // Normal operations should work as expected
    client.register_offering(&issuer, &token, &5_000, &payout_asset);
    client.report_revenue(&issuer, &token, &payout_asset, &1_000_000, &1, &false);

    let summary = client.get_audit_summary(&issuer, &token).unwrap();
    assert_eq!(summary.total_revenue, 1_000_000);
    assert_eq!(summary.report_count, 1);
}

#[test]
fn testnet_mode_blacklist_operations_unaffected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    client.set_admin(&admin);
    client.set_testnet_mode(&true);

    // Blacklist operations should work normally
    client.blacklist_add(&admin, &token, &investor);
    assert!(client.is_blacklisted(&token, &investor));

    client.blacklist_remove(&admin, &token, &investor);
    assert!(!client.is_blacklisted(&token, &investor));
}

#[test]
fn testnet_mode_pagination_unaffected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);

    client.set_admin(&admin);
    client.set_testnet_mode(&true);

    // Register multiple offerings
    for i in 0..10 {
        let token = Address::generate(&env);
        let payout_asset = Address::generate(&env);
        client.register_offering(&issuer, &token, &(1_000 + i * 100), &payout_asset);
    }

    // Pagination should work normally
    let (page, cursor) = client.get_offerings_page(&issuer, &0, &5);
    assert_eq!(page.len(), 5);
    assert_eq!(cursor, Some(5));
}

#[test]
#[should_panic]
fn testnet_mode_requires_auth_to_set() {
    let env = Env::default();
    // No mock_all_auths - should panic
    let client = make_client(&env);
    let admin = Address::generate(&env);

    client.set_admin(&admin);
    // This should panic because we didn't mock auth
    client.set_testnet_mode(&true);
}

// ── Emergency pause tests ───────────────────────────────────────

#[test]
fn pause_unpause_idempotence_and_events() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);

    client.initialize(&admin, &None::<Address>);
    assert!(!client.is_paused());

    // Pause twice (idempotent)
    client.pause_admin(&admin);
    assert!(client.is_paused());
    client.pause_admin(&admin);
    assert!(client.is_paused());

    // Unpause twice (idempotent)
    client.unpause_admin(&admin);
    assert!(!client.is_paused());
    client.unpause_admin(&admin);
    assert!(!client.is_paused());

    // Verify events were emitted
    assert!(env.events().all().len() >= 5); // init + pause + pause + unpause + unpause
}

#[test]
#[should_panic(expected = "contract is paused")]
fn register_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.initialize(&admin, &None::<Address>);
    client.pause_admin(&admin);
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
}

#[test]
#[should_panic(expected = "contract is paused")]
fn report_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.initialize(&admin, &None::<Address>);
    // Register before pausing
    client.register_offering(&issuer, &token, &1_000, &payout_asset);
    client.pause_admin(&admin);
    client.report_revenue(&issuer, &token, &payout_asset, &1_000_000, &1, &false);
}

#[test]
fn pause_safety_role_works() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let safety = Address::generate(&env);

    client.initialize(&admin, &Some(safety.clone()));
    assert!(!client.is_paused());

    // Safety can pause
    client.pause_safety(&safety);
    assert!(client.is_paused());

    // Safety can unpause
    client.unpause_safety(&safety);
    assert!(!client.is_paused());
}

#[test]
#[should_panic(expected = "contract is paused")]
fn blacklist_add_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    client.initialize(&admin, &None::<Address>);
    client.pause_admin(&admin);
    client.blacklist_add(&admin, &token, &investor);
}

#[test]
#[should_panic(expected = "contract is paused")]
fn blacklist_remove_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    client.initialize(&admin, &None::<Address>);
    client.pause_admin(&admin);
    client.blacklist_remove(&admin, &token, &investor);
}

// ===========================================================================
// On-chain revenue distribution calculation (#4)
// ===========================================================================

#[test]
fn calculate_distribution_basic() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    let total_revenue = 1_000_000_i128;
    let total_supply = 10_000_i128;
    let holder_balance = 1_000_i128;

    let payout = client.calculate_distribution(
        &caller,
        &issuer,
        &token,
        &total_revenue,
        &total_supply,
        &holder_balance,
        &holder,
    );

    assert_eq!(payout, 50_000);
}

#[test]
fn calculate_distribution_bps_100_percent() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &10_000, &token);

    let payout =
        client.calculate_distribution(&caller, &issuer, &token, &100_000, &1_000, &100, &holder);

    assert_eq!(payout, 10_000);
}

#[test]
fn calculate_distribution_bps_25_percent() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &2_500, &token);

    let payout =
        client.calculate_distribution(&caller, &issuer, &token, &100_000, &1_000, &200, &holder);

    assert_eq!(payout, 5_000);
}

#[test]
fn calculate_distribution_zero_revenue() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    let payout = client.calculate_distribution(&caller, &issuer, &token, &0, &1_000, &100, &holder);

    assert_eq!(payout, 0);
}

#[test]
fn calculate_distribution_zero_balance() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    let payout =
        client.calculate_distribution(&caller, &issuer, &token, &100_000, &1_000, &0, &holder);

    assert_eq!(payout, 0);
}

#[test]
#[should_panic(expected = "total_supply cannot be zero")]
fn calculate_distribution_zero_supply_panics() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    client.calculate_distribution(&caller, &issuer, &token, &100_000, &0, &100, &holder);
}

#[test]
#[should_panic(expected = "offering not found")]
fn calculate_distribution_nonexistent_offering_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    client.calculate_distribution(&caller, &issuer, &token, &100_000, &1_000, &100, &holder);
}

#[test]
#[should_panic(expected = "holder is blacklisted")]
fn calculate_distribution_blacklisted_holder_panics() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    client.blacklist_add(&issuer, &token, &holder);

    client.calculate_distribution(&caller, &issuer, &token, &100_000, &1_000, &100, &holder);
}

#[test]
fn calculate_distribution_rounds_down() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &3_333, &token);

    let payout = client.calculate_distribution(&caller, &issuer, &token, &100, &100, &10, &holder);

    assert_eq!(payout, 3);
}

#[test]
fn calculate_distribution_rounds_down_exact() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &2_500, &token);

    let payout =
        client.calculate_distribution(&caller, &issuer, &token, &100_000, &1_000, &400, &holder);

    assert_eq!(payout, 10_000);
}

#[test]
fn calculate_distribution_large_values() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    let large_revenue = 1_000_000_000_000_i128;
    let total_supply = 1_000_000_000_i128;
    let holder_balance = 100_000_000_i128;

    let payout = client.calculate_distribution(
        &caller,
        &issuer,
        &token,
        &large_revenue,
        &total_supply,
        &holder_balance,
        &holder,
    );

    assert_eq!(payout, 50_000_000_000);
}

#[test]
fn calculate_distribution_emits_event() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    let before = env.events().all().len();
    client.calculate_distribution(&caller, &issuer, &token, &100_000, &1_000, &100, &holder);
    assert!(env.events().all().len() > before);
}

#[test]
fn calculate_distribution_multiple_holders_sum() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let caller = Address::generate(&env);

    client.register_offering(&issuer, &token, &5_000, &token);

    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);
    let holder_c = Address::generate(&env);

    let total_supply = 1_000_i128;
    let total_revenue = 100_000_i128;

    let payout_a = client.calculate_distribution(
        &caller,
        &issuer,
        &token,
        &total_revenue,
        &total_supply,
        &500,
        &holder_a,
    );
    let payout_b = client.calculate_distribution(
        &caller,
        &issuer,
        &token,
        &total_revenue,
        &total_supply,
        &300,
        &holder_b,
    );
    let payout_c = client.calculate_distribution(
        &caller,
        &issuer,
        &token,
        &total_revenue,
        &total_supply,
        &200,
        &holder_c,
    );

    assert_eq!(payout_a, 25_000);
    assert_eq!(payout_b, 15_000);
    assert_eq!(payout_c, 10_000);
    assert_eq!(payout_a + payout_b + payout_c, 50_000);
}

#[test]
#[should_panic]
fn calculate_distribution_requires_auth() {
    let env = Env::default();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &5_000, &token);

    client.calculate_distribution(&caller, &issuer, &token, &100_000, &1_000, &100, &holder);
}

#[test]
fn calculate_total_distributable_basic() {
    let (_env, client, issuer, token, _payment_token, _contract_id) = claim_setup();

    let total = client.calculate_total_distributable(&issuer, &token, &100_000);

    assert_eq!(total, 50_000);
}

#[test]
fn calculate_total_distributable_bps_100_percent() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &10_000, &token);

    let total = client.calculate_total_distributable(&issuer, &token, &100_000);

    assert_eq!(total, 100_000);
}

#[test]
fn calculate_total_distributable_bps_25_percent() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &2_500, &token);

    let total = client.calculate_total_distributable(&issuer, &token, &100_000);

    assert_eq!(total, 25_000);
}

#[test]
fn calculate_total_distributable_zero_revenue() {
    let (_env, client, issuer, token, _payment_token, _contract_id) = claim_setup();

    let total = client.calculate_total_distributable(&issuer, &token, &0);

    assert_eq!(total, 0);
}

#[test]
fn calculate_total_distributable_rounds_down() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &3_333, &token);

    let total = client.calculate_total_distributable(&issuer, &token, &100);

    assert_eq!(total, 33);
}

#[test]
#[should_panic(expected = "offering not found")]
fn calculate_total_distributable_nonexistent_offering_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.calculate_total_distributable(&issuer, &token, &100_000);
}

#[test]
fn calculate_total_distributable_large_value() {
    let (_env, client, issuer, token, _payment_token, _contract_id) = claim_setup();

    let total = client.calculate_total_distributable(&issuer, &token, &1_000_000_000_000);

    assert_eq!(total, 500_000_000_000);
}

#[test]
fn calculate_distribution_offering_isolation() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let token_b = Address::generate(&env);
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token_b, &8_000, &token_b);

    let payout_a =
        client.calculate_distribution(&caller, &issuer, &token, &100_000, &1_000, &100, &holder);
    let payout_b =
        client.calculate_distribution(&caller, &issuer, &token_b, &100_000, &1_000, &100, &holder);

    assert_eq!(payout_a, 5_000);
    assert_eq!(payout_b, 8_000);
}

#[test]
fn calculate_total_distributable_offering_isolation() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let token_b = Address::generate(&env);

    client.register_offering(&issuer, &token_b, &8_000, &token_b);

    let total_a = client.calculate_total_distributable(&issuer, &token, &100_000);
    let total_b = client.calculate_total_distributable(&issuer, &token_b, &100_000);

    assert_eq!(total_a, 50_000);
    assert_eq!(total_b, 80_000);
}

#[test]
fn calculate_distribution_tiny_balance() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    let payout = client.calculate_distribution(
        &caller,
        &issuer,
        &token,
        &100_000,
        &1_000_000_000,
        &1,
        &holder,
    );

    assert_eq!(payout, 0);
}

#[test]
fn calculate_distribution_all_zeros_except_supply() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    let payout = client.calculate_distribution(&caller, &issuer, &token, &0, &1_000, &0, &holder);

    assert_eq!(payout, 0);
}

#[test]
fn calculate_distribution_single_holder_owns_all() {
    let (env, client, issuer, token, _payment_token, _contract_id) = claim_setup();
    let caller = Address::generate(&env);
    let holder = Address::generate(&env);

    let total_revenue = 100_000_i128;
    let total_supply = 1_000_i128;

    let payout = client.calculate_distribution(
        &caller,
        &issuer,
        &token,
        &total_revenue,
        &total_supply,
        &total_supply,
        &holder,
    );

    assert_eq!(payout, 50_000);
}
