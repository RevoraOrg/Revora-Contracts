use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token::Client as TokenClient,
    Address, Env, Map, Vec,
};

/// Top-level storage keys stored in persistent contract storage.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The contract admin address.
    Admin,
    /// The token contract ID used for all deposits and claims.
    Token,
    /// Authorized offering contract addresses allowed to deposit revenue.
    AuthorizedOfferings,
    /// Counter tracking the next period ID to be assigned.
    PeriodCounter,
    /// All registered period IDs (Vec<u32>).
    PeriodIds,
    /// Per-period metadata, keyed by period ID.
    Period(u32),
    /// Per-period beneficiary list, keyed by period ID.
    Beneficiaries(u32),
    /// Claim record: whether `address` has claimed from `period_id`.
    Claimed(u32, Address),
}

/// Metadata for a single revenue period.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Period {
    /// Unique monotonically-increasing identifier.
    pub id: u32,
    /// Ledger sequence number at which the period opens (inclusive).
    pub start_ledger: u32,
    /// Ledger sequence number at which the period closes (inclusive).
    pub end_ledger: u32,
    /// Total token amount deposited for distribution this period.
    pub revenue_amount: i128,
    /// How many tokens have been claimed so far.
    pub claimed_amount: i128,
}

/// Canonical error codes returned by contract functions.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    /// Caller is not the admin.
    Unauthorized = 1,
    /// Contract has already been initialised.
    AlreadyInitialized = 2,
    /// The referenced period does not exist.
    PeriodNotFound = 3,
    /// The period's end ledger has not been reached yet.
    PeriodNotEnded = 4,
    /// The caller is not registered as a beneficiary for this period.
    NotBeneficiary = 5,
    /// The caller has already claimed their share for this period.
    AlreadyClaimed = 6,
    /// A period with overlapping ledger range already exists.
    PeriodOverlap = 7,
    /// The supplied parameters are logically invalid (e.g. start > end, zero amount).
    InvalidInput = 8,
    /// The revenue deposit failed (e.g. insufficient token balance).
    DepositFailed = 9,
    /// Arithmetic overflow occurred.
    Overflow = 10,
    /// No beneficiaries are registered; nothing to distribute.
    NoBeneficiaries = 11,
    /// The caller is not an authorized offering contract.
    UnauthorizedDepositor = 12,
}

#[contract]
pub struct RevenueDepositContract;

#[contractimpl]
impl RevenueDepositContract {
    /// Initialise the contract.
    pub fn initialize(env: Env, admin: Address, token: Address) -> Result<(), ContractError> {
        if env.storage().persistent().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();

        env.storage().persistent().set(&DataKey::Admin, &admin);
        env.storage().persistent().set(&DataKey::Token, &token);
        env.storage().persistent().set(&DataKey::AuthorizedOfferings, &Vec::<Address>::new(&env));
        env.storage().persistent().set(&DataKey::PeriodCounter, &0u32);
        env.storage().persistent().set(&DataKey::PeriodIds, &Vec::<u32>::new(&env));

        Ok(())
    }

    /// Authorize `offering` to deposit revenue into this contract.
    pub fn add_authorized_offering(env: Env, offering: Address) -> Result<(), ContractError> {
        let admin: Address = env.storage().persistent().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        let mut offerings: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AuthorizedOfferings)
            .unwrap_or_else(|| Vec::new(&env));

        if !offerings.contains(&offering) {
            offerings.push_back(offering.clone());
            env.storage().persistent().set(&DataKey::AuthorizedOfferings, &offerings);
        }

        env.events().publish(&symbol_short!("dep_allow_add"), &offering);
        Ok(())
    }

    /// Remove `offering` from the authorized depositors allowlist.
    pub fn remove_authorized_offering(env: Env, offering: Address) -> Result<(), ContractError> {
        let admin: Address = env.storage().persistent().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        let mut offerings: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AuthorizedOfferings)
            .unwrap_or_else(|| Vec::new(&env));

        let pos = offerings.iter().position(|entry| entry == offering).ok_or(ContractError::UnauthorizedDepositor)?;
        offerings.remove(pos as u32);
        env.storage().persistent().set(&DataKey::AuthorizedOfferings, &offerings);

        env.events().publish(&symbol_short!("dep_allow_rm"), &offering);
        Ok(())
    }

    /// Deposit `amount` of the configured token into `period_id` from an authorized offering.
    pub fn deposit(env: Env, caller: Address, period_id: u32, amount: i128) -> Result<(), ContractError> {
        let offerings: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AuthorizedOfferings)
            .unwrap_or_else(|| Vec::new(&env));

        if !offerings.contains(&caller) {
            return Err(ContractError::UnauthorizedDepositor);
        }

        caller.require_auth();

        if amount <= 0 {
            return Err(ContractError::InvalidInput);
        }

        let mut period: Period = env
            .storage()
            .persistent()
            .get(&DataKey::Period(period_id))
            .ok_or(ContractError::PeriodNotFound)?;

        period.revenue_amount = period.revenue_amount.checked_add(amount).ok_or(ContractError::Overflow)?;
        env.storage().persistent().set(&DataKey::Period(period_id), &period);

        let token: Address = env.storage().persistent().get(&DataKey::Token).unwrap();
        let token_client = TokenClient::new(&env, &token);
        token_client.transfer(&caller, &env.current_contract_address(), &amount);

        Ok(())
    }

    /// Create a new revenue period and transfer `revenue_amount` tokens from the admin.
    pub fn create_period(
        env: Env,
        start_ledger: u32,
        end_ledger: u32,
        revenue_amount: i128,
    ) -> Result<u32, ContractError> {
        let admin: Address = env.storage().persistent().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        if revenue_amount <= 0 || start_ledger >= end_ledger {
            return Err(ContractError::InvalidInput);
        }

        Self::assert_no_overlap(&env, start_ledger, end_ledger)?;

        let mut counter: u32 = env.storage().persistent().get(&DataKey::PeriodCounter).unwrap_or(0);
        let period_id = counter;
        counter = counter.checked_add(1).ok_or(ContractError::Overflow)?;
        env.storage().persistent().set(&DataKey::PeriodCounter, &counter);

        let period =
            Period { id: period_id, start_ledger, end_ledger, revenue_amount, claimed_amount: 0 };
        env.storage().persistent().set(&DataKey::Period(period_id), &period);
        env.storage()
            .persistent()
            .set(&DataKey::Beneficiaries(period_id), &Vec::<Address>::new(&env));

        let mut ids: Vec<u32> =
            env.storage().persistent().get(&DataKey::PeriodIds).unwrap_or_else(|| Vec::new(&env));
        ids.push_back(period_id);
        env.storage().persistent().set(&DataKey::PeriodIds, &ids);

        let token: Address = env.storage().persistent().get(&DataKey::Token).unwrap();
        let token_client = TokenClient::new(&env, &token);
        token_client.transfer(&admin, &env.current_contract_address(), &revenue_amount);

        Ok(period_id)
    }

    /// Register `beneficiary` as eligible to claim from `period_id`.
    pub fn add_beneficiary(
        env: Env,
        period_id: u32,
        beneficiary: Address,
    ) -> Result<(), ContractError> {
        let admin: Address = env.storage().persistent().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        Self::assert_period_exists(&env, period_id)?;

        let mut beneficiaries: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Beneficiaries(period_id))
            .unwrap_or_else(|| Vec::new(&env));

        if !beneficiaries.contains(&beneficiary) {
            beneficiaries.push_back(beneficiary);
            env.storage().persistent().set(&DataKey::Beneficiaries(period_id), &beneficiaries);
        }

        Ok(())
    }

    /// Remove `beneficiary` from `period_id`.
    pub fn remove_beneficiary(
        env: Env,
        period_id: u32,
        beneficiary: Address,
    ) -> Result<(), ContractError> {
        let admin: Address = env.storage().persistent().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        Self::assert_period_exists(&env, period_id)?;

        let mut beneficiaries: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Beneficiaries(period_id))
            .unwrap_or_else(|| Vec::new(&env));

        let pos = beneficiaries
            .iter()
            .position(|b| b == beneficiary)
            .ok_or(ContractError::NotBeneficiary)?;

        beneficiaries.remove(pos as u32);
        env.storage().persistent().set(&DataKey::Beneficiaries(period_id), &beneficiaries);

        Ok(())
    }

    /// Claim an equal share of a completed period's revenue.
    pub fn claim(env: Env, period_id: u32, claimant: Address) -> Result<i128, ContractError> {
        claimant.require_auth();

        let mut period: Period = env
            .storage()
            .persistent()
            .get(&DataKey::Period(period_id))
            .ok_or(ContractError::PeriodNotFound)?;

        let current_ledger = env.ledger().sequence();
        if current_ledger <= period.end_ledger {
            return Err(ContractError::PeriodNotEnded);
        }

        let beneficiaries: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Beneficiaries(period_id))
            .unwrap_or_else(|| Vec::new(&env));

        if beneficiaries.is_empty() {
            return Err(ContractError::NoBeneficiaries);
        }

        if !beneficiaries.contains(&claimant) {
            return Err(ContractError::NotBeneficiary);
        }

        let claim_key = DataKey::Claimed(period_id, claimant.clone());
        if env.storage().persistent().has(&claim_key) {
            return Err(ContractError::AlreadyClaimed);
        }

        let count = beneficiaries.len() as i128;
        let share = period.revenue_amount.checked_div(count).ok_or(ContractError::Overflow)?;

        if share <= 0 {
            return Err(ContractError::InvalidInput);
        }

        env.storage().persistent().set(&claim_key, &true);
        period.claimed_amount =
            period.claimed_amount.checked_add(share).ok_or(ContractError::Overflow)?;
        env.storage().persistent().set(&DataKey::Period(period_id), &period);

        let token: Address = env.storage().persistent().get(&DataKey::Token).unwrap();
        let token_client = TokenClient::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &claimant, &share);

        Ok(share)
    }

    /// Return metadata for a period.
    pub fn get_period(env: Env, period_id: u32) -> Result<Period, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Period(period_id))
            .ok_or(ContractError::PeriodNotFound)
    }

    /// Return all period IDs registered with this contract.
    pub fn get_period_ids(env: Env) -> Vec<u32> {
        env.storage().persistent().get(&DataKey::PeriodIds).unwrap_or_else(|| Vec::new(&env))
    }

    /// Return the beneficiary list for a period.
    pub fn get_beneficiaries(env: Env, period_id: u32) -> Result<Vec<Address>, ContractError> {
        Self::assert_period_exists(&env, period_id)?;
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::Beneficiaries(period_id))
            .unwrap_or_else(|| Vec::new(&env)))
    }

    /// Return whether `address` has claimed from `period_id`.
    pub fn has_claimed(env: Env, period_id: u32, address: Address) -> bool {
        env.storage().persistent().has(&DataKey::Claimed(period_id, address))
    }

    /// Return the current admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage().persistent().get(&DataKey::Admin).unwrap()
    }

    /// Return the token contract address.
    pub fn get_token(env: Env) -> Address {
        env.storage().persistent().get(&DataKey::Token).unwrap()
    }

    /// Return the list of authorized offering contract addresses.
    pub fn get_authorized_offerings(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::AuthorizedOfferings)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Build a summary map of unclaimed amounts per period.
    pub fn unclaimed_summary(env: Env) -> Map<u32, i128> {
        let ids: Vec<u32> =
            env.storage().persistent().get(&DataKey::PeriodIds).unwrap_or_else(|| Vec::new(&env));

        let mut map: Map<u32, i128> = Map::new(&env);
        for id in ids.iter() {
            if let Some(period) =
                env.storage().persistent().get::<DataKey, Period>(&DataKey::Period(id))
            {
                let unclaimed = period.revenue_amount - period.claimed_amount;
                map.set(id, unclaimed);
            }
        }
        map
    }

    fn assert_period_exists(env: &Env, period_id: u32) -> Result<(), ContractError> {
        if !env.storage().persistent().has(&DataKey::Period(period_id)) {
            return Err(ContractError::PeriodNotFound);
        }
        Ok(())
    }

    fn assert_no_overlap(
        env: &Env,
        start_ledger: u32,
        end_ledger: u32,
    ) -> Result<(), ContractError> {
        let ids: Vec<u32> =
            env.storage().persistent().get(&DataKey::PeriodIds).unwrap_or_else(|| Vec::new(env));

        for id in ids.iter() {
            let existing: Period = env.storage().persistent().get(&DataKey::Period(id)).unwrap();
            if !(end_ledger < existing.start_ledger || start_ledger > existing.end_ledger) {
                return Err(ContractError::PeriodOverlap);
            }
        }
        Ok(())
    }
}
