use soroban_sdk::{Address, Env, Symbol, Vec};
use crate::RevoraRevenueShareClient;
use proptest::prelude::*;

// Common proptest strategies for Revora contract testing
pub fn any_offering_id(env: &Env) -> impl Strategy<Value = (Address, Symbol, Address)> {
    (
        any::<Address>(),
        ".*".prop_map(|s: String| Symbol::short(&env, &s.chars().take(4).collect::<String>()[..])),
        any::<Address>(),
    )
}

pub fn any_positive_amount() -> impl Strategy<Value = i128> {
    1i128..=100_000_000
}

/// Generator for strictly increasing period sequences (invariant).
pub fn arb_strictly_increasing_periods(len: usize) -> impl Strategy<Value = Vec<u64>> {
    vec![0u64.prop_map(|_| 1u64); len].prop_map(|mut v| {
        let mut last = 0u64;
        for i in 0..v.len() {
            last += 1 + (i as u64 * 10); // Ensure gaps
            v[i] = last;
        }
        v
    })
}


// Operations for random sequence generation
#[derive(Debug, Clone)]
#[derive(Clone, Debug)]
pub enum TestOperation {
    /// Register offering with valid bps (0-10000)
    RegisterOffering((Address, Symbol, Address, u32, Address)),
    /// Report revenue with non-negative amount, valid period
    ReportRevenue((Address, Symbol, Address, Address, i128, u64, bool)),
    /// Deposit revenue (transfers tokens)
    DepositRevenue((Address, Symbol, Address, Address, i128, u64)),
    /// Set holder share (0-10000 bps)
    SetHolderShare((Address, Symbol, Address, Address, u32)),
    /// Add to blacklist
    BlacklistAdd((Address, Symbol, Address, Address)),
    /// Remove from blacklist
    BlacklistRemove((Address, Symbol, Address, Address)),
    /// Set concentration limit + enforce
    SetConcentrationLimit((Address, Symbol, Address, u32, bool)),
    /// Report concentration (bps value)
    ReportConcentration((Address, Symbol, Address, u32)),
    /// Pause contract
    Pause,
    /// Unpause contract
    Unpause,
    /// Multisig: propose action
    MultisigPropose(ProposalAction),
    /// Multisig: approve proposal
    MultisigApprove(u32),
}


/// Full strategy for arbitrary valid operation sequences.
pub fn any_test_operation(env: &Env) -> impl Strategy<Value = TestOperation> {
    prop_oneof![
        // Valid register (bps 0-10000)
        any_offering_id(env)
            .prop_map(|(i, ns, t)| TestOperation::RegisterOffering((i, ns, t, (0..=10_000u32).prop_map(|x| x), any::<Address>()))),
        // Valid report (amount >=0, period >0)
        (any_offering_id(env), 0i128.., 1u64.., any::<bool>())
            .prop_map(|((i,ns,t), amt, pid, ovr)| TestOperation::ReportRevenue((i, ns, t, t.clone(), amt, pid, ovr))), // payout_asset=token for simplicity
        // Deposit (amount >0)
        (any_offering_id(env), 1i128.., any::<u64>())
            .prop_map(|((i,ns,t), amt, pid)| TestOperation::DepositRevenue((i, ns, t, t.clone(), amt, pid))),
        // Holder share (bps 0-10000)
        (any_offering_id(env), any::<Address>(), (0..=10_000u32))
            .prop_map(|((i,ns,t), holder, bps)| TestOperation::SetHolderShare((i, ns, t, holder, bps))),
        // Blacklist ops
        any_offering_id(env).prop_map(|(i, ns, t)| TestOperation::BlacklistAdd((i, ns, t, any::<Address>()))),
        any_offering_id(env).prop_map(|(i, ns, t)| TestOperation::BlacklistRemove((i, ns, t, any::<Address>()))),
        // Concentration
        any_offering_id(env).prop_map(|(i, ns, t)| TestOperation::SetConcentrationLimit((i, ns, t, (0..=10_000u32), any::<bool>()))),
        any_offering_id(env).prop_map(|(i, ns, t)| TestOperation::ReportConcentration((i, ns, t, (0..=10_000u32)))),
        // Pause state transitions
        0.1.prop_map(|_| TestOperation::Pause).boxed(),
        0.1.prop_map(|_| TestOperation::Unpause).boxed(),
        // Multisig (simplified)
        0.05.prop_map(|_| TestOperation::MultisigPropose(any::<ProposalAction>())).boxed(),
        0.05.prop_map(|id| TestOperation::MultisigApprove(id)).boxed(),
    ]
}

/// Strategy for sequences that preserve key invariants (e.g. period ordering).
pub fn arb_valid_operation_sequence(env: &Env, length: usize) -> impl Strategy<Value = Vec<TestOperation>> {
    prop::collection::vec(any_test_operation(env), length..=length)
        .prop_filter(
            "valid sequences only (period ordering etc)",
            |seq| validate_sequence_preserves_invariants(env, seq),
        )
}

#[cfg(test)]
fn validate_sequence_preserves_invariants(env: &Env, seq: &[TestOperation]) -> bool {
    // Placeholder: implement full validator
    true
}


