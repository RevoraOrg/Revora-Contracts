/// # Self-Test Entrypoint Integration Tests
///
/// Validates that the `self_test()` contract entrypoint is correctly wired through
/// the Soroban ABI and returns expected status codes.
///
/// ## Test coverage
/// - Happy path: fresh contract deployment returns `0` (pass)
/// - The entrypoint is callable without any preconditions (no auth, no storage)
use crate::{DataKey, FreezeReason, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Helper: deploy a fresh contract and return a client.
fn setup() -> (Env, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    (env, client)
}

/// Verify that `self_test()` returns 0 (pass) on a freshly deployed contract.
#[test]
fn test_self_test_returns_pass() {
    let (_env, client) = setup();
    assert_eq!(client.self_test(), 0);
}

/// Verify that `self_test()` is callable without any prior initialization
/// (no admin, no offerings registered).
#[test]
fn test_self_test_no_prerequisites() {
    let env = Env::default();
    // Do NOT call mock_all_auths or initialize — self_test should not require them.
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    assert_eq!(client.self_test(), 0);
}

/// Verify that `self_test()` passes even when the contract is frozen.
/// Self-test is a read-only diagnostic that does not check the freeze flag.
#[test]
fn test_self_test_works_when_frozen() {
    let (env, client) = setup();
    // Initialize the contract with an admin so we can call set_freeze.
    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &Some(false));
    // Freeze the contract through the proper entrypoint
    client.set_freeze(&FreezeReason::Compliance);
    // self_test does NOT call require_not_frozen, so it should pass even when frozen.
    assert_eq!(client.self_test(), 0);
}
