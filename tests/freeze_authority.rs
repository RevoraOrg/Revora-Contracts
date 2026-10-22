//! Regression coverage for admin-owned freezes (issue #1339).
use revora_contracts::{
    DataKey2, FreezeAuthority, FreezeDataKey, FreezeReason, OfferingId, RevoraError,
    RevoraRevenueShare, RevoraRevenueShareClient,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, EnvTestConfig, Events as _},
    Address, BytesN, Env, IntoVal, Symbol, Val, Vec,
};

struct Fixture {
    env: Env,
    contract: Address,
    admin: Address,
    issuer: Address,
    holder: Address,
    namespace: Symbol,
    token: Address,
}
impl Fixture {
    fn new() -> Self {
        // Assertions exercise live state/events; no diagnostic snapshot files are needed.
        let env = Env::new_with_config(EnvTestConfig { capture_snapshot_at_drop: false });
        env.mock_all_auths();
        let contract = env.register_contract(None, RevoraRevenueShare);
        let admin = Address::generate(&env);
        let issuer = Address::generate(&env);
        let holder = Address::generate(&env);
        let namespace = symbol_short!("freeze");
        let token = Address::generate(&env);
        let payout = env.register_stellar_asset_contract_v2(admin.clone()).address();
        let client = RevoraRevenueShareClient::new(&env, &contract);
        client.initialize(&admin, &None, &None);
        client.register_offering(
            &issuer,
            &Vec::new(&env),
            &1,
            &namespace,
            &token,
            &1000,
            &payout,
            &0,
            &symbol_short!("USD"),
            &7,
        );
        Self { env, contract, admin, issuer, holder, namespace, token }
    }
    fn client(&self) -> RevoraRevenueShareClient<'_> {
        RevoraRevenueShareClient::new(&self.env, &self.contract)
    }
    fn offering(&self) -> OfferingId {
        OfferingId {
            issuer: self.issuer.clone(),
            namespace: self.namespace.clone(),
            token: self.token.clone(),
        }
    }
    fn freeze(&self, caller: &Address, reason: FreezeReason) {
        self.client().emergency_freeze_holder(
            caller,
            &self.issuer,
            &self.namespace,
            &self.token,
            &self.holder,
            &reason,
        );
    }
    fn mask(&self) -> u32 {
        self.client().get_holder_freeze_reasons(
            &self.issuer,
            &self.namespace,
            &self.token,
            &self.holder,
        )
    }
}

#[test]
fn admin_offering_freeze_cannot_be_cleared_by_issuer() {
    let f = Fixture::new();
    let c = f.client();
    c.freeze_offering(&f.admin, &f.issuer, &f.namespace, &f.token);
    assert_eq!(
        c.try_unfreeze_offering(&f.issuer, &f.issuer, &f.namespace, &f.token),
        Err(Ok(RevoraError::NotAuthorized))
    );
    assert!(c.is_offering_frozen(&f.issuer, &f.namespace, &f.token));
    c.unfreeze_offering(&f.admin, &f.issuer, &f.namespace, &f.token);
    assert!(!c.is_offering_frozen(&f.issuer, &f.namespace, &f.token));
}

#[test]
fn admin_compliance_freeze_blocks_both_issuer_clear_entrypoints() {
    let f = Fixture::new();
    f.freeze(&f.admin, FreezeReason::Compliance);
    let c = f.client();
    assert_eq!(
        c.try_clear_freeze_reason(
            &f.issuer,
            &f.issuer,
            &f.namespace,
            &f.token,
            &f.holder,
            &FreezeReason::Compliance
        ),
        Err(Ok(RevoraError::NotAuthorized))
    );
    assert_eq!(
        c.try_emergency_unfreeze_holder(
            &f.issuer,
            &f.issuer,
            &f.namespace,
            &f.token,
            &f.holder,
            &FreezeReason::Compliance
        ),
        Err(Ok(RevoraError::NotAuthorized))
    );
    assert_eq!(f.mask(), FreezeReason::Compliance.to_bitmask());
    c.clear_freeze_reason(
        &f.admin,
        &f.issuer,
        &f.namespace,
        &f.token,
        &f.holder,
        &FreezeReason::Compliance,
    );
    assert_eq!(f.mask(), 0);
}

#[test]
fn issuer_freezes_can_be_cleared_by_issuer_or_admin() {
    let f = Fixture::new();
    let c = f.client();
    for clearer in [&f.issuer, &f.admin] {
        c.freeze_offering(&f.issuer, &f.issuer, &f.namespace, &f.token);
        c.unfreeze_offering(clearer, &f.issuer, &f.namespace, &f.token);
        assert!(!c.is_offering_frozen(&f.issuer, &f.namespace, &f.token));
        f.freeze(&f.issuer, FreezeReason::Manual);
        c.emergency_unfreeze_holder(
            clearer,
            &f.issuer,
            &f.namespace,
            &f.token,
            &f.holder,
            &FreezeReason::Manual,
        );
        assert_eq!(f.mask(), 0);
    }
}

#[test]
fn admin_upgrades_existing_issuer_freezes_and_issuer_cannot_downgrade() {
    let f = Fixture::new();
    let c = f.client();
    for caller in [&f.issuer, &f.admin, &f.issuer] {
        c.freeze_offering(caller, &f.issuer, &f.namespace, &f.token);
        f.freeze(caller, FreezeReason::Compliance);
    }
    assert_eq!(
        c.try_unfreeze_offering(&f.issuer, &f.issuer, &f.namespace, &f.token),
        Err(Ok(RevoraError::NotAuthorized))
    );
    assert_eq!(
        c.try_clear_freeze_reason(
            &f.issuer,
            &f.issuer,
            &f.namespace,
            &f.token,
            &f.holder,
            &FreezeReason::Compliance
        ),
        Err(Ok(RevoraError::NotAuthorized))
    );
}

#[test]
fn mixed_reasons_keep_independent_authorities() {
    let f = Fixture::new();
    f.freeze(&f.admin, FreezeReason::Compliance);
    f.freeze(&f.issuer, FreezeReason::Manual);
    let c = f.client();
    c.clear_freeze_reason(
        &f.issuer,
        &f.issuer,
        &f.namespace,
        &f.token,
        &f.holder,
        &FreezeReason::Manual,
    );
    assert_eq!(f.mask(), FreezeReason::Compliance.to_bitmask());
    assert_eq!(
        c.try_clear_freeze_reason(
            &f.issuer,
            &f.issuer,
            &f.namespace,
            &f.token,
            &f.holder,
            &FreezeReason::Compliance
        ),
        Err(Ok(RevoraError::NotAuthorized))
    );
    c.clear_freeze_reason(
        &f.admin,
        &f.issuer,
        &f.namespace,
        &f.token,
        &f.holder,
        &FreezeReason::Compliance,
    );
    assert_eq!(f.mask(), 0);
}

#[test]
fn legacy_offering_flag_is_admin_owned_even_after_issuer_refreeze() {
    let f = Fixture::new();
    f.env.as_contract(&f.contract, || {
        f.env.storage().persistent().set(&DataKey2::FrozenOffering(f.offering()), &true)
    });
    let c = f.client();
    c.freeze_offering(&f.issuer, &f.issuer, &f.namespace, &f.token);
    assert_eq!(
        c.try_unfreeze_offering(&f.issuer, &f.issuer, &f.namespace, &f.token),
        Err(Ok(RevoraError::NotAuthorized))
    );
    c.unfreeze_offering(&f.admin, &f.issuer, &f.namespace, &f.token);
    // A fresh freeze after clearing must not inherit stale admin authority.
    c.freeze_offering(&f.issuer, &f.issuer, &f.namespace, &f.token);
    c.unfreeze_offering(&f.issuer, &f.issuer, &f.namespace, &f.token);
}

#[test]
fn legacy_mask_and_single_reason_fail_closed_without_affecting_new_issuer_bits() {
    for single_reason in [false, true] {
        let f = Fixture::new();
        f.env.as_contract(&f.contract, || {
            if single_reason {
                f.env.storage().persistent().set(
                    &DataKey2::EmergencyFreeze(f.offering(), f.holder.clone()),
                    &FreezeReason::Compliance,
                );
            } else {
                f.env.storage().persistent().set(
                    &DataKey2::HolderFreezeMask(f.offering(), f.holder.clone()),
                    &FreezeReason::Compliance.to_bitmask(),
                );
            }
        });
        f.freeze(&f.issuer, FreezeReason::Compliance);
        f.freeze(&f.issuer, FreezeReason::Manual);
        let c = f.client();
        c.clear_freeze_reason(
            &f.issuer,
            &f.issuer,
            &f.namespace,
            &f.token,
            &f.holder,
            &FreezeReason::Manual,
        );
        assert_eq!(
            c.try_clear_freeze_reason(
                &f.issuer,
                &f.issuer,
                &f.namespace,
                &f.token,
                &f.holder,
                &FreezeReason::Compliance
            ),
            Err(Ok(RevoraError::NotAuthorized))
        );
        assert_eq!(f.mask(), FreezeReason::Compliance.to_bitmask());
        c.clear_freeze_reason(
            &f.admin,
            &f.issuer,
            &f.namespace,
            &f.token,
            &f.holder,
            &FreezeReason::Compliance,
        );
        f.freeze(&f.issuer, FreezeReason::Compliance);
        c.clear_freeze_reason(
            &f.issuer,
            &f.issuer,
            &f.namespace,
            &f.token,
            &f.holder,
            &FreezeReason::Compliance,
        );
        assert_eq!(f.mask(), 0);
    }
}

#[test]
fn duplicate_holder_freeze_with_same_authority_remains_a_noop() {
    let f = Fixture::new();
    f.freeze(&f.issuer, FreezeReason::Manual);
    let before = f.env.events().all().len();
    f.freeze(&f.issuer, FreezeReason::Manual);
    assert_eq!(f.env.events().all().len(), before);
    assert_eq!(f.mask(), FreezeReason::Manual.to_bitmask());
}

fn last_event(f: &Fixture, topic: Symbol, data: Val) {
    let (_, topics, actual) = f.env.events().all().last().expect("freeze event");
    assert_eq!(
        topics,
        (topic, f.issuer.clone(), f.namespace.clone(), f.token.clone()).into_val(&f.env)
    );
    assert_eq!(Vec::from_array(&f.env, [actual]), Vec::from_array(&f.env, [data]));
}

#[test]
fn events_report_effective_authority_including_upgrade_and_admin_clearing_issuer_hold() {
    let f = Fixture::new();
    let c = f.client();
    c.freeze_offering(&f.issuer, &f.issuer, &f.namespace, &f.token);
    last_event(
        &f,
        symbol_short!("frz_off"),
        (f.issuer.clone(), true, FreezeAuthority::Issuer).into_val(&f.env),
    );
    c.freeze_offering(&f.admin, &f.issuer, &f.namespace, &f.token);
    last_event(
        &f,
        symbol_short!("frz_off"),
        (f.admin.clone(), true, FreezeAuthority::Admin).into_val(&f.env),
    );
    c.unfreeze_offering(&f.admin, &f.issuer, &f.namespace, &f.token);
    last_event(
        &f,
        symbol_short!("ufrz_off"),
        (f.admin.clone(), false, FreezeAuthority::Admin).into_val(&f.env),
    );
    f.freeze(&f.issuer, FreezeReason::Manual);
    last_event(
        &f,
        symbol_short!("frz_set"),
        (f.issuer.clone(), f.holder.clone(), FreezeReason::Manual, FreezeAuthority::Issuer)
            .into_val(&f.env),
    );
    f.freeze(&f.admin, FreezeReason::Compliance);
    c.clear_freeze_reason(
        &f.admin,
        &f.issuer,
        &f.namespace,
        &f.token,
        &f.holder,
        &FreezeReason::Manual,
    );
    last_event(
        &f,
        symbol_short!("frz_rc"),
        (
            f.admin.clone(),
            f.holder.clone(),
            FreezeReason::Manual,
            FreezeReason::Compliance.to_bitmask(),
            FreezeAuthority::Issuer,
        )
            .into_val(&f.env),
    );
    c.clear_freeze_reason(
        &f.admin,
        &f.issuer,
        &f.namespace,
        &f.token,
        &f.holder,
        &FreezeReason::Compliance,
    );
    last_event(
        &f,
        symbol_short!("frz_clr"),
        (f.admin.clone(), f.holder.clone(), FreezeReason::Compliance, FreezeAuthority::Admin)
            .into_val(&f.env),
    );
}

#[test]
fn holder_authority_upgrade_emits_event_and_repeat_is_idempotent() {
    let f = Fixture::new();
    f.freeze(&f.issuer, FreezeReason::Manual);
    f.freeze(&f.admin, FreezeReason::Manual);
    last_event(
        &f,
        symbol_short!("frz_set"),
        (f.admin.clone(), f.holder.clone(), FreezeReason::Manual, FreezeAuthority::Admin)
            .into_val(&f.env),
    );
    let before = f.env.events().all().len();
    f.freeze(&f.admin, FreezeReason::Manual);
    assert_eq!(f.env.events().all().len(), before);
    f.env.as_contract(&f.contract, || {
        assert_eq!(
            f.env.storage().persistent().get::<_, FreezeAuthority>(
                &FreezeDataKey::HolderAuthority(
                    f.offering(),
                    f.holder.clone(),
                    FreezeReason::Manual
                )
            ),
            Some(FreezeAuthority::Admin)
        );
    });
}

#[test]
fn authority_is_scoped_to_holder_and_cleared_metadata_cannot_leak() {
    let f = Fixture::new();
    let other_holder = Address::generate(&f.env);
    f.freeze(&f.admin, FreezeReason::Compliance);
    let c = f.client();
    c.emergency_freeze_holder(
        &f.issuer,
        &f.issuer,
        &f.namespace,
        &f.token,
        &other_holder,
        &FreezeReason::Compliance,
    );
    c.clear_freeze_reason(
        &f.issuer,
        &f.issuer,
        &f.namespace,
        &f.token,
        &other_holder,
        &FreezeReason::Compliance,
    );
    assert_eq!(f.mask(), FreezeReason::Compliance.to_bitmask());
    c.clear_freeze_reason(
        &f.admin,
        &f.issuer,
        &f.namespace,
        &f.token,
        &f.holder,
        &FreezeReason::Compliance,
    );
    f.env.as_contract(&f.contract, || {
        assert!(!f.env.storage().persistent().has(&FreezeDataKey::HolderAuthority(
            f.offering(),
            f.holder.clone(),
            FreezeReason::Compliance
        )));
    });
    f.freeze(&f.issuer, FreezeReason::Compliance);
    c.clear_freeze_reason(
        &f.issuer,
        &f.issuer,
        &f.namespace,
        &f.token,
        &f.holder,
        &FreezeReason::Compliance,
    );
}

#[test]
fn missing_authority_metadata_fails_closed() {
    let f = Fixture::new();
    f.freeze(&f.issuer, FreezeReason::Manual);
    f.env.as_contract(&f.contract, || {
        f.env.storage().persistent().remove(&FreezeDataKey::HolderAuthority(
            f.offering(),
            f.holder.clone(),
            FreezeReason::Manual,
        ));
    });
    assert_eq!(
        f.client().try_clear_freeze_reason(
            &f.issuer,
            &f.issuer,
            &f.namespace,
            &f.token,
            &f.holder,
            &FreezeReason::Manual
        ),
        Err(Ok(RevoraError::NotAuthorized))
    );
    assert_eq!(f.mask(), FreezeReason::Manual.to_bitmask());
}

#[test]
fn automatic_sanctions_freeze_upgrades_existing_issuer_reason() {
    let f = Fixture::new();
    f.freeze(&f.issuer, FreezeReason::SanctionsMatch);
    let hash = BytesN::from_array(&f.env, &[42; 32]);
    let c = f.client();
    c.process_ofac_attestation(&hash, &f.issuer, &f.namespace, &f.token, &f.holder);
    last_event(
        &f,
        symbol_short!("auto_frz"),
        (f.holder.clone(), hash, FreezeAuthority::Admin).into_val(&f.env),
    );
    assert_eq!(
        c.try_clear_freeze_reason(
            &f.issuer,
            &f.issuer,
            &f.namespace,
            &f.token,
            &f.holder,
            &FreezeReason::SanctionsMatch
        ),
        Err(Ok(RevoraError::NotAuthorized))
    );
    c.clear_freeze_reason(
        &f.admin,
        &f.issuer,
        &f.namespace,
        &f.token,
        &f.holder,
        &FreezeReason::SanctionsMatch,
    );
}

#[test]
fn auth_and_existing_missing_reason_errors_are_preserved() {
    let f = Fixture::new();
    let c = f.client();
    let outsider = Address::generate(&f.env);
    assert_eq!(
        c.try_freeze_offering(&outsider, &f.issuer, &f.namespace, &f.token),
        Err(Ok(RevoraError::NotAuthorized))
    );
    assert_eq!(
        c.try_clear_freeze_reason(
            &f.issuer,
            &f.issuer,
            &f.namespace,
            &f.token,
            &f.holder,
            &FreezeReason::Manual
        ),
        Err(Ok(RevoraError::HolderFrozen))
    );
    f.freeze(&f.issuer, FreezeReason::Manual);
    assert_eq!(
        c.try_clear_freeze_reason(
            &f.issuer,
            &f.issuer,
            &f.namespace,
            &f.token,
            &f.holder,
            &FreezeReason::Compliance
        ),
        Err(Ok(RevoraError::FreezeReasonMismatch))
    );
    f.env.set_auths(&[]);
    assert!(c
        .try_clear_freeze_reason(
            &f.admin,
            &f.issuer,
            &f.namespace,
            &f.token,
            &f.holder,
            &FreezeReason::Manual
        )
        .is_err());
    assert_eq!(f.mask(), FreezeReason::Manual.to_bitmask());
}
