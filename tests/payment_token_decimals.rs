use revora_contracts::{OfferingId, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    testutils::{Address as _, EnvTestConfig, Events as _},
    token, Address, Env, IntoVal, TryFromVal, Vec,
};

fn test_env() -> Env {
    Env::new_with_config(EnvTestConfig { capture_snapshot_at_drop: false })
}

// These serialized keys model legacy states where only one funding marker exists.
#[contracttype]
#[derive(Clone)]
enum FundingKey {
    PaymentToken(OfferingId),
    PeriodCount(OfferingId),
}

#[contract]
struct MetadataToken;

#[contractimpl]
impl MetadataToken {
    pub fn decimals(env: Env) -> u32 {
        env.storage().instance().get(&symbol_short!("precision")).unwrap()
    }
}

fn setup(env: &Env) -> (Address, Address, Address, Address) {
    env.mock_all_auths();
    let contract = env.register_contract(None, RevoraRevenueShare);
    let issuer = Address::generate(env);
    let offering_token = Address::generate(env);
    let payment_token = env.register_stellar_asset_contract_v2(Address::generate(env)).address();
    let client = RevoraRevenueShareClient::new(env, &contract);
    client.register_offering(
        &issuer,
        &Vec::new(env),
        &1,
        &symbol_short!("decimals"),
        &offering_token,
        &10_000,
        &payment_token,
        &0,
        &symbol_short!("XLM"),
        &7,
    );
    token::StellarAssetClient::new(env, &payment_token).mint(&issuer, &1_000_000);
    (contract, issuer, offering_token, payment_token)
}

#[test]
fn funded_decimals_cannot_be_changed() {
    let env = test_env();
    let (contract, issuer, offering_token, payment_token) = setup(&env);
    let client = RevoraRevenueShareClient::new(&env, &contract);
    let ns = symbol_short!("decimals");
    client.deposit_revenue(&issuer, &ns, &offering_token, &payment_token, &100_000, &1);
    for decimals in [6, 7] {
        assert_eq!(
            client.try_set_payment_token_decimals(&issuer, &ns, &offering_token, &decimals),
            Err(Ok(RevoraError::DecimalsLocked))
        );
    }
    assert_eq!(client.get_period_count(&issuer, &ns, &offering_token), 1);
    assert_eq!(token::Client::new(&env, &payment_token).balance(&contract), 100_000);
    assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &offering_token), 7);
}

#[test]
fn configured_decimals_must_match_token_metadata() {
    let env = test_env();
    let (contract, issuer, offering_token, _) = setup(&env);
    let client = RevoraRevenueShareClient::new(&env, &contract);
    assert_eq!(
        client.try_set_payment_token_decimals(
            &issuer,
            &symbol_short!("decimals"),
            &offering_token,
            &6
        ),
        Err(Ok(RevoraError::DecimalsMismatch)),
    );
}

#[test]
fn each_funding_marker_independently_locks_precision() {
    for token_lock in [false, true] {
        let env = test_env();
        let (contract, issuer, offering_token, payment_token) = setup(&env);
        let ns = symbol_short!("decimals");
        let id = OfferingId {
            issuer: issuer.clone(),
            namespace: ns.clone(),
            token: offering_token.clone(),
        };
        env.as_contract(&contract, || {
            if token_lock {
                env.storage().persistent().set(&FundingKey::PaymentToken(id), &payment_token);
            } else {
                env.storage().persistent().set(&FundingKey::PeriodCount(id), &1u32);
            }
        });
        let client = RevoraRevenueShareClient::new(&env, &contract);
        assert_eq!(
            client.try_set_payment_token_decimals(&issuer, &ns, &offering_token, &7),
            Err(Ok(RevoraError::DecimalsLocked))
        );
        assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &offering_token), 7);
    }
}

fn setup_custom(env: &Env, payout: &Address, display_decimals: u32) -> (Address, Address, Address) {
    env.mock_all_auths();
    let contract = env.register_contract(None, RevoraRevenueShare);
    let issuer = Address::generate(env);
    let offering_token = Address::generate(env);
    RevoraRevenueShareClient::new(env, &contract).register_offering(
        &issuer,
        &Vec::new(env),
        &1,
        &symbol_short!("decimals"),
        &offering_token,
        &10_000,
        payout,
        &0,
        &symbol_short!("USD"),
        &display_decimals,
    );
    (contract, issuer, offering_token)
}

#[test]
fn unfunded_updates_emit_previous_and_new_precision() {
    let env = test_env();
    let payout = env.register_contract(None, MetadataToken);
    env.as_contract(&payout, || env.storage().instance().set(&symbol_short!("precision"), &6u32));
    let (contract, issuer, offering_token) = setup_custom(&env, &payout, 6);
    let client = RevoraRevenueShareClient::new(&env, &contract);
    let ns = symbol_short!("decimals");
    for (old, new) in [(7u32, 6u32), (6, 8)] {
        env.as_contract(&payout, || {
            env.storage().instance().set(&symbol_short!("precision"), &new)
        });
        client.set_payment_token_decimals(&issuer, &ns, &offering_token, &new);
        assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &offering_token), new);
        let (event_contract, topics, data) = env.events().all().last().unwrap();
        assert_eq!(event_contract, contract);
        assert_eq!(
            topics,
            (symbol_short!("pt_dec"), issuer.clone(), ns.clone(), offering_token.clone())
                .into_val(&env)
        );
        assert_eq!(<(u32, u32)>::try_from_val(&env, &data).unwrap(), (old, new));
    }
}

#[test]
fn deposit_checks_actual_token_before_transferring_or_locking() {
    let env = test_env();
    let payout = Address::generate(&env); // No metadata; actual deposit token is checked later.
    let (contract, issuer, offering_token) = setup_custom(&env, &payout, 6);
    let client = RevoraRevenueShareClient::new(&env, &contract);
    let ns = symbol_short!("decimals");
    client.set_payment_token_decimals(&issuer, &ns, &offering_token, &6);
    let payment = env.register_stellar_asset_contract_v2(Address::generate(&env)).address();
    token::StellarAssetClient::new(&env, &payment).mint(&issuer, &100_000);
    assert_eq!(
        client.try_deposit_revenue(&issuer, &ns, &offering_token, &payment, &100_000, &1),
        Err(Ok(RevoraError::DecimalsMismatch))
    );
    assert_eq!(token::Client::new(&env, &payment).balance(&issuer), 100_000);
    assert_eq!(token::Client::new(&env, &payment).balance(&contract), 0);
    assert_eq!(client.get_period_count(&issuer, &ns, &offering_token), 0);
    client.set_payment_token_decimals(&issuer, &ns, &offering_token, &7);
    client.deposit_revenue(&issuer, &ns, &offering_token, &payment, &100_000, &1);
    assert_eq!(
        client.try_set_payment_token_decimals(&issuer, &ns, &offering_token, &6),
        Err(Ok(RevoraError::DecimalsLocked))
    );
}

#[test]
fn missing_metadata_preserves_boundaries_and_range_validation() {
    let env = test_env();
    let (contract, issuer, offering_token) = setup_custom(&env, &Address::generate(&env), 7);
    let client = RevoraRevenueShareClient::new(&env, &contract);
    let ns = symbol_short!("decimals");
    assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &offering_token), 7);
    for decimals in [0, 18] {
        client.set_payment_token_decimals(&issuer, &ns, &offering_token, &decimals);
        assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &offering_token), decimals);
    }
    assert_eq!(
        client.try_set_payment_token_decimals(&issuer, &ns, &offering_token, &19),
        Err(Ok(RevoraError::LimitReached))
    );
    assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &offering_token), 18);
}

#[test]
fn unfunded_setter_still_requires_issuer_authorization() {
    let env = test_env();
    let (contract, issuer, offering_token, _) = setup(&env);
    env.set_auths(&[]);
    let client = RevoraRevenueShareClient::new(&env, &contract);
    assert!(client
        .try_set_payment_token_decimals(&issuer, &symbol_short!("decimals"), &offering_token, &7)
        .is_err());
}
