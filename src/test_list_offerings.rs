//! # Adversarial coverage for `list_offerings` (#1075)
//!
//! `list_offerings` is the convenience reader that off-chain catalogs use to enumerate the
//! tokens an issuer has registered in a namespace. It currently has **no** direct unit
//! coverage on `master`, so these cases pin down the contract that callers rely on:
//!
//! 1. Empty/unknown tenants return an empty vector instead of panicking.
//! 2. Results are ordered by registration index (creation order), deterministically.
//! 3. Tenants are isolated: a different namespace or a different issuer never leaks in.
//! 4. Re-registering a token is idempotent and must not duplicate the listing.
//! 5. The reader is pure: it emits no events and is not gated by the pause switch.
//! 6. **Adversarial:** the reader silently truncates at `MAX_PAGE_LIMIT` (20). Any 21st
//!    offering is invisible through `list_offerings` and only reachable via
//!    `get_offerings_page`, so integrators must page instead of trusting a single call.

extern crate alloc;

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, Symbol, Vec,
};

struct Fixture {
    env: Env,
    client: RevoraRevenueShareClient<'static>,
    admin: Address,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    Fixture { env, client, admin }
}

/// Register `token` under (`issuer`, `namespace`).
///
/// The payout asset is a generated address rather than a real token contract, so the
/// decimals-consistency probe in `register_offering` is skipped (its `try_decimals()`
/// call reverts for non-token addresses) and the path under test stays minimal.
fn register(f: &Fixture, issuer: &Address, namespace: &Symbol, token: &Address) {
    let payout_asset = Address::generate(&f.env);
    let _ = f.client.register_offering(
        issuer,
        &Vec::new(&f.env),
        &1u32,
        namespace,
        token,
        &5000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
}

fn as_vec(env: &Env, values: &[&Address]) -> Vec<Address> {
    let mut out = Vec::new(env);
    for value in values.iter() {
        out.push_back((*value).clone());
    }
    out
}

// ─── 1. Empty / unknown tenants ───────────────────────────────────────────────

#[test]
fn test_list_offerings_empty_tenant_returns_empty_vec() {
    let f = setup();
    let issuer = Address::generate(&f.env);
    let namespace = symbol_short!("prime");

    let listed = f.client.list_offerings(&issuer, &namespace);

    assert_eq!(listed.len(), 0);
    assert_eq!(f.client.get_offering_count(&issuer, &namespace), 0);
}

#[test]
fn test_list_offerings_unknown_issuer_returns_empty_vec() {
    let f = setup();
    let registered = Address::generate(&f.env);
    let stranger = Address::generate(&f.env);
    let namespace = symbol_short!("prime");
    let token = Address::generate(&f.env);

    register(&f, &registered, &namespace, &token);

    // The stranger shares the namespace symbol but never registered an offering.
    assert_eq!(f.client.list_offerings(&stranger, &namespace).len(), 0);
    assert_eq!(f.client.list_offerings(&registered, &namespace).len(), 1);
}

// ─── 2. Ordering ──────────────────────────────────────────────────────────────

#[test]
fn test_list_offerings_preserves_registration_order() {
    let f = setup();
    let issuer = Address::generate(&f.env);
    let namespace = symbol_short!("prime");

    let first = Address::generate(&f.env);
    let second = Address::generate(&f.env);
    let third = Address::generate(&f.env);
    register(&f, &issuer, &namespace, &first);
    register(&f, &issuer, &namespace, &second);
    register(&f, &issuer, &namespace, &third);

    let listed = f.client.list_offerings(&issuer, &namespace);

    assert_eq!(listed.len(), 3);
    assert_eq!(listed, as_vec(&f.env, &[&first, &second, &third]));
    assert_eq!(f.client.get_offering_count(&issuer, &namespace), 3);
}

// ─── 3. Tenant isolation ──────────────────────────────────────────────────────

#[test]
fn test_list_offerings_isolated_by_namespace() {
    let f = setup();
    let issuer = Address::generate(&f.env);
    let alpha = symbol_short!("alpha");
    let beta = symbol_short!("beta");

    let alpha_token = Address::generate(&f.env);
    let beta_token = Address::generate(&f.env);
    register(&f, &issuer, &alpha, &alpha_token);
    register(&f, &issuer, &beta, &beta_token);

    assert_eq!(f.client.list_offerings(&issuer, &alpha), as_vec(&f.env, &[&alpha_token]));
    assert_eq!(f.client.list_offerings(&issuer, &beta), as_vec(&f.env, &[&beta_token]));
    assert_eq!(f.client.get_offering_count(&issuer, &alpha), 1);
    assert_eq!(f.client.get_offering_count(&issuer, &beta), 1);
}

#[test]
fn test_list_offerings_isolated_by_issuer() {
    let f = setup();
    let first_issuer = Address::generate(&f.env);
    let second_issuer = Address::generate(&f.env);
    let namespace = symbol_short!("prime");

    let first_token = Address::generate(&f.env);
    let second_token = Address::generate(&f.env);
    register(&f, &first_issuer, &namespace, &first_token);
    register(&f, &second_issuer, &namespace, &second_token);

    assert_eq!(
        f.client.list_offerings(&first_issuer, &namespace),
        as_vec(&f.env, &[&first_token])
    );
    assert_eq!(
        f.client.list_offerings(&second_issuer, &namespace),
        as_vec(&f.env, &[&second_token])
    );
}

// ─── 4. Idempotency ───────────────────────────────────────────────────────────

#[test]
fn test_list_offerings_does_not_duplicate_re_registered_token() {
    let f = setup();
    let issuer = Address::generate(&f.env);
    let namespace = symbol_short!("prime");
    let token = Address::generate(&f.env);

    register(&f, &issuer, &namespace, &token);
    register(&f, &issuer, &namespace, &token);
    register(&f, &issuer, &namespace, &token);

    let listed = f.client.list_offerings(&issuer, &namespace);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed, as_vec(&f.env, &[&token]));
    assert_eq!(f.client.get_offering_count(&issuer, &namespace), 1);
}

// ─── 5. Purity ────────────────────────────────────────────────────────────────

#[test]
fn test_list_offerings_emits_no_events_and_is_repeatable() {
    let f = setup();
    let issuer = Address::generate(&f.env);
    let namespace = symbol_short!("prime");
    let token = Address::generate(&f.env);
    register(&f, &issuer, &namespace, &token);

    let events_before = f.env.events().all().len();
    let first = f.client.list_offerings(&issuer, &namespace);
    let second = f.client.list_offerings(&issuer, &namespace);

    assert_eq!(first, second, "list_offerings must be deterministic");
    assert_eq!(f.env.events().all().len(), events_before, "list_offerings must not emit events");
    assert_eq!(first, as_vec(&f.env, &[&token]));
}

#[test]
fn test_list_offerings_remains_readable_while_paused() {
    let f = setup();
    let issuer = Address::generate(&f.env);
    let namespace = symbol_short!("prime");
    let token = Address::generate(&f.env);
    register(&f, &issuer, &namespace, &token);

    let _ = f.client.pause_admin(&f.admin);

    // Reads are intentionally not gated by the pause switch: dashboards must stay able to
    // enumerate an issuer's offerings during incident response.
    assert_eq!(f.client.list_offerings(&issuer, &namespace), as_vec(&f.env, &[&token]));
}

// ─── 6. The silent truncation adversary ───────────────────────────────────────

#[test]
fn test_list_offerings_silently_truncates_at_max_page_limit() {
    let f = setup();
    let issuer = Address::generate(&f.env);
    let namespace = symbol_short!("prime");

    // Register one offering beyond the reader's page cap.
    let total = MAX_PAGE_LIMIT + 1;
    let mut tokens: Vec<Address> = Vec::new(&f.env);
    for _ in 0..total {
        let token = Address::generate(&f.env);
        register(&f, &issuer, &namespace, &token);
        tokens.push_back(token);
    }

    assert_eq!(f.client.get_offering_count(&issuer, &namespace), total);

    // Adversarial boundary: the 21st offering is silently dropped from the convenience
    // reader, with no error and no cursor. Callers that trust it will under-report.
    let listed = f.client.list_offerings(&issuer, &namespace);
    assert_eq!(listed.len(), MAX_PAGE_LIMIT);
    let listed_not_containing_tail = listed.iter().all(|token| token != tokens.get(MAX_PAGE_LIMIT).unwrap());
    assert!(listed_not_containing_tail, "tail offering must be absent from list_offerings");

    for index in 0..MAX_PAGE_LIMIT {
        assert_eq!(listed.get(index).unwrap(), tokens.get(index).unwrap());
    }

    // The tail is still reachable through the paginated reader, which is the API
    // integrators must use once a tenant can exceed the page cap.
    let (tail, next_cursor) =
        f.client.get_offerings_page(&issuer, &namespace, &MAX_PAGE_LIMIT, &MAX_PAGE_LIMIT);
    assert_eq!(tail.len(), 1);
    assert_eq!(tail.get(0).unwrap().token, tokens.get(MAX_PAGE_LIMIT).unwrap());
    assert_eq!(next_cursor, None);
}
