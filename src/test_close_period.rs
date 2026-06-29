#![cfg(test)]
use super::*;
use soroban_sdk::{testutils::Events, Env};

#[test]
#[should_panic(expected = "Error(Contract, #456)")]
fn test_claim_on_deferred_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, AmountValidationResult);
    let client = AmountValidationResultClient::new(&env, &contract_id);

    client.report_revenue(&2, &5000, &true);
    client.claim(&2);
}
