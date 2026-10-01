//! Adversarial coverage for `get_revenue_range_chunk` (`src/lib.rs`, Revora-Contracts).
//!
//! `get_revenue_range_chunk(env, issuer, namespace, token, from_period, to_period,
//! max_periods)` is the paged, read-only revenue aggregator that indexers use to
//! walk an offering's history without hitting the CPU/gas ceiling of the unbounded
//! `get_revenue_range`. Its contract is:
//!
//! * An inverted range (`from_period > to_period`) is an empty result, not a panic.
//! * `(sum, None)` means the range was fully consumed; `(sum, Some(next))` means the
//!   caller must resume from `next` to finish the range.
//! * `max_periods` of `0` — and anything above `MAX_CHUNK_PERIODS` — is normalised
//!   to `MAX_CHUNK_PERIODS`. `0` therefore means "as much as allowed", never
//!   "do no work".
//! * The function is read-only: it emits no events and mutates nothing.
//!
//! The cases below pin each of those, including the cap boundary itself and the
//! resume walk that must cover every period exactly once.

#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

/// Revenue reported for every period seeded by the tests below.
const PER_PERIOD: i128 = 10;

struct Ctx {
    env: Env,
    client: RevoraRevenueShareClient<'static>,
    issuer: Address,
    ns: Symbol,
    token: Address,
    payout: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("def");
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client
        .register_offering(
            &issuer,
            &Vec::new(&env),
            &1u32,
            &ns,
            &token,
            &2_500,
            &payout,
            &0i128,
            &symbol_short!(""),
            &0u32,
        )
        .unwrap();
    Ctx { env, client, issuer, ns, token, payout }
}

/// Report `amount` of revenue for every period id in `[from, to]`, inclusive.
fn seed_periods(c: &Ctx, from: u64, to: u64, amount: i128) {
    let mut period = from;
    while period <= to {
        c.client
            .report_revenue(&c.issuer, &c.ns, &c.token, &c.payout, &amount, &period, &false)
            .unwrap();
        period += 1;
    }
}

fn chunk(c: &Ctx, from: u64, to: u64, max_periods: u32) -> (i128, Option<u64>) {
    c.client.get_revenue_range_chunk(&c.issuer, &c.ns, &c.token, &from, &to, &max_periods)
}

// ── Inverted and degenerate ranges ───────────────────────────────────────────

#[test]
fn inverted_range_returns_empty_without_a_cursor() {
    let c = setup();
    seed_periods(&c, 1, 5, PER_PERIOD);

    // No panic and no partial read: an inverted range is simply empty.
    assert_eq!(chunk(&c, 5, 1, 10), (0, None));
    assert_eq!(chunk(&c, 2, 1, 0), (0, None));
}

#[test]
fn single_period_range_sums_only_that_period() {
    let c = setup();
    seed_periods(&c, 1, 5, PER_PERIOD);

    assert_eq!(chunk(&c, 3, 3, 10), (PER_PERIOD, None));
}

#[test]
fn unreported_periods_contribute_zero() {
    let c = setup();

    assert_eq!(chunk(&c, 10, 12, 10), (0, None));
}

#[test]
fn an_unknown_offering_is_not_an_error() {
    let c = setup();
    seed_periods(&c, 1, 3, PER_PERIOD);

    // A never-registered (issuer, namespace, token) reads as zero revenue.
    let stranger = Address::generate(&c.env);
    let other_token = Address::generate(&c.env);
    let other_ns = symbol_short!("other");

    let res = c.client.get_revenue_range_chunk(&stranger, &other_ns, &other_token, &1, &3, &0);

    assert_eq!(res, (0, None));
}

// ── Cursor semantics ─────────────────────────────────────────────────────────

#[test]
fn range_shorter_than_the_cap_is_fully_consumed() {
    let c = setup();
    seed_periods(&c, 1, 5, PER_PERIOD);

    assert_eq!(chunk(&c, 1, 5, 10), (5 * PER_PERIOD, None));
}

#[test]
fn range_exactly_matching_the_cap_returns_no_cursor() {
    let c = setup();
    seed_periods(&c, 1, 5, PER_PERIOD);

    assert_eq!(chunk(&c, 1, 5, 5), (5 * PER_PERIOD, None));
}

#[test]
fn a_capped_range_returns_the_partial_sum_and_a_resume_cursor() {
    let c = setup();
    seed_periods(&c, 1, 5, PER_PERIOD);

    // Two periods are processed; the cursor points at the first unprocessed one.
    assert_eq!(chunk(&c, 1, 5, 2), (2 * PER_PERIOD, Some(3)));
}

#[test]
fn walking_the_cursor_covers_every_period_exactly_once() {
    let c = setup();
    seed_periods(&c, 1, 5, PER_PERIOD);

    let mut total: i128 = 0;
    let mut visits: u32 = 0;
    let mut cursor: Option<u64> = Some(1);

    while let Some(from) = cursor {
        let (page_total, next) = chunk(&c, from, 5, 2);
        total += page_total;
        cursor = next;
        visits += 1;
        assert!(visits <= 10, "cursor walk must terminate");
    }

    // 5 periods at 2 per page: pages of 2, 2 and 1.
    assert_eq!(visits, 3);
    assert_eq!(total, 5 * PER_PERIOD);
}

// ── Cap normalisation ────────────────────────────────────────────────────────

#[test]
fn a_zero_cap_means_the_maximum_chunk_not_no_work() {
    let c = setup();
    seed_periods(&c, 1, 3, PER_PERIOD);

    // A naive `cap = max_periods` would return (0, Some(1)) here.
    assert_eq!(chunk(&c, 1, 3, 0), (3 * PER_PERIOD, None));
}

#[test]
fn an_oversized_cap_is_clamped_rather_than_rejected() {
    let c = setup();
    seed_periods(&c, 1, 3, PER_PERIOD);

    assert_eq!(chunk(&c, 1, 3, u32::MAX), (3 * PER_PERIOD, None));
    assert_eq!(chunk(&c, 1, 3, crate::MAX_CHUNK_PERIODS + 1), (3 * PER_PERIOD, None));
    assert_eq!(chunk(&c, 1, 3, crate::MAX_CHUNK_PERIODS), (3 * PER_PERIOD, None));
}

// ── The cap boundary itself ──────────────────────────────────────────────────

#[test]
fn a_range_longer_than_the_maximum_chunk_is_served_in_pages() {
    let c = setup();
    let last: u64 = crate::MAX_CHUNK_PERIODS as u64 + 1; // 201 periods
    seed_periods(&c, 1, last, 1);

    assert_eq!(crate::MAX_CHUNK_PERIODS, 200);

    // `max_periods = 0` normalises to the maximum, so the first page stops one
    // period short of the end and hands back the resume cursor.
    let (first_page, cursor) = chunk(&c, 1, last, 0);
    assert_eq!(first_page, crate::MAX_CHUNK_PERIODS as i128);
    assert_eq!(cursor, Some(last));

    let (second_page, final_cursor) = chunk(&c, cursor.unwrap(), last, 0);
    assert_eq!(second_page, 1);
    assert_eq!(final_cursor, None);

    assert_eq!(first_page + second_page, last as i128);
}

// ── Purity ───────────────────────────────────────────────────────────────────

#[test]
fn the_chunk_reader_is_read_only_and_deterministic() {
    let c = setup();
    seed_periods(&c, 1, 4, PER_PERIOD);
    let events_before = c.env.events().all().len();

    let first = chunk(&c, 1, 4, 2);
    let second = chunk(&c, 1, 4, 2);

    assert_eq!(first, second);
    assert_eq!(first, (2 * PER_PERIOD, Some(3)));
    // A view call must not mutate state or emit indexer-visible events.
    assert_eq!(c.env.events().all().len(), events_before);
}
