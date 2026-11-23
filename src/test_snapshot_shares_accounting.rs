//! Regression + property tests for `apply_snapshot_shares` accounting:
//! `TotalSharesIssued` must equal the sum of live `HolderShare`s (no-cap path),
//! `SnapshotEntry.total_bps` must equal the sum of current slot values, and
//! duplicate holders within a batch are rejected.

#![cfg(test)]

use proptest::prelude::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, BytesN, Env, Symbol, Vec};

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Symbol, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("test");
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns,
        &token,
        &2500,
        &payout,
        &0,
        &symbol_short!(""),
        &0u32,
    );
    client.set_snapshot_config(&issuer, &ns, &token, &true);
    client.commit_snapshot(&issuer, &ns, &token, &1u64, &BytesN::from_array(&env, &[0u8; 32]));
    (env, client, issuer, ns, token)
}

#[test]
fn no_cap_snapshot_total_shares_and_reapply() {
    let (env, client, issuer, ns, token) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let mut batch = Vec::new(&env);
    batch.push_back((a.clone(), 3_000u32));
    batch.push_back((b.clone(), 2_000u32));
    client.apply_snapshot_shares(&issuer, &ns, &token, &1u64, &0u32, &batch);
    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 5_000);

    // Re-apply A (slot 0) with a smaller share.
    let mut again = Vec::new(&env);
    again.push_back((a.clone(), 1_000u32));
    client.apply_snapshot_shares(&issuer, &ns, &token, &1u64, &0u32, &again);
    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 3_000);
}

#[test]
fn reapplying_slot_does_not_inflate_total_bps() {
    let (env, client, issuer, ns, token) = setup();
    let a = Address::generate(&env);
    let mut batch = Vec::new(&env);
    batch.push_back((a.clone(), 3_000u32));
    client.apply_snapshot_shares(&issuer, &ns, &token, &1u64, &0u32, &batch);
    client.apply_snapshot_shares(&issuer, &ns, &token, &1u64, &0u32, &batch);
    let e = client.get_snapshot_entry(&issuer, &ns, &token, &1u64).unwrap();
    assert_eq!(e.total_bps, 3_000);

    let mut smaller = Vec::new(&env);
    smaller.push_back((a, 1_000u32));
    client.apply_snapshot_shares(&issuer, &ns, &token, &1u64, &0u32, &smaller);
    let e = client.get_snapshot_entry(&issuer, &ns, &token, &1u64).unwrap();
    assert_eq!(e.total_bps, 1_000);
}

#[test]
fn duplicate_holder_in_batch_is_rejected() {
    let (env, client, issuer, ns, token) = setup();
    let a = Address::generate(&env);
    let mut batch = Vec::new(&env);
    batch.push_back((a.clone(), 1_000u32));
    batch.push_back((a, 2_000u32));
    assert!(client.try_apply_snapshot_shares(&issuer, &ns, &token, &1u64, &0u32, &batch).is_err());
    assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// After any mix of set_holder_share / apply_snapshot_shares,
    /// TotalSharesIssued == sum(HolderShare) (tracked in a model).
    #[test]
    fn total_shares_issued_matches_sum_of_holder_shares(
        ops in proptest::collection::vec((any::<bool>(), 0usize..4, 0u32..=2_000u32), 1..20)
    ) {
        let (env, client, issuer, ns, token) = setup();
        let holders: std::vec::Vec<Address> = (0..4).map(|_| Address::generate(&env)).collect();
        let mut model = [0u32; 4];
        let mut nonce = 0u64;

        for (use_snapshot, idx, share) in ops {
            let others: u32 = model.iter().enumerate().filter(|(i, _)| *i != idx).map(|(_, s)| *s).sum();
            if others + share > 10_000 { continue; }
            if use_snapshot {
                let mut batch = Vec::new(&env);
                batch.push_back((holders[idx].clone(), share));
                client.apply_snapshot_shares(&issuer, &ns, &token, &1u64, &(idx as u32), &batch);
            } else {
                nonce += 1;
                client.set_holder_share(&issuer, &ns, &token, &holders[idx], &share, &nonce);
            }
            model[idx] = share;
            let sum: u32 = model.iter().sum();
            prop_assert_eq!(client.get_total_shares_issued(&issuer, &ns, &token), sum as i128);
        }
    }
}
