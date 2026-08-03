#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use proptest::prelude::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env, Symbol,
};

// ── Constants ────────────────────────────────────────────────────────────────

/// Amounts are multiples of 10 000 so that `amount * share_bps / 10_000` is exact
/// for every share_bps ∈ [0, 10_000], eliminating rounding dust from integer division.
const AMOUNT_UNIT: i128 = 10_000;

// ── Test helpers ─────────────────────────────────────────────────────────────

fn setup_fresh_env() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    // Default ledger timestamp so claim-delay checks pass immediately.
    env.ledger().with_mut(|li| li.timestamp = 1_000_000);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_admin = Address::generate(&env);
    let payout_asset = crate::test_utils::create_token(&env, &payout_admin);
    crate::test_utils::mint_tokens(&env, &payout_asset, &issuer, 10_000_000);

    // Register offering with 0 claim delay → all periods immediately mature.
    client.register_offering(&issuer, &symbol_short!("def"), &token, &10_000, &payout_asset, &0);

    (env, client, issuer, token, payout_asset)
}

// ── Operation types ──────────────────────────────────────────────────────────

/// A single operation in the accrual lifecycle.
///
/// The test harness maintains `sum(current_bps) == 10_000` at all times by
/// rebalancing after every `SetShare`. This guarantees that every deposited
/// unit of revenue is fully allocable to holders, making the reconciliation
/// invariant exact.
#[derive(Debug, Clone)]
enum AccrualOp {
    /// Set a holder's share (bps). The harness rebalances other holders so
    /// total_bps stays at 10_000.
    SetShare(usize, u32),
    /// Deposit revenue. Amount must be a multiple of 10_000.
    Deposit(i128),
    /// Claim up to `max_periods` for a holder.
    Claim(usize, u32),
}

// ── Strategy helpers ─────────────────────────────────────────────────────────

/// Amounts ∈ [10 000, 1 000 000] in steps of 10 000.
fn arb_clean_amount() -> impl Strategy<Value = i128> {
    (1i128..=100i128).prop_map(|n| n * AMOUNT_UNIT)
}

/// Basis points ∈ [0, 10_000].
fn arb_bps() -> impl Strategy<Value = u32> {
    0u32..=10_000u32
}

/// A single accrual operation (holders are 0..holder_count).
fn arb_accrual_op(holder_count: usize) -> impl Strategy<Value = AccrualOp> {
    prop_oneof![
        3 => (0usize..holder_count, arb_bps()).prop_map(|(i, b)| AccrualOp::SetShare(i, b)),
        3 => arb_clean_amount().prop_map(AccrualOp::Deposit),
        2 => (0usize..holder_count, 0u32..=10u32).prop_map(|(i, m)| AccrualOp::Claim(i, m)),
    ]
}

/// Sequence of accrual operations. Periods are assigned in increasing
/// order (1, 2, 3, …) so the contract's period-ordering invariant is satisfied.
fn arb_accrual_sequence(
    holder_count: usize,
    min_len: usize,
    max_len: usize,
) -> impl Strategy<Value = Vec<AccrualOp>> {
    prop::collection::vec(arb_accrual_op(holder_count), min_len..=max_len)
}

// ── Rebalancing helpers ──────────────────────────────────────────────────────

/// After changing `changed_idx`'s share to `new_bps`, rebalance the other
/// holders so that `sum(current_bps) == 10_000`.
///
/// Strategy: give any excess to the first holder that can absorb it, or take
/// from the first holder that has enough. This keeps the operation O(n) and
/// deterministic.
fn rebalance_shares(
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    ns: &Symbol,
    token: &Address,
    holders: &[Address],
    current_bps: &mut [u32],
    changed_idx: usize,
    new_bps: u32,
) {
    let old_bps = current_bps[changed_idx];
    current_bps[changed_idx] = new_bps;
    client.set_holder_share(issuer, ns, token, &holders[changed_idx], &new_bps);

    let delta = (new_bps as i64) - (old_bps as i64);

    if delta == 0 {
        return;
    }

    // Find another holder to compensate (prefer one with enough room).
    for j in 0..holders.len() {
        if j == changed_idx {
            continue;
        }
        let other_share = current_bps[j] as i64;
        let adjusted = other_share.saturating_sub(delta);
        if adjusted >= 0 && adjusted <= 10_000i64 {
            current_bps[j] = adjusted as u32;
            client.set_holder_share(issuer, ns, token, &holders[j], &(adjusted as u32));
            return;
        }
    }

    // Fallback: iterate linearly.  This is deterministic.
    let mut remainder = delta;
    for j in 0..holders.len() {
        if j == changed_idx || remainder == 0 {
            continue;
        }
        let other_share = current_bps[j] as i64;
        if delta > 0 {
            // Need to reduce others; take up to `delta` from this one.
            let take = other_share.min(remainder);
            current_bps[j] = (other_share - take) as u32;
            client.set_holder_share(issuer, ns, token, &holders[j], &current_bps[j]);
            remainder -= take;
        } else {
            // Need to increase others; add up to `-delta`.
            let give = (10_000i64 - other_share).min(-remainder);
            current_bps[j] = (other_share + give) as u32;
            client.set_holder_share(issuer, ns, token, &holders[j], &current_bps[j]);
            remainder += give;
        }
    }
}

// ── Invariant check ──────────────────────────────────────────────────────────

/// Assert the core reconciliation invariant after every operation.
fn check_invariant(
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    ns: &Symbol,
    token: &Address,
    holders: &[Address],
    total_deposited: i128,
    total_claimed_by_holder: &[i128],
    op_label: &str,
) {
    let mut sum_accrued: i128 = 0;
    for holder in holders {
        let accrued = client.get_holder_accrued_unclaimed(issuer, ns, token, holder);
        sum_accrued = sum_accrued.saturating_add(accrued);
    }

    let sum_claimed: i128 = total_claimed_by_holder.iter().sum();
    let lhs = sum_accrued.saturating_add(sum_claimed);

    prop_assert_eq!(
        lhs,
        total_deposited,
        "Invariant violated [{}]: sum(accrued={}) + sum(claimed={}) = {} != total_deposited={}",
        op_label,
        sum_accrued,
        sum_claimed,
        lhs,
        total_deposited,
    );
}

// ── Edge-case tests (standalone, outside proptest) ───────────────────────────

/// Scenario: no holder has any share → deposited revenue stays unallocated.
/// The invariant does NOT hold in this case (only ≈ 0% of revenue is allocable),
/// which is expected and documented.
#[test]
fn edge_no_share_holder_sequence() {
    let (env, client, issuer, token, payout_asset) = setup_fresh_env();
    let ns = symbol_short!("def");

    // Deposit revenue with no holder having a share.
    client.deposit_revenue(&issuer, &ns, &token, &payout_asset, &100_000, &1);

    // No one has a share → nothing is accrued.
    let no_holder = Address::generate(&env);
    let accrued = client.get_holder_accrued_unclaimed(&issuer, &ns, &token, &no_holder);
    assert_eq!(accrued, 0, "no share means no accrual");

    // The deposited revenue (100_000) sits in the accrual index, awaiting
    // distribution when holders eventually get shares.  This is correct
    // contract behaviour — the invariant sum(accrued) + sum(claimed) ==
    // total_revenue only holds when total_shares == 10_000.
    //
    // We cannot directly query DepositedRevenue from outside the contract,
    // but we can confirm it was deposited successfully (no error thrown).
}

/// Scenario: single holder with 100% share → full drain possible.
#[test]
fn edge_single_holder_full_drain() {
    let (env, client, issuer, token, payout_asset) = setup_fresh_env();
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);

    // Single holder gets 100% share.
    client.set_holder_share(&issuer, &ns, &token, &holder, &10_000);

    // Deposit.
    client.deposit_revenue(&issuer, &ns, &token, &payout_asset, &100_000, &1);

    // Pre-claim invariant.
    let pre_accrued = client.get_holder_accrued_unclaimed(&issuer, &ns, &token, &holder);
    assert_eq!(pre_accrued, 100_000, "100% holder gets full amount");

    // Claim everything.
    let payout = client.claim(&holder, &issuer, &ns, &token, &0);
    assert_eq!(payout, 100_000);

    // Post-claim invariant.
    let post_accrued = client.get_holder_accrued_unclaimed(&issuer, &ns, &token, &holder);
    assert_eq!(post_accrued, 0, "nothing left to claim");
}

// ── Proptest ─────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig {
        // At least 512 sequences as required.  Use a deterministic RNG for
        // reproducibility: failing seeds can be re-run directly.
        cases: 512,
        max_local_rng: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_accrual_reconciliation(
        ops in arb_accrual_sequence(4, 12, 25),
    ) {
        // ── Setup ────────────────────────────────────────────────────────
        let (env, client, issuer, token, payout_asset) = setup_fresh_env();
        let ns = symbol_short!("def");

        // Pre-generate 4 holders.
        let holders: Vec<Address> = (0..4).map(|_| Address::generate(&env)).collect();

        // ── Initialise holder shares (total must be exactly 10_000) ──────
        let base_share = 10_000u32 / holders.len() as u32; // 2_500
        let remainder = 10_000u32 - base_share * holders.len() as u32; // 0
        let mut current_bps: Vec<u32> = vec![base_share; holders.len()];
        current_bps[0] = base_share + remainder; // holder 0 gets any rounding dust

        for (i, holder) in holders.iter().enumerate() {
            client.set_holder_share(&issuer, &ns, &token, holder, &current_bps[i]);
        }

        // ── Running state ────────────────────────────────────────────────
        let mut total_deposited: i128 = 0;
        let mut total_claimed_by_holder: Vec<i128> = vec![0; holders.len()];
        let mut next_period_id: u64 = 1;

        // Verify initial invariant (shares are balanced).
        check_invariant(
            &client, &issuer, &ns, &token, &holders,
            total_deposited, &total_claimed_by_holder, "initial",
        );

        // ── Execute sequence ─────────────────────────────────────────────
        for op in &ops {
            match *op {
                AccrualOp::SetShare(idx, bps) => {
                    let h = idx % holders.len();
                    rebalance_shares(
                        &client, &issuer, &ns, &token,
                        &holders, &mut current_bps, h, bps,
                    );

                    check_invariant(
                        &client, &issuer, &ns, &token, &holders,
                        total_deposited, &total_claimed_by_holder,
                        &format!("SetShare({}, {})", h, bps),
                    );
                }
                AccrualOp::Deposit(amount) => {
                    // Advance timestamp so claim-delay checks pass.
                    env.ledger().with_mut(|li| li.timestamp = li.timestamp.saturating_add(1));
                    client.deposit_revenue(
                        &issuer, &ns, &token, &payout_asset, &amount, &next_period_id,
                    );
                    total_deposited = total_deposited.saturating_add(amount);
                    next_period_id += 1;

                    check_invariant(
                        &client, &issuer, &ns, &token, &holders,
                        total_deposited, &total_claimed_by_holder,
                        &format!("Deposit({})", amount),
                    );
                }
                AccrualOp::Claim(idx, max_periods) => {
                    let h = idx % holders.len();
                    env.ledger().with_mut(|li| li.timestamp = li.timestamp.saturating_add(1));
                    let result = client.try_claim(
                        &holders[h], &issuer, &ns, &token, &max_periods,
                    );
                    if let Ok(payout) = result {
                        total_claimed_by_holder[h] =
                            total_claimed_by_holder[h].saturating_add(payout);
                    }

                    check_invariant(
                        &client, &issuer, &ns, &token, &holders,
                        total_deposited, &total_claimed_by_holder,
                        &format!("Claim({}, {})", h, max_periods),
                    );
                }
            }
        }

        // ── Final assertions ─────────────────────────────────────────────
        // Verify total shares never exceed 10_000 after all operations.
        let final_total: u32 = current_bps.iter().sum();
        prop_assert!(
            final_total <= 10_000,
            "total bps {} exceeds 10_000",
            final_total,
        );

        // Final invariant check (redundant but confirms post-sequence state).
        check_invariant(
            &client, &issuer, &ns, &token, &holders,
            total_deposited, &total_claimed_by_holder, "final",
        );
    }
}
