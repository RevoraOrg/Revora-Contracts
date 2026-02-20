#![cfg(test)]
use soroban_sdk::{
    testutils::{Address as _, Events, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, InvokeError, Symbol, TryFromVal, Val,
};

use crate::{
    DataKey, OfferingKey, OfferingPeriods, RevoraRevenueShare, RevoraRevenueShareClient,
};

// Auth matrix:
// - register_offering(env, issuer, token, revenue_share_bps):
//   requires issuer.require_auth(), emits offer_reg event
// - report_revenue(env, issuer, token, amount, period_id):
//   requires issuer.require_auth(), updates periods, emits rev_rep
// - close_period(env, issuer, token, period_id):
//   requires issuer.require_auth(), updates closed_through, emits per_close
// - latest_accepted_period(env, issuer, token) -> Option<u64>:
//   read-only, no auth
// - closed_through_period(env, issuer, token) -> Option<u64>:
//   read-only, no auth

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    (env, contract_id, issuer, token)
}

fn setup_without_auth_mocking() -> (Env, Address, Address, Address) {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    (env, contract_id, issuer, token)
}

#[test]
fn it_emits_events_on_register_and_report() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.register_offering(&issuer, &token, &1_000);
    client.report_revenue(&issuer, &token, &1_000_000, &1);

    assert!(env.events().all().len() >= 2);
}

#[test]
fn it_tracks_latest_period_and_allows_equal_period_reports() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.register_offering(&issuer, &token, &1_000);
    client.report_revenue(&issuer, &token, &1_000_000, &1);
    client.report_revenue(&issuer, &token, &2_000_000, &2);
    client.report_revenue(&issuer, &token, &3_000_000, &2);

    let latest = client.latest_accepted_period(&issuer, &token);
    assert_eq!(latest, Some(2));

    let events = env.events().all();
    assert!(events.len() >= 4);
}

#[test]
#[should_panic(expected = "backdated_period")]
fn it_rejects_backdated_periods() {
    let (_env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&_env, &contract_id);

    client.report_revenue(&issuer, &token, &1_000_000, &5);
    client.report_revenue(&issuer, &token, &1_000_000, &3);
}

#[test]
#[should_panic(expected = "period_closed")]
fn it_rejects_reports_for_closed_periods() {
    let (_env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&_env, &contract_id);

    client.report_revenue(&issuer, &token, &1_000_000, &5);
    client.close_period(&issuer, &token, &5);

    client.report_revenue(&issuer, &token, &2_000_000, &5);
}

#[test]
fn it_emits_period_closed_events_and_updates_state() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.report_revenue(&issuer, &token, &1_000_000, &1);
    client.report_revenue(&issuer, &token, &2_000_000, &2);
    client.close_period(&issuer, &token, &2);

    let latest = client.latest_accepted_period(&issuer, &token);
    let closed = client.closed_through_period(&issuer, &token);

    assert_eq!(latest, Some(2));
    assert_eq!(closed, Some(2));

    let events = env.events().all();
    assert!(!events.is_empty());
    let last = events.last().unwrap();
    let expected_data: Val = 2u64.into_val(&env);
    let last_period: u64 = u64::try_from_val(&env, &last.2).unwrap();
    let expected_period: u64 = u64::try_from_val(&env, &expected_data).unwrap();
    assert_eq!(last_period, expected_period);
}

#[test]
fn it_handles_large_period_ids_and_boundary_transitions() {
    let (_env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&_env, &contract_id);

    let large = u64::MAX - 1;

    client.report_revenue(&issuer, &token, &1_000_000, &large);
    client.report_revenue(&issuer, &token, &2_000_000, &u64::MAX);

    let latest = client.latest_accepted_period(&issuer, &token);
    assert_eq!(latest, Some(u64::MAX));
}

#[test]
fn register_offering_fails_without_auth_and_emits_no_events() {
    let (env, contract_id, issuer, token) = setup_without_auth_mocking();

    let args = (&issuer, &token, 1_000_u32).into_val(&env);
    let res = env.try_invoke_contract::<Val, InvokeError>(
        &contract_id,
        &Symbol::new(&env, "register_offering"),
        args,
    );

    assert!(res.is_err());
    assert!(env.events().all().is_empty());
}

#[test]
fn register_offering_succeeds_with_mock_auths() {
    let (env, contract_id, issuer, token) = setup_without_auth_mocking();

    env.mock_auths(&[MockAuth {
        address: &issuer,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "register_offering",
            args: (&issuer, &token, 1_000_u32).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let args = (&issuer, &token, 1_000_u32).into_val(&env);
    let res = env.try_invoke_contract::<Val, InvokeError>(
        &contract_id,
        &Symbol::new(&env, "register_offering"),
        args,
    );

    assert!(res.is_ok());
    assert!(!env.events().all().is_empty());
}

#[test]
fn report_revenue_fails_without_auth_and_does_not_write_periods() {
    let (env, contract_id, issuer, token) = setup_without_auth_mocking();

    let key = DataKey::Offering(OfferingKey {
        issuer: issuer.clone(),
        token: token.clone(),
    });

    let before = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get::<DataKey, OfferingPeriods>(&key)
    });
    assert!(before.is_none());

    let args = (&issuer, &token, 1_000_000_i128, 1_u64).into_val(&env);
    let res = env.try_invoke_contract::<Val, InvokeError>(
        &contract_id,
        &Symbol::new(&env, "report_revenue"),
        args,
    );
    assert!(res.is_err());

    let after = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get::<DataKey, OfferingPeriods>(&key)
    });
    assert!(after.is_none());
}

#[test]
fn report_revenue_succeeds_with_mock_auths_and_updates_periods() {
    let (env, contract_id, issuer, token) = setup_without_auth_mocking();

    env.mock_auths(&[MockAuth {
        address: &issuer,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "report_revenue",
            args: (&issuer, &token, 1_000_000_i128, 1_u64).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let args = (&issuer, &token, 1_000_000_i128, 1_u64).into_val(&env);
    let res = env.try_invoke_contract::<Val, InvokeError>(
        &contract_id,
        &Symbol::new(&env, "report_revenue"),
        args,
    );
    assert!(res.is_ok());

    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let latest = client.latest_accepted_period(&issuer, &token);
    assert_eq!(latest, Some(1));
}

#[test]
fn close_period_fails_without_auth_and_preserves_state() {
    let (env, contract_id, issuer, token) = setup_without_auth_mocking();

    let key = DataKey::Offering(OfferingKey {
        issuer: issuer.clone(),
        token: token.clone(),
    });

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(
            &key,
            &OfferingPeriods {
                latest_accepted: Some(5),
                closed_through: None,
            },
        );
    });

    let args = (&issuer, &token, 5_u64).into_val(&env);
    let res = env.try_invoke_contract::<Val, InvokeError>(
        &contract_id,
        &Symbol::new(&env, "close_period"),
        args,
    );
    assert!(res.is_err());

    let periods = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get::<DataKey, OfferingPeriods>(&key)
            .unwrap()
    });
    assert_eq!(periods.closed_through, None);
}

#[test]
fn attacker_cannot_close_other_offering_even_with_its_own_auth() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);

    let attacker = Address::generate(&env);
    let victim = Address::generate(&env);
    let token = Address::generate(&env);

    let key = DataKey::Offering(OfferingKey {
        issuer: victim.clone(),
        token: token.clone(),
    });

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(
            &key,
            &OfferingPeriods {
                latest_accepted: Some(5),
                closed_through: None,
            },
        );
    });

    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "close_period",
            args: (&victim, &token, 5_u64).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let args = (&victim, &token, 5_u64).into_val(&env);
    let res = env.try_invoke_contract::<Val, InvokeError>(
        &contract_id,
        &Symbol::new(&env, "close_period"),
        args,
    );
    assert!(res.is_err());

    let periods = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get::<DataKey, OfferingPeriods>(&key)
            .unwrap()
    });
    assert_eq!(periods.closed_through, None);
}

#[test]
fn getters_do_not_require_auth() {
    let (env, contract_id, issuer, token) = setup_without_auth_mocking();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let latest = client.latest_accepted_period(&issuer, &token);
    let closed = client.closed_through_period(&issuer, &token);

    assert_eq!(latest, None);
    assert_eq!(closed, None);
}

