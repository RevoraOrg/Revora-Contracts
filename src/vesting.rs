//! # Token Vesting Core — `vesting.rs`
//!
//! Minimal stub implementation that compiles cleanly under the current build.
//! The full vesting flow is disabled pending a re-implementation; only the
//! types, storage keys, and a handful of read-only helpers remain.

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
    /// Graded quarterly vesting curve with milestone timestamps and per-milestone BPS.
    /// Each tuple is (timestamp, bps) and milestones must be strictly monotonic
    /// with total BPS summing to 10000.
    Graded(Vec<(u64, u32)>),
    Step(u32),
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
            (total_amount, cliff_ts, start_ts, end_ts, curve),
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
        _ => {
            // Linear / Cliff / Step: all use the same time-proportional logic.
            if now >= schedule.cliff_ts {
                if now >= schedule.end_ts {
                    schedule.total_amount
                } else if now > schedule.start_ts {
                    let elapsed = (now - schedule.start_ts) as i128;
                    let duration = (schedule.end_ts - schedule.start_ts) as i128;
                    if duration == 0 {
                        schedule.total_amount
                    } else {
                        schedule.total_amount.checked_mul(elapsed).map(|m| m / duration).unwrap_or(0)
                    }
                } else {
                    0
                }
            } else {
                0
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
