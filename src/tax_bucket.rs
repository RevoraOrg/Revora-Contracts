use soroban_sdk::{contracttype, Address, Env, symbol_short, Symbol};
use crate::{DataKey2, OfferingId};

pub const EVENT_TAX_ROLLOVER: Symbol = symbol_short!("tax_roll");

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TaxBucketResult {
    pub return_of_capital: i128,
    pub capital_gains: i128,
}

pub fn track_cost_basis(env: &Env, offering_id: &OfferingId, holder: &Address, cost_basis: i128) {
    let key = DataKey2::RemainingBasis(offering_id.clone(), holder.clone());
    env.storage().persistent().set(&key, &cost_basis);
}

pub fn rollover_distribution(
    env: &Env,
    offering_id: &OfferingId,
    holder: &Address,
    amount: i128,
) -> TaxBucketResult {
    let key = DataKey2::RemainingBasis(offering_id.clone(), holder.clone());
    let remaining_basis: i128 = env.storage().persistent().get(&key).unwrap_or(0);

    if remaining_basis >= amount {
        let new_basis = remaining_basis - amount;
        env.storage().persistent().set(&key, &new_basis);
        TaxBucketResult {
            return_of_capital: amount,
            capital_gains: 0,
        }
    } else {
        let return_of_capital = remaining_basis;
        let capital_gains = amount - remaining_basis;
        
        env.events().publish(
            (EVENT_TAX_ROLLOVER, offering_id.issuer.clone(), offering_id.namespace.clone(), offering_id.token.clone()),
            (holder.clone(), remaining_basis, 0i128)
        );

        env.storage().persistent().set(&key, &0i128);
        TaxBucketResult {
            return_of_capital,
            capital_gains,
        }
    }
}
