#![no_std]

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, symbol_short, xdr::ToXdr,
    Address, Bytes, BytesN, Env, Symbol, Vec,
};

// ── Cross-Contract Client Interface for Core ─────────────────────────────────

#[contractclient(name = "CoreContractClient")]
pub trait CoreContractInterface {
    fn get_holder_share(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        holder: Address,
    ) -> u32;

    fn get_last_snapshot_commit_ref(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> u64;

    fn get_snapshot_holder_share(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        snapshot_id: u64,
        holder: Address,
    ) -> u32;

    fn get_issuer(env: Env, issuer: Address, namespace: Symbol, token: Address) -> Option<Address>;

    fn is_frozen(env: Env) -> bool;
    fn is_paused(env: Env) -> bool;

    fn set_admin_from_governance(env: Env, new_admin: Address);
    fn freeze_from_governance(env: Env);
}

// ── Types and Models ──────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct OfferingId {
    pub issuer: Address,
    pub namespace: Symbol,
    pub token: Address,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
pub enum DisputeStatus {
    Open,
    Resolved,
    Rejected,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
pub enum DisputeSeverity {
    Standard,
    Critical,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Dispute {
    pub id: BytesN<32>,
    pub holder: Address,
    pub offering_id: OfferingId,
    pub opened_at: u64,
    pub severity: DisputeSeverity,
    pub meta_hash: BytesN<32>,
    pub status: DisputeStatus,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalAction {
    SetAdmin(Address),
    Freeze,
    SetThreshold(u32),
    AddOwner(Address),
    RemoveOwner(Address),
    SetProposalDuration(u64),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub id: u32,
    pub action: ProposalAction,
    pub proposer: Address,
    pub approvals: Vec<Address>,
    pub executed: bool,
    pub expiry: u64,
    pub epoch: u64,
    pub quorum_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct GovernanceProposal {
    pub id: u32,
    pub meta_hash: BytesN<32>,
    pub quorum_bps: u32,
    pub ends_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct GovProposalEntry {
    pub id: u32,
    pub description: Symbol,
    pub snapshot_id: u64,
    pub created_at: u64,
    pub yes_weight: u32,
    pub no_weight: u32,
    pub open: bool,
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum VoteChoice {
    No = 0,
    Yes = 1,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct EventIndexTopicV2 {
    pub version: u32,
    pub event_type: Symbol,
    pub issuer: Address,
    pub namespace: Symbol,
    pub token: Address,
    pub period_id: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct EventIndexTopicV3 {
    pub version: u32,
    pub event_type: Symbol,
    pub issuer: Address,
    pub namespace: Symbol,
    pub token: Address,
    pub period_id: u64,
    pub _reserved: u32,
}

// ── Errors ────────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum GovernanceError {
    LimitReached = 2,
    OfferingNotFound = 4,
    ContractFrozen = 10,
    NotAuthorized = 19,
    NotInitialized = 20,
    InvalidAmount = 21,
    ProposalExpired = 30,
    InvalidShareBps = 8,
    AlreadyApproved = 43,
    StaleProposal = 44,
    DisputeNotFound = 58,
    DisputeAlreadyOpen = 59,
    MaxDisputesReached = 60,
    DisputeZeroShare = 61,
    NotDisputeIssuer = 86,
    DisputeAlreadyResolved = 87,
    DisputeFreezeActive = 88,
}

// ── Storage Keys ──────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    CoreContract,
    Admin,
    MultisigOwners,
    MultisigThreshold,
    MultisigProposalCount,
    MultisigProposalDuration,
    MultisigProposal(u32),
    MultisigQuorumBps,
    MultisigEpoch,
    VoterWeight(Address),
    GovProposalCount(OfferingId),
    GovProposal(OfferingId, u32),
    VoteRecord(OfferingId, u32, Address),
    GovernanceProposalCount(OfferingId),
    GovernanceProposal(OfferingId, u32),
    GovernanceProposalMeta(OfferingId, BytesN<32>),
    Dispute(BytesN<32>),
    CriticalDisputeCount(OfferingId),
    DisputeCount(OfferingId, Address),
}

// ── Constants & Events ────────────────────────────────────────────────────────

const MAX_MULTISIG_OWNERS: u32 = 20;
const MAX_PROPOSAL_DURATION: u64 = 365 * 24 * 60 * 60;
const MAX_OPEN_DISPUTES_PER_HOLDER: u32 = 5;

const EVENT_MULTISIG_INIT: Symbol = symbol_short!("ms_init");
const EVENT_PROPOSAL_CREATED: Symbol = symbol_short!("prop_new");
const EVENT_PROPOSAL_APPROVED: Symbol = symbol_short!("prop_app");
const EVENT_PROPOSAL_EXECUTED: Symbol = symbol_short!("prop_exe");
const EVENT_STALE_PROPOSAL_REJECT: Symbol = symbol_short!("stale_pr");
const EVENT_DURATION_SET: Symbol = symbol_short!("dur_set");

const EVENT_DISPUTE_FREEZE_ON: Symbol = symbol_short!("dsp_frzon");
const EVENT_DISPUTE_FREEZE_OFF: Symbol = symbol_short!("dsp_fzoff");

const EVENT_PROPOSAL_CREATED_GOV: Symbol = symbol_short!("prop_crt");
const EVENT_GOV_PROP_CREATED: Symbol = symbol_short!("gov_new");
const EVENT_GOV_VOTE_CAST: Symbol = symbol_short!("gov_vote");
const EVENT_WEIGHT_PIN: Symbol = symbol_short!("wt_pin");
const EVENT_TYPE_VOTE_V3: Symbol = symbol_short!("vote_v3");
const EVENT_INDEXED_V2: Symbol = symbol_short!("ev_idx2");
const EVENT_INDEXED_V3: Symbol = symbol_short!("ev_idx3");

// ── Contract Implementation ───────────────────────────────────────────────────

#[contract]
pub struct RevoraGovernance;

#[contractimpl]
impl RevoraGovernance {
    /// Initialize the governance contract with an admin and the core contract address.
    pub fn initialize(
        env: Env,
        admin: Address,
        core_contract: Address,
    ) -> Result<(), GovernanceError> {
        admin.require_auth();
        if env.storage().persistent().has(&DataKey::Admin) {
            return Err(GovernanceError::LimitReached);
        }
        env.storage().persistent().set(&DataKey::Admin, &admin);
        env.storage().persistent().set(&DataKey::CoreContract, &core_contract);
        Ok(())
    }

    /// Update the core contract address (admin-only).
    pub fn set_core_contract(env: Env, core_contract: Address) -> Result<(), GovernanceError> {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .ok_or(GovernanceError::NotInitialized)?;
        admin.require_auth();
        env.storage().persistent().set(&DataKey::CoreContract, &core_contract);
        Ok(())
    }

    /// Get configured core contract address.
    pub fn get_core_contract(env: Env) -> Option<Address> {
        env.storage().persistent().get(&DataKey::CoreContract)
    }

    /// Set individual voter weight in basis points (admin or multisig).
    pub fn set_voter_weight(
        env: Env,
        voter: Address,
        weight_bps: u32,
    ) -> Result<(), GovernanceError> {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .ok_or(GovernanceError::NotInitialized)?;
        admin.require_auth();
        env.storage().persistent().set(&DataKey::VoterWeight(voter), &weight_bps);
        Ok(())
    }

    // ── Multisig ──────────────────────────────────────────────────────────────

    pub fn init_multisig(
        env: Env,
        caller: Address,
        owners: Vec<Address>,
        threshold: u32,
        proposal_duration: u64,
        quorum_bps: u32,
    ) -> Result<(), GovernanceError> {
        caller.require_auth();

        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .ok_or(GovernanceError::NotInitialized)?;
        if caller != admin {
            return Err(GovernanceError::NotAuthorized);
        }

        if env.storage().persistent().has(&DataKey::MultisigThreshold) {
            return Err(GovernanceError::LimitReached);
        }
        if owners.is_empty() {
            return Err(GovernanceError::LimitReached);
        }
        if owners.len() > MAX_MULTISIG_OWNERS {
            return Err(GovernanceError::LimitReached);
        }
        if threshold == 0 || threshold > owners.len() {
            return Err(GovernanceError::LimitReached);
        }
        if proposal_duration == 0 {
            return Err(GovernanceError::InvalidAmount);
        }
        if quorum_bps == 0 || quorum_bps > 10_000 {
            return Err(GovernanceError::InvalidShareBps);
        }

        for i in 0..owners.len() {
            let owner_i = owners.get(i).unwrap();
            for j in (i + 1)..owners.len() {
                if owner_i == owners.get(j).unwrap() {
                    return Err(GovernanceError::LimitReached);
                }
            }
        }

        if proposal_duration > MAX_PROPOSAL_DURATION {
            return Err(GovernanceError::InvalidAmount);
        }

        env.storage().persistent().set(&DataKey::MultisigThreshold, &threshold);
        env.storage().persistent().set(&DataKey::MultisigOwners, &owners.clone());
        env.storage().persistent().set(&DataKey::MultisigProposalCount, &0_u32);
        env.storage().persistent().set(&DataKey::MultisigProposalDuration, &proposal_duration);
        env.storage().persistent().set(&DataKey::MultisigQuorumBps, &quorum_bps);
        env.events().publish((EVENT_MULTISIG_INIT, caller), (owners.len(), threshold));
        Ok(())
    }

    pub fn propose_action(
        env: Env,
        proposer: Address,
        action: ProposalAction,
    ) -> Result<u32, GovernanceError> {
        proposer.require_auth();
        Self::require_multisig_owner(&env, &proposer)?;

        let count_key = DataKey::MultisigProposalCount;
        let id: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let duration: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::MultisigProposalDuration)
            .ok_or(GovernanceError::NotInitialized)?;
        let current_epoch: u64 =
            env.storage().persistent().get(&DataKey::MultisigEpoch).unwrap_or(0);
        let now = env.ledger().timestamp();
        let expiry = now.checked_add(duration).ok_or(GovernanceError::InvalidAmount)?;

        let mut initial_approvals = Vec::new(&env);
        initial_approvals.push_back(proposer.clone());

        let quorum_bps: u32 =
            env.storage().persistent().get(&DataKey::MultisigQuorumBps).unwrap_or(5100);

        let proposal = Proposal {
            id,
            action,
            proposer: proposer.clone(),
            approvals: initial_approvals,
            executed: false,
            expiry,
            epoch: current_epoch,
            quorum_bps,
        };

        env.storage().persistent().set(&DataKey::MultisigProposal(id), &proposal);
        env.storage().persistent().set(&count_key, &(id + 1));

        env.events().publish((EVENT_PROPOSAL_CREATED, proposer.clone()), (id, expiry));
        env.events().publish((EVENT_PROPOSAL_APPROVED, proposer), id);
        Ok(id)
    }

    pub fn approve_action(
        env: Env,
        approver: Address,
        proposal_id: u32,
    ) -> Result<(), GovernanceError> {
        approver.require_auth();
        Self::require_multisig_owner(&env, &approver)?;

        let key = DataKey::MultisigProposal(proposal_id);
        let mut proposal: Proposal =
            env.storage().persistent().get(&key).ok_or(GovernanceError::OfferingNotFound)?;

        if proposal.executed {
            return Err(GovernanceError::LimitReached);
        }

        if env.ledger().timestamp() >= proposal.expiry {
            return Err(GovernanceError::ProposalExpired);
        }

        for i in 0..proposal.approvals.len() {
            if proposal.approvals.get(i).unwrap() == approver {
                return Err(GovernanceError::AlreadyApproved);
            }
        }

        proposal.approvals.push_back(approver.clone());
        env.events().publish((EVENT_PROPOSAL_APPROVED, approver), proposal_id);
        env.storage().persistent().set(&key, &proposal);
        Ok(())
    }

    pub fn execute_action(
        env: Env,
        executor: Address,
        proposal_id: u32,
    ) -> Result<(), GovernanceError> {
        executor.require_auth();
        Self::require_multisig_owner(&env, &executor)?;

        let key = DataKey::MultisigProposal(proposal_id);
        let mut proposal: Proposal =
            env.storage().persistent().get(&key).ok_or(GovernanceError::OfferingNotFound)?;

        if proposal.executed {
            return Err(GovernanceError::LimitReached);
        }

        if env.ledger().timestamp() >= proposal.expiry {
            return Err(GovernanceError::ProposalExpired);
        }

        let current_epoch: u64 =
            env.storage().persistent().get(&DataKey::MultisigEpoch).unwrap_or(0);
        if proposal.epoch != current_epoch {
            env.events().publish(
                (EVENT_STALE_PROPOSAL_REJECT, executor.clone()),
                (proposal_id, proposal.epoch, current_epoch),
            );
            return Err(GovernanceError::StaleProposal);
        }

        let threshold: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::MultisigThreshold)
            .ok_or(GovernanceError::NotInitialized)?;
        if proposal.approvals.len() < threshold {
            return Err(GovernanceError::NotAuthorized);
        }

        if !Self::check_quorum_inner(&env, &proposal) {
            return Err(GovernanceError::NotAuthorized);
        }

        proposal.executed = true;
        env.storage().persistent().set(&key, &proposal);

        match proposal.action.clone() {
            ProposalAction::SetAdmin(new_admin) => {
                if let Some(core) = Self::get_core_contract(env.clone()) {
                    let client = CoreContractClient::new(&env, &core);
                    client.set_admin_from_governance(&new_admin);
                }
                env.storage().persistent().set(&DataKey::Admin, &new_admin);
            }
            ProposalAction::Freeze => {
                if let Some(core) = Self::get_core_contract(env.clone()) {
                    let client = CoreContractClient::new(&env, &core);
                    client.freeze_from_governance();
                }
            }
            ProposalAction::SetThreshold(new_threshold) => {
                let owners: Vec<Address> =
                    env.storage().persistent().get(&DataKey::MultisigOwners).unwrap();
                if new_threshold == 0 || new_threshold > owners.len() {
                    return Err(GovernanceError::InvalidShareBps);
                }
                env.storage().persistent().set(&DataKey::MultisigThreshold, &new_threshold);
                let next_epoch = current_epoch + 1;
                env.storage().persistent().set(&DataKey::MultisigEpoch, &next_epoch);
            }
            ProposalAction::AddOwner(new_owner) => {
                let mut owners: Vec<Address> =
                    env.storage().persistent().get(&DataKey::MultisigOwners).unwrap();
                if owners.len() >= MAX_MULTISIG_OWNERS {
                    return Err(GovernanceError::LimitReached);
                }
                if owners.contains(&new_owner) {
                    return Err(GovernanceError::LimitReached);
                }
                owners.push_back(new_owner);
                env.storage().persistent().set(&DataKey::MultisigOwners, &owners);
                let next_epoch = current_epoch + 1;
                env.storage().persistent().set(&DataKey::MultisigEpoch, &next_epoch);
            }
            ProposalAction::RemoveOwner(old_owner) => {
                let owners: Vec<Address> =
                    env.storage().persistent().get(&DataKey::MultisigOwners).unwrap();
                if !owners.contains(&old_owner) {
                    return Err(GovernanceError::NotAuthorized);
                }
                if (owners.len() - 1) < threshold {
                    return Err(GovernanceError::LimitReached);
                }
                let mut new_owners = Vec::new(&env);
                for i in 0..owners.len() {
                    let owner = owners.get(i).unwrap();
                    if owner != old_owner {
                        new_owners.push_back(owner);
                    }
                }
                env.storage().persistent().set(&DataKey::MultisigOwners, &new_owners);
                let next_epoch = current_epoch + 1;
                env.storage().persistent().set(&DataKey::MultisigEpoch, &next_epoch);
            }
            ProposalAction::SetProposalDuration(new_duration) => {
                if new_duration == 0 {
                    return Err(GovernanceError::InvalidAmount);
                }
                env.storage().persistent().set(&DataKey::MultisigProposalDuration, &new_duration);
                env.events().publish((EVENT_DURATION_SET, proposal.proposer.clone()), new_duration);
            }
        }

        env.events().publish((EVENT_PROPOSAL_EXECUTED, executor), proposal_id);
        Ok(())
    }

    pub fn check_quorum(env: Env, proposal_id: u32) -> bool {
        let proposal: Proposal = env
            .storage()
            .persistent()
            .get(&DataKey::MultisigProposal(proposal_id))
            .expect("Proposal not found");
        Self::check_quorum_inner(&env, &proposal)
    }

    fn check_quorum_inner(env: &Env, proposal: &Proposal) -> bool {
        if proposal.approvals.is_empty() {
            return false;
        }
        let mut total_voted_bps: u32 = 0;
        for i in 0..proposal.approvals.len() {
            let voter = proposal.approvals.get(i).unwrap();
            let weight: u32 =
                env.storage().persistent().get(&DataKey::VoterWeight(voter)).unwrap_or(0);
            total_voted_bps = total_voted_bps.saturating_add(weight);
        }
        total_voted_bps >= proposal.quorum_bps
    }

    pub fn get_proposal(env: Env, proposal_id: u32) -> Option<Proposal> {
        env.storage().persistent().get(&DataKey::MultisigProposal(proposal_id))
    }

    pub fn get_multisig_owners(env: Env) -> Option<Vec<Address>> {
        env.storage().persistent().get(&DataKey::MultisigOwners)
    }

    pub fn get_multisig_threshold(env: Env) -> Option<u32> {
        env.storage().persistent().get(&DataKey::MultisigThreshold)
    }

    fn require_multisig_owner(env: &Env, caller: &Address) -> Result<(), GovernanceError> {
        let owners: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::MultisigOwners)
            .ok_or(GovernanceError::NotInitialized)?;
        if !owners.contains(caller) {
            return Err(GovernanceError::NotAuthorized);
        }
        Ok(())
    }

    // ── Disputes ───────────────────────────────────────────────────────────────

    pub fn open_dispute(
        env: Env,
        holder: Address,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        severity: DisputeSeverity,
        meta_hash: BytesN<32>,
    ) -> Result<BytesN<32>, GovernanceError> {
        holder.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        // Query holder share from core contract
        let share = if let Some(core) = Self::get_core_contract(env.clone()) {
            let client = CoreContractClient::new(&env, &core);
            if client.is_frozen() {
                return Err(GovernanceError::ContractFrozen);
            }
            client.get_holder_share(&issuer, &namespace, &token, &holder)
        } else {
            0
        };

        if share == 0 {
            return Err(GovernanceError::DisputeZeroShare);
        }

        let mut input = Bytes::new(&env);
        input.append(&offering_id.issuer.clone().to_xdr(&env));
        input.append(&offering_id.namespace.clone().to_xdr(&env));
        input.append(&offering_id.token.clone().to_xdr(&env));
        input.append(&holder.clone().to_xdr(&env));
        input.append(&meta_hash.clone().to_xdr(&env));
        let dispute_id: BytesN<32> = env.crypto().sha256(&input).into();

        if env.storage().persistent().has(&DataKey::Dispute(dispute_id.clone())) {
            return Err(GovernanceError::DisputeAlreadyOpen);
        }

        let count_key = DataKey::DisputeCount(offering_id.clone(), holder.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_OPEN_DISPUTES_PER_HOLDER {
            return Err(GovernanceError::MaxDisputesReached);
        }

        let opened_at = env.ledger().timestamp();
        let dispute = Dispute {
            id: dispute_id.clone(),
            holder: holder.clone(),
            offering_id: offering_id.clone(),
            opened_at,
            severity,
            meta_hash: meta_hash.clone(),
            status: DisputeStatus::Open,
        };

        env.storage().persistent().set(&DataKey::Dispute(dispute_id.clone()), &dispute);
        env.storage().persistent().set(&count_key, &(count + 1));

        if severity == DisputeSeverity::Critical {
            let crit_key = DataKey::CriticalDisputeCount(offering_id.clone());
            let crit_count: u32 = env.storage().persistent().get(&crit_key).unwrap_or(0);
            env.storage().persistent().set(&crit_key, &(crit_count + 1));
            if crit_count == 0 {
                env.events().publish(
                    (EVENT_DISPUTE_FREEZE_ON,),
                    (offering_id.clone(), holder.clone(), dispute_id.clone()),
                );
            }
        }

        env.events().publish(
            (Symbol::new(&env, "dispute_open"),),
            (dispute_id.clone(), offering_id, holder, meta_hash, severity),
        );

        Ok(dispute_id)
    }

    pub fn is_dispute_freeze_active(env: Env, offering_id: OfferingId) -> bool {
        let crit_key = DataKey::CriticalDisputeCount(offering_id);
        env.storage().persistent().get::<DataKey, u32>(&crit_key).unwrap_or(0) > 0
    }

    pub fn get_dispute(env: Env, dispute_id: BytesN<32>) -> Option<Dispute> {
        env.storage().persistent().get(&DataKey::Dispute(dispute_id))
    }

    pub fn resolve_dispute(
        env: Env,
        caller: Address,
        dispute_id: BytesN<32>,
        resolution: DisputeStatus,
    ) -> Result<(), GovernanceError> {
        caller.require_auth();

        let mut dispute: Dispute = env
            .storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id.clone()))
            .ok_or(GovernanceError::DisputeNotFound)?;

        if dispute.status != DisputeStatus::Open {
            return Err(GovernanceError::DisputeAlreadyResolved);
        }
        if caller != dispute.offering_id.issuer {
            return Err(GovernanceError::NotDisputeIssuer);
        }

        let was_critical = dispute.severity == DisputeSeverity::Critical;
        dispute.status = resolution;
        env.storage().persistent().set(&DataKey::Dispute(dispute_id), &dispute);

        if was_critical {
            let crit_key = DataKey::CriticalDisputeCount(dispute.offering_id.clone());
            let crit_count: u32 = env.storage().persistent().get(&crit_key).unwrap_or(0);
            if crit_count > 0 {
                let new_count = crit_count - 1;
                env.storage().persistent().set(&crit_key, &new_count);
                if new_count == 0 {
                    env.events().publish(
                        (EVENT_DISPUTE_FREEZE_OFF,),
                        (dispute.offering_id, dispute.holder, dispute.id),
                    );
                }
            }
        }

        Ok(())
    }

    // ── Holder Governance & Proposals ─────────────────────────────────────────

    pub fn create_proposal(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        meta_hash: BytesN<32>,
        quorum_bps: u32,
        voting_window: u64,
    ) -> Result<u32, GovernanceError> {
        issuer.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if let Some(core) = Self::get_core_contract(env.clone()) {
            let client = CoreContractClient::new(&env, &core);
            if client.is_frozen() {
                return Err(GovernanceError::ContractFrozen);
            }
            if client.is_paused() {
                return Err(GovernanceError::NotAuthorized);
            }
            let current_issuer = client
                .get_issuer(&issuer, &namespace, &token)
                .ok_or(GovernanceError::OfferingNotFound)?;
            if current_issuer != issuer {
                return Err(GovernanceError::OfferingNotFound);
            }
        }

        if quorum_bps == 0 || quorum_bps > 10_000 {
            return Err(GovernanceError::InvalidAmount);
        }
        if voting_window == 0 {
            return Err(GovernanceError::InvalidAmount);
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::GovernanceProposalMeta(offering_id.clone(), meta_hash.clone()))
        {
            return Err(GovernanceError::LimitReached);
        }

        let count_key = DataKey::GovernanceProposalCount(offering_id.clone());
        let proposal_id: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        let ends_at = env
            .ledger()
            .timestamp()
            .checked_add(voting_window)
            .ok_or(GovernanceError::InvalidAmount)?;
        let proposal = GovernanceProposal {
            id: proposal_id,
            meta_hash: meta_hash.clone(),
            quorum_bps,
            ends_at,
        };

        env.storage()
            .persistent()
            .set(&DataKey::GovernanceProposal(offering_id.clone(), proposal_id), &proposal);
        env.storage().persistent().set(&count_key, &(proposal_id + 1));
        env.storage()
            .persistent()
            .set(&DataKey::GovernanceProposalMeta(offering_id.clone(), meta_hash.clone()), &true);
        env.events().publish(
            (EVENT_PROPOSAL_CREATED_GOV, issuer, namespace, token),
            (proposal_id, meta_hash, quorum_bps, ends_at),
        );
        Ok(proposal_id)
    }

    pub fn get_governance_proposal(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        proposal_id: u32,
    ) -> Option<GovernanceProposal> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey::GovernanceProposal(offering_id, proposal_id))
    }

    pub fn create_gov_proposal(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        description: Symbol,
    ) -> Result<u32, GovernanceError> {
        issuer.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        let snapshot_id: u64 = if let Some(core) = Self::get_core_contract(env.clone()) {
            let client = CoreContractClient::new(&env, &core);
            if client.is_frozen() {
                return Err(GovernanceError::ContractFrozen);
            }
            if client.is_paused() {
                return Err(GovernanceError::NotAuthorized);
            }
            let current_issuer = client
                .get_issuer(&issuer, &namespace, &token)
                .ok_or(GovernanceError::OfferingNotFound)?;
            if current_issuer != issuer {
                return Err(GovernanceError::OfferingNotFound);
            }
            let snap = client.get_last_snapshot_commit_ref(&issuer, &namespace, &token);
            if snap == 0 {
                return Err(GovernanceError::LimitReached);
            }
            snap
        } else {
            return Err(GovernanceError::NotInitialized);
        };

        let count_key = DataKey::GovProposalCount(offering_id.clone());
        let proposal_id: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);

        let created_at = env.ledger().timestamp();
        let proposal = GovProposalEntry {
            id: proposal_id,
            description,
            snapshot_id,
            created_at,
            yes_weight: 0,
            no_weight: 0,
            open: true,
        };

        env.storage()
            .persistent()
            .set(&DataKey::GovProposal(offering_id.clone(), proposal_id), &proposal);
        env.storage().persistent().set(&count_key, &proposal_id.saturating_add(1));

        env.events().publish(
            (EVENT_GOV_PROP_CREATED, issuer, namespace, token),
            (proposal_id, snapshot_id, created_at),
        );

        Ok(proposal_id)
    }

    pub fn cast_vote(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        proposal_id: u32,
        voter: Address,
        approve: bool,
    ) -> Result<u32, GovernanceError> {
        voter.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if let Some(core) = Self::get_core_contract(env.clone()) {
            let client = CoreContractClient::new(&env, &core);
            if client.is_frozen() {
                return Err(GovernanceError::ContractFrozen);
            }
            if client.is_paused() {
                return Err(GovernanceError::NotAuthorized);
            }
            let _ = client
                .get_issuer(&issuer, &namespace, &token)
                .ok_or(GovernanceError::OfferingNotFound)?;
        }

        let prop_key = DataKey::GovProposal(offering_id.clone(), proposal_id);
        let mut proposal: GovProposalEntry =
            env.storage().persistent().get(&prop_key).ok_or(GovernanceError::LimitReached)?;
        if !proposal.open {
            return Err(GovernanceError::LimitReached);
        }

        let vote_key = DataKey::VoteRecord(offering_id.clone(), proposal_id, voter.clone());
        if env.storage().persistent().has(&vote_key) {
            return Err(GovernanceError::AlreadyApproved);
        }

        let weight: u32 = if let Some(core) = Self::get_core_contract(env.clone()) {
            let client = CoreContractClient::new(&env, &core);
            client.get_snapshot_holder_share(
                &issuer,
                &namespace,
                &token,
                &proposal.snapshot_id,
                &voter,
            )
        } else {
            0
        };

        if approve {
            proposal.yes_weight = proposal.yes_weight.saturating_add(weight);
        } else {
            proposal.no_weight = proposal.no_weight.saturating_add(weight);
        }

        env.storage().persistent().set(&prop_key, &proposal);
        env.storage().persistent().set(&vote_key, &approve);

        env.events().publish(
            (EVENT_WEIGHT_PIN, voter.clone()),
            (proposal_id, proposal.snapshot_id, weight),
        );

        env.events().publish(
            (EVENT_GOV_VOTE_CAST, issuer.clone(), namespace.clone(), token.clone()),
            (proposal_id, voter.clone(), approve, weight),
        );

        let choice = if approve { VoteChoice::Yes } else { VoteChoice::No };
        let v2_topic = EventIndexTopicV2 {
            version: 2,
            event_type: EVENT_TYPE_VOTE_V3,
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
            period_id: 0,
        };
        let v3_topic = EventIndexTopicV3 {
            version: 3,
            event_type: EVENT_TYPE_VOTE_V3,
            issuer,
            namespace,
            token,
            period_id: 0,
            _reserved: 0,
        };
        let data = (proposal_id, voter, choice, weight);

        env.events().publish((EVENT_INDEXED_V3, v3_topic), data.clone());
        env.events().publish((EVENT_INDEXED_V2, v2_topic), data);

        Ok(weight)
    }

    pub fn get_gov_proposal(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        proposal_id: u32,
    ) -> Option<GovProposalEntry> {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey::GovProposal(offering_id, proposal_id))
    }

    pub fn get_gov_proposal_count(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
    ) -> u32 {
        let offering_id = OfferingId { issuer, namespace, token };
        env.storage().persistent().get(&DataKey::GovProposalCount(offering_id)).unwrap_or(0)
    }

    pub fn close_gov_proposal(
        env: Env,
        issuer: Address,
        namespace: Symbol,
        token: Address,
        proposal_id: u32,
    ) -> Result<(), GovernanceError> {
        issuer.require_auth();

        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: namespace.clone(),
            token: token.clone(),
        };

        if let Some(core) = Self::get_core_contract(env.clone()) {
            let client = CoreContractClient::new(&env, &core);
            if client.is_frozen() {
                return Err(GovernanceError::ContractFrozen);
            }
            if client.is_paused() {
                return Err(GovernanceError::NotAuthorized);
            }
            let current_issuer = client
                .get_issuer(&issuer, &namespace, &token)
                .ok_or(GovernanceError::OfferingNotFound)?;
            if current_issuer != issuer {
                return Err(GovernanceError::OfferingNotFound);
            }
        }

        let prop_key = DataKey::GovProposal(offering_id, proposal_id);
        let mut proposal: GovProposalEntry =
            env.storage().persistent().get(&prop_key).ok_or(GovernanceError::LimitReached)?;
        if !proposal.open {
            return Err(GovernanceError::LimitReached);
        }
        proposal.open = false;
        env.storage().persistent().set(&prop_key, &proposal);
        Ok(())
    }
}
