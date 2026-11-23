#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

use crate::{
    ClassConfig, DataKey, DataKey3, OfferingId, RevoraError, RevoraRevenueShare, ShareClass,
};

fn setup() -> (Env, Address, OfferingId, Address) {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);
    let offering_id = OfferingId { issuer, namespace: symbol_short!("def"), token };
    (env, contract_id, offering_id, holder)
}

fn configured_classes(env: &Env) -> Vec<(ShareClass, ClassConfig)> {
    Vec::from_array(
        env,
        [
            (ShareClass::A, ClassConfig { bps: 5_000, voting: true }),
            (ShareClass::B, ClassConfig { bps: 5_000, voting: false }),
        ],
    )
}

#[test]
fn unconfigured_class_is_rejected_without_changing_state() {
    let (env, contract_id, offering_id, holder) = setup();

    env.as_contract(&contract_id, || {
        let storage = env.storage().persistent();
        storage.set(&DataKey3::OfferingClasses(offering_id.clone()), &configured_classes(&env));
        storage.set(&DataKey::HolderShare(offering_id.clone(), holder.clone()), &1_000u32);
        storage.set(&DataKey::HolderShareTotal(offering_id.clone()), &1_000u32);
        storage.set(&DataKey3::TotalSharesIssued(offering_id.clone()), &1_000i128);
        storage
            .set(&DataKey3::TotalClassSharesIssued(offering_id.clone(), ShareClass::A), &1_000i128);
        storage.set(&DataKey3::HolderShareNonce(offering_id.clone(), holder.clone()), &1u64);
    });

    let before = env.as_contract(&contract_id, || {
        let storage = env.storage().persistent();
        (
            storage
                .get::<_, u32>(&DataKey::HolderShare(offering_id.clone(), holder.clone()))
                .unwrap_or(0),
            storage.get::<_, u32>(&DataKey::HolderShareTotal(offering_id.clone())).unwrap_or(0),
            storage.get::<_, i128>(&DataKey3::TotalSharesIssued(offering_id.clone())).unwrap_or(0),
            storage
                .get::<_, i128>(&DataKey3::TotalClassSharesIssued(
                    offering_id.clone(),
                    ShareClass::A,
                ))
                .unwrap_or(0),
            storage
                .get::<_, i128>(&DataKey3::TotalClassSharesIssued(
                    offering_id.clone(),
                    ShareClass::Custom(symbol_short!("C")),
                ))
                .unwrap_or(0),
            storage
                .get::<_, u64>(&DataKey3::HolderShareNonce(offering_id.clone(), holder.clone()))
                .unwrap_or(0),
        )
    });

    let result = env.as_contract(&contract_id, || {
        RevoraRevenueShare::set_holder_share_internal(
            &env,
            offering_id.issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
            holder.clone(),
            2_000,
            Some(ShareClass::Custom(symbol_short!("C"))),
            Some(2),
        )
    });
    assert_eq!(result, Err(RevoraError::InvalidShareClass));

    let after = env.as_contract(&contract_id, || {
        let storage = env.storage().persistent();
        (
            storage
                .get::<_, u32>(&DataKey::HolderShare(offering_id.clone(), holder.clone()))
                .unwrap_or(0),
            storage.get::<_, u32>(&DataKey::HolderShareTotal(offering_id.clone())).unwrap_or(0),
            storage.get::<_, i128>(&DataKey3::TotalSharesIssued(offering_id.clone())).unwrap_or(0),
            storage
                .get::<_, i128>(&DataKey3::TotalClassSharesIssued(
                    offering_id.clone(),
                    ShareClass::A,
                ))
                .unwrap_or(0),
            storage
                .get::<_, i128>(&DataKey3::TotalClassSharesIssued(
                    offering_id.clone(),
                    ShareClass::Custom(symbol_short!("C")),
                ))
                .unwrap_or(0),
            storage
                .get::<_, u64>(&DataKey3::HolderShareNonce(offering_id.clone(), holder.clone()))
                .unwrap_or(0),
        )
    });
    assert_eq!(after, before);
}

#[test]
fn configured_class_updates_class_totals() {
    let (env, contract_id, offering_id, holder) = setup();
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey3::OfferingClasses(offering_id.clone()), &configured_classes(&env));
    });

    let result = env.as_contract(&contract_id, || {
        RevoraRevenueShare::set_holder_share_internal(
            &env,
            offering_id.issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
            holder.clone(),
            2_500,
            Some(ShareClass::A),
            Some(1),
        )
    });
    assert_eq!(result, Ok(()));

    env.as_contract(&contract_id, || {
        let storage = env.storage().persistent();
        assert_eq!(
            storage
                .get::<_, i128>(&DataKey3::TotalClassSharesIssued(
                    offering_id.clone(),
                    ShareClass::A,
                ))
                .unwrap_or(0),
            2_500
        );
        assert_eq!(
            storage.get::<_, i128>(&DataKey3::TotalSharesIssued(offering_id.clone())).unwrap_or(0),
            2_500
        );
    });
}

#[test]
fn class_is_rejected_when_offering_has_no_configured_classes() {
    let (env, contract_id, offering_id, holder) = setup();
    let result = env.as_contract(&contract_id, || {
        RevoraRevenueShare::set_holder_share_internal(
            &env,
            offering_id.issuer.clone(),
            offering_id.namespace.clone(),
            offering_id.token.clone(),
            holder.clone(),
            2_500,
            Some(ShareClass::A),
            Some(1),
        )
    });
    assert_eq!(result, Err(RevoraError::InvalidShareClass));

    env.as_contract(&contract_id, || {
        assert_eq!(
            env.storage()
                .persistent()
                .get::<_, u64>(&DataKey3::HolderShareNonce(offering_id.clone(), holder.clone(),))
                .unwrap_or(0),
            0
        );
        assert_eq!(
            env.storage()
                .persistent()
                .get::<_, i128>(&DataKey3::TotalSharesIssued(offering_id.clone()))
                .unwrap_or(0),
            0
        );
    });
}
