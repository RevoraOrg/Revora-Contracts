//! Adversarial test coverage for `get_total_shares_issued` (#1035)
//!
//! `get_total_shares_issued(env, issuer, namespace, token) -> i128` is a
//! pure storage-read that returns the running sum of all `share_bps` values
//! that have been committed for an offering via `set_holder_share`.  It has
//! no auth requirement and cannot panic, but its correctness depends on the
//! write path in `set_holder_share` accumulating and subtracting the right
//! deltas.
//!
//! ## Coverage matrix
//!
//! | # | Case                                              | Expected outcome                     |
//! |---|---------------------------------------------------|--------------------------------------|
//! | 1 | Unknown offering (no registration, no shares set) | returns 0 (default)                  |
//! | 2 | Offering registered but no shares set yet         | returns 0                            |
//! | 3 | One holder assigned a non-zero share              | returns that holder's share_bps      |
//! | 4 | Two holders assigned shares                       | returns sum of both shares           |
//! | 5 | Multiple holders summing to exactly 10 000        | returns 10 000                       |
//! | 6 | Holder share reduced (delta accounting)           | total decreases by the delta         |
//! | 7 | Holder share set to 0 (removed)                   | total decreases by old share         |
//! | 8 | Same holder updated twice                         | total reflects only the latest value |
//! | 9 | Different namespace same issuer+token             | namespaces are isolated              |
//! |10 | Different issuer same namespace+token             | issuers are isolated                 |
//! |11 | Different token same issuer+namespace             | tokens are isolated                  |
//! |12 | State unchanged after rejected `set_holder_share` | too-high bps rejected; total intact  |
//! |13 | State unchanged after rejected cap-exceeded share | cap breach rejected; total intact    |
//! |14 | Large number of holders accumulate correctly      | total == sum of all share_bps        |
//! |15 | Total tracks correctly after cap removed (cap=0)  | subsequent writes update freely      |

#![cfg(test)]

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::Address as _,
    Address, Env, Symbol, Vec,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Boot a fresh contract and return (env, client, admin).
fn boot() -> (Env, RevoraRevenueShareClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    (env, client, admin)
}

/// Register a minimal offering for (issuer, namespace, token) and return the
/// payout asset address.
fn reg(
    env: &Env,
    client: &RevoraRevenueShareClient<'static>,
    issuer: &Address,
    namespace: Symbol,
    token: &Address,
) -> Address {
    let payout = Address::generate(env);
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        &namespace,
        token,
        &5_000u32,
        &payout,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
    payout
}

/// Assign `share_bps` to `holder` for an offering, using nonce 1.
/// Helper assumes each holder only needs one set in the common test path.
fn set_share(
    client: &RevoraRevenueShareClient<'static>,
    issuer: &Address,
    ns: Symbol,
    token: &Address,
    holder: &Address,
    share_bps: u32,
    nonce: u64,
) {
    client
        .set_holder_share(issuer, &ns, token, holder, &share_bps, &nonce)
        .unwrap();
}

// ── Case 1: unknown offering returns 0 ───────────────────────────────────────

#[test]
fn get_total_shares_issued_unknown_offering_returns_zero() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");

    // No registration, no set_holder_share — must default to 0.
    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 0);
}

// ── Case 2: registered offering with no shares set returns 0 ─────────────────

#[test]
fn get_total_shares_issued_registered_offering_no_shares_returns_zero() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");

    reg(&env, &client, &issuer, ns.clone(), &token);

    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 0);
}

// ── Case 3: one holder assigned a non-zero share ──────────────────────────────

#[test]
fn get_total_shares_issued_single_holder_reflects_share() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let holder = Address::generate(&env);

    reg(&env, &client, &issuer, ns.clone(), &token);
    set_share(&client, &issuer, ns.clone(), &token, &holder, 3_000, 1);

    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 3_000);
}

// ── Case 4: two holders' shares are summed ────────────────────────────────────

#[test]
fn get_total_shares_issued_two_holders_sum() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);

    reg(&env, &client, &issuer, ns.clone(), &token);
    set_share(&client, &issuer, ns.clone(), &token, &h1, 3_000, 1);
    set_share(&client, &issuer, ns.clone(), &token, &h2, 4_000, 1);

    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 7_000);
}

// ── Case 5: multiple holders summing to exactly 10 000 ────────────────────────

#[test]
fn get_total_shares_issued_full_allocation_equals_ten_thousand() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);
    let h3 = Address::generate(&env);
    let h4 = Address::generate(&env);

    reg(&env, &client, &issuer, ns.clone(), &token);
    // 2500 + 2500 + 2500 + 2500 = 10_000
    set_share(&client, &issuer, ns.clone(), &token, &h1, 2_500, 1);
    set_share(&client, &issuer, ns.clone(), &token, &h2, 2_500, 1);
    set_share(&client, &issuer, ns.clone(), &token, &h3, 2_500, 1);
    set_share(&client, &issuer, ns.clone(), &token, &h4, 2_500, 1);

    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 10_000);
}

// ── Case 6: holder share reduced – total decreases by delta ──────────────────

#[test]
fn get_total_shares_issued_decreases_when_holder_share_reduced() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let holder = Address::generate(&env);

    reg(&env, &client, &issuer, ns.clone(), &token);
    set_share(&client, &issuer, ns.clone(), &token, &holder, 6_000, 1);
    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 6_000);

    // Reduce by 2_000 (nonce must be strictly greater)
    set_share(&client, &issuer, ns.clone(), &token, &holder, 4_000, 2);
    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 4_000);
}

// ── Case 7: holder share set to 0 ────────────────────────────────────────────

#[test]
fn get_total_shares_issued_decreases_when_holder_share_zeroed() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let holder = Address::generate(&env);

    reg(&env, &client, &issuer, ns.clone(), &token);
    set_share(&client, &issuer, ns.clone(), &token, &holder, 5_000, 1);
    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 5_000);

    set_share(&client, &issuer, ns.clone(), &token, &holder, 0, 2);
    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 0);
}

// ── Case 8: same holder updated twice – total reflects only the latest value ──

#[test]
fn get_total_shares_issued_same_holder_updated_twice_reflects_latest() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let holder = Address::generate(&env);

    reg(&env, &client, &issuer, ns.clone(), &token);
    set_share(&client, &issuer, ns.clone(), &token, &holder, 1_000, 1);
    set_share(&client, &issuer, ns.clone(), &token, &holder, 3_000, 2);
    set_share(&client, &issuer, ns.clone(), &token, &holder, 2_000, 3);

    // Only the last write (2_000) should count.
    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 2_000);
}

// ── Case 9: different namespaces are isolated ─────────────────────────────────

#[test]
fn get_total_shares_issued_different_namespaces_are_isolated() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns_a = symbol_short!("ns_a");
    let ns_b = symbol_short!("ns_b");
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);

    reg(&env, &client, &issuer, ns_a.clone(), &token);
    reg(&env, &client, &issuer, ns_b.clone(), &token);

    set_share(&client, &issuer, ns_a.clone(), &token, &h1, 4_000, 1);
    set_share(&client, &issuer, ns_b.clone(), &token, &h2, 6_000, 1);

    // Each namespace tracks its own total independently.
    assert_eq!(client.get_total_shares_issued(&issuer, &ns_a, &token), 4_000);
    assert_eq!(client.get_total_shares_issued(&issuer, &ns_b, &token), 6_000);
}

// ── Case 10: different issuers are isolated ───────────────────────────────────

#[test]
fn get_total_shares_issued_different_issuers_are_isolated() {
    let (env, client, _) = boot();
    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);

    reg(&env, &client, &issuer_a, ns.clone(), &token);
    reg(&env, &client, &issuer_b, ns.clone(), &token);

    set_share(&client, &issuer_a, ns.clone(), &token, &h1, 1_000, 1);
    set_share(&client, &issuer_b, ns.clone(), &token, &h2, 8_000, 1);

    assert_eq!(client.get_total_shares_issued(&issuer_a, &ns, &token), 1_000);
    assert_eq!(client.get_total_shares_issued(&issuer_b, &ns, &token), 8_000);
}

// ── Case 11: different tokens are isolated ────────────────────────────────────

#[test]
fn get_total_shares_issued_different_tokens_are_isolated() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let ns = symbol_short!("ns");
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);

    reg(&env, &client, &issuer, ns.clone(), &token_a);
    reg(&env, &client, &issuer, ns.clone(), &token_b);

    set_share(&client, &issuer, ns.clone(), &token_a, &h1, 2_500, 1);
    set_share(&client, &issuer, ns.clone(), &token_b, &h2, 7_500, 1);

    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token_a), 2_500);
    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token_b), 7_500);
}

// ── Case 12: total unchanged after a rejected set_holder_share (bps > 10_000) ─

#[test]
fn get_total_shares_issued_unchanged_after_invalid_share_bps_rejection() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let holder = Address::generate(&env);

    reg(&env, &client, &issuer, ns.clone(), &token);
    set_share(&client, &issuer, ns.clone(), &token, &holder, 3_000, 1);
    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 3_000);

    // Attempt to assign 10_001 bps — must be rejected.
    let bad_result =
        client.try_set_holder_share(&issuer, &ns, &token, &holder, &10_001u32, &2u64);
    assert!(bad_result.is_err(), "share_bps > 10_000 must be rejected");
    assert_eq!(
        bad_result.unwrap_err().unwrap(),
        RevoraError::InvalidShareBps as u32,
        "expected InvalidShareBps error"
    );

    // State must be unchanged.
    assert_eq!(
        client.get_total_shares_issued(&issuer, &ns, &token),
        3_000,
        "total_shares_issued must not change after a rejected set_holder_share"
    );
}

// ── Case 13: total unchanged after a rejected set_holder_share (exceeds sum) ──

#[test]
fn get_total_shares_issued_unchanged_after_sum_exceeded_rejection() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);

    reg(&env, &client, &issuer, ns.clone(), &token);
    // h1 holds 8_000; only 2_000 remain.
    set_share(&client, &issuer, ns.clone(), &token, &h1, 8_000, 1);
    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 8_000);

    // Attempting to give h2 3_000 would push the sum to 11_000 — reject.
    let bad_result =
        client.try_set_holder_share(&issuer, &ns, &token, &h2, &3_000u32, &1u64);
    assert!(bad_result.is_err(), "sum > 10_000 must be rejected");
    assert_eq!(
        bad_result.unwrap_err().unwrap(),
        RevoraError::InvalidShareBps as u32,
        "expected InvalidShareBps error when sum exceeds 10_000"
    );

    // Total must remain at 8_000.
    assert_eq!(
        client.get_total_shares_issued(&issuer, &ns, &token),
        8_000,
        "total_shares_issued must not change after a rejected write"
    );
}

// ── Case 14: large number of holders accumulate correctly ─────────────────────

#[test]
fn get_total_shares_issued_many_holders_accumulate_correctly() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");

    reg(&env, &client, &issuer, ns.clone(), &token);

    // Assign 100 bps to each of 100 holders: total = 10_000.
    let n: u32 = 100;
    let bps_each: u32 = 100; // 100 * 100 = 10_000
    for i in 1..=n {
        let holder = Address::generate(&env);
        set_share(&client, &issuer, ns.clone(), &token, &holder, bps_each, i as u64);
    }

    assert_eq!(
        client.get_total_shares_issued(&issuer, &ns, &token),
        10_000,
        "accumulation across many holders must be exact"
    );
}

// ── Case 15: total tracks correctly after supply cap is removed ───────────────

#[test]
fn get_total_shares_issued_tracks_after_cap_cleared() {
    let (env, client, _) = boot();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);

    reg(&env, &client, &issuer, ns.clone(), &token);

    // Set a tight cap of 5_000.
    client
        .set_max_total_supply_shares(&issuer, &ns, &token, &5_000i128)
        .unwrap();

    // Fill up to the cap.
    set_share(&client, &issuer, ns.clone(), &token, &h1, 5_000, 1);
    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 5_000);

    // Adding h2 while cap is active must fail.
    let capped = client.try_set_holder_share(&issuer, &ns, &token, &h2, &1u32, &1u64);
    assert!(capped.is_err(), "cap must prevent going over 5_000");
    assert_eq!(
        capped.unwrap_err().unwrap(),
        RevoraError::MaxTotalSupplySharesExceeded as u32
    );
    assert_eq!(
        client.get_total_shares_issued(&issuer, &ns, &token),
        5_000,
        "total must not change after cap rejection"
    );

    // Remove the cap (set to 0) and verify subsequent writes proceed normally.
    client
        .set_max_total_supply_shares(&issuer, &ns, &token, &0i128)
        .unwrap();

    // Reduce h1 to make room, then assign h2 — no cap enforced.
    // Note: the failed cap-rejection attempt already stored nonce=1 for h2, so
    // we must use nonce=2 here to satisfy the StaleNonce guard.
    set_share(&client, &issuer, ns.clone(), &token, &h1, 4_000, 2);
    set_share(&client, &issuer, ns.clone(), &token, &h2, 3_000, 2);

    assert_eq!(
        client.get_total_shares_issued(&issuer, &ns, &token),
        7_000,
        "total must reflect h1(4000) + h2(3000) after cap removal"
    );
}
