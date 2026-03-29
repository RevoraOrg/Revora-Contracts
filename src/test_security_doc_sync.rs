#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

use crate::{RevoraRevenueShare, RevoraRevenueShareClient, RevoraError, CONTRACT_VERSION};

#[test]
fn security_doc_sync_returns_expected_markers() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let payload = client.get_security_doc_sync();

    assert_eq!(payload.get(symbol_short!("ver")).unwrap(), CONTRACT_VERSION);
    assert_eq!(payload.get(symbol_short!("ev_sch")).unwrap(), 1u32);
    assert_eq!(payload.get(symbol_short!("idx_sch")).unwrap(), 2u32);
    assert_eq!(
        payload.get(symbol_short!("err_xfer")).unwrap(),
        RevoraError::TransferFailed as u32
    );
    assert_eq!(
        payload.get(symbol_short!("err_auth")).unwrap(),
        RevoraError::NotAuthorized as u32
    );
}

#[test]
fn security_doc_sync_is_deterministic() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let first = client.get_security_doc_sync();
    let second = client.get_security_doc_sync();
    assert_eq!(first, second);

    // Ensure key set size is stable for doc tooling.
    assert_eq!(first.len(), 6);

    let _issuer = Address::generate(&env);
}
