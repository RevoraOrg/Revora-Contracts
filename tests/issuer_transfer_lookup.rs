//! Regression coverage for #1335: accepting/rejecting an issuer transfer must
//! resolve the pending record directly from the old issuer instead of scanning
//! every registered issuer and namespace.
//!
//! The budget assertion uses `Env::budget()` to compare the CPU cost of accept
//! with one registered issuer against 50 issuers x 5 namespaces. A scan would
//! add storage reads for every issuer/namespace pair; a direct key lookup stays
//! flat.

use revora_contracts::{DataKey2, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

const NAMESPACES_PER_ISSUER: u32 = 5;
const LARGE_ISSUER_COUNT: u32 = 50;

/// Seed the same issuer/namespace registry entries that
/// `ensure_issuer_registered` / `ensure_namespace_registered` maintain.
fn seed_registry(
    env: &Env,
    contract_id: &Address,
    issuers: &Vec<Address>,
    namespaces: &Vec<Symbol>,
    scanned_issuer_count: u32,
) {
    env.as_contract(contract_id, || {
        for i in 0..issuers.len() {
            let issuer = issuers.get(i).unwrap();
            env.storage().persistent().set(&DataKey2::IssuerItem(i), &issuer);
            env.storage().persistent().set(&DataKey2::IssuerRegistered(issuer.clone()), &true);
            env.storage()
                .persistent()
                .set(&DataKey2::NamespaceCount(issuer.clone()), &namespaces.len());

            for j in 0..namespaces.len() {
                let namespace = namespaces.get(j).unwrap();
                env.storage()
                    .persistent()
                    .set(&DataKey2::NamespaceItem(issuer.clone(), j), &namespace);
                env.storage()
                    .persistent()
                    .set(&DataKey2::NamespaceRegistered(issuer.clone(), namespace), &true);
            }
        }

        // Both variants keep the same 50 x 5 storage footprint; only the count
        // the old scan iterates is varied to isolate lookup complexity.
        env.storage().persistent().set(&DataKey2::IssuerCount, &scanned_issuer_count);
    });
}

/// Register one offering, propose a transfer, and return the fixture plus the
/// pending-transfer identities. `issuer_count` controls how many issuers and
/// namespaces exist in the registry before accept is called.
fn pending_transfer_fixture(
    issuer_count: u32,
    scanned_issuer_count: u32,
) -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Symbol, Address) {
    let env = Env::default();
    env.mock_all_auths();
    // Setup writes the full registry; the measured accept call starts from a
    // clean tracker below. Unlimited budget keeps setup from tripping the
    // default per-test host limit while seeding 50 x 5 entries.
    env.budget().reset_unlimited();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let mut issuers = Vec::new(&env);
    for _ in 0..issuer_count {
        issuers.push_back(Address::generate(&env));
    }

    let mut namespaces = Vec::new(&env);
    for i in 0..NAMESPACES_PER_ISSUER {
        namespaces.push_back(Symbol::new(&env, &format!("ns{}", i)));
    }

    let old_issuer = issuers.get(0).unwrap();
    let namespace = namespaces.get(0).unwrap();
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.register_offering(
        &old_issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &5_000u32,
        &payout_asset,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    seed_registry(&env, &contract_id, &issuers, &namespaces, scanned_issuer_count);

    let new_issuer = Address::generate(&env);
    // Pre-register the recipient so accept's `ensure_issuer_registered` is a
    // no-op in both variants; otherwise writing IssuerItem(1) vs IssuerItem(50)
    // would add an unrelated new-entry cost to the comparison.
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey2::IssuerRegistered(new_issuer.clone()), &true);
    });
    client.propose_issuer_transfer(&old_issuer, &namespace, &token, &new_issuer);

    (env, client, old_issuer, new_issuer, namespace, token)
}

/// Measure only the accept call. The registry is already populated, so any
/// growth in this delta is the lookup cost under test.
fn accept_cpu_cost(issuer_count: u32, scanned_issuer_count: u32) -> (u64, u64) {
    let (env, client, old_issuer, new_issuer, namespace, token) =
        pending_transfer_fixture(issuer_count, scanned_issuer_count);

    let before = env.budget().cpu_instruction_cost();
    client.accept_issuer_transfer(&old_issuer, &new_issuer, &namespace, &token);
    let after = env.budget().cpu_instruction_cost();

    // Sanity check: the transfer actually completed and cleared the record.
    assert!(client.get_offering(&new_issuer, &namespace, &token).is_some());
    assert_eq!(client.get_pending_issuer_transfer(&old_issuer, &namespace, &token), None);

    (before, after)
}

#[test]
fn accept_cpu_cost_is_independent_of_registered_issuer_count() {
    let (small_before, small_after) = accept_cpu_cost(LARGE_ISSUER_COUNT, 1);
    let (large_before, large_after) = accept_cpu_cost(LARGE_ISSUER_COUNT, LARGE_ISSUER_COUNT);

    let small_cost = small_after - small_before;
    let large_cost = large_after - large_before;

    // Direct lookup touches the same keys regardless of registry size. Allow a
    // small tolerance for key serialisation/bookkeeping, but stay far below the
    // cost of scanning even one namespace per issuer.
    assert!(
        large_cost <= small_cost + 5_000,
        "accept cost grew with issuer count: scan=1 = {small_cost} CPU, \
         scan={LARGE_ISSUER_COUNT} = {large_cost} CPU"
    );
}

#[test]
fn accept_with_wrong_old_issuer_returns_no_transfer_pending() {
    let (env, client, old_issuer, new_issuer, namespace, token) =
        pending_transfer_fixture(LARGE_ISSUER_COUNT, LARGE_ISSUER_COUNT);

    let wrong_old_issuer = Address::generate(&env);
    let result =
        client.try_accept_issuer_transfer(&wrong_old_issuer, &new_issuer, &namespace, &token);

    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));

    // The real pending transfer must be untouched.
    assert_eq!(
        client.get_pending_issuer_transfer(&old_issuer, &namespace, &token),
        Some(new_issuer)
    );
}
