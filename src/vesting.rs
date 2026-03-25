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

/// Maximum schedules returned in a single `list_schedules_page` call.
const MAX_SCHEDULES_PAGE: u32 = 50;

/// Maximum schedules returned per beneficiary in a single `list_schedules_for_beneficiary` call.
const MAX_SCHEDULES_PER_BENEFICIARY: u32 = 100;

#[contracttype]
pub enum VestingDataKey {
    Admin,
    ScheduleCount(Address),
    Schedule(Address, u32),
    /// Number of schedule indices recorded for (admin, beneficiary). Secondary index.
    BeneficiaryScheduleCount(Address, Address),
    /// The global `schedule_index` stored at position `i` for (admin, beneficiary). Secondary index.
    BeneficiaryScheduleItem(Address, Address, u32),
}

const EVENT_VESTING_CREATED: Symbol = symbol_short!("vest_crt");
const EVENT_VESTING_CLAIMED: Symbol = symbol_short!("vest_clm");
const EVENT_VESTING_CANCELLED: Symbol = symbol_short!("vest_can");

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

        // Maintain secondary beneficiary index (append-only).
        let b_count_key =
            VestingDataKey::BeneficiaryScheduleCount(admin.clone(), beneficiary.clone());
        let b_count: u32 = env.storage().persistent().get(&b_count_key).unwrap_or(0);
        let b_item_key =
            VestingDataKey::BeneficiaryScheduleItem(admin.clone(), beneficiary.clone(), b_count);
        env.storage().persistent().set(&b_item_key, &count);
        env.storage().persistent().set(&b_count_key, &(b_count + 1));

        env.events().publish(
            (EVENT_VESTING_CREATED, admin, beneficiary),
            (token, total_amount, start_time, cliff_time, end_time, count),
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
        Ok(claimable)
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

    /// Return a paginated slice of all schedules for `admin`.
    ///
    /// - `start`: first index to return (0-based).
    /// - `limit`: maximum items per page; clamped to [`MAX_SCHEDULES_PAGE`]. Pass `0` to use the
    ///   maximum.
    ///
    /// Returns `(schedules, Some(next_start))` when a following page exists, or
    /// `(schedules, None)` on the final page.
    pub fn list_schedules_page(
        env: Env,
        admin: Address,
        start: u32,
        limit: u32,
    ) -> (soroban_sdk::Vec<VestingSchedule>, Option<u32>) {
        let count = Self::get_schedule_count(env.clone(), admin.clone());
        if start >= count {
            return (soroban_sdk::Vec::new(&env), None);
        }
        let effective_limit = match limit {
            0 => MAX_SCHEDULES_PAGE,
            l if l > MAX_SCHEDULES_PAGE => MAX_SCHEDULES_PAGE,
            l => l,
        };
        let end = core::cmp::min(start.saturating_add(effective_limit), count);
        let mut results = soroban_sdk::Vec::new(&env);
        for i in start..end {
            let key = VestingDataKey::Schedule(admin.clone(), i);
            if let Some(schedule) = env.storage().persistent().get::<VestingDataKey, VestingSchedule>(&key) {
                results.push_back(schedule);
            }
        }
        let next_cursor = if end < count { Some(end) } else { None };
        (results, next_cursor)
    }

    /// Number of schedules recorded under the secondary beneficiary index for `(admin, beneficiary)`.
    ///
    /// Returns 0 when no schedules exist. Does not require authentication.
    pub fn get_beneficiary_schedule_count(env: Env, admin: Address, beneficiary: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&VestingDataKey::BeneficiaryScheduleCount(admin, beneficiary))
            .unwrap_or(0)
    }

    /// Return all schedules assigned to `beneficiary` under `admin`.
    ///
    /// Results are ordered by creation time (ascending schedule index). Capped at
    /// [`MAX_SCHEDULES_PER_BENEFICIARY`].
    ///
    /// Does not require authentication.
    pub fn list_schedules_for_beneficiary(
        env: Env,
        admin: Address,
        beneficiary: Address,
    ) -> soroban_sdk::Vec<VestingSchedule> {
        let b_count = Self::get_beneficiary_schedule_count(
            env.clone(),
            admin.clone(),
            beneficiary.clone(),
        );
        let cap = core::cmp::min(b_count, MAX_SCHEDULES_PER_BENEFICIARY);
        let mut results = soroban_sdk::Vec::new(&env);
        for i in 0..cap {
            let item_key =
                VestingDataKey::BeneficiaryScheduleItem(admin.clone(), beneficiary.clone(), i);
            if let Some(schedule_index) =
                env.storage().persistent().get::<VestingDataKey, u32>(&item_key)
            {
                let sched_key = VestingDataKey::Schedule(admin.clone(), schedule_index);
                if let Some(schedule) =
                    env.storage().persistent().get::<VestingDataKey, VestingSchedule>(&sched_key)
                {
                    results.push_back(schedule);
                }
            }
        }
        results
    }

    /// Return only **active** schedules for `beneficiary` under `admin`.
    ///
    /// A schedule is _active_ when it is not cancelled and at least one of the following holds:
    /// - tokens remain claimable (`vested − claimed > 0`), or
    /// - vesting is still in progress (`now < end_time`).
    ///
    /// Capped at [`MAX_SCHEDULES_PER_BENEFICIARY`]. Does not require authentication.
    pub fn active_schedules_for_beneficiary(
        env: Env,
        admin: Address,
        beneficiary: Address,
    ) -> soroban_sdk::Vec<VestingSchedule> {
        let b_count = Self::get_beneficiary_schedule_count(
            env.clone(),
            admin.clone(),
            beneficiary.clone(),
        );
        let cap = core::cmp::min(b_count, MAX_SCHEDULES_PER_BENEFICIARY);
        let now = env.ledger().timestamp();
        let mut results = soroban_sdk::Vec::new(&env);
        for i in 0..cap {
            let item_key =
                VestingDataKey::BeneficiaryScheduleItem(admin.clone(), beneficiary.clone(), i);
            if let Some(schedule_index) =
                env.storage().persistent().get::<VestingDataKey, u32>(&item_key)
            {
                let sched_key = VestingDataKey::Schedule(admin.clone(), schedule_index);
                if let Some(schedule) =
                    env.storage().persistent().get::<VestingDataKey, VestingSchedule>(&sched_key)
                {
                    if schedule.cancelled {
                        continue;
                    }
                    let vested = Self::vested_amount(&env, &schedule);
                    let claimable = vested.saturating_sub(schedule.claimed_amount);
                    if claimable > 0 || now < schedule.end_time {
                        results.push_back(schedule);
                    }
                }
            }
        }
        results
    }
}
