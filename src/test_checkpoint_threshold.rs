//! Adversarial coverage for `set_checkpoint_threshold` (#1127).
//!
//! ```text
//! set_checkpoint_threshold(env, issuer, namespace, token, threshold)
//! ```
//!
//! The setter persists `threshold: u32` under
//! `DataKey2::CheckpointThreshold(OfferingId { issuer, namespace, token })` and
//! announces the transition with the `chk_pt` v2 event carrying
//! `(EVENT_SCHEMA_VERSION_V2, (previous, threshold))`.
//!
//! Its authorization and state surface is deliberately narrow, so the
//! adversarial cases are:
//!
//! 1. **Keying / isolation** - the record is scoped by the *whole*
//!    `(issuer, namespace, token)` triple. Writing one offering must never
//!    bleed into a sibling namespace, token, issuer, or contract instance, and
//!    a write for an unregistered triple must not create a phantom record.
//! 2. **Boundary values** - `threshold` is an unvalidated `u32`, so `0`
//!    (documented as "compression disabled"), `1` (lowest compressing value)
//!    and `u32::MAX` (the saturating edge) must all round-trip verbatim and
//!    must never be clamped, wrapped, or silently replaced by the default.
//! 3. **Event fidelity** - every accepted call emits exactly one `chk_pt`
//!    event whose `previous` is the value read *before* the write (the
//!    documented `CHECKPOINT_THRESHOLD_DEFAULT` when unset), including the
//!    no-op `previous == threshold` case. Rejected calls emit nothing.
//! 4. **Authorization** - a non-primary address, a co-issuer (which is a
//!    quorum member but not the primary) and an unknown issuer are all
//!    rejected with `OfferingNotFound`; an issuer that does not sign the
//!    invocation fails the quorum gate; the global admin is not implicitly an
//!    issuer.
//! 5. **Rejection isolation** - every rejected call must leave the previously
//!    persisted threshold byte-for-byte intact and must not publish `chk_pt`.
//! 6. **Pre-conditions** - the global freeze short-circuits the write with
//!    `ContractFrozen`; a stored layout version newer than this binary is
//!    rejected with `MigrationDowngradeNotAllowed`.
//!
//! Test matrix (issue #1127):
//!
//! | Case                                         | Expected outcome                     |
//! |--------------------------------------------|--------------------------------------|
//! | offering registered, threshold never set     | reads `CHECKPOINT_THRESHOLD_DEFAULT` |
//! | `threshold = 500`                            | stored and read back verbatim        |
//! | `threshold = 0`                              | stored as `0` (not the default)      |
//! | `threshold = 1`                              | stored as `1`                        |
//! | `threshold = u32::MAX`                       | stored as `u32::MAX`                 |
//! | full `u32` domain sweep                      | every value round-trips verbatim     |
//! | repeated writes                              | last write wins, no accumulation     |
//! | rewrite with the same value                  | accepted, `previous == threshold`    |
//! | sibling namespace / token / issuer           | independent default-`1000` records   |
//! | empty and 9-character namespaces             | independent records                  |
//! | second contract instance                     | independent default-`1000` record    |
//! | unknown namespace / token / issuer           | `OfferingNotFound`, nothing created  |
//! | non-issuer caller                            | `OfferingNotFound`, value intact     |
//! | co-issuer caller                             | `OfferingNotFound`, value intact     |
//! | admin (non-issuer) caller                    | `OfferingNotFound`                   |
//! | issuer call with no auth entries             | quorum gate fails, value intact      |
//! | 2-of-2 quorum, primary alone signs           | quorum gate fails, value intact      |
//! | 2-of-2 quorum, both issuers sign             | accepted                             |
//! | contract globally frozen                     | `ContractFrozen`, value intact       |
//! | stored layout version newer than this binary | `MigrationDowngradeNotAllowed`       |
//! | contract soft-paused                         | write still accepted                 |
//! | issuer transferred to a new issuer           | new issuer rejected, old record kept |
//! | holder with a pending accrual                | accrual untouched by the write       |

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, MockAuth, MockAuthInvoke},
    token::StellarAssetClient,
    Address, ConversionError, Env, IntoVal, InvokeError, Symbol, TryFromVal, Val, Vec as SdkVec,
};

/// Topic published by `set_checkpoint_threshold` on every accepted write.
const EVENT_CHK_PT: Symbol = symbol_short!("chk_pt");

/// The `data` payload of a `chk_pt` event.
///
/// `emit_v2_event` publishes `(EVENT_SCHEMA_VERSION_V2, data)`, so the logged
/// value decodes as `(schema_version, (previous, new))`.
type ChkPtData = (u32, (u32, u32));

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Raw persistent key that backs `get_checkpoint_threshold` for a triple.
///
/// Reading it directly (instead of only through the getter) lets the tests
/// distinguish "configured to `0`" from "never configured", and proves a
/// rejected call did not create a brand new record.
fn threshold_key(issuer: &Address, namespace: &Symbol, token: &Address) -> DataKey2 {
    DataKey2::CheckpointThreshold(OfferingId {
        issuer: issuer.clone(),
        namespace: namespace.clone(),
        token: token.clone(),
    })
}

/// `Some(stored)` when the offering has an explicit threshold record, `None`
/// when the getter is falling back to `CHECKPOINT_THRESHOLD_DEFAULT`.
fn stored_threshold(
    env: &Env,
    contract: &Address,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) -> Option<u32> {
    let key = threshold_key(issuer, namespace, token);
    env.as_contract(contract, || env.storage().persistent().get(&key))
}

/// Register one offering. `payout_asset` is a plain generated address (not a
/// token contract) so registration performs no transfer and the decimals probe
/// is skipped, keeping the only state under test the threshold record.
fn register_offering(
    env: &Env,
    client: &RevoraRevenueShareClient<'static>,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    payout_asset: &Address,
    display_decimals: u32,
) {
    client.register_offering(
        issuer,
        &SdkVec::new(env),
        &1u32,
        namespace,
        token,
        &1_000u32,
        payout_asset,
        &0i128,
        &symbol_short!(""),
        &display_decimals,
    );
}

/// Default fixture: initialized contract, admin, and one single-issuer offering
/// under the `("def")` namespace. `mock_all_auths` keeps every unrelated call
/// out of the way; tests that probe authorization override it explicitly.
fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    register_offering(&env, &client, &issuer, &symbol_short!("def"), &token, &payout, 0);

    (env, client, admin, issuer, token)
}

/// Collect every `chk_pt` event currently in the ledger log, paired with its
/// decoded `(schema_version, previous, new)` payload.
fn chk_pt_events(env: &Env) -> alloc::vec::Vec<(SdkVec<Val>, ChkPtData)> {
    let topic: Val = EVENT_CHK_PT.into_val(env);
    let mut out = alloc::vec::Vec::new();
    for (_, topics, data) in env.events().all().iter() {
        if !topics.contains(topic) {
            continue;
        }
        let data: ChkPtData = TryFromVal::try_from_val(env, &data)
            .expect("chk_pt data must decode as (u32, u32, u32)");
        out.push((topics.clone(), data));
    }
    out
}

/// Eagerly widen an SDK value to `Val` so `Vec::contains` can infer its element
/// type from the argument.
fn to_val<T: IntoVal<Env, Val>>(env: &Env, value: T) -> Val {
    value.into_val(env)
}

/// Number of `chk_pt` events currently in the log. Used to prove that rejected
/// calls stay silent.
fn chk_pt_event_count(env: &Env) -> usize {
    chk_pt_events(env).len()
}

/// Assert a `try_*` call was rejected by the contract with exactly `expected`.
///
/// The outer `Result` is the host-level outcome, the inner one the typed
/// contract error. Landing on `Err(Ok(_))` is the checked-rejection path; a
/// host error (or a success) means the call failed for some other reason and
/// the panic message below points at it.
fn assert_rejected(
    result: Result<Result<(), ConversionError>, Result<RevoraError, InvokeError>>,
    expected: RevoraError,
) {
    let code = result.unwrap_err().expect("expected a contract error, not a host error");
    assert_eq!(code, expected, "unexpected error code");
}

/// Deploy a second, independent copy of the contract.
fn second_instance(env: &Env) -> RevoraRevenueShareClient<'static> {
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client
}

// ── Defaults and happy paths ──────────────────────────────────────────────────

/// A freshly registered offering has no threshold record at all: the getter
/// reports the documented default and the storage slot is genuinely empty, so
/// "unset" and "explicitly `0`" are distinguishable.
#[test]
fn set_checkpoint_threshold_reads_default_before_any_write() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), CHECKPOINT_THRESHOLD_DEFAULT);
    assert_eq!(CHECKPOINT_THRESHOLD_DEFAULT, 1_000, "documented default must stay stable");
    assert_eq!(
        stored_threshold(&env, &client.address, &issuer, &ns, &token),
        None,
        "no record must exist before the first write"
    );
    assert_eq!(chk_pt_event_count(&env), 0, "reads must not publish chk_pt");
}

/// A representative mid-range value is persisted verbatim and read back.
#[test]
fn set_checkpoint_threshold_persists_value_verbatim() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.set_checkpoint_threshold(&issuer, &ns, &token, &500);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 500);
    assert_eq!(stored_threshold(&env, &client.address, &issuer, &ns, &token), Some(500));
}

/// Successive writes overwrite instead of accumulating: the last write wins.
#[test]
fn set_checkpoint_threshold_last_write_wins() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    for value in [1_234u32, 7u32, 4_096u32] {
        client.set_checkpoint_threshold(&issuer, &ns, &token, &value);
        assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), value);
    }
    assert_eq!(stored_threshold(&env, &client.address, &issuer, &ns, &token), Some(4_096));
}

/// Writing the same value twice is accepted and is observably a no-op in the
/// event payload (`previous == threshold`).
#[test]
fn set_checkpoint_threshold_accepts_idempotent_rewrite() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.set_checkpoint_threshold(&issuer, &ns, &token, &321);
    let before = chk_pt_event_count(&env);
    client.set_checkpoint_threshold(&issuer, &ns, &token, &321);

    let events = chk_pt_events(&env);
    assert_eq!(events.len() - before, 1, "a rewrite still emits exactly one event");
    assert_eq!(
        events.last().unwrap().1,
        (EVENT_SCHEMA_VERSION_V2, (321, 321)),
        "previous must equal the value being rewritten"
    );
    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 321);
}

// ── Boundary values for `threshold` ───────────────────────────────────────────

/// `0` is the documented "disable compression" sentinel. It must be stored as
/// `0` and must not be confused with "unset" (which reads as the default).
#[test]
fn set_checkpoint_threshold_accepts_zero_without_falling_back_to_default() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.set_checkpoint_threshold(&issuer, &ns, &token, &0);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 0);
    assert_eq!(
        stored_threshold(&env, &client.address, &issuer, &ns, &token),
        Some(0),
        "an explicit 0 must be persisted, not treated as absent"
    );

    // Re-enabling compression must announce the previous explicit value (0),
    // not the default, which proves the write path read the stored record.
    client.set_checkpoint_threshold(&issuer, &ns, &token, &64);
    assert_eq!(chk_pt_events(&env).last().unwrap().1, (EVENT_SCHEMA_VERSION_V2, (0, 64)));
}

/// `1` is the lowest value that can actually trigger compression; it must be
/// accepted and must not be rejected as "out of range".
#[test]
fn set_checkpoint_threshold_accepts_smallest_compressing_value() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.set_checkpoint_threshold(&issuer, &ns, &token, &1);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 1);
    assert_eq!(stored_threshold(&env, &client.address, &issuer, &ns, &token), Some(1));
}

/// `u32::MAX` must round-trip without wrapping to a small value, and rewriting
/// it must still report the correct `previous`.
#[test]
fn set_checkpoint_threshold_round_trips_u32_max() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.set_checkpoint_threshold(&issuer, &ns, &token, &u32::MAX);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), u32::MAX);
    assert_eq!(stored_threshold(&env, &client.address, &issuer, &ns, &token), Some(u32::MAX));

    client.set_checkpoint_threshold(&issuer, &ns, &token, &u32::MAX);
    assert_eq!(
        chk_pt_events(&env).last().unwrap().1,
        (EVENT_SCHEMA_VERSION_V2, (u32::MAX, u32::MAX)),
        "no wrap-around in the payload"
    );
}

/// The whole `u32` domain is accepted verbatim - no clamping band, no
/// rejection range - so integrators cannot be surprised by an undocumented
/// ceiling. The sweep deliberately straddles `u16::MAX` and the default.
#[test]
fn set_checkpoint_threshold_accepts_entire_u32_domain() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    let domain = [
        0u32,
        1,
        2,
        99,
        100,
        999,
        1_000,
        1_001,
        65_535,
        65_536,
        u32::MAX.saturating_sub(1),
        u32::MAX,
    ];
    for value in domain {
        client.set_checkpoint_threshold(&issuer, &ns, &token, &value);
        assert_eq!(
            client.get_checkpoint_threshold(&issuer, &ns, &token),
            value,
            "threshold {value} must round-trip verbatim"
        );
    }
    assert_eq!(stored_threshold(&env, &client.address, &issuer, &ns, &token), Some(u32::MAX));
}

/// Walking down from the ceiling and back up must not leave a stale record and
/// must keep the announced `previous` chain consistent.
#[test]
fn set_checkpoint_threshold_recovers_after_saturating_value() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.set_checkpoint_threshold(&issuer, &ns, &token, &u32::MAX);
    client.set_checkpoint_threshold(&issuer, &ns, &token, &5);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 5);
    assert_eq!(stored_threshold(&env, &client.address, &issuer, &ns, &token), Some(5));

    client.set_checkpoint_threshold(&issuer, &ns, &token, &u32::MAX);
    assert_eq!(chk_pt_events(&env).last().unwrap().1, (EVENT_SCHEMA_VERSION_V2, (5, u32::MAX)));
}

// ── Event fidelity ────────────────────────────────────────────────────────────

/// The first write on an offering announces the documented default as
/// `previous`, and the event is indexed with the full offering identity.
#[test]
fn set_checkpoint_threshold_emits_chk_pt_with_default_as_previous() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.set_checkpoint_threshold(&issuer, &ns, &token, &42);

    let events = chk_pt_events(&env);
    assert_eq!(events.len(), 1, "exactly one chk_pt event per accepted call");
    assert_eq!(events[0].1, (EVENT_SCHEMA_VERSION_V2, (CHECKPOINT_THRESHOLD_DEFAULT, 42)));

    let topics = &events[0].0;
    assert_eq!(topics.len(), 4, "topics are (chk_pt, issuer, namespace, token)");
    assert!(topics.contains(to_val(&env, EVENT_CHK_PT)), "topic 0 must be chk_pt");
    assert!(topics.contains(to_val(&env, issuer.clone())), "topic 1 must be the issuer");
    assert!(topics.contains(to_val(&env, ns)), "topic 2 must be the namespace");
    assert!(topics.contains(to_val(&env, token.clone())), "topic 3 must be the token");
}

/// Every accepted transition is announced, in order, with the correct chain of
/// `previous` values.
#[test]
fn set_checkpoint_threshold_chains_previous_values_across_writes() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.set_checkpoint_threshold(&issuer, &ns, &token, &10);
    client.set_checkpoint_threshold(&issuer, &ns, &token, &20);
    client.set_checkpoint_threshold(&issuer, &ns, &token, &0);

    let events = chk_pt_events(&env);
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].1, (EVENT_SCHEMA_VERSION_V2, (CHECKPOINT_THRESHOLD_DEFAULT, 10)));
    assert_eq!(events[1].1, (EVENT_SCHEMA_VERSION_V2, (10, 20)));
    assert_eq!(events[2].1, (EVENT_SCHEMA_VERSION_V2, (20, 0)));
}

// ── Keying / isolation ────────────────────────────────────────────────────────

/// The record is scoped by the full `(issuer, namespace, token)` triple:
/// sibling namespaces and sibling tokens keep the default.
#[test]
fn set_checkpoint_threshold_is_scoped_by_namespace_and_token() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let other_ns = symbol_short!("alt");
    let other_token = Address::generate(&env);
    let payout = Address::generate(&env);
    register_offering(&env, &client, &issuer, &other_ns, &other_token, &payout, 0);
    register_offering(&env, &client, &issuer, &other_ns, &token, &payout, 0);

    client.set_checkpoint_threshold(&issuer, &ns, &token, &77);

    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 77);
    assert_eq!(
        client.get_checkpoint_threshold(&issuer, &other_ns, &token),
        CHECKPOINT_THRESHOLD_DEFAULT
    );
    assert_eq!(
        client.get_checkpoint_threshold(&issuer, &other_ns, &other_token),
        CHECKPOINT_THRESHOLD_DEFAULT
    );
    assert_eq!(
        stored_threshold(&env, &client.address, &issuer, &other_ns, &other_token),
        None,
        "unrelated offerings must not gain a record"
    );
}

/// Two issuers sharing a namespace never observe each other's threshold (no
/// cross-tenant leak in either direction).
#[test]
fn set_checkpoint_threshold_does_not_leak_across_issuers() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    let other_issuer = Address::generate(&env);
    let other_token = Address::generate(&env);
    let payout = Address::generate(&env);
    register_offering(&env, &client, &other_issuer, &ns, &other_token, &payout, 0);

    client.set_checkpoint_threshold(&issuer, &ns, &token, &111);
    client.set_checkpoint_threshold(&other_issuer, &ns, &other_token, &222);

    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 111);
    assert_eq!(client.get_checkpoint_threshold(&other_issuer, &ns, &other_token), 222);
    assert_eq!(
        stored_threshold(&env, &client.address, &other_issuer, &ns, &token),
        None,
        "same token, other tenant"
    );
}

/// A second deployment of the same contract starts empty for the same triple
/// and stays independent in both directions.
#[test]
fn set_checkpoint_threshold_is_isolated_across_contract_instances() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    client.set_checkpoint_threshold(&issuer, &ns, &token, &55);

    let other_client = second_instance(&env);
    let payout = Address::generate(&env);
    register_offering(&env, &other_client, &issuer, &ns, &token, &payout, 0);

    assert_eq!(
        other_client.get_checkpoint_threshold(&issuer, &ns, &token),
        CHECKPOINT_THRESHOLD_DEFAULT
    );
    assert_eq!(
        client.get_checkpoint_threshold(&issuer, &ns, &token),
        55,
        "first instance unchanged"
    );

    other_client.set_checkpoint_threshold(&issuer, &ns, &token, &66);
    assert_eq!(other_client.get_checkpoint_threshold(&issuer, &ns, &token), 66);
    assert_eq!(
        client.get_checkpoint_threshold(&issuer, &ns, &token),
        55,
        "no cross-instance write"
    );
}

/// Extreme but legal `Symbol` namespaces (empty and a full 9-character symbol)
/// key independently from `("def")`. Soroban rejects symbols longer than 9
/// characters outright, so 9 is the real upper bound being probed here.
#[test]
fn set_checkpoint_threshold_is_scoped_by_unusual_symbols() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let empty_ns: Symbol = Symbol::new(&env, "");
    let max_len_ns: Symbol = Symbol::new(&env, "a_b_c_d_e");
    let payout = Address::generate(&env);
    register_offering(&env, &client, &issuer, &empty_ns, &token, &payout, 0);
    register_offering(&env, &client, &issuer, &max_len_ns, &token, &payout, 0);

    client.set_checkpoint_threshold(&issuer, &ns, &token, &11);
    client.set_checkpoint_threshold(&issuer, &empty_ns, &token, &22);
    client.set_checkpoint_threshold(&issuer, &max_len_ns, &token, &33);

    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 11);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &empty_ns, &token), 22);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &max_len_ns, &token), 33);
    assert_eq!(stored_threshold(&env, &client.address, &issuer, &empty_ns, &token), Some(22));
    assert_eq!(stored_threshold(&env, &client.address, &issuer, &max_len_ns, &token), Some(33));
}

// ── Rejected writes: unknown / non-issuer identity ────────────────────────────

/// An unregistered namespace is rejected and leaves the offering's own
/// threshold untouched, without creating a record for the unknown namespace.
#[test]
fn set_checkpoint_threshold_rejects_unregistered_namespace() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    client.set_checkpoint_threshold(&issuer, &ns, &token, &808);

    let unknown_ns = symbol_short!("zzz");
    assert_rejected(
        client.try_set_checkpoint_threshold(&issuer, &unknown_ns, &token, &1),
        RevoraError::OfferingNotFound,
    );

    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 808);
    assert_eq!(
        client.get_checkpoint_threshold(&issuer, &unknown_ns, &token),
        CHECKPOINT_THRESHOLD_DEFAULT
    );
    assert_eq!(stored_threshold(&env, &client.address, &issuer, &unknown_ns, &token), None);
    assert_eq!(chk_pt_event_count(&env), 1, "only the accepted write is announced");
}

/// An unregistered token under the same issuer/namespace is rejected the same
/// way, without disturbing the sibling token's record.
#[test]
fn set_checkpoint_threshold_rejects_unregistered_token() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    client.set_checkpoint_threshold(&issuer, &ns, &token, &909);

    let unknown_token = Address::generate(&env);
    assert_rejected(
        client.try_set_checkpoint_threshold(&issuer, &ns, &unknown_token, &2),
        RevoraError::OfferingNotFound,
    );

    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 909);
    assert_eq!(stored_threshold(&env, &client.address, &issuer, &ns, &unknown_token), None);
    assert_eq!(chk_pt_event_count(&env), 1);
}

/// A brand-new issuer that never registered the offering cannot configure it,
/// even with the correct namespace and token.
#[test]
fn set_checkpoint_threshold_rejects_unknown_issuer() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    client.set_checkpoint_threshold(&issuer, &ns, &token, &707);

    let attacker = Address::generate(&env);
    assert_rejected(
        client.try_set_checkpoint_threshold(&attacker, &ns, &token, &0),
        RevoraError::OfferingNotFound,
    );

    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 707);
    assert_eq!(
        client.get_checkpoint_threshold(&attacker, &ns, &token),
        CHECKPOINT_THRESHOLD_DEFAULT
    );
    assert_eq!(stored_threshold(&env, &client.address, &attacker, &ns, &token), None);
    assert_eq!(chk_pt_event_count(&env), 1, "rejected write must stay silent");
}

/// The global admin is not implicitly an issuer: even with full authorization
/// the admin cannot reconfigure another tenant's threshold.
#[test]
fn set_checkpoint_threshold_rejects_admin_that_is_not_the_issuer() {
    let (env, client, admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    client.set_checkpoint_threshold(&issuer, &ns, &token, &606);

    assert_rejected(
        client.try_set_checkpoint_threshold(&admin, &ns, &token, &0),
        RevoraError::OfferingNotFound,
    );

    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 606);
    assert_eq!(chk_pt_event_count(&env), 1);
}

// ── Rejected writes: multi-issuer offerings ───────────────────────────────────

/// A co-issuer is a quorum member but is *not* the primary issuer, so it cannot
/// set the threshold on its own: the write is rejected and the stored value is
/// preserved.
#[test]
fn set_checkpoint_threshold_rejects_co_issuer_caller() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(&env);
    let co_issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");
    let co = SdkVec::from_array(&env, [co_issuer.clone()]);
    client.register_offering(
        &issuer,
        &co,
        &2u32,
        &ns,
        &token,
        &1_000u32,
        &Address::generate(&env),
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    client.set_checkpoint_threshold(&issuer, &ns, &token, &1234);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 1234);

    assert_rejected(
        client.try_set_checkpoint_threshold(&co_issuer, &ns, &token, &0),
        RevoraError::OfferingNotFound,
    );
    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 1234, "value preserved");
    assert_eq!(chk_pt_event_count(&env), 1, "only the issuer write is announced");
}

// ── Rejected writes: missing authorization ────────────────────────────────────

/// When the issuer does not sign the invocation the quorum gate fails and the
/// persisted threshold is left untouched.
#[test]
fn set_checkpoint_threshold_fails_quorum_without_issuer_authorization() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    client.set_checkpoint_threshold(&issuer, &ns, &token, &1500);
    let events_before = chk_pt_event_count(&env);

    // Drop every authorization entry: the issuer no longer signs the call.
    env.set_auths(&[]);
    let result = client.try_set_checkpoint_threshold(&issuer, &ns, &token, &42);
    assert!(result.is_err(), "an unauthorized issuer must not pass the quorum gate");

    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 1500, "value preserved");
    assert_eq!(chk_pt_event_count(&env), events_before, "rejected write must stay silent");
}

/// A 2-of-2 offering is unwritable when only the primary issuer signs, and
/// writable when both sign. Both outcomes are deterministic.
#[test]
fn set_checkpoint_threshold_enforces_multi_issuer_quorum() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(&env);
    let co_issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");
    let co = SdkVec::from_array(&env, [co_issuer.clone()]);
    client.register_offering(
        &issuer,
        &co,
        &2u32,
        &ns,
        &token,
        &1_000u32,
        &Address::generate(&env),
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    // The authorization frames below name the exact invocation being signed, so
    // a signer can never be counted for a different call than the one made.
    let signed_args = SdkVec::from_array(
        &env,
        [
            to_val(&env, issuer.clone()),
            to_val(&env, ns.clone()),
            to_val(&env, token.clone()),
            to_val(&env, 10u32),
        ],
    );

    // Primary alone does not meet the 2-of-2 quorum.
    let primary_invoke = MockAuthInvoke {
        contract: &contract_id,
        fn_name: "set_checkpoint_threshold",
        args: signed_args.clone(),
        sub_invokes: &[],
    };
    env.mock_auths(&[MockAuth { address: &issuer, invoke: &primary_invoke }]);
    let result = client.try_set_checkpoint_threshold(&issuer, &ns, &token, &10);
    assert!(result.is_err(), "1 of 2 issuers must not meet the quorum");
    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), CHECKPOINT_THRESHOLD_DEFAULT);
    assert_eq!(
        stored_threshold(&env, &client.address, &issuer, &ns, &token),
        None,
        "no record on rejection"
    );

    // Both issuers authorizing meets the quorum.
    let co_invoke = MockAuthInvoke {
        contract: &contract_id,
        fn_name: "set_checkpoint_threshold",
        args: signed_args,
        sub_invokes: &[],
    };
    env.mock_auths(&[
        MockAuth { address: &issuer, invoke: &primary_invoke },
        MockAuth { address: &co_issuer, invoke: &co_invoke },
    ]);
    client.set_checkpoint_threshold(&issuer, &ns, &token, &10);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 10);
    assert_eq!(stored_threshold(&env, &client.address, &issuer, &ns, &token), Some(10));
}

// ── Rejected writes: global pre-conditions ────────────────────────────────────

/// The global freeze short-circuits the setter before any storage write, and
/// the previously configured threshold survives the freeze. The read path keeps
/// serving while frozen.
#[test]
fn set_checkpoint_threshold_rejected_while_contract_frozen() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    client.set_checkpoint_threshold(&issuer, &ns, &token, &250);
    let events_before = chk_pt_event_count(&env);

    client.set_freeze(&FreezeReason::Compliance);
    assert_rejected(
        client.try_set_checkpoint_threshold(&issuer, &ns, &token, &999),
        RevoraError::ContractFrozen,
    );

    assert_eq!(
        client.get_checkpoint_threshold(&issuer, &ns, &token),
        250,
        "read path still serves"
    );
    assert_eq!(
        stored_threshold(&env, &client.address, &issuer, &ns, &token),
        Some(250),
        "value preserved"
    );
    assert_eq!(chk_pt_event_count(&env), events_before, "frozen write must stay silent");
}

/// A layout version newer than this binary blocks the setter, proving the
/// downgrade guard is wired into this entrypoint and not only into migrations.
#[test]
fn set_checkpoint_threshold_rejected_when_layout_version_is_newer() {
    let (env, client, admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    client.set_checkpoint_threshold(&issuer, &ns, &token, &300);
    let events_before = chk_pt_event_count(&env);

    client.set_storage_layout_version(&admin, &STORAGE_LAYOUT_VERSION.saturating_add(1));
    assert_rejected(
        client.try_set_checkpoint_threshold(&issuer, &ns, &token, &301),
        RevoraError::MigrationDowngradeNotAllowed,
    );

    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 300, "value preserved");
    assert_eq!(chk_pt_event_count(&env), events_before);
}

// ── Characterization: documented-scope boundaries ─────────────────────────────

/// A soft pause blocks revenue reporting but is *not* a gate on this setter:
/// the write still succeeds. The global freeze is the real stop switch. This
/// pins the current boundary so that changing it is a deliberate decision.
#[test]
fn set_checkpoint_threshold_remains_available_while_soft_paused() {
    let (env, client, admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.pause_admin(&admin);
    client.set_checkpoint_threshold(&issuer, &ns, &token, &123);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 123);

    client.unpause_admin(&admin);
    client.set_checkpoint_threshold(&issuer, &ns, &token, &124);
    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 124);
    assert_eq!(chk_pt_event_count(&env), 2, "both writes are announced while paused or not");
}

/// `set_checkpoint_threshold` authorizes against the stored offering record
/// rather than the current-issuer index. After an accepted issuer transfer the
/// record is copied to the new issuer key while `issuers.primary` still names
/// the old issuer, so the *new* issuer is refused with `OfferingNotFound` (and
/// never creates a record of its own) while the former issuer still resolves
/// the old record. This documents existing behaviour of the shared
/// offering-lookup path deterministically, including state preservation.
#[test]
fn set_checkpoint_threshold_authorization_follows_stored_offering_record_after_transfer() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    client.set_checkpoint_threshold(&issuer, &ns, &token, &450);

    let new_issuer = Address::generate(&env);
    client.propose_issuer_transfer(&issuer, &ns, &token, &new_issuer);
    client.accept_issuer_transfer(&new_issuer, &ns, &token);

    // The new issuer is not yet the recorded primary: rejected, and no record
    // is created under the new issuer's key.
    assert_rejected(
        client.try_set_checkpoint_threshold(&new_issuer, &ns, &token, &451),
        RevoraError::OfferingNotFound,
    );
    assert_eq!(stored_threshold(&env, &client.address, &new_issuer, &ns, &token), None);
    assert_eq!(
        client.get_checkpoint_threshold(&new_issuer, &ns, &token),
        CHECKPOINT_THRESHOLD_DEFAULT
    );

    // The record written before the transfer is untouched by the rejection.
    assert_eq!(client.get_checkpoint_threshold(&issuer, &ns, &token), 450);
    assert_eq!(chk_pt_event_count(&env), 1, "only the pre-transfer write is announced");
}

/// Reconfiguring the threshold must not disturb the accrual ledger: a holder's
/// already-accrued, not-yet-claimed balance is identical before and after a
/// threshold change, including at the `0` and `u32::MAX` edges.
#[test]
fn set_checkpoint_threshold_preserves_pending_holder_accrual() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    // A real SAC so `deposit_revenue` can move value; display_decimals must
    // agree with the asset's `decimals()` or registration is rejected.
    let asset_admin = Address::generate(&env);
    let payout_asset = env.register_stellar_asset_contract_v2(asset_admin).address();
    StellarAssetClient::new(&env, &payout_asset).mint(&issuer, &1_000_000);
    register_offering(&env, &client, &issuer, &ns, &token, &payout_asset, 7);

    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &ns, &token, &holder, &5_000, &1);
    client.deposit_revenue(&issuer, &ns, &token, &payout_asset, &100_000, &1);

    let accrued = client.get_claimable(&issuer, &ns, &token, &holder);
    assert!(accrued > 0, "5_000 bps of a 100_000 deposit must accrue something");

    for value in [0u32, 1, u32::MAX, 1_000] {
        client.set_checkpoint_threshold(&issuer, &ns, &token, &value);
        assert_eq!(
            client.get_claimable(&issuer, &ns, &token, &holder),
            accrued,
            "threshold {value} must not alter accrued balances"
        );
    }

    assert_eq!(
        client.claim(&holder, &issuer, &ns, &token, &0),
        accrued,
        "the holder still claims exactly what accrued before the rewrites"
    );
}
