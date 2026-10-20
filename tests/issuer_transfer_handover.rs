//! Regression tests for the issuer-transfer state-handover bug:
//! after `accept_issuer_transfer`, the new issuer must gain full control of
//! the offering and the old issuer must lose it — while the offering's
//! canonical identity (`OfferingId`) and all per-offering state stay under
//! the original registrant's namespace (see `docs/ISSUER_TRANSFER.md`).
//!
//! The root cause on `master` was that `accept_issuer_transfer` moved the
//! `OfferingIssuer` alias to the new tenant key but never migrated the
//! `OfferingRecord`, so the new issuer's `get_offering` resolved to nothing
//! (`OfferingNotFound`) while the old issuer's record still said
//! `issuers.primary == old`. These tests pin the fixed behaviour and are
//! expected to fail on `master`.

use revora_contracts::{DataKey2, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{symbol_short, token, Address, Env, Symbol, Vec};

fn ns() -> Symbol {
    symbol_short!("ns")
}

/// Seed the global issuer/namespace registries for `issuer`.
///
/// `register_offering` does **not** populate `DataKey2::IssuerCount` /
/// `IssuerItem` / `NamespaceCount` / `NamespaceItem` (only
/// `ensure_issuer_registered` does, which runs on the accept path), but
/// `find_pending_transfer_for_new_issuer` scans them. Same workaround as
/// `src/test_pending_issuer_transfer.rs`.
fn seed_issuer_registry(env: &Env, contract_id: &Address, issuer: &Address) {
    env.as_contract(contract_id, || {
        let count: u32 = env.storage().persistent().get(&DataKey2::IssuerCount).unwrap_or(0);
        env.storage().persistent().set(&DataKey2::IssuerCount, &(count + 1));
        env.storage().persistent().set(&DataKey2::IssuerItem(count), issuer);
        env.storage().persistent().set(&DataKey2::IssuerRegistered(issuer.clone()), &true);

        env.storage().persistent().set(&DataKey2::NamespaceCount(issuer.clone()), &1u32);
        env.storage().persistent().set(&DataKey2::NamespaceItem(issuer.clone(), 0u32), &ns());
        env.storage().persistent().set(&DataKey2::NamespaceRegistered(issuer.clone(), ns()), &true);
    });
}

struct Fixture {
    env: Env,
    client: RevoraRevenueShareClient<'static>,
    contract_id: Address,
    issuer: Address,
    offering_token: Address,
    payment_token: Address,
}

/// Single-issuer offering backed by a real Stellar asset contract so the
/// token transfer inside `deposit_revenue` / `claim` can succeed.
fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payment_token = env.register_stellar_asset_contract_v2(issuer.clone()).address();

    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns(),
        &offering_token,
        &5_000u32,
        &payment_token,
        &0i128,
        &symbol_short!(""),
        // Must match the SAC's `decimals()` (7), otherwise `DecimalsMismatch`.
        &7u32,
    );
    token::StellarAssetClient::new(&env, &payment_token).mint(&issuer, &1_000_000);

    seed_issuer_registry(&env, &contract_id, &issuer);

    Fixture { env, client, contract_id, issuer, offering_token, payment_token }
}

fn propose_and_accept(f: &Fixture, old: &Address, new: &Address) {
    f.client.propose_issuer_transfer(old, &ns(), &f.offering_token, new);
    f.client.accept_issuer_transfer(new, &ns(), &f.offering_token);
}

/// Give the (post-transfer) issuer a token balance so `deposit_revenue` can
/// pull the payment from it.
fn mint(f: &Fixture, to: &Address, amount: i128) {
    token::StellarAssetClient::new(&f.env, &f.payment_token).mint(to, &amount);
}

#[test]
fn new_issuer_gains_full_control_after_accept() {
    let f = setup();
    let new_issuer = Address::generate(&f.env);
    let holder = Address::generate(&f.env);

    propose_and_accept(&f, &f.issuer.clone(), &new_issuer);
    mint(&f, &new_issuer, 1_000_000);

    // The offering record is resolvable under the new issuer and names it primary.
    let offering = f
        .client
        .get_offering(&new_issuer, &ns(), &f.offering_token)
        .expect("new issuer must be able to resolve the offering");
    assert_eq!(offering.issuers.primary, new_issuer);

    // Full management surface works for the new issuer.
    f.client.deposit_revenue(
        &new_issuer,
        &ns(),
        &f.offering_token,
        &f.payment_token,
        &1_000i128,
        &1u64,
    );
    f.client.set_holder_share(&new_issuer, &ns(), &f.offering_token, &holder, &1_000u32, &1u64);
    f.client.set_offering_fee_bps(&new_issuer, &ns(), &f.offering_token, &f.payment_token, &100u32);
    f.client.set_claim_delay(&new_issuer, &ns(), &f.offering_token, &3_600u64);
    assert_eq!(f.client.get_claim_delay(&new_issuer, &ns(), &f.offering_token), 3_600);
}

#[test]
fn old_issuer_is_rejected_after_accept() {
    let f = setup();
    let new_issuer = Address::generate(&f.env);
    let holder = Address::generate(&f.env);

    // Seed state under the old issuer, then hand over.
    f.client.set_holder_share(
        &f.issuer.clone(),
        &ns(),
        &f.offering_token,
        &holder,
        &1_000u32,
        &1u64,
    );
    propose_and_accept(&f, &f.issuer.clone(), &new_issuer);

    // No stale record: the old tenant key resolves to the handover record
    // naming the new issuer as primary (not the old one).
    let old_view = f
        .client
        .get_offering(&f.issuer.clone(), &ns(), &f.offering_token)
        .expect("old key must resolve to the handover record");
    assert_ne!(old_view.issuers.primary, f.issuer);
    assert_eq!(old_view.issuers.primary, new_issuer);

    // Every issuer-authorised entry point now rejects the old issuer.
    assert_eq!(
        f.client
            .try_deposit_revenue(
                &f.issuer.clone(),
                &ns(),
                &f.offering_token,
                &f.payment_token,
                &1_000i128,
                &2u64
            )
            .err(),
        Some(Ok(RevoraError::OfferingNotFound))
    );
    assert_eq!(
        f.client
            .try_set_holder_share(
                &f.issuer.clone(),
                &ns(),
                &f.offering_token,
                &holder,
                &2_000u32,
                &0u64
            )
            .err(),
        Some(Ok(RevoraError::OfferingNotFound))
    );
    assert_eq!(
        f.client
            .try_set_offering_fee_bps(
                &f.issuer.clone(),
                &ns(),
                &f.offering_token,
                &f.payment_token,
                &100u32
            )
            .err(),
        Some(Ok(RevoraError::OfferingNotFound))
    );
    assert_eq!(
        f.client
            .try_report_revenue(
                &f.issuer.clone(),
                &ns(),
                &f.offering_token,
                &f.payment_token,
                &1_000i128,
                &1u64,
                &false
            )
            .err(),
        Some(Ok(RevoraError::NotAuthorized))
    );
    // And the old issuer can no longer start another transfer.
    let next = Address::generate(&f.env);
    assert_eq!(
        f.client
            .try_propose_issuer_transfer(&f.issuer.clone(), &ns(), &f.offering_token, &next)
            .err(),
        Some(Ok(RevoraError::OfferingNotFound))
    );

    // The pending-transfer record is consumed by the accept. (Note: before an
    // accept, the new issuer's raw id has no canonical alias yet, so pending
    // state is only observable via the offering's canonical identity.)
    assert_eq!(
        f.client.get_pending_issuer_transfer(&f.issuer.clone(), &ns(), &f.offering_token),
        None
    );
}

#[test]
fn holder_can_claim_after_transfer() {
    let f = setup();
    let new_issuer = Address::generate(&f.env);
    let holder = Address::generate(&f.env);

    // Pre-transfer: 100% share to one holder, then a 100_000 deposit.
    f.client.set_holder_share(
        &f.issuer.clone(),
        &ns(),
        &f.offering_token,
        &holder,
        &10_000u32,
        &1u64,
    );
    f.client.deposit_revenue(
        &f.issuer.clone(),
        &ns(),
        &f.offering_token,
        &f.payment_token,
        &100_000i128,
        &1u64,
    );

    propose_and_accept(&f, &f.issuer.clone(), &new_issuer);

    // The holder claims through the *new* issuer's identity (the alias resolves
    // back to the canonical offering) and is paid the full period amount.
    let payout = f.client.claim(&holder, &new_issuer, &ns(), &f.offering_token, &10u32);
    assert_eq!(payout, 100_000);

    // Claiming through the *old* identity also resolves the canonical offering,
    // but there is nothing left to claim (already claimed for period 1).
    assert_eq!(
        f.client.try_claim(&holder, &f.issuer.clone(), &ns(), &f.offering_token, &10u32).err(),
        Some(Ok(RevoraError::NoPendingClaims))
    );
}

#[test]
fn chained_transfer_a_to_b_to_c_resolves_canonical_id() {
    let f = setup();
    let issuer_b = Address::generate(&f.env);
    let issuer_c = Address::generate(&f.env);

    propose_and_accept(&f, &f.issuer.clone(), &issuer_b);
    // Second hop: proposed by B, accepted by C. Pending is stored under the
    // canonical (original) offering id and must be found via B's tenant entry.
    propose_and_accept(&f, &issuer_b, &issuer_c);
    mint(&f, &issuer_c, 1_000_000);

    let view = f
        .client
        .get_offering(&issuer_c, &ns(), &f.offering_token)
        .expect("final issuer must resolve the offering");
    assert_eq!(view.issuers.primary, issuer_c);

    // Every historical key resolves to the same record naming C as primary.
    for key in [&f.issuer.clone(), &issuer_b, &issuer_c] {
        let offering = f
            .client
            .get_offering(key, &ns(), &f.offering_token)
            .expect("all historical identities must resolve");
        assert_eq!(offering.issuers.primary, issuer_c);
    }

    // Only the final issuer can operate the offering.
    f.client.deposit_revenue(
        &issuer_c,
        &ns(),
        &f.offering_token,
        &f.payment_token,
        &500i128,
        &1u64,
    );
    assert_eq!(
        f.client
            .try_deposit_revenue(
                &issuer_b,
                &ns(),
                &f.offering_token,
                &f.payment_token,
                &500i128,
                &2u64
            )
            .err(),
        Some(Ok(RevoraError::OfferingNotFound))
    );
    assert_eq!(
        f.client
            .try_deposit_revenue(
                &f.issuer.clone(),
                &ns(),
                &f.offering_token,
                &f.payment_token,
                &500i128,
                &3u64
            )
            .err(),
        Some(Ok(RevoraError::OfferingNotFound))
    );
}

#[test]
fn config_follows_the_offering_after_transfer() {
    let f = setup();
    let new_issuer = Address::generate(&f.env);

    // Config written under the old issuer is visible to (and updatable by) the new one.
    f.client.set_claim_delay(&f.issuer.clone(), &ns(), &f.offering_token, &3_600u64);
    f.client.set_concentration_limit(
        &f.issuer.clone(),
        &ns(),
        &f.offering_token,
        &5_000u32,
        &true,
        &0u64,
    );

    propose_and_accept(&f, &f.issuer.clone(), &new_issuer);

    assert_eq!(f.client.get_claim_delay(&new_issuer, &ns(), &f.offering_token), 3_600);
    let limit = f
        .client
        .get_concentration_limit(&new_issuer, &ns(), &f.offering_token)
        .expect("concentration config must survive the transfer");
    assert_eq!(limit.max_bps, 5_000);
    assert!(limit.enforce);

    // Updating under the new issuer round-trips through the old identity too.
    f.client.set_claim_delay(&new_issuer, &ns(), &f.offering_token, &7_200u64);
    assert_eq!(f.client.get_claim_delay(&f.issuer.clone(), &ns(), &f.offering_token), 7_200);
}

#[test]
fn co_issuers_and_quorum_survive_the_transfer() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let co_issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payment_token = env.register_stellar_asset_contract_v2(issuer.clone()).address();

    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::from_array(&env, [co_issuer.clone()]),
        &2u32,
        &ns(),
        &offering_token,
        &5_000u32,
        &payment_token,
        &0i128,
        &symbol_short!(""),
        &7u32,
    );
    seed_issuer_registry(&env, &contract_id, &issuer);

    let new_issuer = Address::generate(&env);
    client.propose_issuer_transfer(&issuer, &ns(), &offering_token, &new_issuer);
    client.accept_issuer_transfer(&new_issuer, &ns(), &offering_token);

    // Primary replaced, co-issuers preserved (minus the new issuer if present),
    // quorum clamped to the surviving issuer count.
    let offering = client
        .get_offering(&new_issuer, &ns(), &offering_token)
        .expect("new issuer must resolve the offering");
    assert_eq!(offering.issuers.primary, new_issuer);
    assert_eq!(offering.issuers.co.len(), 1);
    assert_eq!(offering.issuers.co.get(0).unwrap(), co_issuer);
    assert_eq!(offering.issuers.quorum, 2);

    // The new issuer operates the offering under the surviving quorum.
    client.set_offering_fee_bps(&new_issuer, &ns(), &offering_token, &payment_token, &100u32);
}

#[test]
fn offering_count_stays_append_only_across_transfer() {
    let f = setup();
    let new_issuer = Address::generate(&f.env);

    propose_and_accept(&f, &f.issuer.clone(), &new_issuer);

    // The handover adds a record under the new tenant but must not remove the
    // old tenant's (now alias) record — mirrors the compiled assertion in
    // `test_offering_count_adversarial.rs`.
    assert_eq!(f.client.get_offering_count(&f.issuer.clone(), &ns()), 1);
    assert_eq!(f.client.get_offering_count(&new_issuer, &ns()), 1);
}

#[test]
fn duplicate_pending_blocks_a_second_proposal() {
    let f = setup();
    let new_issuer = Address::generate(&f.env);
    let other = Address::generate(&f.env);

    f.client.propose_issuer_transfer(&f.issuer.clone(), &ns(), &f.offering_token, &new_issuer);
    assert_eq!(
        f.client
            .try_propose_issuer_transfer(&f.issuer.clone(), &ns(), &f.offering_token, &other)
            .err(),
        Some(Ok(RevoraError::IssuerTransferPending))
    );
    assert_eq!(
        f.client.get_pending_issuer_transfer(&f.issuer.clone(), &ns(), &f.offering_token),
        Some(new_issuer.clone())
    );
}

#[test]
fn accept_by_a_non_pending_address_finds_nothing() {
    let f = setup();
    let new_issuer = Address::generate(&f.env);
    let stranger = Address::generate(&f.env);

    f.client.propose_issuer_transfer(&f.issuer.clone(), &ns(), &f.offering_token, &new_issuer);
    assert_eq!(
        f.client.try_accept_issuer_transfer(&stranger, &ns(), &f.offering_token).err(),
        Some(Ok(RevoraError::NoTransferPending))
    );
    // The real pending survives for the intended recipient (observable via the
    // canonical/old identity — the new issuer's alias is written only on accept).
    assert_eq!(
        f.client.get_pending_issuer_transfer(&f.issuer.clone(), &ns(), &f.offering_token),
        Some(new_issuer)
    );
}

#[test]
fn transfer_back_to_the_original_issuer_regains_control() {
    let f = setup();
    let new_issuer = Address::generate(&f.env);
    propose_and_accept(&f, &f.issuer.clone(), &new_issuer);

    // A→B→A is a real transfer back (the self-transfer early-return compares
    // against the record's primary, not the raw caller-supplied issuer).
    f.client.propose_issuer_transfer(&new_issuer, &ns(), &f.offering_token, &f.issuer.clone());
    f.client.accept_issuer_transfer(&f.issuer.clone(), &ns(), &f.offering_token);

    let offering = f
        .client
        .get_offering(&f.issuer.clone(), &ns(), &f.offering_token)
        .expect("original issuer must resolve the offering again");
    assert_eq!(offering.issuers.primary, f.issuer);
    // The intermediate holder of control is locked out.
    assert_eq!(
        f.client
            .try_deposit_revenue(
                &new_issuer,
                &ns(),
                &f.offering_token,
                &f.payment_token,
                &100i128,
                &1u64
            )
            .err(),
        Some(Ok(RevoraError::OfferingNotFound))
    );
}
