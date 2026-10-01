//! Adversarial coverage for [`RevoraRevenueShare::set_offering_platform_fee`]
//! and its read-side counterpart
//! [`RevoraRevenueShare::get_offering_platform_fee`].
//!
//! `set_offering_platform_fee` is admin-gated and has to keep the invariant
//! `fee_bps + aggregate_holder_share_bps <= 10_000` for the offering.  These
//! tests pin that contract:
//!
//! * the setter is rejected on an uninitialised contract (`NotInitialized`) and
//!   for unknown offerings (`OfferingNotFound`);
//! * a value that consumes the whole remaining headroom (`10_000` with no
//!   holders) is accepted, and one basis point more is rejected with
//!   `FeeExceedsHolderShare`;
//! * the headroom shrinks as holder shares are allocated and the rejection
//!   boundary moves accordingly;
//! * a rejected call leaves any previously stored model untouched;
//! * a frozen contract rejects the write (`ContractFrozen`);
//! * the model is overwritten in place and isolated per offering.

#![cfg(test)]

use crate::{PlatformFeeModel, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

fn register_offering(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
) {
    let payout_asset = Address::generate(env);
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        &symbol_short!("def"),
        token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
}

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &token);
    (env, client, admin, issuer, token)
}

#[test]
fn rejects_uninitialized_contract_without_storing_a_model() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let treasury = Address::generate(&env);
    let ns = symbol_short!("def");

    let result = client.try_set_offering_platform_fee(&issuer, &ns, &token, &100, &treasury);
    assert!(matches!(result.err(), Some(Ok(RevoraError::NotInitialized))));
    assert_eq!(client.get_offering_platform_fee(&issuer, &ns, &token), None);
}

#[test]
fn rejects_unknown_offering_without_storing_a_model() {
    let (env, client, _admin, issuer, _token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);
    let treasury = Address::generate(&env);

    let result =
        client.try_set_offering_platform_fee(&issuer, &ns, &unknown_token, &100, &treasury);
    assert!(matches!(result.err(), Some(Ok(RevoraError::OfferingNotFound))));
    assert_eq!(client.get_offering_platform_fee(&issuer, &ns, &unknown_token), None);
}

#[test]
fn stores_model_and_reads_it_back_including_zero_fee() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let treasury = Address::generate(&env);

    client.set_offering_platform_fee(&issuer, &ns, &token, &1_250, &treasury);
    assert_eq!(
        client.get_offering_platform_fee(&issuer, &ns, &token),
        Some(PlatformFeeModel { fee_bps: 1_250, treasury: treasury.clone() })
    );

    // A zero fee is still a stored model (the fee is simply disabled), not an absence.
    let other_treasury = Address::generate(&env);
    client.set_offering_platform_fee(&issuer, &ns, &token, &0, &other_treasury);
    assert_eq!(
        client.get_offering_platform_fee(&issuer, &ns, &token),
        Some(PlatformFeeModel { fee_bps: 0, treasury: other_treasury })
    );
}

#[test]
fn accepts_full_headroom_boundary_and_rejects_one_basis_point_more() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let treasury = Address::generate(&env);

    // No holder shares are allocated, so the whole 10_000 bps is available to the fee.
    client.set_offering_platform_fee(&issuer, &ns, &token, &10_000, &treasury);
    assert_eq!(
        client.get_offering_platform_fee(&issuer, &ns, &token),
        Some(PlatformFeeModel { fee_bps: 10_000, treasury: treasury.clone() })
    );

    let result = client.try_set_offering_platform_fee(&issuer, &ns, &token, &10_001, &treasury);
    assert!(matches!(result.err(), Some(Ok(RevoraError::FeeExceedsHolderShare))));

    // The rejected write must not have mutated the stored model.
    assert_eq!(
        client.get_offering_platform_fee(&issuer, &ns, &token),
        Some(PlatformFeeModel { fee_bps: 10_000, treasury })
    );
}

#[test]
fn rejection_boundary_tracks_allocated_holder_shares() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let treasury = Address::generate(&env);
    let holder = Address::generate(&env);

    // Allocate 7_000 bps of the offering to a holder, leaving 3_000 bps of headroom.
    client.set_holder_share(&issuer, &ns, &token, &holder, &7_000, &1);

    client.set_offering_platform_fee(&issuer, &ns, &token, &3_000, &treasury);
    assert_eq!(
        client.get_offering_platform_fee(&issuer, &ns, &token),
        Some(PlatformFeeModel { fee_bps: 3_000, treasury: treasury.clone() })
    );

    let result = client.try_set_offering_platform_fee(&issuer, &ns, &token, &3_001, &treasury);
    assert!(matches!(result.err(), Some(Ok(RevoraError::FeeExceedsHolderShare))));

    // Unchanged after the rejected call.
    assert_eq!(
        client.get_offering_platform_fee(&issuer, &ns, &token),
        Some(PlatformFeeModel { fee_bps: 3_000, treasury })
    );
}

#[test]
fn rejects_freezes_contract_and_leaves_no_model() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let treasury = Address::generate(&env);
    let _ = env;

    client.freeze();

    let result = client.try_set_offering_platform_fee(&issuer, &ns, &token, &100, &treasury);
    assert!(matches!(result.err(), Some(Ok(RevoraError::ContractFrozen))));
    assert_eq!(client.get_offering_platform_fee(&issuer, &ns, &token), None);
}

#[test]
fn overwrites_existing_model_in_place() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    client.set_offering_platform_fee(&issuer, &ns, &token, &500, &first);
    client.set_offering_platform_fee(&issuer, &ns, &token, &750, &second);

    assert_eq!(
        client.get_offering_platform_fee(&issuer, &ns, &token),
        Some(PlatformFeeModel { fee_bps: 750, treasury: second })
    );
}

#[test]
fn scopes_model_per_offering() {
    let (env, client, _admin, issuer_a, token_a) = setup();
    let ns = symbol_short!("def");
    let treasury = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token_b = Address::generate(&env);
    register_offering(&env, &client, &issuer_b, &token_b);

    client.set_offering_platform_fee(&issuer_a, &ns, &token_a, &800, &treasury);

    assert_eq!(
        client.get_offering_platform_fee(&issuer_a, &ns, &token_a),
        Some(PlatformFeeModel { fee_bps: 800, treasury })
    );
    assert_eq!(client.get_offering_platform_fee(&issuer_b, &ns, &token_b), None);
}
