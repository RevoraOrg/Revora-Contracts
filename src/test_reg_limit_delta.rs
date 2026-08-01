#![cfg(test)]

use crate::{RevoraRevenueShareClient, EVENT_REG_LIMIT_DELTA};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, IntoVal, Symbol};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup_offering(env: &Env) -> (RevoraRevenueShareClient<'static>, Address, Address, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, crate::RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let token = Address::generate(env);
    let payout = env.register_stellar_asset_contract_v2(admin.clone()).address();
    soroban_sdk::token::StellarAssetClient::new(env, &payout).mint(&admin, &1_000_000);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(
        &admin,
        &symbol_short!("def"),
        &token,
        &1_000,
        &payout,
        &0,
        &symbol_short!(""),
        &0,
    );
    (client, admin, token, payout)
}

fn set_jurisdiction(
    client: &RevoraRevenueShareClient<'static>,
    issuer: &Address,
    token: &Address,
    holder: &Address,
    jurisdiction: Symbol,
) {
    client.set_holder_jurisdiction(
        issuer,
        &symbol_short!("def"),
        token,
        holder,
        &jurisdiction,
        &0u64,
    );
}

fn find_reg_limit_events(env: &Env, start: u32) -> Vec<(Address, Symbol, i128, i128)> {
    let mut results: Vec<(Address, Symbol, i128, i128)> = soroban_sdk::vec![env];
    let all = env.events().all();
    for i in start..all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        if topics.len() >= 4 {
            let t0: soroban_sdk::Symbol = topics.get(0).unwrap().into_val(env);
            if t0 == EVENT_REG_LIMIT_DELTA {
                let (holder, jurisdiction, delta_bps, new_aggregate): (
                    Address,
                    Symbol,
                    i128,
                    i128,
                ) = data.into_val(env);
                results.push_back((holder, jurisdiction, delta_bps, new_aggregate));
            }
        }
    }
    results
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Setting a holder share with a jurisdiction emits a reg_limit_delta event
/// with the correct delta and new aggregate.
#[test]
fn test_reg_limit_delta_emitted_on_set_holder_share() {
    let env = Env::default();
    let (client, issuer, token, _payout) = setup_offering(&env);
    let holder = Address::generate(&env);

    set_jurisdiction(&client, &issuer, &token, &holder, symbol_short!("us"));

    let before = env.events().all().len();
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &2_500);

    let events = find_reg_limit_events(&env, before as u32);
    assert_eq!(events.len(), 1, "exactly one reg_limit_delta event expected");

    let (ev_holder, ev_jur, ev_delta, ev_agg) = events.get(0).unwrap();
    assert_eq!(ev_holder, holder);
    assert_eq!(ev_jur, symbol_short!("us"));
    assert_eq!(ev_delta, 2_500, "delta should equal the new share (old was 0)");
    assert_eq!(ev_agg, 2_500, "aggregate for US should now be 2500 bps");
}

/// Setting a holder share to zero emits a negative delta event.
#[test]
fn test_reg_limit_delta_emitted_on_zeroing_share() {
    let env = Env::default();
    let (client, issuer, token, _payout) = setup_offering(&env);
    let holder = Address::generate(&env);

    set_jurisdiction(&client, &issuer, &token, &holder, symbol_short!("us"));
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &2_500);

    let before = env.events().all().len();
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &0);

    let events = find_reg_limit_events(&env, before as u32);
    assert_eq!(events.len(), 1);
    let (_, ev_jur, ev_delta, ev_agg) = events.get(0).unwrap();
    assert_eq!(ev_jur, symbol_short!("us"));
    assert_eq!(ev_delta, -2_500, "delta should be negative when reducing to zero");
    assert_eq!(ev_agg, 0, "aggregate for US should be zero after zeroing");
}

/// No reg_limit_delta event when holder has no jurisdiction.
#[test]
fn test_no_reg_limit_delta_without_jurisdiction() {
    let env = Env::default();
    let (client, issuer, token, _payout) = setup_offering(&env);
    let holder = Address::generate(&env);

    let before = env.events().all().len();
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &2_500);

    let events = find_reg_limit_events(&env, before as u32);
    assert_eq!(events.len(), 0, "no reg_limit_delta without jurisdiction");
}

/// Transfer between two holders in different jurisdictions emits events for both.
#[test]
fn test_reg_limit_delta_on_transfer_different_jurisdictions() {
    let env = Env::default();
    let (client, issuer, token, _payout) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    set_jurisdiction(&client, &issuer, &token, &from, symbol_short!("us"));
    set_jurisdiction(&client, &issuer, &token, &to, symbol_short!("sg"));

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &from, &5_000);

    let before = env.events().all().len();
    client.transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &2_000,
        &symbol_short!("RegD"),
    );

    let events = find_reg_limit_events(&env, before as u32);
    assert_eq!(events.len(), 2, "two reg_limit_delta events for different-jurisdiction transfer");

    // from event: jurisdiction=us, delta=-2000
    let (ev_from_holder, ev_from_jur, ev_from_delta, ev_from_agg) = events.get(0).unwrap();
    assert_eq!(ev_from_holder, from);
    assert_eq!(ev_from_jur, symbol_short!("us"));
    assert_eq!(ev_from_delta, -2_000, "from should decrease us aggregate");
    assert_eq!(ev_from_agg, 3_000, "us aggregate = 5000-2000 = 3000");

    // to event: jurisdiction=sg, delta=+2000
    let (ev_to_holder, ev_to_jur, ev_to_delta, ev_to_agg) = events.get(1).unwrap();
    assert_eq!(ev_to_holder, to);
    assert_eq!(ev_to_jur, symbol_short!("sg"));
    assert_eq!(ev_to_delta, 2_000, "to should increase sg aggregate");
    assert_eq!(ev_to_agg, 2_000, "sg aggregate = 2000");
}

/// Transfer between two holders in the same jurisdiction: two events, net zero delta.
#[test]
fn test_reg_limit_delta_on_transfer_same_jurisdiction() {
    let env = Env::default();
    let (client, issuer, token, _payout) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    set_jurisdiction(&client, &issuer, &token, &from, symbol_short!("us"));
    set_jurisdiction(&client, &issuer, &token, &to, symbol_short!("us"));

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &from, &5_000);

    let before = env.events().all().len();
    client.transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &2_000,
        &symbol_short!("RegD"),
    );

    let events = find_reg_limit_events(&env, before as u32);
    assert_eq!(events.len(), 2, "two reg_limit_delta events for same-jurisdiction transfer");

    // from event: delta = -2000
    let (_, ev_from_jur, ev_from_delta, ev_from_agg) = events.get(0).unwrap();
    assert_eq!(ev_from_jur, symbol_short!("us"));
    assert_eq!(ev_from_delta, -2_000);
    assert_eq!(ev_from_agg, 3_000);

    // to event: delta = +2000
    let (_, ev_to_jur, ev_to_delta, _ev_to_agg) = events.get(1).unwrap();
    assert_eq!(ev_to_jur, symbol_short!("us"));
    assert_eq!(ev_to_delta, 2_000);

    // Net delta is zero: -2000 + 2000 = 0
    let net_delta = ev_from_delta + ev_to_delta;
    assert_eq!(net_delta, 0, "net delta for same-jurisdiction transfer must be zero");
}

/// Multiple holders tracked independently across jurisdictions.
#[test]
fn test_reg_limit_delta_multiple_holders_multiple_jurisdictions() {
    let env = Env::default();
    let (client, issuer, token, _payout) = setup_offering(&env);

    let holder_us1 = Address::generate(&env);
    let holder_us2 = Address::generate(&env);
    let holder_sg = Address::generate(&env);

    set_jurisdiction(&client, &issuer, &token, &holder_us1, symbol_short!("us"));
    set_jurisdiction(&client, &issuer, &token, &holder_us2, symbol_short!("us"));
    set_jurisdiction(&client, &issuer, &token, &holder_sg, symbol_short!("sg"));

    let before = env.events().all().len();
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder_us1, &3_000);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder_us2, &2_000);
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder_sg, &5_000);

    let events = find_reg_limit_events(&env, before as u32);
    assert_eq!(events.len(), 3);

    let us_sum: i128 = events
        .clone()
        .into_iter()
        .filter(|(_, j, _, _)| *j == symbol_short!("us"))
        .map(|(_, _, d, _)| d)
        .sum();
    assert_eq!(us_sum, 5_000, "total US jurisdiction shares should be 5000 bps");

    let sg_sum: i128 =
        events.iter().filter(|(_, j, _, _)| *j == symbol_short!("sg")).map(|(_, _, d, _)| *d).sum();
    assert_eq!(sg_sum, 5_000, "total SG jurisdiction shares should be 5000 bps");
}

/// Holder without jurisdiction does not trigger event, even during transfer.
#[test]
fn test_transfer_from_no_jurisdiction_to_jurisdiction() {
    let env = Env::default();
    let (client, issuer, token, _payout) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    set_jurisdiction(&client, &issuer, &token, &to, symbol_short!("us"));

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &from, &5_000);

    let before = env.events().all().len();
    client.transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &2_000,
        &symbol_short!("RegD"),
    );

    let events = find_reg_limit_events(&env, before as u32);
    // Only one event: for `to` (US jurisdiction gaining shares)
    assert_eq!(events.len(), 1, "only to should emit reg_limit_delta");
    let (ev_holder, ev_jur, ev_delta, ev_agg) = events.get(0).unwrap();
    assert_eq!(ev_holder, to);
    assert_eq!(ev_jur, symbol_short!("us"));
    assert_eq!(ev_delta, 2_000);
    assert_eq!(ev_agg, 2_000);
}

/// Transfer where neither holder has jurisdiction: no events.
#[test]
fn test_transfer_both_no_jurisdiction_no_events() {
    let env = Env::default();
    let (client, issuer, token, _payout) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &from, &5_000);

    let before = env.events().all().len();
    client.transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &2_000,
        &symbol_short!("RegD"),
    );

    let events = find_reg_limit_events(&env, before as u32);
    assert_eq!(events.len(), 0, "no events when neither holder has a jurisdiction");
}

/// reg_limit_delta event data tuple has stable field ordering.
#[test]
fn test_reg_limit_delta_event_data_shape() {
    let env = Env::default();
    let (client, issuer, token, _payout) = setup_offering(&env);
    let holder = Address::generate(&env);

    set_jurisdiction(&client, &issuer, &token, &holder, symbol_short!("uk"));

    let before = env.events().all().len();
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &1_000);

    let all = env.events().all();
    for i in before..all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        if topics.len() >= 4 {
            let t0: soroban_sdk::Symbol = topics.get(0).unwrap().into_val(&env);
            if t0 == EVENT_REG_LIMIT_DELTA {
                // Field ordering: (holder, jurisdiction, delta_bps, new_aggregate_bps)
                let (h, j, d, a): (Address, Symbol, i128, i128) = data.into_val(&env);
                assert_eq!(h, holder);
                assert_eq!(j, symbol_short!("uk"));
                assert_eq!(d, 1_000);
                assert_eq!(a, 1_000);
                return;
            }
        }
    }
    panic!("reg_limit_delta event not found");
}

/// Verify the rg_lim_d symbol constant is correct.
#[test]
fn test_reg_limit_delta_symbol_is_correct() {
    let expected: soroban_sdk::Symbol = symbol_short!("rg_lim_d");
    assert_eq!(EVENT_REG_LIMIT_DELTA, expected, "EVENT_REG_LIMIT_DELTA must be rg_lim_d");
}

// ── Gas budget tests ────────────────────────────────────────────────────────────

/// Gas budget for reg_limit_delta emission on set_holder_share.
/// Chosen to safely accommodate the cost of one event emission plus share bookkeeping
/// across CI runners. Calibrated from measurement; adjust if CI reports consistent
/// overruns. The existing EVENT_EMISSION_GAS_BUDGET for the full report_revenue path
/// is 5_000_000 (see test_event_indexed_v3.rs).
const REG_LIMIT_DELTA_GAS_BUDGET: u64 = 1_000_000;

/// Gas budget for transfer_with_attestation emitting two reg_limit_delta events.
const TRANSFER_REG_LIMIT_DELTA_GAS_BUDGET: u64 = 2_000_000;

/// set_holder_share with a jurisdiction-tagged holder must stay within gas budget.
/// This bounds the cost of emitting one reg_limit_delta event.
#[test]
fn set_holder_share_reg_limit_delta_gas_budget() {
    let env = Env::default();
    let (client, issuer, token, _payout) = setup_offering(&env);
    let holder = Address::generate(&env);

    set_jurisdiction(&client, &issuer, &token, &holder, symbol_short!("us"));

    let cpu_before = env.budget().cpu_instruction_cost();
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &2_500);
    let cpu_after = env.budget().cpu_instruction_cost();
    let cost = cpu_after - cpu_before;
    std::println!("CPU cost for set_holder_share (with reg_limit_delta): {}", cost);
    assert!(
        cost <= REG_LIMIT_DELTA_GAS_BUDGET,
        "Gas budget exceeded: {} > {}",
        cost,
        REG_LIMIT_DELTA_GAS_BUDGET
    );
}

/// transfer_with_attestation emitting two reg_limit_delta events must stay within budget.
#[test]
fn transfer_reg_limit_delta_gas_budget() {
    let env = Env::default();
    let (client, issuer, token, _payout) = setup_offering(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    set_jurisdiction(&client, &issuer, &token, &from, symbol_short!("us"));
    set_jurisdiction(&client, &issuer, &token, &to, symbol_short!("sg"));
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &from, &5_000);

    let cpu_before = env.budget().cpu_instruction_cost();
    client.transfer_with_attestation(
        &issuer,
        &symbol_short!("def"),
        &token,
        &from,
        &to,
        &2_000,
        &symbol_short!("RegD"),
    );
    let cpu_after = env.budget().cpu_instruction_cost();
    let cost = cpu_after - cpu_before;
    std::println!("CPU cost for transfer (2 reg_limit_delta events): {}", cost);
    assert!(
        cost <= TRANSFER_REG_LIMIT_DELTA_GAS_BUDGET,
        "Gas budget exceeded: {} > {}",
        cost,
        TRANSFER_REG_LIMIT_DELTA_GAS_BUDGET
    );
}

/// set_holder_share WITHOUT a jurisdiction (no reg_limit_delta emitted) stays within budget.
#[test]
fn set_holder_share_no_jurisdiction_gas_budget() {
    let env = Env::default();
    let (client, issuer, token, _payout) = setup_offering(&env);
    let holder = Address::generate(&env);

    let cpu_before = env.budget().cpu_instruction_cost();
    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &2_500);
    let cpu_after = env.budget().cpu_instruction_cost();
    let cost = cpu_after - cpu_before;
    std::println!("CPU cost for set_holder_share (no jurisdiction): {}", cost);
    // The no-jurisdiction path skips reg_limit_delta entirely, so it should be cheaper
    assert!(
        cost <= REG_LIMIT_DELTA_GAS_BUDGET,
        "Gas budget exceeded: {} > {}",
        cost,
        REG_LIMIT_DELTA_GAS_BUDGET
    );
}
