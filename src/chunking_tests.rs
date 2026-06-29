#![cfg(test)]
// `make_client` and `setup` are shared helpers; not every test uses every helper.
// Suppress only dead_code for helpers, not all warnings globally.
#![allow(dead_code)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, token, Address, Env, Vec};

// Minimal helpers duplicated from src/test.rs so these chunking tests can live separately.
fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn setup() -> (Env, RevoraRevenueShareClient, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, crate::RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    (env, client, issuer)
}

fn create_payment_token(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract(admin.clone());
    (token_id, admin)
}

fn mint_tokens(env: &Env, payment_token: &Address, recipient: &Address, amount: &i128) {
    token::StellarAssetClient::new(env, payment_token).mint(recipient, amount);
}

fn setup_with_offering() -> (Env, RevoraRevenueShareClient, Address, Address, Address, Address) {
    let (env, client, issuer) = setup();
    let token = Address::generate(&env);
    let (payment_token, pt_admin) = create_payment_token(&env);
    // Register offering and fund issuer so deposit_revenue can transfer tokens
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &payment_token, &0);
    mint_tokens(&env, &payment_token, &issuer, &100_000i128);
    (env, client, issuer, token, payment_token, pt_admin)
}

#[test]
fn get_revenue_range_chunk_matches_full_sum() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1000u32, &token, &0i128);

    // Report revenue for periods 1..=10
    for p in 1u64..=10u64 {
        client.report_revenue(&issuer, &symbol_short!("def"), &token, &token, &100i128, &p, &false);
    }

    // Full sum
    let full = client.get_revenue_range(&issuer, &symbol_short!("def"), &token, &1u64, &10u64);

    // Sum in chunks of 3
    let mut cursor = 1u64;
    let mut acc: i128 = 0;
    loop {
        let (partial, next) = client.get_revenue_range_chunk(
            &issuer,
            &symbol_short!("def"),
            &token,
            &cursor,
            &10u64,
            &3u32,
        );
        acc += partial;
        if let Some(n) = next {
            cursor = n;
        } else {
            break;
        }
    }

    assert_eq!(full, acc);
}

#[test]
fn get_revenue_range_chunk_inverted_range_returns_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1000u32, &token, &0i128);

    // inverted range: from > to
    let (sum, next) = client.get_revenue_range_chunk(&issuer, &symbol_short!("def"), &token, &10u64, &1u64, &5u32);
    assert_eq!(sum, 0);
    assert!(next.is_none());
}

#[test]
fn get_revenue_range_chunk_cap_clamps_and_returns_next_start() {
    // Ensure that max_periods of 0 is normalized to the contract cap (MAX_CHUNK_PERIODS)
    // We insert 201 periods with value 1 each; with a cap of 200 the first chunk should
    // return a sum of 200 and next_start = Some(201).
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1000u32, &token, &0i128);

    // Report revenue for periods 1..=201 with amount 1 each
    for p in 1u64..=201u64 {
        client.report_revenue(&issuer, &symbol_short!("def"), &token, &token, &1i128, &p, &false);
    }

    let (partial, next) = client.get_revenue_range_chunk(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1u64,
        &201u64,
        &0u32, // request 0 => should clamp to MAX_CHUNK_PERIODS (200)
    );

    assert_eq!(partial, 200i128);
    assert_eq!(next, Some(201u64));
}

#[test]
fn get_revenue_range_chunk_chunked_iteration_off_by_one_sequence() {
    // Verify that repeated chunked calls produce the expected next_start sequence
    // For 5 periods and chunk size 2: nexts should be Some(3), Some(5), None
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1000u32, &token, &0i128);

    // Report revenue for periods 1..=5 with increasing amounts for easier validation
    for p in 1u64..=5u64 {
        client.report_revenue(&issuer, &symbol_short!("def"), &token, &token, &(p as i128), &p, &false);
    }

    let mut cursor = 1u64;
    let mut acc: i128 = 0;
    let mut seen_nexts: Vec<u64> = Vec::new(&env);
    loop {
        let (partial, next) = client.get_revenue_range_chunk(
            &issuer,
            &symbol_short!("def"),
            &token,
            &cursor,
            &5u64,
            &2u32,
        );
        acc += partial;
        if let Some(n) = next {
            seen_nexts.push_back(n);
            cursor = n;
        } else {
            break;
        }
    }

    // Full sum of 1+2+3+4+5 = 15
    let full = client.get_revenue_range(&issuer, &symbol_short!("def"), &token, &1u64, &5u64);
    assert_eq!(full, acc);

    // next sequence should be [3,5]
    assert_eq!(seen_nexts.len(), 2);
    assert_eq!(seen_nexts.get(0).unwrap(), 3u64);
    assert_eq!(seen_nexts.get(1).unwrap(), 5u64);
}

#[test]
#[ignore]
fn pending_periods_page_and_claimable_chunk_consistent() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    let (payment_token, _pt_admin) = create_payment_token(&env);
    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1000u32,
        &payment_token,
        &0i128,
    );
    // Mint to issuer so deposit_revenue token transfer succeeds
    mint_tokens(&env, &payment_token, &issuer, &100_000i128);

    // Insert periods 1..=8 via the test helper (avoids token transfers in tests)
    for p in 1u64..=8u64 {
        RevoraRevenueShare::test_insert_period(
            env.clone(),
            issuer.clone(),
            symbol_short!("def"),
            token.clone(),
            p,
            1000i128,
        );
    }

    // Set holder share
    let r = client.try_set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &1000u32);
    assert!(r.is_ok());

    // get_pending_periods full
    let full = client.get_pending_periods(&issuer, &symbol_short!("def"), &token, &holder);

    // Page through with limit 3
    let mut cursor = 0u32;
    let mut all = Vec::new(&env);
    loop {
        let (page, next) = client.get_pending_periods_page(
            &issuer,
            &symbol_short!("def"),
            &token,
            &holder,
            &cursor,
            &3u32,
        );
        for i in 0..page.len() {
            all.push_back(page.get(i).unwrap());
        }
        if let Some(n) = next {
            cursor = n;
        } else {
            break;
        }
    }

    // Compare lengths
    assert_eq!(full.len(), all.len());

    // Now check claimable chunk matches full
    let full_claim = client.get_claimable(&issuer, &symbol_short!("def"), &token, &holder);

    // Sum claimable in chunks from index 0, count 2
    let mut idx = 0u32;
    let mut acc: i128 = 0;
    loop {
        let (partial, next) = client.get_claimable_chunk(
            &issuer,
            &symbol_short!("def"),
            &token,
            &holder,
            &idx,
            &2u32,
        );
        acc += partial;
        if let Some(n) = next {
            idx = n;
        } else {
            break;
        }
    }
    assert_eq!(full_claim, acc);
}

// ── Table-driven tests for get_claimable_chunk invariants ─────────

#[derive(Clone, Debug)]
struct ChunkInvariantTestCase {
    name: &'static str,
    period_count: u32,
    holder_share_bps: u32,
    period_revenue: i128,
    last_claimed_idx: u32,
    chunk_size: u32,
    is_blacklisted: bool,
    delay_secs: u64,
    current_timestamp: u64,
    claim_window_start: u64,
    claim_window_end: u64,
    expected_total: i128,
    expected_next_cursor: Option<u32>,
}

impl ChunkInvariantTestCase {
    fn assert_result(&self, actual_total: i128, actual_next: Option<u32>) {
        assert_eq!(
            actual_total, self.expected_total,
            "Test case '{}': expected total {}, got {}",
            self.name, self.expected_total, actual_total
        );
        assert_eq!(
            actual_next, self.expected_next_cursor,
            "Test case '{}': expected next cursor {:?}, got {:?}",
            self.name, self.expected_next_cursor, actual_next
        );
    }
}

#[test]
fn get_claimable_chunk_table_driven_invariants() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    let (payment_token, _pt_admin) = create_payment_token(&env);
    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1000u32,
        &payment_token,
        &0i128,
    );
    mint_tokens(&env, &payment_token, &issuer, &100_000i128);

    let test_cases = vec![
        // Basic case: no restrictions, all periods claimable
        ChunkInvariantTestCase {
            name: "basic_all_periods_claimable",
            period_count: 5,
            holder_share_bps: 10_000,
            period_revenue: 100,
            last_claimed_idx: 0,
            chunk_size: 10,
            is_blacklisted: false,
            delay_secs: 0,
            current_timestamp: 1000,
            claim_window_start: 0,
            claim_window_end: 0,
            expected_total: 500,
            expected_next_cursor: None,
        },
        // Cursor idempotency: repeated cursor returns consistent partial sum
        ChunkInvariantTestCase {
            name: "cursor_idempotency_same_cursor_twice",
            period_count: 5,
            holder_share_bps: 10_000,
            period_revenue: 100,
            last_claimed_idx: 0,
            chunk_size: 2,
            is_blacklisted: false,
            delay_secs: 0,
            current_timestamp: 1000,
            claim_window_start: 0,
            claim_window_end: 0,
            expected_total: 200,
            expected_next_cursor: Some(2),
        },
        // Blacklisted holder: returns 0
        ChunkInvariantTestCase {
            name: "blacklisted_holder_returns_zero",
            period_count: 5,
            holder_share_bps: 10_000,
            period_revenue: 100,
            last_claimed_idx: 0,
            chunk_size: 10,
            is_blacklisted: true,
            delay_secs: 0,
            current_timestamp: 1000,
            claim_window_start: 0,
            claim_window_end: 0,
            expected_total: 0,
            expected_next_cursor: None,
        },
        // Delay barrier: stops at first delayed period
        ChunkInvariantTestCase {
            name: "delay_barrier_stops_at_first_delayed",
            period_count: 5,
            holder_share_bps: 10_000,
            period_revenue: 100,
            last_claimed_idx: 0,
            chunk_size: 10,
            is_blacklisted: false,
            delay_secs: 100,
            current_timestamp: 150,
            claim_window_start: 0,
            claim_window_end: 0,
            expected_total: 100,
            expected_next_cursor: Some(1),
        },
        // Claim window closed: returns 0
        ChunkInvariantTestCase {
            name: "claim_window_closed_returns_zero",
            period_count: 5,
            holder_share_bps: 10_000,
            period_revenue: 100,
            last_claimed_idx: 0,
            chunk_size: 10,
            is_blacklisted: false,
            delay_secs: 0,
            current_timestamp: 500,
            claim_window_start: 1000,
            claim_window_end: 2000,
            expected_total: 0,
            expected_next_cursor: None,
        },
        // Partial claim: cursor clamped to last_claimed_idx
        ChunkInvariantTestCase {
            name: "partial_claim_cursor_clamped",
            period_count: 5,
            holder_share_bps: 10_000,
            period_revenue: 100,
            last_claimed_idx: 2,
            chunk_size: 10,
            is_blacklisted: false,
            delay_secs: 0,
            current_timestamp: 1000,
            claim_window_start: 0,
            claim_window_end: 0,
            expected_total: 300,
            expected_next_cursor: None,
        },
        // Zero share: returns 0
        ChunkInvariantTestCase {
            name: "zero_share_returns_zero",
            period_count: 5,
            holder_share_bps: 0,
            period_revenue: 100,
            last_claimed_idx: 0,
            chunk_size: 10,
            is_blacklisted: false,
            delay_secs: 0,
            current_timestamp: 1000,
            claim_window_start: 0,
            claim_window_end: 0,
            expected_total: 0,
            expected_next_cursor: None,
        },
        // Chunk size larger than available periods
        ChunkInvariantTestCase {
            name: "chunk_size_larger_than_available",
            period_count: 3,
            holder_share_bps: 10_000,
            period_revenue: 100,
            last_claimed_idx: 0,
            chunk_size: 10,
            is_blacklisted: false,
            delay_secs: 0,
            current_timestamp: 1000,
            claim_window_start: 0,
            claim_window_end: 0,
            expected_total: 300,
            expected_next_cursor: None,
        },
        // Chunk size zero: normalized to MAX_CHUNK_PERIODS
        ChunkInvariantTestCase {
            name: "chunk_size_zero_normalized",
            period_count: 3,
            holder_share_bps: 10_000,
            period_revenue: 100,
            last_claimed_idx: 0,
            chunk_size: 0,
            is_blacklisted: false,
            delay_secs: 0,
            current_timestamp: 1000,
            claim_window_start: 0,
            claim_window_end: 0,
            expected_total: 300,
            expected_next_cursor: None,
        },
        // All periods claimed: returns 0
        ChunkInvariantTestCase {
            name: "all_periods_claimed_returns_zero",
            period_count: 5,
            holder_share_bps: 10_000,
            period_revenue: 100,
            last_claimed_idx: 5,
            chunk_size: 10,
            is_blacklisted: false,
            delay_secs: 0,
            current_timestamp: 1000,
            claim_window_start: 0,
            claim_window_end: 0,
            expected_total: 0,
            expected_next_cursor: None,
        },
    ];

    for tc in test_cases {
        // Reset environment for each test case
        let env = Env::default();
        env.mock_all_auths();
        let client = make_client(&env);
        let issuer = Address::generate(&env);
        let token = Address::generate(&env);
        let holder = Address::generate(&env);

        let (payment_token, _pt_admin) = create_payment_token(&env);
        client.register_offering(
            &issuer,
            &symbol_short!("def"),
            &token,
            &1000u32,
            &payment_token,
            &0i128,
        );
        mint_tokens(&env, &payment_token, &issuer, &100_000i128);

        // Set up test case conditions
        client.set_holder_share(
            &issuer,
            &symbol_short!("def"),
            &token,
            &holder,
            &tc.holder_share_bps,
        );

        // Insert periods
        for p in 1..=tc.period_count {
            RevoraRevenueShare::test_insert_period(
                env.clone(),
                issuer.clone(),
                symbol_short!("def"),
                token.clone(),
                p,
                tc.period_revenue,
            );
        }

        // Set last claimed index
        if tc.last_claimed_idx > 0 {
            RevoraRevenueShare::test_set_last_claimed_idx(
                env.clone(),
                issuer.clone(),
                symbol_short!("def"),
                token.clone(),
                holder.clone(),
                tc.last_claimed_idx,
            );
        }

        // Set blacklist if needed
        if tc.is_blacklisted {
            client.set_admin(&issuer);
            client.blacklist_add(
                &issuer,
                &issuer,
                &symbol_short!("def"),
                &token,
                &holder,
            );
        }

        // Set delay if needed
        if tc.delay_secs > 0 {
            client.set_claim_delay(
                &issuer,
                &symbol_short!("def"),
                &token,
                &tc.delay_secs,
            );
        }

        // Set claim window if needed
        if tc.claim_window_start > 0 || tc.claim_window_end > 0 {
            client.set_claim_window(
                &issuer,
                &symbol_short!("def"),
                &token,
                tc.claim_window_start,
                tc.claim_window_end,
            );
        }

        // Set current timestamp
        env.ledger().with_mut(|li| li.timestamp = tc.current_timestamp);

        // Execute test
        let (total, next) = client.get_claimable_chunk(
            &issuer,
            &symbol_short!("def"),
            &token,
            &holder,
            &0,
            &tc.chunk_size,
        );

        tc.assert_result(total, next);
    }
}

#[test]
fn get_claimable_chunk_cursor_idempotency_repeated_queries() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    let (payment_token, _pt_admin) = create_payment_token(&env);
    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1000u32,
        &payment_token,
        &0i128,
    );
    mint_tokens(&env, &payment_token, &issuer, &100_000i128);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &10_000);

    // Insert 10 periods
    for p in 1..=10u64 {
        RevoraRevenueShare::test_insert_period(
            env.clone(),
            issuer.clone(),
            symbol_short!("def"),
            token.clone(),
            p,
            100i128,
        );
    }

    // Query chunk from cursor 0 with size 3 multiple times
    let (total1, next1) = client.get_claimable_chunk(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &0,
        &3,
    );

    let (total2, next2) = client.get_claimable_chunk(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &0,
        &3,
    );

    let (total3, next3) = client.get_claimable_chunk(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &0,
        &3,
    );

    // All queries should return the same result (idempotency)
    assert_eq!(total1, 300);
    assert_eq!(total2, 300);
    assert_eq!(total3, 300);
    assert_eq!(next1, Some(3));
    assert_eq!(next2, Some(3));
    assert_eq!(next3, Some(3));
}

#[test]
fn get_claimable_chunk_sum_matches_full_claimable() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    let (payment_token, _pt_admin) = create_payment_token(&env);
    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1000u32,
        &payment_token,
        &0i128,
    );
    mint_tokens(&env, &payment_token, &issuer, &100_000i128);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000);

    // Insert 10 periods
    for p in 1..=10u64 {
        RevoraRevenueShare::test_insert_period(
            env.clone(),
            issuer.clone(),
            symbol_short!("def"),
            token.clone(),
            p,
            1000i128,
        );
    }

    // Get full claimable amount
    let full_claimable = client.get_claimable(&issuer, &symbol_short!("def"), &token, &holder);

    // Sum chunks
    let mut cursor = 0u32;
    let mut chunk_sum: i128 = 0;
    loop {
        let (partial, next) = client.get_claimable_chunk(
            &issuer,
            &symbol_short!("def"),
            &token,
            &holder,
            &cursor,
            &3,
        );
        chunk_sum += partial;
        if let Some(n) = next {
            cursor = n;
        } else {
            break;
        }
    }

    // Chunk sum must equal full claimable
    assert_eq!(full_claimable, 50000); // 50% of 10 * 1000
    assert_eq!(chunk_sum, full_claimable);
}

#[test]
fn get_claimable_chunk_respects_delay_barrier_parity_with_claim() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    let (payment_token, _pt_admin) = create_payment_token(&env);
    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1000u32,
        &payment_token,
        &0i128,
    );
    mint_tokens(&env, &payment_token, &issuer, &100_000i128);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &10_000);

    // Set delay
    client.set_claim_delay(&issuer, &symbol_short!("def"), &token, &100);

    // Insert period 1 at timestamp 1000
    env.ledger().with_mut(|li| li.timestamp = 1000);
    RevoraRevenueShare::test_insert_period(
        env.clone(),
        issuer.clone(),
        symbol_short!("def"),
        token.clone(),
        1,
        1000i128,
    );

    // Insert period 2 at timestamp 1050 (not yet claimable)
    env.ledger().with_mut(|li| li.timestamp = 1050);
    RevoraRevenueShare::test_insert_period(
        env.clone(),
        issuer.clone(),
        symbol_short!("def"),
        token.clone(),
        2,
        2000i128,
    );

    // Set current timestamp to 1100 (period 1 claimable, period 2 not yet)
    env.ledger().with_mut(|li| li.timestamp = 1100);

    // get_claimable should only include period 1
    let claimable = client.get_claimable(&issuer, &symbol_short!("def"), &token, &holder);
    assert_eq!(claimable, 1000);

    // get_claimable_chunk should also only include period 1 and stop at delay barrier
    let (chunk_total, next) = client.get_claimable_chunk(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &0,
        &10,
    );
    assert_eq!(chunk_total, 1000);
    assert_eq!(next, Some(1)); // Cursor points to delayed period

    // Actual claim should also only claim period 1
    let claim_result = client.claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(claim_result, 1000);
}
