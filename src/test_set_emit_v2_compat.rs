//! # Adversarial coverage for `set_emit_v2_compat` — Issue #1068
//!
//! ## Function under test
//!
//! ```text
//! pub fn set_emit_v2_compat(env: Env, caller: Address, enabled: bool) -> Result<(), RevoraError>
//! ```
//!
//! ### Behaviour summary
//! - Persists `enabled` to `DataKey2::EmitV2Compat` (default is `true` when key absent).
//! - **Auth layer 1** — `caller.require_auth()` (Soroban host; non-catchable panic in WASM).
//! - **Auth layer 2** — typed identity check: `caller` must equal the stored admin;
//!   returns `Err(RevoraError::NotAuthorized)` when it does not.
//! - Returns `Err(RevoraError::NotInitialized)` when no admin has been set yet.
//! - Emits `EVENT_V2_COMPAT_SET` (`"ev_v2c"`) on success.
//! - Has no `require_not_frozen` / `require_not_paused` guard — state-flag changes
//!   must remain possible even when the contract is frozen or paused.
//! - `supported_event_versions` reflects the stored flag: includes `ev_idx2` (version 2)
//!   only when the flag is `true`.
//!
//! ## Coverage map
//!
//! | Category                                    | Test name                                                         |
//! |---------------------------------------------|-------------------------------------------------------------------|
//! | Default true before any call                | `default_is_true_before_set`                                      |
//! | Admin sets false — state stored             | `admin_can_disable`                                               |
//! | Admin sets true — state stored              | `admin_can_enable`                                                |
//! | Toggle false → true                         | `admin_can_re_enable_after_disable`                               |
//! | Toggle true → false → true round-trip       | `set_emit_v2_compat_full_roundtrip`                               |
//! | Idempotent: set true when already true      | `set_same_value_true_is_idempotent`                               |
//! | Idempotent: set false when already false    | `set_same_value_false_is_idempotent`                              |
//! | NotInitialized when no admin set            | `returns_not_initialized_when_no_admin`                           |
//! | NotAuthorized for wrong caller              | `wrong_caller_returns_not_authorized`                             |
//! | NotAuthorized does not mutate state         | `wrong_caller_does_not_mutate_state`                              |
//! | Layer-1 auth (no mock) — ignored in WASM   | `no_auth_mock_panics_layer1` (ignored)                            |
//! | Event emitted on success (enable)           | `emits_event_on_enable`                                           |
//! | Event emitted on success (disable)          | `emits_event_on_disable`                                          |
//! | No event on rejected call                   | `no_event_emitted_on_rejected_call`                               |
//! | supported_event_versions reflects flag      | `supported_event_versions_reflects_flag`                          |
//! | V2 events suppressed when flag false        | `v2_events_suppressed_when_compat_disabled`                       |
//! | V2 events emitted when flag true            | `v2_events_emitted_when_compat_enabled`                           |
//! | V3 events always emitted regardless of flag | `v3_events_always_emitted`                                        |
//! | Frozen contract still accepts flag change   | `flag_can_be_set_when_contract_frozen`                            |
//! | Paused contract still accepts flag change   | `flag_can_be_set_when_contract_paused`                            |
//! | State isolated per contract instance        | `flag_is_isolated_per_contract_instance`                          |

#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Vec,
};

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Deploy a contract and set an admin.  Returns `(client, admin)`.
/// `env.mock_all_auths()` must already be active before calling this.
fn setup_with_admin(env: &Env) -> (RevoraRevenueShareClient<'_>, Address) {
    let client = make_client(env);
    let admin = Address::generate(env);
    client.set_admin(&admin);
    (client, admin)
}

/// Deploy, set admin, register one offering, and return `(client, admin, issuer, token)`.
fn setup_with_offering(
    env: &Env,
) -> (RevoraRevenueShareClient<'_>, Address, Address, Address) {
    let (client, admin) = setup_with_admin(env);
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let payout = Address::generate(env);
    client.register_offering(
        &issuer,
        &Vec::new(env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000u32,
        &payout,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
    (client, admin, issuer, token)
}

/// Find the first event whose first topic symbol equals `topic_sym`,
/// starting from `start_idx`.  Returns the full `(topics, data)` pair.
fn find_event_by_topic(
    env: &Env,
    topic_sym: Symbol,
    start_idx: u32,
) -> Option<(soroban_sdk::Vec<soroban_sdk::Val>, soroban_sdk::Val)> {
    let all = env.events().all();
    for i in start_idx..all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        if !topics.is_empty() {
            let t0: Symbol = topics.get(0).unwrap().into_val(env);
            if t0 == topic_sym {
                return Some((topics, data));
            }
        }
    }
    None
}

// ── 1. Default state ──────────────────────────────────────────────────────────

/// Before any call to `set_emit_v2_compat`, the internal `is_emit_v2_compat`
/// helper defaults to `true`.  We observe this indirectly via
/// `supported_event_versions` — which includes `ev_idx2` only when the flag
/// is `true`.
#[test]
fn default_is_true_before_set() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let versions = client.supported_event_versions();
    // Default = true → ev_idx2 must be present
    let has_v2 = versions.iter().any(|v| v.topic == symbol_short!("ev_idx2"));
    assert!(has_v2, "ev_idx2 must be in supported_event_versions by default (flag defaults true)");

    // V3 is always present
    let has_v3 = versions.iter().any(|v| v.topic == symbol_short!("ev_idx3"));
    assert!(has_v3, "ev_idx3 must always be in supported_event_versions");
}

// ── 2. Happy-path: admin enables / disables ───────────────────────────────────

/// Admin calling `set_emit_v2_compat(false)` must disable V2 emission.
/// The flag is reflected immediately by `supported_event_versions`.
#[test]
fn admin_can_disable() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_admin(&env);

    client.set_emit_v2_compat(&admin, &false);

    let versions = client.supported_event_versions();
    let has_v2 = versions.iter().any(|v| v.topic == symbol_short!("ev_idx2"));
    assert!(!has_v2, "ev_idx2 must NOT be in supported_event_versions after disabling compat");
}

/// Admin calling `set_emit_v2_compat(true)` must keep / restore V2 emission.
#[test]
fn admin_can_enable() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_admin(&env);

    // Explicitly set to true (same as default, but tests the write path).
    client.set_emit_v2_compat(&admin, &true);

    let versions = client.supported_event_versions();
    let has_v2 = versions.iter().any(|v| v.topic == symbol_short!("ev_idx2"));
    assert!(has_v2, "ev_idx2 must be present after explicitly enabling compat");
}

/// Admin can re-enable after disabling — toggle false → true.
#[test]
fn admin_can_re_enable_after_disable() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_admin(&env);

    client.set_emit_v2_compat(&admin, &false);
    // Confirm disabled.
    let v1 = client.supported_event_versions();
    assert!(!v1.iter().any(|v| v.topic == symbol_short!("ev_idx2")));

    client.set_emit_v2_compat(&admin, &true);
    let v2 = client.supported_event_versions();
    assert!(
        v2.iter().any(|v| v.topic == symbol_short!("ev_idx2")),
        "ev_idx2 must return after re-enabling"
    );
}

/// Full round-trip: true (default) → disable → re-enable → disable again.
/// Each intermediate state must be consistent.
#[test]
fn set_emit_v2_compat_full_roundtrip() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_admin(&env);

    // Step 1: disable
    client.set_emit_v2_compat(&admin, &false);
    assert!(!client.supported_event_versions().iter().any(|v| v.topic == symbol_short!("ev_idx2")));

    // Step 2: enable
    client.set_emit_v2_compat(&admin, &true);
    assert!(client.supported_event_versions().iter().any(|v| v.topic == symbol_short!("ev_idx2")));

    // Step 3: disable again
    client.set_emit_v2_compat(&admin, &false);
    assert!(!client.supported_event_versions().iter().any(|v| v.topic == symbol_short!("ev_idx2")));
}

// ── 3. Idempotency ────────────────────────────────────────────────────────────

/// Setting the flag to `true` when it is already `true` must succeed silently.
#[test]
fn set_same_value_true_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_admin(&env);

    let r1 = client.try_set_emit_v2_compat(&admin, &true);
    let r2 = client.try_set_emit_v2_compat(&admin, &true);

    assert!(r1.is_ok(), "first set-true must succeed");
    assert!(r2.is_ok(), "second set-true must also succeed (idempotent)");
    assert!(client.supported_event_versions().iter().any(|v| v.topic == symbol_short!("ev_idx2")));
}

/// Setting the flag to `false` twice must succeed both times and leave the
/// flag disabled.
#[test]
fn set_same_value_false_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_admin(&env);

    let r1 = client.try_set_emit_v2_compat(&admin, &false);
    let r2 = client.try_set_emit_v2_compat(&admin, &false);

    assert!(r1.is_ok(), "first set-false must succeed");
    assert!(r2.is_ok(), "second set-false must succeed (idempotent)");
    assert!(!client.supported_event_versions().iter().any(|v| v.topic == symbol_short!("ev_idx2")));
}

// ── 4. Error paths — uninitialized and wrong caller ──────────────────────────

/// When no admin has been set (contract never initialized), the call must
/// return `NotInitialized`.  State is untouched (flag remains at default true).
#[test]
fn returns_not_initialized_when_no_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env); // no set_admin

    let caller = Address::generate(&env);
    let result = client.try_set_emit_v2_compat(&caller, &false);
    assert_eq!(
        result,
        Err(Ok(RevoraError::NotInitialized)),
        "must return NotInitialized when no admin is set"
    );

    // Default flag (true) must be unchanged — ev_idx2 still present.
    assert!(client.supported_event_versions().iter().any(|v| v.topic == symbol_short!("ev_idx2")));
}

/// A non-admin caller (with mock auth satisfied) must receive `NotAuthorized`.
#[test]
fn wrong_caller_returns_not_authorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup_with_admin(&env);

    let attacker = Address::generate(&env);
    let result = client.try_set_emit_v2_compat(&attacker, &false);
    assert_eq!(
        result,
        Err(Ok(RevoraError::NotAuthorized)),
        "non-admin caller must receive NotAuthorized"
    );
}

/// After a rejected call from a non-admin, the flag must remain unchanged.
#[test]
fn wrong_caller_does_not_mutate_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_admin(&env);

    // Set to false as admin first, so we have a known non-default starting state.
    client.set_emit_v2_compat(&admin, &false);
    assert!(!client.supported_event_versions().iter().any(|v| v.topic == symbol_short!("ev_idx2")));

    // Non-admin tries to re-enable — must be rejected.
    let attacker = Address::generate(&env);
    let _ = client.try_set_emit_v2_compat(&attacker, &true);

    // Flag must still be false.
    assert!(
        !client.supported_event_versions().iter().any(|v| v.topic == symbol_short!("ev_idx2")),
        "flag must remain false after rejected non-admin call"
    );
}

/// Layer-1 auth check (`caller.require_auth()`) without mock_all_auths causes
/// a non-unwinding host panic in no_std WASM.  Cannot be caught by `try_*`.
#[test]
#[ignore = "require_auth causes non-unwinding panic in no_std; cannot be caught by try_* in WASM"]
fn no_auth_mock_panics_layer1() {
    let env = Env::default(); // intentionally no mock_all_auths
    let client = make_client(&env);
    let admin = Address::generate(&env);
    client.set_admin(&admin);

    // This will abort the process in WASM; documents Layer-1 boundary.
    let _ = client.try_set_emit_v2_compat(&admin, &false);
}

// ── 5. Event emission ─────────────────────────────────────────────────────────

/// Successful `set_emit_v2_compat(true)` must emit one `ev_v2c` event.
#[test]
fn emits_event_on_enable() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_admin(&env);

    let before = env.events().all().len() as u32;
    client.set_emit_v2_compat(&admin, &true);

    let ev = find_event_by_topic(&env, symbol_short!("ev_v2c"), before);
    assert!(ev.is_some(), "ev_v2c event must be emitted on set_emit_v2_compat(true)");

    // Verify the data payload carries the boolean value `true`.
    let (_, data) = ev.unwrap();
    let value: bool = data.into_val(&env);
    assert!(value, "event data must carry the enabled=true value");
}

/// Successful `set_emit_v2_compat(false)` must emit one `ev_v2c` event.
#[test]
fn emits_event_on_disable() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_admin(&env);

    let before = env.events().all().len() as u32;
    client.set_emit_v2_compat(&admin, &false);

    let ev = find_event_by_topic(&env, symbol_short!("ev_v2c"), before);
    assert!(ev.is_some(), "ev_v2c event must be emitted on set_emit_v2_compat(false)");

    let (_, data) = ev.unwrap();
    let value: bool = data.into_val(&env);
    assert!(!value, "event data must carry the enabled=false value");
}

/// A rejected call (wrong caller) must not emit any `ev_v2c` event.
#[test]
fn no_event_emitted_on_rejected_call() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup_with_admin(&env);

    let before = env.events().all().len() as u32;
    let attacker = Address::generate(&env);
    let _ = client.try_set_emit_v2_compat(&attacker, &false);

    let ev = find_event_by_topic(&env, symbol_short!("ev_v2c"), before);
    assert!(ev.is_none(), "ev_v2c must NOT be emitted when the call is rejected");
}

// ── 6. supported_event_versions reflects flag ────────────────────────────────

/// `supported_event_versions` must include exactly ev_idx2 (v2) when the flag
/// is true, and exclude it when false.  ev_idx3 (v3) must always be present.
#[test]
fn supported_event_versions_reflects_flag() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_admin(&env);

    // --- flag = true (default) ---
    let versions_on = client.supported_event_versions();
    assert!(
        versions_on.iter().any(|v| v.topic == symbol_short!("ev_idx2") && v.version == 2),
        "flag=true must expose ev_idx2 at version 2"
    );
    assert!(
        versions_on.iter().any(|v| v.topic == symbol_short!("ev_idx3") && v.version == 3),
        "ev_idx3 must always be present"
    );

    // --- flag = false ---
    client.set_emit_v2_compat(&admin, &false);
    let versions_off = client.supported_event_versions();
    assert!(
        !versions_off.iter().any(|v| v.topic == symbol_short!("ev_idx2")),
        "flag=false must remove ev_idx2 from supported_event_versions"
    );
    assert!(
        versions_off.iter().any(|v| v.topic == symbol_short!("ev_idx3")),
        "ev_idx3 must remain present when flag=false"
    );
}

// ── 7. Effect on V2 / V3 event emission ──────────────────────────────────────

/// When the compat flag is `false`, `report_revenue` must NOT emit any
/// `ev_idx2` event.  V3 events must still be emitted.
#[test]
fn v2_events_suppressed_when_compat_disabled() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, issuer, token) = setup_with_offering(&env);
    let payout = Address::generate(&env);

    // Register a second offering to have a valid payout for report_revenue.
    // (Re-use token address; payout is the asset address here.)
    let payout2 = Address::generate(&env);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns2"),
        &token,
        &1_000u32,
        &payout2,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    // Disable V2 compat.
    client.set_emit_v2_compat(&admin, &false);

    let before = env.events().all().len() as u32;
    client.report_revenue(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &payout,
        &10_000i128,
        &1u64,
        &false,
    );

    // No ev_idx2 events after the disable.
    let ev_idx2 = symbol_short!("ev_idx2");
    let all = env.events().all();
    let v2_count = (before..all.len() as u32)
        .filter(|&i| {
            let (_, topics, _) = all.get(i).unwrap();
            if topics.is_empty() {
                return false;
            }
            let t0: Symbol = topics.get(0).unwrap().into_val(&env);
            t0 == ev_idx2
        })
        .count();
    assert_eq!(v2_count, 0, "no ev_idx2 events must be emitted when compat flag is false");

    // But ev_idx3 must be present.
    let ev_idx3 = symbol_short!("ev_idx3");
    let v3_count = (before..all.len() as u32)
        .filter(|&i| {
            let (_, topics, _) = all.get(i).unwrap();
            if topics.is_empty() {
                return false;
            }
            let t0: Symbol = topics.get(0).unwrap().into_val(&env);
            t0 == ev_idx3
        })
        .count();
    assert!(v3_count > 0, "ev_idx3 events must still be emitted when compat flag is false");
}

/// When the compat flag is `true` (default), `report_revenue` must emit
/// at least one `ev_idx2` event alongside the `ev_idx3` event.
#[test]
fn v2_events_emitted_when_compat_enabled() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, issuer, token) = setup_with_offering(&env);
    let payout = Address::generate(&env);

    // Ensure flag is explicitly true.
    client.set_emit_v2_compat(&admin, &true);

    let before = env.events().all().len() as u32;
    client.report_revenue(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &payout,
        &10_000i128,
        &1u64,
        &false,
    );

    let ev_idx2 = symbol_short!("ev_idx2");
    let all = env.events().all();
    let v2_count = (before..all.len() as u32)
        .filter(|&i| {
            let (_, topics, _) = all.get(i).unwrap();
            if topics.is_empty() {
                return false;
            }
            let t0: Symbol = topics.get(0).unwrap().into_val(&env);
            t0 == ev_idx2
        })
        .count();
    assert!(v2_count > 0, "ev_idx2 events must be emitted when compat flag is true");
}

/// V3 events are always emitted regardless of the compat flag value.
#[test]
fn v3_events_always_emitted() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, issuer, token) = setup_with_offering(&env);
    let payout = Address::generate(&env);

    for enabled in [true, false] {
        client.set_emit_v2_compat(&admin, &enabled);

        let before = env.events().all().len() as u32;
        client.report_revenue(
            &issuer,
            &symbol_short!("ns"),
            &token,
            &payout,
            &5_000i128,
            &(if enabled { 1u64 } else { 2u64 }),
            &false,
        );

        let ev_idx3 = symbol_short!("ev_idx3");
        let all = env.events().all();
        let v3_count = (before..all.len() as u32)
            .filter(|&i| {
                let (_, topics, _) = all.get(i).unwrap();
                if topics.is_empty() {
                    return false;
                }
                let t0: Symbol = topics.get(0).unwrap().into_val(&env);
                t0 == ev_idx3
            })
            .count();
        assert!(
            v3_count > 0,
            "ev_idx3 must always be emitted (compat_enabled={})",
            enabled
        );
    }
}

// ── 8. Frozen / paused contract still accepts the flag change ─────────────────

/// A globally frozen contract must still accept `set_emit_v2_compat` because
/// the function has no `require_not_frozen` guard — indexer migration must
/// not be blocked by an operational freeze.
#[test]
fn flag_can_be_set_when_contract_frozen() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_admin(&env);

    // Freeze the contract.
    client.freeze();

    // set_emit_v2_compat must still succeed.
    let result = client.try_set_emit_v2_compat(&admin, &false);
    assert!(
        result.is_ok(),
        "set_emit_v2_compat must succeed even when the contract is globally frozen"
    );
    assert!(!client.supported_event_versions().iter().any(|v| v.topic == symbol_short!("ev_idx2")));
}

/// A paused contract must also accept `set_emit_v2_compat` — no
/// `require_not_paused` guard exists on this function.
#[test]
fn flag_can_be_set_when_contract_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_admin(&env);

    // Soft-pause via admin tier.
    client.pause_admin(&admin);

    let result = client.try_set_emit_v2_compat(&admin, &false);
    assert!(
        result.is_ok(),
        "set_emit_v2_compat must succeed even when the contract is paused"
    );
    assert!(!client.supported_event_versions().iter().any(|v| v.topic == symbol_short!("ev_idx2")));
}

// ── 9. Isolation between contract instances ───────────────────────────────────

/// The flag is stored in per-contract persistent storage.  Changing it on
/// one contract instance must have no effect on a second deployed instance.
#[test]
fn flag_is_isolated_per_contract_instance() {
    let env = Env::default();
    env.mock_all_auths();

    let client_a = make_client(&env);
    let client_b = make_client(&env);

    let admin_a = Address::generate(&env);
    let admin_b = Address::generate(&env);

    client_a.set_admin(&admin_a);
    client_b.set_admin(&admin_b);

    // Disable on contract A.
    client_a.set_emit_v2_compat(&admin_a, &false);

    // Contract B must be unaffected — still at default true.
    let versions_b = client_b.supported_event_versions();
    assert!(
        versions_b.iter().any(|v| v.topic == symbol_short!("ev_idx2")),
        "contract B flag must remain at default true when contract A is disabled"
    );

    // Contract A must be disabled.
    let versions_a = client_a.supported_event_versions();
    assert!(
        !versions_a.iter().any(|v| v.topic == symbol_short!("ev_idx2")),
        "contract A flag must be false after disable"
    );
}
