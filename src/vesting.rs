//! # Token Vesting Core — `vesting.rs`
//!
//! Token vesting schedules and deterministic fixed-point vesting curves.
//!
//! Schedule writes include a curve. Legacy schedules that predate the curve
//! field are converted to `Linear` by the migration helper below.

#![allow(clippy::too_many_arguments)]
#![allow(dead_code)]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec};

// ── Storage keys ─────────────────────────────────────────────────────────────

/// Persistent storage keys for vesting state.
#[contracttype]
#[derive(Clone)]
pub enum VestingKey {
    /// The full [`VestingSchedule`] for a given beneficiary.
    Schedule(Address),
    /// How many tokens the beneficiary has already claimed.
    Claimed(Address),
    /// Number of scheduled beneficiaries for a given issuer/token pair.
    OfferingScheduleCount(VestingOfferingId),
    /// A scheduled beneficiary entry for an issuer/token pair.
    OfferingScheduleItem(VestingOfferingId, u32),
    /// Idempotency key for vesting acceleration (beneficiary, trigger_id)
    Acceleration(Address, Symbol),
}

/// A simple vesting offering identifier with issuer and token.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct VestingOfferingId {
    pub issuer: Address,
    pub token: Address,
}

// ── Public types ──────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VestingCurve {
    Linear,
    Cliff,
    /// Graded vesting curve with milestone timestamps and per-milestone BPS.
    /// Each tuple is (timestamp, bps) and milestones must be strictly monotonic
    /// with total BPS summing to 10000.
    Graded(Vec<(u64, u32)>),
    /// Discrete vesting buckets; value is the bucket duration in seconds.
    Step(u64),
    /// Back-loaded curve using the fixed-point exponent `k_num / k_den`.
    Exponential(u32, u32),
}

/// A single vesting tranche for a beneficiary.
#[contracttype]
#[derive(Clone)]
pub struct VestingSchedule {
    pub issuer: Address,
    pub beneficiary: Address,
    pub token: Address,
    pub total_amount: i128,
    pub cliff_ts: u64,
    pub start_ts: u64,
    pub end_ts: u64,
    pub curve: VestingCurve,
    pub accelerated_amount: i128,
}

/// Errors produced by the vesting module.
#[contracterror]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum VestingError {
    /// A schedule already exists for this beneficiary.
    ScheduleAlreadyExists = 100,
    /// No schedule found for the given beneficiary.
    ScheduleNotFound = 101,
    /// `total_amount` must be > 0.
    InvalidAmount = 102,
    /// Timestamp ordering violated.
    InvalidTimestamps = 103,
    /// Nothing to claim at the current ledger time.
    NothingToClaimYet = 104,
    /// Caller is not authorised for this operation.
    Unauthorized = 105,
    /// A vesting schedule is pre-cliff and blocks issuer transfer migration.
    SchedulePreCliff = 106,
    /// Acceleration trigger already processed for this beneficiary.
    AlreadyAccelerated = 107,
    /// Acceleration bps must not exceed 10000.
    InvalidAccelerationBps = 108,
    /// Curve parameters are invalid or cannot be evaluated safely.
    InvalidCurveParameters = 109,
}

/// Shared schema version for vesting events.
pub const VESTING_EVENT_SCHEMA_VERSION: u32 = 1;

// Legacy event symbols (for backward compatibility).
const EVENT_VESTING_CREATED: Symbol = symbol_short!("vest_crt");
const EVENT_VESTING_CLAIMED: Symbol = symbol_short!("vest_clm");
const EVENT_VESTING_ACCEL: Symbol = symbol_short!("vest_accl");

#[contract]
pub struct VestingContract;

#[contractimpl]
impl VestingContract {
    /// Register a new vesting schedule for `beneficiary`.
    pub fn vesting_register(
        env: Env,
        issuer: Address,
        beneficiary: Address,
        token: Address,
        total_amount: i128,
        cliff_ts: u64,
        start_ts: u64,
        end_ts: u64,
        curve: VestingCurve,
    ) -> Result<(), VestingError> {
        issuer.require_auth();

        if total_amount <= 0 {
            return Err(VestingError::InvalidAmount);
        }
        if start_ts < cliff_ts || end_ts <= start_ts {
            return Err(VestingError::InvalidTimestamps);
        }

        // Validate Graded curve milestones (#525): must be non-empty, strictly
        // monotonic timestamps, and total BPS must equal 10_000 (100%).
        if let VestingCurve::Graded(milestones) = &curve {
            if milestones.is_empty() {
                return Err(VestingError::InvalidTimestamps);
            }
            let mut total_bps: u32 = 0;
            let mut prev_ts: Option<u64> = None;
            for milestone in milestones.iter() {
                let ts = milestone.0;
                let bps = milestone.1;
                if let Some(prev) = prev_ts {
                    if ts <= prev {
                        return Err(VestingError::InvalidTimestamps);
                    }
                }
                prev_ts = Some(ts);
                total_bps = total_bps.checked_add(bps).ok_or(VestingError::InvalidTimestamps)?;
            }
            if total_bps != 10_000 {
                return Err(VestingError::InvalidTimestamps);
            }
        }
        validate_curve(&curve)?;

        let key = VestingKey::Schedule(beneficiary.clone());
        if env.storage().persistent().has(&key) {
            return Err(VestingError::ScheduleAlreadyExists);
        }

        let offering_id = VestingOfferingId { issuer: issuer.clone(), token: token.clone() };
        let schedule = VestingSchedule {
            issuer: issuer.clone(),
            beneficiary: beneficiary.clone(),
            token: token.clone(),
            total_amount,
            cliff_ts,
            start_ts,
            end_ts,
            curve: curve.clone(),
            accelerated_amount: 0,
        };
        env.storage().persistent().set(&key, &schedule);
        env.storage().persistent().set(&VestingKey::Claimed(beneficiary.clone()), &0_i128);
        let count_key = VestingKey::OfferingScheduleCount(offering_id.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        env.storage().persistent().set(
            &VestingKey::OfferingScheduleItem(offering_id.clone(), count),
            &beneficiary.clone(),
        );
        env.storage().persistent().set(&count_key, &(count + 1));

        env.events().publish(
            (EVENT_VESTING_CREATED, beneficiary),
            (total_amount, cliff_ts, start_ts, end_ts, curve.clone()),
        );

        Ok(())
    }

    /// Accelerate vesting for a beneficiary by a given bps (up to 10000) based on a trigger.
    pub fn accelerate_vesting(
        env: Env,
        beneficiary: Address,
        trigger_id: Symbol,
        acceleration_bps: u32,
    ) -> Result<(), VestingError> {
        if acceleration_bps > 10000 {
            return Err(VestingError::InvalidAccelerationBps);
        }

        let sched_key = VestingKey::Schedule(beneficiary.clone());
        let mut schedule: VestingSchedule =
            env.storage().persistent().get(&sched_key).ok_or(VestingError::ScheduleNotFound)?;

        schedule.issuer.require_auth();

        let accel_key = VestingKey::Acceleration(beneficiary.clone(), trigger_id.clone());
        if env.storage().persistent().has(&accel_key) {
            return Err(VestingError::AlreadyAccelerated);
        }

        let raw_accel =
            schedule.total_amount.checked_mul(acceleration_bps as i128).unwrap_or(0) / 10000;

        schedule.accelerated_amount = schedule.accelerated_amount.saturating_add(raw_accel);
        if schedule.accelerated_amount > schedule.total_amount {
            schedule.accelerated_amount = schedule.total_amount;
        }

        env.storage().persistent().set(&accel_key, &true);
        env.storage().persistent().set(&sched_key, &schedule);

        env.events().publish((EVENT_VESTING_ACCEL, beneficiary, trigger_id), raw_accel);

        Ok(())
    }

    /// Claim all tokens that have vested up to the current ledger timestamp.
    pub fn vesting_claim(env: Env, beneficiary: Address) -> Result<i128, VestingError> {
        beneficiary.require_auth();

        let sched_key = VestingKey::Schedule(beneficiary.clone());
        let claimed_key = VestingKey::Claimed(beneficiary.clone());

        let schedule: VestingSchedule =
            env.storage().persistent().get(&sched_key).ok_or(VestingError::ScheduleNotFound)?;

        let already_claimed: i128 = env.storage().persistent().get(&claimed_key).unwrap_or(0_i128);

        let now = env.ledger().timestamp();
        if now < schedule.cliff_ts {
            return Err(VestingError::NothingToClaimYet);
        }

        let claimable = compute_claimable(&schedule, already_claimed, now);
        if claimable == 0 {
            return Ok(0);
        }

        let new_claimed = already_claimed.saturating_add(claimable);
        env.storage().persistent().set(&claimed_key, &new_claimed);

        env.events().publish((EVENT_VESTING_CLAIMED, beneficiary), claimable);
        Ok(claimable)
    }

    /// Return the total tokens already claimed by `beneficiary`.
    pub fn get_claimed_amount(env: Env, beneficiary: Address) -> i128 {
        env.storage().persistent().get(&VestingKey::Claimed(beneficiary)).unwrap_or(0_i128)
    }

    /// Return the tokens vested (but not necessarily claimed) at the current
    /// ledger timestamp.
    pub fn get_vested_amount(env: Env, beneficiary: Address) -> Option<i128> {
        let schedule: VestingSchedule =
            env.storage().persistent().get(&VestingKey::Schedule(beneficiary))?;
        let now = env.ledger().timestamp();
        Some(compute_vested(&schedule, now))
    }

    /// Return the currently claimable amount for `beneficiary`.
    pub fn get_claimable_amount(env: Env, beneficiary: Address) -> Option<i128> {
        let schedule: VestingSchedule =
            env.storage().persistent().get(&VestingKey::Schedule(beneficiary.clone()))?;
        let claimed: i128 =
            env.storage().persistent().get(&VestingKey::Claimed(beneficiary)).unwrap_or(0_i128);
        let now = env.ledger().timestamp();
        Some(compute_claimable(&schedule, claimed, now))
    }

    /// Return all schedules for a batch of beneficiaries.
    pub fn get_vesting_schedules(
        env: Env,
        beneficiaries: Vec<Address>,
    ) -> Vec<Option<VestingSchedule>> {
        let mut out = Vec::new(&env);
        for b in beneficiaries.iter() {
            let s = env.storage().persistent().get(&VestingKey::Schedule(b));
            out.push_back(s);
        }
        out
    }

    /// Calculate the vested progress in basis points (BPS) at an arbitrary timestamp.
    /// Returns a value clamped between 0 and 10000.
    pub fn vested_progress_at(
        env: Env,
        offering_id: VestingOfferingId,
        holder: Address,
        at_ts: u64,
    ) -> u32 {
        let schedule: VestingSchedule =
            match env.storage().persistent().get(&VestingKey::Schedule(holder)) {
                Some(s) => s,
                None => return 0,
            };

        if schedule.issuer != offering_id.issuer || schedule.token != offering_id.token {
            return 0;
        }

        if schedule.total_amount <= 0 {
            return 0;
        }

        let vested = compute_vested(&schedule, at_ts);
        let bps = (vested.saturating_mul(10000) / schedule.total_amount) as u32;

        if bps > 10000 {
            10000
        } else {
            bps
        }
    }
}

/// Migrate all vesting schedules for an issuer/token pair to a new issuer.
///
/// This is used by the issuer transfer workflow to preserve existing schedules
/// when the underlying offering is re-keyed to a new issuer.
pub fn migrate_offering_schedules(
    env: &Env,
    offering_id: &VestingOfferingId,
    new_issuer: Address,
    now: u64,
) -> Result<Vec<Address>, VestingError> {
    let count_key = VestingKey::OfferingScheduleCount(offering_id.clone());
    let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
    if count == 0 {
        return Ok(Vec::new(env));
    }

    let mut beneficiaries: Vec<Address> = Vec::new(env);
    for i in 0..count {
        if let Some(beneficiary) = env
            .storage()
            .persistent()
            .get(&VestingKey::OfferingScheduleItem(offering_id.clone(), i))
        {
            beneficiaries.push_back(beneficiary);
        }
    }

    let new_offering_id =
        VestingOfferingId { issuer: new_issuer.clone(), token: offering_id.token.clone() };
    let mut new_count: u32 = env
        .storage()
        .persistent()
        .get(&VestingKey::OfferingScheduleCount(new_offering_id.clone()))
        .unwrap_or(0);
    let mut migrated: Vec<Address> = Vec::new(env);

    // First pass: validate that no schedule is pre-cliff.
    for beneficiary in beneficiaries.iter() {
        if let Some(schedule) = env
            .storage()
            .persistent()
            .get::<VestingKey, VestingSchedule>(&VestingKey::Schedule(beneficiary.clone()))
        {
            if schedule.issuer == offering_id.issuer
                && schedule.token == offering_id.token
                && now < schedule.cliff_ts
            {
                return Err(VestingError::SchedulePreCliff);
            }
        }
    }

    // Second pass: migrate matching schedules and rebuild the beneficiary index.
    for beneficiary in beneficiaries.iter() {
        if let Some(mut schedule) = env
            .storage()
            .persistent()
            .get::<VestingKey, VestingSchedule>(&VestingKey::Schedule(beneficiary.clone()))
        {
            if schedule.issuer == offering_id.issuer && schedule.token == offering_id.token {
                schedule.issuer = new_issuer.clone();
                env.storage()
                    .persistent()
                    .set(&VestingKey::Schedule(beneficiary.clone()), &schedule);
                env.storage().persistent().set(
                    &VestingKey::OfferingScheduleItem(new_offering_id.clone(), new_count),
                    &beneficiary,
                );
                new_count = new_count.saturating_add(1);
                migrated.push_back(beneficiary.clone());
            }
        }
    }

    for i in 0..count {
        env.storage()
            .persistent()
            .remove(&VestingKey::OfferingScheduleItem(offering_id.clone(), i));
    }
    env.storage().persistent().remove(&count_key);
    if new_count > 0 {
        env.storage()
            .persistent()
            .set(&VestingKey::OfferingScheduleCount(new_offering_id), &new_count);
    }

    Ok(migrated)
}

/// Convert a legacy schedule into the versioned representation.
///
/// Legacy deployments had no curve field; their behavior was linear. This
/// helper is intentionally pure so migration callers can validate and persist
/// the converted value atomically in their own transaction.
pub fn migrate_legacy_schedule(
    legacy: LegacyVestingSchedule,
) -> Result<VestingSchedule, VestingError> {
    if legacy.total_amount <= 0 {
        return Err(VestingError::InvalidAmount);
    }
    if legacy.start_ts < legacy.cliff_ts || legacy.end_ts <= legacy.start_ts {
        return Err(VestingError::InvalidTimestamps);
    }
    Ok(VestingSchedule {
        issuer: legacy.issuer,
        beneficiary: legacy.beneficiary,
        token: legacy.token,
        total_amount: legacy.total_amount,
        cliff_ts: legacy.cliff_ts,
        start_ts: legacy.start_ts,
        end_ts: legacy.end_ts,
        curve: VestingCurve::Linear,
        accelerated_amount: legacy.accelerated_amount,
    })
}

/// The pre-curve schedule layout used by legacy deployments.
#[derive(Clone)]
pub struct LegacyVestingSchedule {
    pub issuer: Address,
    pub beneficiary: Address,
    pub token: Address,
    pub total_amount: i128,
    pub cliff_ts: u64,
    pub start_ts: u64,
    pub end_ts: u64,
    pub accelerated_amount: i128,
}

/// Validate curve parameters before they are persisted.
fn validate_curve(curve: &VestingCurve) -> Result<(), VestingError> {
    if let VestingCurve::Step(period_secs) = curve {
        if *period_secs == 0 {
            return Err(VestingError::InvalidCurveParameters);
        }
    }
    if let VestingCurve::Exponential(_, k_den) = curve {
        if *k_den == 0 {
            return Err(VestingError::InvalidCurveParameters);
        }
    }
    Ok(())
}

/// Evaluate a curve and return the vested amount.
///
/// All intermediate values use checked i128 fixed-point arithmetic with scale
/// 1e18. Exponential curves support rational exponents with bounded parameters;
/// no floating point is used.
pub fn evaluate_curve(
    curve: &VestingCurve,
    elapsed: u64,
    duration: u64,
    total: i128,
) -> Result<i128, VestingError> {
    if total <= 0 || duration == 0 {
        return Err(VestingError::InvalidCurveParameters);
    }
    validate_curve(curve)?;
    let elapsed = elapsed.min(duration);
    let linear = (elapsed as i128)
        .checked_mul(1_000_000_000_000_000_000_i128)
        .and_then(|v| v.checked_div(duration as i128))
        .ok_or(VestingError::InvalidCurveParameters)?;
    let fraction = match curve {
        VestingCurve::Linear | VestingCurve::Cliff | VestingCurve::Graded(_) => linear,
        VestingCurve::Step(period_secs) => {
            let period = *period_secs;
            let buckets = duration
                .checked_add(period.checked_sub(1).ok_or(VestingError::InvalidCurveParameters)?)
                .and_then(|v| v.checked_div(period))
                .ok_or(VestingError::InvalidCurveParameters)?;
            let completed = elapsed / period;
            completed
                .checked_mul(1_000_000_000_000_000_000_u64)
                .and_then(|v| v.checked_div(buckets))
                .unwrap_or(1_000_000_000_000_000_000_u64)
                .min(1_000_000_000_000_000_000) as i128
        }
        VestingCurve::Exponential(k_num, k_den) => {
            // Evaluate x^(k_num/k_den) by finding the greatest fixed-point y
            // whose y^k_den <= x^k_num. The bounded search is deterministic,
            // monotonic, and uses checked i128 arithmetic only.
            if *k_num > 32 || *k_den > 32 || (*k_num == 0 && *k_den == 0) {
                return Err(VestingError::InvalidCurveParameters);
            }
            let target = fixed_pow(linear, *k_num)?;
            let mut low = 0_i128;
            let mut high = 1_000_000_000_000_000_000_i128;
            for _ in 0..60 {
                let mid = low.checked_add(high).ok_or(VestingError::InvalidCurveParameters)? / 2;
                if fixed_pow(mid, *k_den)? <= target {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            low
        }
    };
    total
        .checked_mul(fraction)
        .and_then(|v| v.checked_div(1_000_000_000_000_000_000_i128))
        .ok_or(VestingError::InvalidCurveParameters)
}

fn fixed_pow(mut value: i128, exponent: u32) -> Result<i128, VestingError> {
    let scale = 1_000_000_000_000_000_000_i128;
    if exponent == 0 {
        return Ok(scale);
    }
    let mut result = scale;
    let mut remaining = exponent;
    while remaining > 0 {
        if remaining % 2 == 1 {
            result = result
                .checked_mul(value)
                .and_then(|v| v.checked_div(scale))
                .ok_or(VestingError::InvalidCurveParameters)?;
        }
        remaining /= 2;
        if remaining > 0 {
            value = value
                .checked_mul(value)
                .and_then(|v| v.checked_div(scale))
                .ok_or(VestingError::InvalidCurveParameters)?;
        }
    }
    Ok(result)
}

/// Helper: compute total vested tokens at a given timestamp.
fn compute_vested(schedule: &VestingSchedule, now: u64) -> i128 {
    let base_vested = match &schedule.curve {
        VestingCurve::Graded(milestones) => {
            if now < schedule.cliff_ts {
                0
            } else {
                let mut cumulative_bps: u32 = 0;
                for (ts, bps) in milestones.iter() {
                    if now >= ts {
                        cumulative_bps = cumulative_bps.saturating_add(bps);
                    } else {
                        break;
                    }
                }
                schedule
                    .total_amount
                    .checked_mul(cumulative_bps as i128)
                    .map(|m| m / 10_000)
                    .unwrap_or(0)
            }
        }
        VestingCurve::Linear
        | VestingCurve::Cliff
        | VestingCurve::Step(_)
        | VestingCurve::Exponential(_, _) => {
            if now < schedule.cliff_ts || now <= schedule.start_ts {
                0
            } else if now >= schedule.end_ts {
                schedule.total_amount
            } else {
                evaluate_curve(
                    &schedule.curve,
                    now - schedule.start_ts,
                    schedule.end_ts - schedule.start_ts,
                    schedule.total_amount,
                )
                .unwrap_or(0)
            }
        }
    };

    let total = base_vested.saturating_add(schedule.accelerated_amount);
    if total > schedule.total_amount {
        schedule.total_amount
    } else {
        total
    }
}

/// Helper: compute claimable tokens given prior claimed amount.
fn compute_claimable(schedule: &VestingSchedule, already_claimed: i128, now: u64) -> i128 {
    let vested = compute_vested(schedule, now);
    let claimable = vested.saturating_sub(already_claimed);
    if claimable < 0 {
        0
    } else {
        claimable
    }
}

use soroban_sdk::contracterror;

#[cfg(test)]
mod tests {
    use super::{evaluate_curve, VestingCurve, VestingError};

    const SCALE: i128 = 1_000_000_000_000_000_000;

    #[test]
    fn linear_is_backward_compatible() {
        assert_eq!(evaluate_curve(&VestingCurve::Linear, 0, 100, 1_000).unwrap(), 0);
        assert_eq!(evaluate_curve(&VestingCurve::Linear, 50, 100, 1_000).unwrap(), 500);
        assert_eq!(evaluate_curve(&VestingCurve::Linear, 100, 100, 1_000).unwrap(), 1_000);
    }

    #[test]
    fn step_vests_only_completed_buckets() {
        let curve = VestingCurve::Step(10);
        assert_eq!(evaluate_curve(&curve, 9, 100, 1_000).unwrap(), 0);
        assert_eq!(evaluate_curve(&curve, 10, 100, 1_000).unwrap(), 100);
        assert_eq!(evaluate_curve(&curve, 99, 100, 1_000).unwrap(), 900);
        assert_eq!(evaluate_curve(&curve, 100, 100, 1_000).unwrap(), 1_000);
    }

    #[test]
    fn exponential_is_back_loaded_without_floats() {
        let curve = VestingCurve::Exponential(2, 1);
        assert_eq!(evaluate_curve(&curve, 50, 100, SCALE).unwrap(), 250_000_000_000_000_000);
        assert_eq!(evaluate_curve(&curve, 100, 100, SCALE).unwrap(), SCALE);
    }

    #[test]
    fn exponential_linear_ratio_is_compatible() {
        let curve = VestingCurve::Exponential(1, 1);
        assert_eq!(evaluate_curve(&curve, 37, 100, 10_000).unwrap(), 3_700);
    }

    #[test]
    fn elapsed_is_clamped_at_end() {
        let curve = VestingCurve::Step(10);
        assert_eq!(evaluate_curve(&curve, 1_000, 100, 1_000).unwrap(), 1_000);
    }

    #[test]
    fn invalid_curve_parameters_are_rejected() {
        assert_eq!(
            evaluate_curve(&VestingCurve::Step(0), 1, 10, 100),
            Err(VestingError::InvalidCurveParameters)
        );
        assert_eq!(
            evaluate_curve(&VestingCurve::Exponential(1, 0), 1, 10, 100),
            Err(VestingError::InvalidCurveParameters)
        );
        assert_eq!(
            evaluate_curve(&VestingCurve::Exponential(33, 1), 1, 10, 100),
            Err(VestingError::InvalidCurveParameters)
        );
        assert_eq!(
            evaluate_curve(&VestingCurve::Exponential(0, 0), 1, 10, 100),
            Err(VestingError::InvalidCurveParameters)
        );
    }
}
