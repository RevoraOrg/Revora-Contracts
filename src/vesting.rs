//! Token vesting contract for team, advisors, and other stakeholders.
//!
//! Supports multiple vesting schedules per beneficiary, with linear and cliff-based vesting.
//! Standalone primitive that can integrate with Revora token or revenue-share logic.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Symbol,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum VestingError {
    Unauthorized = 1,
    ScheduleNotFound = 2,
    ScheduleNotStarted = 3,
    NothingToClaim = 4,
    CancelNotAllowed = 5,
    InvalidAmount = 6,
    InvalidDuration = 7,
    InvalidCliff = 8,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct VestingSchedule {
    pub beneficiary: Address,
    pub token: Address,
    pub total_amount: i128,
    pub claimed_amount: i128,
    pub start_time: u64,
    pub cliff_time: u64,
    pub end_time: u64,
    pub cancelled: bool,
}

#[contracttype]
pub enum VestingDataKey {
    Admin,
    ScheduleCount(Address),
    Schedule(Address, u32),
    /// Number of partial claim records stored for a schedule
    ClaimCount(Address, u32),
    /// Partial claim record for (admin, schedule_index, claim_index)
    ClaimRecord(Address, u32, u32),
}

const EVENT_VESTING_CREATED: Symbol = symbol_short!("vest_crt");
const EVENT_VESTING_CLAIMED: Symbol = symbol_short!("vest_clm");
const EVENT_VESTING_CANCELLED: Symbol = symbol_short!("vest_can");
const EVENT_VESTING_CREATED_V1: Symbol = symbol_short!("vst_crt1");
const EVENT_VESTING_CLAIMED_V1: Symbol = symbol_short!("vst_clm1");
const EVENT_VESTING_CANCELLED_V1: Symbol = symbol_short!("vst_can1");

/// Version tag for versioned vesting event payloads.
pub const VESTING_EVENT_SCHEMA_VERSION: u32 = 1;

#[contract]
pub struct RevoraVesting;

#[contractimpl]
impl RevoraVesting {
    /// Initialize the vesting contract with an admin.
    /// Renamed to `initialize_vesting` to avoid symbol conflicts with other contracts.
    pub fn initialize_vesting(env: Env, admin: Address) -> Result<(), VestingError> {
        if env.storage().persistent().has(&VestingDataKey::Admin) {
            return Err(VestingError::Unauthorized);
        }
        admin.require_auth();
        env.storage().persistent().set(&VestingDataKey::Admin, &admin);
        Ok(())
    }

    /// Create a vesting schedule. Admin only.
    /// Linear vesting: amount vests linearly from start_time to end_time.
    /// Cliff: nothing vests before cliff_time; after cliff, linear to end_time.
    #[allow(clippy::too_many_arguments)]
    pub fn create_schedule(
        env: Env,
        admin: Address,
        beneficiary: Address,
        token: Address,
        total_amount: i128,
        start_time: u64,
        cliff_duration_secs: u64,
        duration_secs: u64,
    ) -> Result<u32, VestingError> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&VestingDataKey::Admin)
            .ok_or(VestingError::Unauthorized)?;
        if admin != stored_admin {
            return Err(VestingError::Unauthorized);
        }
        if total_amount <= 0 {
            return Err(VestingError::InvalidAmount);
        }
        if duration_secs == 0 {
            return Err(VestingError::InvalidDuration);
        }
        if cliff_duration_secs > duration_secs {
            return Err(VestingError::InvalidCliff);
        }

        let end_time = start_time.saturating_add(duration_secs);
        let cliff_time = start_time.saturating_add(cliff_duration_secs);

        let count_key = VestingDataKey::ScheduleCount(admin.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        let schedule = VestingSchedule {
            beneficiary: beneficiary.clone(),
            token: token.clone(),
            total_amount,
            claimed_amount: 0,
            start_time,
            cliff_time,
            end_time,
            cancelled: false,
        };
        let schedule_key = VestingDataKey::Schedule(admin.clone(), count);
        env.storage().persistent().set(&schedule_key, &schedule);
        env.storage().persistent().set(&count_key, &(count + 1));

        env.events().publish(
            (EVENT_VESTING_CREATED, admin, beneficiary),
            (token, total_amount, start_time, cliff_time, end_time, count),
        );
        env.events().publish(
            (EVENT_VESTING_CREATED_V1, admin, beneficiary),
            (
                VESTING_EVENT_SCHEMA_VERSION,
                token,
                total_amount,
                start_time,
                cliff_time,
                end_time,
                count,
            ),
        );
        Ok(count)
    }

    /// Cancel a schedule (admin only). Business rules: only future unvested amount is forfeit.
    pub fn cancel_schedule(
        env: Env,
        admin: Address,
        beneficiary: Address,
        schedule_index: u32,
    ) -> Result<(), VestingError> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&VestingDataKey::Admin)
            .ok_or(VestingError::Unauthorized)?;
        if admin != stored_admin {
            return Err(VestingError::Unauthorized);
        }
        let key = VestingDataKey::Schedule(admin.clone(), schedule_index);
        let mut schedule: VestingSchedule =
            env.storage().persistent().get(&key).ok_or(VestingError::ScheduleNotFound)?;
        if schedule.beneficiary != beneficiary {
            return Err(VestingError::ScheduleNotFound);
        }
        if schedule.cancelled {
            return Err(VestingError::CancelNotAllowed);
        }
        schedule.cancelled = true;
        env.storage().persistent().set(&key, &schedule);
        env.events().publish(
            (EVENT_VESTING_CANCELLED, admin, beneficiary),
            (schedule_index, schedule.token.clone()),
        );
        env.events().publish(
            (EVENT_VESTING_CANCELLED_V1, admin, beneficiary),
            (
                VESTING_EVENT_SCHEMA_VERSION,
                schedule_index,
                schedule.token.clone(),
            ),
        );
        Ok(())
    }

    /// Compute currently vested amount (linear from cliff to end).
    fn vested_amount(env: &Env, schedule: &VestingSchedule) -> i128 {
        let now = env.ledger().timestamp();
        if now < schedule.cliff_time || schedule.cancelled {
            return 0;
        }
        if now >= schedule.end_time {
            return schedule.total_amount;
        }
        let vesting_duration = schedule.end_time - schedule.cliff_time;
        let elapsed = now - schedule.cliff_time;
        let vested = (schedule.total_amount as u128)
            .saturating_mul(elapsed as u128)
            .checked_div(vesting_duration as u128)
            .unwrap_or(0) as i128;
        core::cmp::min(vested, schedule.total_amount)
    }

    /// Claim vested tokens. Callable by beneficiary.
    /// Renamed to `claim_vesting` to avoid symbol conflicts with other contracts.
    pub fn claim_vesting(
        env: Env,
        beneficiary: Address,
        admin: Address,
        schedule_index: u32,
    ) -> Result<i128, VestingError> {
        beneficiary.require_auth();
        let key = VestingDataKey::Schedule(admin.clone(), schedule_index);
        let mut schedule: VestingSchedule =
            env.storage().persistent().get(&key).ok_or(VestingError::ScheduleNotFound)?;
        if schedule.beneficiary != beneficiary {
            return Err(VestingError::ScheduleNotFound);
        }
        if schedule.cancelled {
            return Err(VestingError::ScheduleNotFound);
        }
        let vested = Self::vested_amount(&env, &schedule);
        let claimable = vested.saturating_sub(schedule.claimed_amount);
        if claimable <= 0 {
            return Err(VestingError::NothingToClaim);
        }
        schedule.claimed_amount = schedule.claimed_amount.saturating_add(claimable);
        env.storage().persistent().set(&key, &schedule);

        let contract_addr = env.current_contract_address();
        token::Client::new(&env, &schedule.token).transfer(
            &contract_addr,
            &beneficiary,
            &claimable,
        );

        env.events().publish(
            (EVENT_VESTING_CLAIMED, beneficiary.clone(), admin),
            (schedule_index, schedule.token, claimable),
        );
        env.events().publish(
            (EVENT_VESTING_CLAIMED_V1, beneficiary.clone(), admin),
            (
                VESTING_EVENT_SCHEMA_VERSION,
                schedule_index,
                schedule.token,
                claimable,
            ),
        );
        Ok(claimable)
    }

    /// Claim a specific amount of currently claimable tokens (partial claim).
    /// Emits a dedicated partial-claim event and records the claim in history.
    pub fn claim_vesting_partial(
        env: Env,
        beneficiary: Address,
        admin: Address,
        schedule_index: u32,
        amount: i128,
    ) -> Result<i128, VestingError> {
        beneficiary.require_auth();
        if amount <= 0 {
            return Err(VestingError::InvalidAmount);
        }
        let key = VestingDataKey::Schedule(admin.clone(), schedule_index);
        let mut schedule: VestingSchedule =
            env.storage().persistent().get(&key).ok_or(VestingError::ScheduleNotFound)?;
        if schedule.beneficiary != beneficiary {
            return Err(VestingError::ScheduleNotFound);
        }
        if schedule.cancelled {
            return Err(VestingError::ScheduleNotFound);
        }
        let vested = Self::vested_amount(&env, &schedule);
        let claimable = vested.saturating_sub(schedule.claimed_amount);
        if claimable <= 0 {
            return Err(VestingError::NothingToClaim);
        }
        if amount > claimable {
            return Err(VestingError::InvalidAmount);
        }

        // Update claimed and persist
        schedule.claimed_amount = schedule.claimed_amount.saturating_add(amount);
        env.storage().persistent().set(&key, &schedule);

        // Transfer tokens from this contract to beneficiary
        let contract_addr = env.current_contract_address();
        token::Client::new(&env, &schedule.token).transfer(&contract_addr, &beneficiary, &amount);

        // Record claim history: append (timestamp, amount)
        let cnt_key = VestingDataKey::ClaimCount(admin.clone(), schedule_index);
        let count: u32 = env.storage().persistent().get(&cnt_key).unwrap_or(0);
        let rec_key = VestingDataKey::ClaimRecord(admin.clone(), schedule_index, count);
        let record: (u64, i128) = (env.ledger().timestamp(), amount);
        env.storage().persistent().set(&rec_key, &record);
        env.storage().persistent().set(&cnt_key, &(count + 1));

        // Emit event for partial claim
        env.events().publish(
            (EVENT_VESTING_PCLAIM, beneficiary.clone(), admin),
            (schedule_index, schedule.token, amount, count),
        );
        Ok(amount)
    }

    /// Return number of partial-claim records for a schedule.
    pub fn get_partial_claim_count(env: Env, admin: Address, schedule_index: u32) -> u32 {
        env.storage()
            .persistent()
            .get(&VestingDataKey::ClaimCount(admin, schedule_index))
            .unwrap_or(0)
    }

    /// Return a partial-claim record (timestamp, amount) by index.
    pub fn get_partial_claim_record(
        env: Env,
        admin: Address,
        schedule_index: u32,
        claim_index: u32,
    ) -> Option<(u64, i128)> {
        env.storage()
            .persistent()
            .get(&VestingDataKey::ClaimRecord(admin, schedule_index, claim_index))
    }

    /// Query a schedule by admin and index.
    pub fn get_schedule(
        env: Env,
        admin: Address,
        schedule_index: u32,
    ) -> Result<VestingSchedule, VestingError> {
        let key = VestingDataKey::Schedule(admin, schedule_index);
        env.storage().persistent().get(&key).ok_or(VestingError::ScheduleNotFound)
    }

    /// Claimable amount for a schedule (vested minus already claimed).
    /// Renamed to `get_claimable_vesting` to avoid symbol conflicts with other contracts.
    pub fn get_claimable_vesting(
        env: Env,
        admin: Address,
        schedule_index: u32,
    ) -> Result<i128, VestingError> {
        let schedule = Self::get_schedule(env.clone(), admin, schedule_index)?;
        let vested = Self::vested_amount(&env, &schedule);
        Ok(vested.saturating_sub(schedule.claimed_amount))
    }

    /// Number of schedules created by an admin.
    pub fn get_schedule_count(env: Env, admin: Address) -> u32 {
        env.storage().persistent().get(&VestingDataKey::ScheduleCount(admin)).unwrap_or(0)
    }

    /// Returns the current vesting event schema version.
    pub fn get_event_schema_version(env: Env) -> u32 {
        let _ = env;
        VESTING_EVENT_SCHEMA_VERSION
    }
}
