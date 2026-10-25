#![cfg(test)]
use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env,
};

// ── Test helpers ──────────────────────────────────────────────────────────────

fn setup_test() -> (Env, RevoraRevenueShareClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    (env, client, admin)
}

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Symbol, Address) {
    let (env, client, issuer) = setup_test();
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let namespace = symbol_short!("ns");

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &5000,
        &payout_asset,
        &0,
        &symbol_short!("USD"),
        &2,
    );

    (env, client, issuer, namespace, token)
}

// ── Tests: Basic Queue Operations ────────────────────────────────────────────

#[test]
fn enqueue_deferred_assigns_incrementing_queue_id() {
    let (env, client, issuer, namespace, token) = setup_offering();

    // Enqueue entries with same priority and timestamp
    let queue_id_1 = client.enqueue_deferred(&issuer, &namespace, &token, &1000, &1, &101);
    let queue_id_2 = client.enqueue_deferred(&issuer, &namespace, &token, &1000, &1, &102);
    let queue_id_3 = client.enqueue_deferred(&issuer, &namespace, &token, &1000, &1, &103);

    assert_eq!(queue_id_1, 0);
    assert_eq!(queue_id_2, 1);
    assert_eq!(queue_id_3, 2);

    let queue = client.get_deferred_queue(&issuer, &namespace, &token);
    assert_eq!(queue.len(), 3);
}

#[test]
fn get_deferred_queue_returns_empty_vec_before_any_enqueues() {
    let (_env, client, issuer, namespace, token) = setup_offering();

    let queue = client.get_deferred_queue(&issuer, &namespace, &token);
    assert!(queue.is_empty());
}

#[test]
fn enqueue_deferred_emits_deferred_priority_set_event() {
    let (env, client, issuer, namespace, token) = setup_offering();

    let before = env.events().all().len();
    let queue_id = client.enqueue_deferred(&issuer, &namespace, &token, &2000, &5, &42);

    let events = env.events().all();
    assert!(events.len() > before, "expected at least one new event");

    // Find the deferred_priority_set event by topic
    let def_pset_event = events.iter().rev().find(|(_contract_id, topics_val, _data)| {
        let topics: soroban_sdk::Vec<soroban_sdk::Val> = topics_val.clone().into_val(&env);
        if let Some(first_topic) = topics.get(0).map(|v| v.into_val(&env)) {
            let sym: Symbol = first_topic;
            sym == EVENT_DEFERRED_PRIORITY_SET
        } else {
            false
        }
    });

    assert!(def_pset_event.is_some(), "deferred_priority_set event not found");

    // Verify event payload: (queue_id, release_ts, priority, payload_id)
    let (_contract_id, _topics, data) = def_pset_event.unwrap();
    let payload: (u32, u64, u32, u64) = data.into_val(&env);
    assert_eq!(payload, (queue_id, 2000, 5, 42));
}

// ── Tests: Priority Ordering ─────────────────────────────────────────────────

#[test]
fn entries_sorted_by_release_ts_ascending() {
    let (_env, client, issuer, namespace, token) = setup_offering();

    // Enqueue entries with different release timestamps
    client.enqueue_deferred(&issuer, &namespace, &token, &3000, &0, &3); // ts=3000
    client.enqueue_deferred(&issuer, &namespace, &token, &1000, &0, &1); // ts=1000
    client.enqueue_deferred(&issuer, &namespace, &token, &2000, &0, &2); // ts=2000

    let queue = client.get_deferred_queue(&issuer, &namespace, &token);

    assert_eq!(queue.len(), 3);
    assert_eq!(queue.get(0).unwrap().release_ts, 1000);
    assert_eq!(queue.get(1).unwrap().release_ts, 2000);
    assert_eq!(queue.get(2).unwrap().release_ts, 3000);
}

#[test]
fn entries_with_same_release_ts_sorted_by_priority_ascending() {
    let (_env, client, issuer, namespace, token) = setup_offering();

    // Enqueue entries with same release_ts, different priorities (lower priority = higher urgency)
    client.enqueue_deferred(&issuer, &namespace, &token, &5000, &10, &103); // priority=10
    client.enqueue_deferred(&issuer, &namespace, &token, &5000, &5, &102); // priority=5
    client.enqueue_deferred(&issuer, &namespace, &token, &5000, &0, &101); // priority=0 (highest)

    let queue = client.get_deferred_queue(&issuer, &namespace, &token);

    assert_eq!(queue.len(), 3);
    // Priority 0 comes first
    assert_eq!(queue.get(0).unwrap().priority, 0);
    assert_eq!(queue.get(0).unwrap().payload_id, 101);
    // Then priority 5
    assert_eq!(queue.get(1).unwrap().priority, 5);
    assert_eq!(queue.get(1).unwrap().payload_id, 102);
    // Then priority 10
    assert_eq!(queue.get(2).unwrap().priority, 10);
    assert_eq!(queue.get(2).unwrap().payload_id, 103);
}

#[test]
fn entries_with_same_release_ts_and_priority_sorted_by_queue_id_ascending() {
    let (_env, client, issuer, namespace, token) = setup_offering();

    // Enqueue three entries with identical release_ts and priority
    client.enqueue_deferred(&issuer, &namespace, &token, &7000, &1, &201); // queue_id=0
    client.enqueue_deferred(&issuer, &namespace, &token, &7000, &1, &202); // queue_id=1
    client.enqueue_deferred(&issuer, &namespace, &token, &7000, &1, &203); // queue_id=2

    let queue = client.get_deferred_queue(&issuer, &namespace, &token);

    assert_eq!(queue.len(), 3);
    // Verify queue_id order (tie-breaker)
    assert_eq!(queue.get(0).unwrap().queue_id, 0);
    assert_eq!(queue.get(0).unwrap().payload_id, 201);
    assert_eq!(queue.get(1).unwrap().queue_id, 1);
    assert_eq!(queue.get(1).unwrap().payload_id, 202);
    assert_eq!(queue.get(2).unwrap().queue_id, 2);
    assert_eq!(queue.get(2).unwrap().payload_id, 203);
}

#[test]
fn complex_multi_key_sorting_is_deterministic() {
    let (_env, client, issuer, namespace, token) = setup_offering();

    // Enqueue entries with mixed (release_ts, priority, queue_id) ordering
    // Expected final order: (1000,0,_), (1000,1,_), (2000,0,_), (2000,5,_), (3000,0,_)
    client.enqueue_deferred(&issuer, &namespace, &token, &2000, &5, &401); // queue_id=0
    client.enqueue_deferred(&issuer, &namespace, &token, &1000, &1, &402); // queue_id=1
    client.enqueue_deferred(&issuer, &namespace, &token, &3000, &0, &403); // queue_id=2
    client.enqueue_deferred(&issuer, &namespace, &token, &1000, &0, &404); // queue_id=3
    client.enqueue_deferred(&issuer, &namespace, &token, &2000, &0, &405); // queue_id=4

    let queue = client.get_deferred_queue(&issuer, &namespace, &token);

    assert_eq!(queue.len(), 5);

    // Verify sorted order: (release_ts, priority, queue_id) ascending
    assert_eq!(
        (
            queue.get(0).unwrap().release_ts,
            queue.get(0).unwrap().priority,
            queue.get(0).unwrap().queue_id
        ),
        (1000, 0, 3) // (1000, 0, _) - queue_id=3 from payload 404
    );
    assert_eq!(
        (
            queue.get(1).unwrap().release_ts,
            queue.get(1).unwrap().priority,
            queue.get(1).unwrap().queue_id
        ),
        (1000, 1, 1) // (1000, 1, _) - queue_id=1 from payload 402
    );
    assert_eq!(
        (
            queue.get(2).unwrap().release_ts,
            queue.get(2).unwrap().priority,
            queue.get(2).unwrap().queue_id
        ),
        (2000, 0, 4) // (2000, 0, _) - queue_id=4 from payload 405
    );
    assert_eq!(
        (
            queue.get(3).unwrap().release_ts,
            queue.get(3).unwrap().priority,
            queue.get(3).unwrap().queue_id
        ),
        (2000, 5, 0) // (2000, 5, _) - queue_id=0 from payload 401
    );
    assert_eq!(
        (
            queue.get(4).unwrap().release_ts,
            queue.get(4).unwrap().priority,
            queue.get(4).unwrap().queue_id
        ),
        (3000, 0, 2) // (3000, 0, _) - queue_id=2 from payload 403
    );
}

// ── Tests: Edge Cases ─────────────────────────────────────────────────────────

#[test]
fn zero_priority_is_highest_urgency() {
    let (_env, client, issuer, namespace, token) = setup_offering();

    // Enqueue with priority 0, 10, 5
    client.enqueue_deferred(&issuer, &namespace, &token, &1000, &10, &1);
    client.enqueue_deferred(&issuer, &namespace, &token, &1000, &0, &2);
    client.enqueue_deferred(&issuer, &namespace, &token, &1000, &5, &3);

    let queue = client.get_deferred_queue(&issuer, &namespace, &token);

    // Priority 0 should be first (highest urgency)
    assert_eq!(queue.get(0).unwrap().priority, 0);
    assert_eq!(queue.get(0).unwrap().payload_id, 2);
}

#[test]
fn large_priority_values_sort_correctly() {
    let (_env, client, issuer, namespace, token) = setup_offering();

    // Test with large u32 values close to max
    let max_u32 = u32::MAX;
    client.enqueue_deferred(&issuer, &namespace, &token, &1000, &(max_u32 - 1), &1);
    client.enqueue_deferred(&issuer, &namespace, &token, &1000, &max_u32, &2);
    client.enqueue_deferred(&issuer, &namespace, &token, &1000, &(max_u32 - 2), &3);

    let queue = client.get_deferred_queue(&issuer, &namespace, &token);

    assert_eq!(queue.get(0).unwrap().priority, max_u32 - 2);
    assert_eq!(queue.get(1).unwrap().priority, max_u32 - 1);
    assert_eq!(queue.get(2).unwrap().priority, max_u32);
}

#[test]
fn large_release_timestamps_sort_correctly() {
    let (_env, client, issuer, namespace, token) = setup_offering();

    // Test with timestamps near u64::MAX
    let max_u64 = u64::MAX;
    client.enqueue_deferred(&issuer, &namespace, &token, &max_u64, &0, &1);
    client.enqueue_deferred(&issuer, &namespace, &token, &(max_u64 - 1000), &0, &2);
    client.enqueue_deferred(&issuer, &namespace, &token, &(max_u64 - 1), &0, &3);

    let queue = client.get_deferred_queue(&issuer, &namespace, &token);

    assert_eq!(queue.get(0).unwrap().release_ts, max_u64 - 1000);
    assert_eq!(queue.get(1).unwrap().release_ts, max_u64 - 1);
    assert_eq!(queue.get(2).unwrap().release_ts, max_u64);
}

#[test]
fn single_entry_queue_returns_correctly() {
    let (_env, client, issuer, namespace, token) = setup_offering();

    client.enqueue_deferred(&issuer, &namespace, &token, &9999, &42, &555);

    let queue = client.get_deferred_queue(&issuer, &namespace, &token);

    assert_eq!(queue.len(), 1);
    let entry = queue.get(0).unwrap();
    assert_eq!(entry.release_ts, 9999);
    assert_eq!(entry.priority, 42);
    assert_eq!(entry.queue_id, 0);
    assert_eq!(entry.payload_id, 555);
}

// ── Tests: Authorization & Security ───────────────────────────────────────────

#[test]
fn enqueue_deferred_requires_issuer_auth() {
    let (env, client, issuer, namespace, token) = setup_offering();
    let attacker = Address::generate(&env);

    // Attempt to enqueue as an unauthorized address
    let result = client.try_enqueue_deferred(&attacker, &namespace, &token, &1000, &1, &1);

    // Expect OfferingNotFound (issuer.require_auth() passes mock, but issuer lookup fails)
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn enqueue_deferred_rejects_for_unknown_offering() {
    let (env, client, _issuer, _namespace, _token) = setup_offering();
    let attacker = Address::generate(&env);
    let unknown_token = Address::generate(&env);
    let unknown_namespace = symbol_short!("xxx");

    let result =
        client.try_enqueue_deferred(&attacker, &unknown_namespace, &unknown_token, &1000, &1, &1);

    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn get_deferred_queue_does_not_require_auth() {
    let (_env, client, issuer, namespace, token) = setup_offering();

    // get_deferred_queue is read-only and should not require auth
    let queue = client.get_deferred_queue(&issuer, &namespace, &token);
    assert!(queue.is_empty());
}

// ── Tests: Multi-Offering Isolation ──────────────────────────────────────────

#[test]
fn queues_are_isolated_per_offering() {
    let (env, client, issuer1, namespace1, token1) = setup_offering();

    // Create a second offering
    let issuer2 = Address::generate(&env);
    let token2 = Address::generate(&env);
    let namespace2 = symbol_short!("ns2");
    let payout_asset2 = Address::generate(&env);
    client.register_offering(
        &issuer2,
        &Vec::new(&env),
        &1u32,
        &namespace2,
        &token2,
        &3000,
        &payout_asset2,
        &0,
        &symbol_short!("EUR"),
        &2,
    );

    // Enqueue entries in each offering
    client.enqueue_deferred(&issuer1, &namespace1, &token1, &100, &1, &11);
    client.enqueue_deferred(&issuer1, &namespace1, &token1, &200, &2, &12);

    client.enqueue_deferred(&issuer2, &namespace2, &token2, &300, &3, &21);

    // Verify queues are independent
    let queue1 = client.get_deferred_queue(&issuer1, &namespace1, &token1);
    let queue2 = client.get_deferred_queue(&issuer2, &namespace2, &token2);

    assert_eq!(queue1.len(), 2);
    assert_eq!(queue2.len(), 1);

    assert_eq!(queue1.get(0).unwrap().payload_id, 11);
    assert_eq!(queue1.get(1).unwrap().payload_id, 12);

    assert_eq!(queue2.get(0).unwrap().payload_id, 21);
}

// ── Tests: Stress & Performance ──────────────────────────────────────────────

#[test]
fn large_queue_maintains_correct_order() {
    let (_env, client, issuer, namespace, token) = setup_offering();

    // Enqueue 50 entries with varying timestamps and priorities
    for i in 0u32..50u32 {
        let timestamp: u64 = 1000 + (i as u64 % 10) * 100; // 10 distinct timestamps
        let priority: u32 = i % 5; // 5 distinct priorities
        client.enqueue_deferred(
            &issuer,
            &namespace,
            &token,
            &timestamp,
            &priority,
            &(1000 + i as u64),
        );
    }

    let queue = client.get_deferred_queue(&issuer, &namespace, &token);
    assert_eq!(queue.len(), 50);

    // Verify sorted order: for each adjacent pair, (ts, priority, queue_id) must be ascending
    for i in 0..(queue.len() - 1) {
        let current = queue.get(i).unwrap();
        let next = queue.get(i + 1).unwrap();

        let current_key = (current.release_ts, current.priority, current.queue_id);
        let next_key = (next.release_ts, next.priority, next.queue_id);

        assert!(
            current_key <= next_key,
            "queue order violated at index {}: {:?} should be <= {:?}",
            i,
            current_key,
            next_key
        );
    }
}

// ── Tests: Queue Release & Lifecycle ──────────────────────────────────────────

#[test]
fn enqueue_3_entries_advance_time_release_returns_in_order_and_queue_shrinks() {
    let (env, client, issuer, namespace, token) = setup_offering();
    env.ledger().set_timestamp(1_000);

    // Enqueue 3 entries with release timestamps 1500, 2000, 2500 and distinct priorities
    let q1 = client.enqueue_deferred(&issuer, &namespace, &token, &2000, &5, &102);
    let q2 = client.enqueue_deferred(&issuer, &namespace, &token, &1500, &1, &101);
    let q3 = client.enqueue_deferred(&issuer, &namespace, &token, &2500, &0, &103);

    // Initial queue has 3 entries
    let queue_initial = client.get_deferred_queue(&issuer, &namespace, &token);
    assert_eq!(queue_initial.len(), 3);
    // Verified sorted order: q2 (1500), q1 (2000), q3 (2500)
    assert_eq!(queue_initial.get(0).unwrap().queue_id, q2);
    assert_eq!(queue_initial.get(1).unwrap().queue_id, q1);
    assert_eq!(queue_initial.get(2).unwrap().queue_id, q3);

    // Advance time to 2100 (matures q2 and q1, but not q3)
    env.ledger().set_timestamp(2_100);

    // Release up to 10 entries
    let released = client.release_deferred(&issuer, &namespace, &token, &10);
    assert_eq!(released.len(), 2);
    assert_eq!(released.get(0).unwrap().queue_id, q2);
    assert_eq!(released.get(0).unwrap().release_ts, 1500);
    assert_eq!(released.get(0).unwrap().payload_id, 101);

    assert_eq!(released.get(1).unwrap().queue_id, q1);
    assert_eq!(released.get(1).unwrap().release_ts, 2000);
    assert_eq!(released.get(1).unwrap().payload_id, 102);

    // Queue has shrunk to 1 entry (q3)
    let queue_shrunk = client.get_deferred_queue(&issuer, &namespace, &token);
    assert_eq!(queue_shrunk.len(), 1);
    assert_eq!(queue_shrunk.get(0).unwrap().queue_id, q3);
    assert_eq!(queue_shrunk.get(0).unwrap().payload_id, 103);

    // Advance time past q3 (ts = 3000) and release
    env.ledger().set_timestamp(3_000);
    let released_final = client.release_deferred(&issuer, &namespace, &token, &10);
    assert_eq!(released_final.len(), 1);
    assert_eq!(released_final.get(0).unwrap().queue_id, q3);

    // Queue is now empty
    let queue_empty = client.get_deferred_queue(&issuer, &namespace, &token);
    assert!(queue_empty.is_empty());
}

#[test]
fn queue_ids_remain_unique_after_releases() {
    let (env, client, issuer, namespace, token) = setup_offering();
    env.ledger().set_timestamp(1_000);

    // Enqueue 2 entries: queue_id must be 0 and 1
    let q0 = client.enqueue_deferred(&issuer, &namespace, &token, &1200, &0, &1);
    let q1 = client.enqueue_deferred(&issuer, &namespace, &token, &1300, &0, &2);
    assert_eq!(q0, 0);
    assert_eq!(q1, 1);

    // Advance time past both entries and release them
    env.ledger().set_timestamp(1_500);
    let released = client.release_deferred(&issuer, &namespace, &token, &10);
    assert_eq!(released.len(), 2);

    // Queue is now empty
    let queue = client.get_deferred_queue(&issuer, &namespace, &token);
    assert!(queue.is_empty());

    // Enqueue a 3rd entry into the now-empty queue: queue_id must be 2, NOT 0
    let q2 = client.enqueue_deferred(&issuer, &namespace, &token, &1600, &0, &3);
    assert_eq!(q2, 2);

    // Enqueue a 4th entry: queue_id must be 3, NOT 1
    let q3 = client.enqueue_deferred(&issuer, &namespace, &token, &1700, &0, &4);
    assert_eq!(q3, 3);
}

#[test]
fn enqueue_beyond_cap_returns_limit_reached() {
    let (_env, client, issuer, namespace, token) = setup_offering();

    // Fill queue to MAX_DEFERRED_QUEUE_LEN
    for i in 0..MAX_DEFERRED_QUEUE_LEN {
        client.enqueue_deferred(&issuer, &namespace, &token, &(1000 + i as u64), &0, &(i as u64));
    }

    let queue = client.get_deferred_queue(&issuer, &namespace, &token);
    assert_eq!(queue.len(), MAX_DEFERRED_QUEUE_LEN);

    // Enqueue beyond cap must fail with LimitReached
    let result = client.try_enqueue_deferred(&issuer, &namespace, &token, &99999, &0, &999);
    assert_eq!(result, Err(Ok(RevoraError::LimitReached)));
}

#[test]
fn release_before_maturity_is_noop() {
    let (env, client, issuer, namespace, token) = setup_offering();
    env.ledger().set_timestamp(500);

    // Enqueue entries with release timestamps strictly in the future (1000, 2000)
    client.enqueue_deferred(&issuer, &namespace, &token, &1000, &1, &201);
    client.enqueue_deferred(&issuer, &namespace, &token, &2000, &2, &202);

    // Calling release_deferred before maturity (timestamp 500 < 1000)
    let released = client.release_deferred(&issuer, &namespace, &token, &10);
    assert!(released.is_empty());

    // Queue must remain untouched
    let queue = client.get_deferred_queue(&issuer, &namespace, &token);
    assert_eq!(queue.len(), 2);
}

#[test]
fn release_deferred_emits_event_and_respects_max_cap() {
    let (env, client, issuer, namespace, token) = setup_offering();
    env.ledger().set_timestamp(100);

    // Enqueue 3 entries with the same release timestamp
    let q0 = client.enqueue_deferred(&issuer, &namespace, &token, &500, &0, &301);
    let q1 = client.enqueue_deferred(&issuer, &namespace, &token, &500, &1, &302);
    let _q2 = client.enqueue_deferred(&issuer, &namespace, &token, &500, &2, &303);

    // Advance time to 600
    env.ledger().set_timestamp(600);

    let before_events = env.events().all().len();

    // Release with max = 2
    let released = client.release_deferred(&issuer, &namespace, &token, &2);
    assert_eq!(released.len(), 2);
    assert_eq!(released.get(0).unwrap().queue_id, q0);
    assert_eq!(released.get(1).unwrap().queue_id, q1);

    // 1 entry remains in queue
    let remaining = client.get_deferred_queue(&issuer, &namespace, &token);
    assert_eq!(remaining.len(), 1);

    // Check emitted EVENT_DEFERRED_RELEASED events
    let events = env.events().all();
    assert!(events.len() >= before_events + 2);

    let def_rel_events: std::vec::Vec<_> = events
        .iter()
        .filter(|(_cid, topics_val, _data)| {
            let topics: soroban_sdk::Vec<soroban_sdk::Val> = topics_val.clone().into_val(&env);
            if let Some(first_topic) = topics.get(0).map(|v| v.into_val(&env)) {
                let sym: Symbol = first_topic;
                sym == EVENT_DEFERRED_RELEASED
            } else {
                false
            }
        })
        .collect();

    assert_eq!(def_rel_events.len(), 2);
}

#[test]
fn release_deferred_requires_issuer_auth() {
    let (env, client, _issuer, namespace, token) = setup_offering();
    let attacker = Address::generate(&env);

    let result = client.try_release_deferred(&attacker, &namespace, &token, &10);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}
