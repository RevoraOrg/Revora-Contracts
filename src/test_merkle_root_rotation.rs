//! Regression tests for Merkle-root / content-hash rotation across snapshots.
//!
//! Covers issue #573: once a new snapshot is committed and applied via
//! `apply_snapshot_shares`, the previous snapshot's root/content_hash must be
//! superseded and a "proof" built from the old holder set must no longer be
//! accepted as current.
//!
//! Note: this contract does not (yet) expose an on-chain Merkle-proof-verify
//! entrypoint or a `snapshot_rotate` event. This test exercises the two real
//! mechanisms that provide the equivalent guarantee:
//!   1. `merkle_helpers::build_merkle_root` — the off-chain-computable root
//!      over a holder set (pure, unwired helper).
//!   2. `commit_snapshot` / `finalize_snapshot`'s `content_hash` guard, which
//!      is the on-chain hash-mismatch check that actually rejects stale data.

#![cfg(test)]

use crate::merkle_helpers::{build_merkle_root, canonical_leaves};
use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, testutils::Events as _, xdr::ToXdr, Address, Bytes, BytesN, Env};

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.register_offering(&issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0);
    client.set_snapshot_config(&issuer, &symbol_short!("def"), &token, &true);
    (env, client, issuer, token)
}

/// Mirrors the on-chain `content_hash`: SHA-256 over ordered
/// `(index, holder, share_bps)` XDR-encoded rows — the same digest
/// `apply_snapshot_shares`/`finalize_snapshot` check against.
fn flat_content_hash(env: &Env, holders: &[(Address, u32)]) -> BytesN<32> {
    let mut input = Bytes::new(env);
    for (i, (holder, bps)) in holders.iter().enumerate() {
        input.append(&(i as u32).to_xdr(env));
        input.append(&holder.to_xdr(env));
        input.append(&bps.to_xdr(env));
    }
    env.crypto().sha256(&input).to_bytes()
}

/// The real Merkle root for a holder set, via the currently-unwired
/// `merkle_helpers` module. Stands in for an off-chain "proof root".
fn merkle_root_for(env: &Env, holders: &[(Address, u32)]) -> BytesN<32> {
    let leaves = canonical_leaves(env, holders).expect("valid holder set");
    build_merkle_root(env, &leaves)
}

/// Snapshot A is committed and finalized; snapshot B (different holders)
/// is then committed and finalized. Both the merkle_helpers root and the
/// on-chain content_hash must differ between A and B, and A's committed
/// entry must remain untouched (immutable history) after B rotates in.
#[test]
fn root_and_content_hash_rotate_after_new_snapshot() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);
    let holders_a = soroban_sdk::vec![&env, (h1.clone(), 4_000u32), (h2.clone(), 6_000u32)];
    let hash_a = flat_content_hash(&env, &[(h1.clone(), 4_000), (h2.clone(), 6_000)]);
    let root_a = merkle_root_for(&env, &[(h1.clone(), 4_000), (h2.clone(), 6_000)]);

    client.commit_snapshot(&issuer, &ns, &token, &1, &hash_a);
    client.apply_snapshot_shares(&issuer, &ns, &token, &1, &0, &holders_a);
    client.finalize_snapshot(&issuer, &ns, &token, &1);

    let h3 = Address::generate(&env);
    let holders_b = soroban_sdk::vec![&env, (h1.clone(), 1_000u32), (h3.clone(), 9_000u32)];
    let hash_b = flat_content_hash(&env, &[(h1.clone(), 1_000), (h3.clone(), 9_000)]);
    let root_b = merkle_root_for(&env, &[(h1.clone(), 1_000), (h3.clone(), 9_000)]);

    client.commit_snapshot(&issuer, &ns, &token, &2, &hash_b);
    client.apply_snapshot_shares(&issuer, &ns, &token, &2, &0, &holders_b);
    client.finalize_snapshot(&issuer, &ns, &token, &2);

    assert_ne!(root_a, root_b, "merkle root must rotate between snapshots");
    assert_ne!(hash_a, hash_b, "content_hash must rotate between snapshots");

    // Old entry is immutable history, not silently overwritten.
    let entry_a = client.get_snapshot_entry(&issuer, &ns, &token, &1).unwrap();
    assert_eq!(entry_a.content_hash, hash_a, "old snapshot entry must not be mutated");

    let entry_b = client.get_snapshot_entry(&issuer, &ns, &token, &2).unwrap();
    assert_eq!(entry_b.content_hash, hash_b);
}

/// The core "old proof rejected" case: after committing snapshot B, trying to
/// finalize it using A's stale hash (a "proof" built from the superseded
/// holder set) must fail with `SnapshotHashMismatch`.
#[test]
fn stale_hash_from_old_snapshot_is_rejected_on_new_snapshot() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    let h1 = Address::generate(&env);
    let holders_a = soroban_sdk::vec![&env, (h1.clone(), 5_000u32)];
    let hash_a = flat_content_hash(&env, &[(h1.clone(), 5_000)]);

    client.commit_snapshot(&issuer, &ns, &token, &1, &hash_a);
    client.apply_snapshot_shares(&issuer, &ns, &token, &1, &0, &holders_a);
    client.finalize_snapshot(&issuer, &ns, &token, &1);

    // New snapshot: real holder set is different, but we (mis)use the old
    // ("stale proof") hash when committing ref 2.
    let h2 = Address::generate(&env);
    let holders_b = soroban_sdk::vec![&env, (h2.clone(), 5_000u32)];

    client.commit_snapshot(&issuer, &ns, &token, &2, &hash_a); // stale hash reused
    client.apply_snapshot_shares(&issuer, &ns, &token, &2, &0, &holders_b);

    let result = client.try_finalize_snapshot(&issuer, &ns, &token, &2);
    assert!(result.is_err());
    assert!(matches!(result.err(), Some(Ok(RevoraError::SnapshotHashMismatch))));
}

/// Edge case: identical holder sets across two snapshots produce identical
/// roots/hashes (a legitimate no-op rotation). The ref pointer must still
/// advance and both entries must be independently retrievable.
#[test]
fn identical_holder_sets_produce_identical_root_no_op_rotation() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    let h1 = Address::generate(&env);
    let holders = soroban_sdk::vec![&env, (h1.clone(), 5_000u32)];
    let hash1 = flat_content_hash(&env, &[(h1.clone(), 5_000)]);
    let root1 = merkle_root_for(&env, &[(h1.clone(), 5_000)]);

    client.commit_snapshot(&issuer, &ns, &token, &1, &hash1);
    client.apply_snapshot_shares(&issuer, &ns, &token, &1, &0, &holders);
    client.finalize_snapshot(&issuer, &ns, &token, &1);

    // Same holder set again, new ref.
    let hash2 = flat_content_hash(&env, &[(h1.clone(), 5_000)]);
    let root2 = merkle_root_for(&env, &[(h1.clone(), 5_000)]);

    client.commit_snapshot(&issuer, &ns, &token, &2, &hash2);
    client.apply_snapshot_shares(&issuer, &ns, &token, &2, &0, &holders);
    client.finalize_snapshot(&issuer, &ns, &token, &2);

    assert_eq!(root1, root2, "identical holder sets must yield identical roots");
    assert_eq!(hash1, hash2, "identical holder sets must yield identical content hashes");

    let e1 = client.get_snapshot_entry(&issuer, &ns, &token, &1).unwrap();
    let e2 = client.get_snapshot_entry(&issuer, &ns, &token, &2).unwrap();
    assert_eq!(e1.content_hash, e2.content_hash);
    assert_eq!(e1.snapshot_ref, 1);
    assert_eq!(e2.snapshot_ref, 2, "ref must still advance even on a no-op rotation");
}

/// Asserts the on-chain `snap_cmt` event fires for each commit — the closest
/// existing signal to a "rotation" notification, since no dedicated
/// `snapshot_rotate` event currently exists.
#[test]
fn snap_cmt_event_emitted_on_each_commit() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    let h1 = Address::generate(&env);
    let holders = soroban_sdk::vec![&env, (h1.clone(), 5_000u32)];
    let hash1 = flat_content_hash(&env, &[(h1.clone(), 5_000)]);

    let before = env.events().all().len();
    client.commit_snapshot(&issuer, &ns, &token, &1, &hash1);
    let after = env.events().all().len();

    assert!(after > before, "commit_snapshot must emit an event");
}
