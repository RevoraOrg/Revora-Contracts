# Per-Class Dividend Priority Ordering — Issue #523

This document describes the per-class dividend priority feature added to the
RevoraRevenueShare contract. Preferred classes (e.g. senior / preferred stock)
are paid out before common classes in each close-period distribution cycle.
The order is configurable per `(offering, class)` tuple and is **stable and
deterministic** across reruns and across the single-sig / dual-sig close paths.

## Storage

Two new `DataKey2` variants are added (additive — no migration required):

| Variant                          | Type                              | Purpose                                                  |
|----------------------------------|-----------------------------------|----------------------------------------------------------|
| `ClassPriority(offering, class)` | `u32`                             | Configured priority index for a registered class.        |
| `ClassPayOrder(offering, period)`| `Vec<ShareClass>`                 | Canonical payout order resolved at `close_period` time.  |

`DEFAULT_CLASS_PRIORITY = 0` is the index used for classes that have no
explicit `set_class_priority` call.

## Public API

### `set_class_priority(env, issuer, ns, token, share_class, priority_index)`

Configure or update the priority index for a registered class on an offering.

- **Auth:** `issuer.require_auth()` + the caller must be the offering's
  current issuer (`OfferingNotFound` otherwise).
- **Validation:** the class must already be registered in the offering's
  `DataKey2::OfferingClasses` (`InvalidShareClass` otherwise). This guards
  against storage pollution and DoS via arbitrarily many priority entries.
- **State:** writes `ClassPriority(offering, class) -> priority_index`.
- **Event:** emits `EVENT_CLASS_PRIORITY_SET` with
  `(event_symbol, issuer, namespace, token, share_class)` and `priority_index`
  as the data payload.

### `get_class_priority(env, issuer, ns, token, share_class) -> u32`

Returns the configured priority index, or `DEFAULT_CLASS_PRIORITY = 0` if
no priority has been set.

### `get_class_pay_order(env, issuer, ns, token, period_id) -> Vec<ShareClass>`

Returns the canonical payout order resolved at the previous `close_period` /
`close_period_dual_sig` call. Returns an empty `Vec` for periods that were
never closed via the updated contract.

## Resolution Semantics

When `close_period` (or `close_period_dual_sig`) seals a period, the contract
computes the canonical payout order by:

1. Reading `DataKey2::OfferingClasses(offering_id)` → `Vec<(ShareClass, ClassConfig)>`
2. For each class, reading `DataKey2::ClassPriority(offering_id, class)` (default `0`)
3. Computing each class's XDR-serialized bytes via `ShareClass::to_xdr()`
4. Sorting ascending by `(priority_index, xdr_bytes)`
5. Storing the result in `DataKey2::ClassPayOrder(offering_id, period_id)`
6. Emitting `EVENT_CLASS_PAY_ORDER` with
   `(event_symbol, issuer, namespace, token)` topic and
   `(period_id, ordered_classes)` data payload.

The sort key is `(priority_index, xdr_bytes)`:

- **Primary:** `priority_index` ascending — lower index = paid earlier.
  Set preferred classes (e.g. senior) to a lower index than common classes.
- **Secondary tie-break:** canonical XDR byte order of the `ShareClass`.
  `ShareClass::to_xdr()` produces a deterministic, on-chain-comparable byte
  string. The variant tag is encoded first (so `A < B < Custom(...)`), and
  `Custom(Symbol)` variants tie-break further on the inner Symbol's bytes.
  This stable order is identical across reruns, across hosts, and across
  clients with no further configuration.

### Worked example

Three registered classes with priorities:

- `ShareClass::A`            → priority `1`
- `ShareClass::B`            → priority `0` (preferred — paid first)
- `ShareClass::Custom(Symbol::new(env, "junior"))` → priority `1`

Resolved order (ascending `(priority, xdr_bytes)`):
1. `ShareClass::B`            (priority `0`)
2. `ShareClass::A`            (priority `1`, XDR tag for `A` first)
3. `ShareClass::Custom(junior)` (priority `1`, XDR sorts after `A`)

If `junior`'s priority were lowered to `0`, the tie-break by XDR bytes alone
determines its position relative to `B` (still deterministic across reruns).

## Why this design

1. **Idempotent & deterministic.** The order is computed once per closed period
   and stored. Indexers and off-chain auditors see an identical list on every
   re-run of any close command.
2. **Lightweight.** No distribution math changes — each holder's payout is
   still determined by their per-class share and the period's normalized
   revenue. This change only records the canonical *order* the contract
   intends per-holder shares to be evaluated in a distribution cycle.
3. **Explicit configuration.** Storage is gated — `set_class_priority` only
   accepts already-registered classes, preventing unbounded growth.
4. **Coherent with existing dual-sig path.** Both `close_period` and
   `close_period_dual_sig` route through `record_and_emit_pay_order`, so the
   same canonical order is produced regardless of which close signature path
   is used.

## Security Notes

- **Authorization.** `set_class_priority` requires `issuer.require_auth()` and
  the caller must be the offering's current issuer. The current-issuer check
  defers to `get_current_issuer`, which already incorporates pending issuer
  transfer semantics (post-acceptance transfers update the issuer lookup).
- **DoS surface.** We reject unregistered class IDs so an attacker cannot
  pin storage to a malicious enumerate of unused `ShareClass::Custom("...")`s.
  The per-offering class count remains bounded by what the issuer has previously
  configured via the existing class-config flow.
- **Tie-break determinism.** The XDR-bytes tie-break is fully on-chain and
  corresponds exactly to what the host produces. No external oracle is
  consulted and there is no observable difference across host versions.
- **Event ordering.** `EVENT_CLASS_PAY_ORDER` is emitted *after* the period
  is sealed (`DataKey2::ClosedPeriod`). An indexer that observes a closed
  period must see the matching pay-order event for the same period_id in
  the same transaction — there is no opportunity for a third party to
  interleave writes between seal and pay-order emission.
- **Idempotency / re-close protection.** A second `close_period` for the same
  `period_id` is rejected with `PeriodAlreadyClosed`, so the pay order cannot
  be silently re-computed (e.g. by an issuer who updated priorities after
  close). Callers who want a re-resolution must use a new `period_id`.

## Event payload reference

### `EVENT_CLASS_PAY_ORDER` ("clspayo")

Topic: `(event_symbol, issuer, namespace, token)`.
Data: `(period_id: u64, ordered_classes: Vec<ShareClass>)`.

### `EVENT_CLASS_PRIORITY_SET` ("clprio")

Topic: `(event_symbol, issuer, namespace, token, share_class: ShareClass)`.
Data: `priority_index: u32`.

Both are emitted through `env.events().publish(...)` against the standard
Soroban event stream and become part of the indexed event history.

## Migration & Backwards-Compatibility

- This change adds two new `DataKey2` variants. `STORAGE_LAYOUT_VERSION`
  is intentionally NOT bumped in this PR because the additions are purely
  additive — no existing key shape changes, no removals.
- Periods that were closed *before* this contract upgrade continue to be
  sealed (`is_period_closed` returns `true`) but `get_class_pay_order`
  returns an empty `Vec` for them. This is the correct, conservative
  fallback: an empty list conveys "we have no recorded pay order for this
  period," which indexers should treat as "legacy / pre-upgrade period".
- Downgrades: if a deployment rolls back to a contract binary that lacks
  this feature, `get_class_pay_order` and `get_class_priority` cannot be
  called (they don't exist on the older binary). The closed-period flag and
  all other storage remain valid against the older binary.

## Test coverage

Tests live in `src/test_close_period.rs` alongside the existing
`close_period` and `close_period_dual_sig` tests. Coverage spans:

- `set_class_priority` happy path / wrong issuer / unknown offering /
  unregistered class.
- `get_class_priority` default-zero fallback.
- `close_period` emits `EVENT_CLASS_PAY_ORDER` with priority-sorted order,
  including the XDR-bytes tie-break.
- `close_period` with no registered classes emits an empty order.
- `close_period_dual_sig` produces the same canonical order.
- `get_class_pay_order` returns `Vec::new()` for periods closed under an
  older contract.
