use soroban_sdk::{contracttype, Address, Env, symbol_short, Symbol};
use crate::{DataKey2, OfferingId};

pub const EVENT_TAX_ROLLOVER: Symbol = symbol_short!("tax_roll");
/// Emitted on each tax-bucket update to enable off-chain tax-lot reconstruction.
///
/// Topic:  `(tax_lot_v1, issuer, namespace, token)`
/// Data:   `(holder: Address, return_of_capital: i128, capital_gains: i128,
///           amount: i128, period_id: u64, timestamp: u64)`
///
/// ### Field order (for indexer deserialization)
/// 0. `holder`          — Address of the holder whose bucket was updated.
/// 1. `return_of_capital` — Amount treated as return of capital (non-taxable).
/// 2. `capital_gains`    — Amount treated as capital gains (taxable).
/// 3. `amount`           — Total payout amount (`return_of_capital + capital_gains`).
/// 4. `period_id`        — The period associated with this distribution.
/// 5. `timestamp`        — Ledger timestamp at the time of the event.
pub const EVENT_TAX_LOT_V1: Symbol = symbol_short!("tax_lt1");

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
    period_id: u64,
    timestamp: u64,
) -> TaxBucketResult {
    let key = DataKey2::RemainingBasis(offering_id.clone(), holder.clone());
    let remaining_basis: i128 = env.storage().persistent().get(&key).unwrap_or(0);

    let (return_of_capital, capital_gains) = if remaining_basis >= amount {
        let new_basis = remaining_basis - amount;
        env.storage().persistent().set(&key, &new_basis);
        (amount, 0i128)
    } else {
        let roc = remaining_basis;
        let cg = amount - remaining_basis;
        
        env.events().publish(
            (EVENT_TAX_ROLLOVER, offering_id.issuer.clone(), offering_id.namespace.clone(), offering_id.token.clone()),
            (holder.clone(), remaining_basis, 0i128)
        );

        env.storage().persistent().set(&key, &0i128);
        (roc, cg)
    };

    // Emit tax_lot_v1 event for every tax-bucket update
    env.events().publish(
        (EVENT_TAX_LOT_V1, offering_id.issuer.clone(), offering_id.namespace.clone(), offering_id.token.clone()),
        (holder.clone(), return_of_capital, capital_gains, amount, period_id, timestamp),
    );

    TaxBucketResult {
        return_of_capital,
        capital_gains,
    }
}
