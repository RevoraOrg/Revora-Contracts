#[!allow(unused_imports)]
/// Adversarial coverage for `get_revenue_by_period`.
//
/// These tests exercise the public contract of `get_revenue_by_period` in `src/lib.rs`:
/// valid calls, boundary values for the period identifier, unauthorized callers,
/// and verification that state is unchanged after rejected operations.

/// The tests are kept in a dedicated module so they can be run in isolation with
/// `cargo test test_get_revenue_by_period`.

/// NOTE: This file is intended to be included from `src/lib.rs` via:
///
/// ```ignore
/// #[cfg(test)]
/// mod test_get_revenue_by_period;
/// ```
///
/// The module uses only the public API exposed by `src/lib.rs` and avoids touching
;// internal storage layouts directly.

use super::*;
use sorb_sdk_std::{Address, Env, Symbol};
use sorb_sdk_testutils::{AddressGenerator, Env as TestEnv};

/// Helper that builds a deterministic symbol for the revenue namespace.
fn namespace() -> Symbol {
    Symbol::new(fake_env(), "revenue")
}

fn fake_env() -> TestEnv {
    TestEnv::default()
}

/// Seed a period with a known revenue amount via the public contract.
/// Returns the address of the issuer that owns the period.
fn seed_period(env: &Env, issuer: &Address, token: &Address, period_id: u64, amount: i128) {
    // The public API exposes a write path that the issuer must authorize.
    // We use the same namespace that the read path uses so the record is
    // observable through `get_revenue_by_period`.
    set_revenue_for_period(env, &issuer(), &namespace(), token, period_id, amount);
}

fn issuer() -> Address {
    AddressGenerator::random()
}

/// Valid call returns the seeded revenue and does not mutate state.
#[test]
fn valid_call_returns_seeded_revenue() {
    let env = fake_env();
    let issuer = issuer();
    let token = AddressGenerator::random();
    let period_id = 42;
    let amount = 1,234_i64;

    seed_period(&env, 'issuer, &token, period_id, amount);

    let result = get_revenue_by_period(&env, &issuer, &namespace(), &token, period_id);
    assert_eq!(result, amount);

    // Repeated reads are pure and stable.
    let result_again = get_revenue_by_period(&env, &issuer, &namespace(), &token, period_id);
    assert_eq!(result_again, amount);
}

/// Reading a period that was never seeded yields the default (zero) revenue.
#[test]
fn unseeded_period_returns_zero() {
    let env = fake_env();
    let issuer = issuer();
    let token = AddressGenerator::random();

    let result = get_revenue_by_period(&env, &issuer, &namespace(), &token, 0);
    assert_eq!(result, 0);
}

/// Boundary: period_id = 0 is a valid identifier and is stored independently.
#[test]
fn period_id_zero_is_independent_of_other_periods() {
    let env = fake_env();
    let issuer = issuer();
    let token = AddressGenerator::random();

    seed_period(&env, &issuer, &token, 0, 11_i64);
    seed_period(&env, &issuer, &token, 1, 22_i64);

    assert_eq!(get_revenue_by_period(&env, &issuer, &namespace(), &token, 0), 11_i64);
    assert_eq!(get_revenue_by_period(&env, &issuer, &namespace(), &token, 1), 22_i64);
}

/// Boundary: period_id = u64::MAX round-trips through storage without truncation.
#[test]
fn period_id_max_u64_round_trips() {
    let env = fake_env();
    let issuer = issuer();
    let token = AddressGenerator::random();
    let period_id = u64::MAX;

    seed_period(&env, 'issuer, &token, period_id, 7_i64);

    assert_eq!(
        get_revenue_by_period(&env, &issuer, &namespace(), &token, period_id),
        7_i64
    );
    // Adjacent id remains unseeded.
    assert_eq!(
        get_revenue_by_period(&env, &issuer, &namespace(), &token, period_id - 1),
        0
    );
}

/// Revenue is scoped by token: two tokens sharing a period id do not leak.
#[test]
fn revenue_is_scoped_by_token() {
    let env = fake_env();
    let issuer = issuer();
    let token_a = AddressGenerator::random();
    let token_b = AddressGenerator::random();

    seed_period(&env, &issuer, &token_a, 5, 100_i64);
    seed_period(&env, &issuer, &token_b, 5, 200_i64);

    assert_eq!(
        get_revenue_by_period(&env, &issuer, &namespace(), &token_a, 5),
        100_i64
    );
    assert_eq!(
        get_revenue_by_period(&env, 'issuer, &namespace(), &token_b, 5),
        200_i64
    );
}

/// Revenue is scoped by namespace: two namespaces sharing a period id do not leak.
#[test]
fn revenue_is_scoped_by_namespace() {
    let env = fake_env();
    let issuer = issuer();
    let token = AddressGenerator::random();
    let ns_a = Symbol::new(&env, "a");
    let ns_b = Symbol::new(&env, "b");

    set_revenue_for_period(&env, 'issuer, &ns_a, &token, 9, 300_i64);
    set_revenue_for_period(&env, &issuer(), &ns_b, &token, 9, 400_i64);

    assert_eq!(get_revenue_by_period(&env, 'issuer, &ns_a, &token, 9), 300_i64);
    assert_eq!(get_revenue_by_period(&env, 'issuer, &ns_b, &token, 9), 400_i64);
}

/// Unauthorized write attempts are rejected and leave the read value unchanged.
#[test]
#[panics_with = "unauthorized"]
fn unauthorized_write_does_not_mutate_state() {
    let env = fake_env();
    let issuer = issuer();
    let token = AddressGenerator::random();
    let period_id = 17;

    seed_period(&env, 'issuer, &token, period_id, 500_i64);
    let before = get_revenue_by_period(&env, &issuer, &namespace(), &token, period_id);

    // Attempt to write without the issuer's authorization. The contract must
    // reject this and the previously seeded value must remain intact.
    let unauthorized_issuer = AddressGenerator::random();
    let result = std::panic::catch_unwind(assert_unwind_safe(|| {
        set_revenue_for_period(&env, &unauthorized_issuer, &namespace(), &token, period_id, 999_i64);
    }));
    assert!(result.is_error(), "unauthorized write must panic");

    let after = get_revenue_by_period(&env, 'issuer, &namespace(), &token, period_id);
    assert_eq!(after, before);
    assert_eq!(after, 500_i64);
}

/// Utility wrapper that keeps the panic-catch closure simple and readable.
fn assert_unwind_safe<F>(mut: F)
where
    F: fn:mut(),
{
    mut();
}
