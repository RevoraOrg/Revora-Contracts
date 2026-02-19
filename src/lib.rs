#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol,
};

/// Basic skeleton for a revenue-share contract.
///
/// This is intentionally minimal and focuses on the high-level shape:
/// - Registering a startup "offering"
/// - Recording a revenue report
/// - Emitting events that an off-chain distribution engine can consume

#[contract]
pub struct RevoraRevenueShare;

#[derive(Clone)]
pub struct Offering {
    pub issuer: Address,
    pub token: Address,
    pub revenue_share_bps: u32,
}

#[derive(Clone)]
#[contracttype]
pub struct OfferingKey {
    pub issuer: Address,
    pub token: Address,
}

#[derive(Clone)]
#[contracttype]
pub struct OfferingPeriods {
    pub latest_accepted: Option<u64>,
    pub closed_through: Option<u64>,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Offering(OfferingKey),
}

const EVENT_REVENUE_REPORTED: Symbol = symbol_short!("rev_rep");
const EVENT_PERIOD_CLOSED: Symbol = symbol_short!("per_close");

fn read_periods(env: &Env, issuer: &Address, token: &Address) -> OfferingPeriods {
    let key = DataKey::Offering(OfferingKey {
        issuer: issuer.clone(),
        token: token.clone(),
    });

    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(OfferingPeriods {
            latest_accepted: None,
            closed_through: None,
        })
}

fn write_periods(env: &Env, issuer: &Address, token: &Address, periods: &OfferingPeriods) {
    let key = DataKey::Offering(OfferingKey {
        issuer: issuer.clone(),
        token: token.clone(),
    });

    env.storage().persistent().set(&key, periods);
}

#[contractimpl]
impl RevoraRevenueShare {
    /// Register a new revenue-share offering.
    /// In a production contract this would handle access control, supply caps,
    /// and issuance hooks. Here we only emit an event.
    pub fn register_offering(env: Env, issuer: Address, token: Address, revenue_share_bps: u32) {
        issuer.require_auth();

        env.events().publish(
            (symbol_short!("offer_reg"), issuer.clone()),
            (token, revenue_share_bps),
        );
    }

    /// Record a revenue report for an offering.
    ///
    /// Period semantics:
    /// - `period_id` is an arbitrary, integrator-chosen identifier that must be
    ///   monotonically non-decreasing per (issuer, token) offering.
    /// - Reporting windows (e.g., calendar months) are configured off-chain by
    ///   integrators; the contract only enforces ordering.
    /// - Once a period is explicitly closed, further reports with a
    ///   `period_id` less than or equal to the closed-through period are
    ///   rejected.
    pub fn report_revenue(
        env: Env,
        issuer: Address,
        token: Address,
        amount: i128,
        period_id: u64,
    ) {
        issuer.require_auth();

        let mut periods = read_periods(&env, &issuer, &token);

        if let Some(closed) = periods.closed_through {
            if period_id <= closed {
                panic!("period_closed");
            }
        }

        if let Some(latest) = periods.latest_accepted {
            if period_id < latest {
                panic!("backdated_period");
            }
        }

        periods.latest_accepted = match periods.latest_accepted {
            Some(existing) if period_id <= existing => Some(existing),
            _ => Some(period_id),
        };

        write_periods(&env, &issuer, &token, &periods);

        env.events().publish(
            (EVENT_REVENUE_REPORTED, issuer.clone(), token.clone()),
            (amount, period_id),
        );
    }

    /// Get the latest accepted period for an offering, if any.
    pub fn latest_accepted_period(env: Env, issuer: Address, token: Address) -> Option<u64> {
        let periods = read_periods(&env, &issuer, &token);
        periods.latest_accepted
    }

    /// Get the closed-through period for an offering, if any.
    ///
    /// A closed-through period N means that the contract will reject any
    /// revenue reports with `period_id <= N` for this offering.
    pub fn closed_through_period(env: Env, issuer: Address, token: Address) -> Option<u64> {
        let periods = read_periods(&env, &issuer, &token);
        periods.closed_through
    }

    /// Explicitly close a period (or set of periods) for an offering.
    ///
    /// Closing period N means:
    /// - All periods `<= N` are considered finalized and cannot accept further
    ///   revenue reports.
    /// - An event is emitted so that off-chain distribution engines can
    ///   reconcile and advance their own period windows.
    pub fn close_period(env: Env, issuer: Address, token: Address, period_id: u64) {
        issuer.require_auth();

        let mut periods = read_periods(&env, &issuer, &token);

        if let Some(closed) = periods.closed_through {
            if period_id <= closed {
                panic!("period_already_closed");
            }
        }

        if let Some(latest) = periods.latest_accepted {
            if period_id < latest {
                panic!("cannot_close_before_latest_report");
            }
        }

        periods.closed_through = Some(period_id);
        write_periods(&env, &issuer, &token, &periods);

        env.events().publish(
            (EVENT_PERIOD_CLOSED, issuer.clone(), token.clone()),
            period_id,
        );
    }
}

mod test;

