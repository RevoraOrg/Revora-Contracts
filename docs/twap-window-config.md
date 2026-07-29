# Per-offering TWAP window configurability (#546)

## Summary

Different offerings have different acceptable smoothing horizons for NAV computation:

- Fast-moving tokenized equities need sub-minute responsiveness.
- Private credit offerings benefit from multi-week smoothing to dampen late
  payments and delayed reporting.

This feature allows each offering to configure its own
TWAP (time-weighted average price) smoothing window with hard, documented
bounds. Off-chain NAV readers and on-chain integrations can later consume the
configured window to apply the right smoothing horizon per offering.

The feature is **config-only in this revision**: the contract persists the
window but does not yet compute TWAP. Future revisions will read
`TwapConfig` when computing NAV.

---

## API surface

### `set_twap_window`

```rust
pub fn set_twap_window(
    env: Env,
    caller: Address,
    issuer: Address,
    namespace: Symbol,
    token: Address,
    window_secs: u64,
) -> Result<(), RevoraError>
```

Persist a TWAP smoothing window for `(issuer, namespace, token)`.

| Parameter     | Meaning                                                   |
|---------------|-----------------------------------------------------------|
| `caller`      | Authorization principal (admin OR current primary issuer) |
| `issuer`      | Offering primary issuer (the holding key)                 |
| `namespace`   | Offering namespace symbol                                 |
| `token`       | Offering token contract address                           |
| `window_secs` | Smoothing window in seconds; bounded see below            |

### `get_twap_window`

```rust
pub fn get_twap_window(
    env: Env,
    issuer: Address,
    namespace: Symbol,
    token: Address,
) -> Option<TwapConfig>
```

Read the current `TwapConfig` for an offering. Returns `None` when the
contract has not been configured for that offering — readers should treat
absence as **"no smoothing"** until NAV readers are introduced.

### `TwapConfig`

```rust
pub struct TwapConfig {
    pub window_secs: u64,
    pub set_at: u64,
    pub set_by: Address,
}
```

- `window_secs` — the configured smoothing window in seconds.
- `set_at` — ledger timestamp at which the window was last set.
- `set_by` — `Address` that authored the most recent `set_twap_window`
  call. Useful for off-chain audit. **The contract does not authorize
  writes based on this field**; authorization is enforced in the contract
  method itself.

---

## Bounds

| Bound                       | Value            | Rationale                                                        |
|-----------------------------|------------------|-------------------------------------------------------------------|
| `MIN_TWAP_WINDOW_SECS`      | `60` (1 minute)  | Prevents super-instantaneous smoothing that aliases single-tick price manipulation. |
| `MAX_TWAP_WINDOW_SECS`      | `30 * 24 * 60 * 60` (30 days) | Caps the largest acceptable smoothing horizon — never silently averages over a stale revenue stream. |

Values outside `[MIN_TWAP_WINDOW_SECS, MAX_TWAP_WINDOW_SECS]` are rejected
with [`RevoraError::TwapWindowOutOfBounds`]. Both bounds are inclusive: a
configured value of exactly `MIN` or exactly `MAX` is accepted.

### Why these bounds?

- **One minute** is shorter than the typical Stellar ledger close time
  (~5s under load) plus some buffer; very short windows are useful for
  liquid tokenized equities.
- **Thirty days** is generous enough to absorb month-end reporting lags
  for illiquid offerings but small enough to never silently smooth over
  information that the market or auditors might reasonably expect to be
  fresh.

---

## Authorization

`set_twap_window` mirrors the [`set_dispute_window`](./dispute-window-flow.md)
authorization model: a financial-config write is sensitive, but the issuer
knows their asset class best. Therefore either the **global admin** or
the offering's **current primary issuer** may call `set_twap_window`.

| Caller              | Result                                                       |
|---------------------|--------------------------------------------------------------|
| Global admin        | ✅ Succeeds                                                  |
| Current primary issuer | ✅ Succeeds                                               |
| Anyone else         | ❌ `RevoraError::NotAuthorized`                              |
| Quiescent contract  | ❌ `RevoraError::ContractFrozen` or `RevoraError::ContractPaused` |
| Unregistered offering | ❌ `RevoraError::OfferingNotFound`                         |
| Out-of-range value  | ❌ `RevoraError::TwapWindowOutOfBounds` (checked before auth to avoid auth-failure side-channels) |

> **Operationally:** the issuer-transfer flow remaps `primary_issuer` when
> an issuer transfer is accepted. After such a remap, the new primary
> issuer (and only the new primary issuer, plus admin) can reconfigure the
> TWAP window for the offering.

---

## Events

### `EVENT_TWAP_WINDOW_SET`

Topic: `(twap_set, issuer, namespace, token)`
Data:  `(previous_window_secs: u64, new_window_secs: u64, caller: Address)`

Emitted **once per successful `set_twap_window` call**, including
reconfigurations. Off-chain indexers can rebuild the full per-offering
history from the event stream.

#### Important: `previous_window_secs == 0` sentinel

When an offering has never been configured, the first successful
`set_twap_window` call publishes a `previous_window_secs` of `0`. Off-chain
indexers **must not** interpret this as "previously set to MIN"; the value
`0` is a sentinel for "no prior configuration". The presence/absence of
the underlying `TwapConfig` storage entry is the canonical source of
truth — use `get_twap_window` to read the full struct (including
`set_at`).

---

## Default behavior

When an offering has no `TwapConfig` entry:

- `get_twap_window` returns `None`.
- NAV readers should treat absence as **"no smoothing"** (use the raw
  value).

The contract never writes a default `TwapConfig` automatically. This is
intentional: introducing an implicit smoothing horizon silently could
change downstream economics in unexpected ways.

---

## Storage layout

| Field                  | Value                                           |
|------------------------|-------------------------------------------------|
| Module                 | `revora_revenue_share`                          |
| Key variant            | `DataKey2::TwapConfig(OfferingId)`              |
| Value type             | `TwapConfig { window_secs: u64, set_at: u64, set_by: Address }` |
| Ownership scope        | per offering                                    |
| Layout version         | 2 (additive — does not require migration)       |

The key variant was added to the existing **`DataKey2`** overflow enum to
avoid pushing the primary `DataKey` union past Soroban's 50-variant XDR
limit. All readers and writers must import the updated
`tools/storage_layout_schema.rs` and `docs/STORAGE_LAYOUT.json`.

---

## Security notes

1. **Bounds validation runs before authentication.** Because the bound
   check is a cheap, deterministic, caller-independent predicate,
   performing it first avoids revealing auth state to unauthorized
   callers via an auth-failure-differential side channel.

2. **Write-overwrite, no history compaction.** Each successful
   `set_twap_window` overwrites the prior config and emits a new event.
   There is no batching or compaction; off-chain readers should always
   re-read via `get_twap_window` rather than caching.

3. **No cross-offering coupling.** The window is configured per offering;
   changes to one offering do not affect any other offering.

4. **Pausable / freezable.** The setter respects the contract's global
   freeze and pause gates; an admin can temporarily lock all
   `set_twap_window` writes without affecting any existing config.

5. **No race or replay risk.** `caller.require_auth()` enforces the Soroban
   host signature check; the event payload records `set_by` for audit.
   An attacker cannot replay an old event because Soroban's event stream
   is monotonic and content-addressed by sequence number.

---

## Migration

This is an additive change: no migration is required for existing
offerings. Offerings without a `TwapConfig` continue to function
identically; absence means **"no smoothing"** by default. Operators who
want a specific smoothing horizon must explicitly call `set_twap_window`
on each offering they care about.

The `STORAGE_LAYOUT_VERSION` constant is **not bumped**: the new key
variant is an additive extension that existing readers can ignore.
Future revisions that change the *semantics* of `TwapConfig` (e.g.
changing the unit or adding constraints) must bump the version and
implement a migration hook.

---

## Test coverage

`src/test_twap_window.rs` covers:

| Section | Coverage                                                |
|---------|---------------------------------------------------------|
| 1       | Setup helpers (`make_client`, `setup_offering`)         |
| 2       | Bounds: below-min, exact-min, min+1, exact-max, max+1, zero, `u64::MAX` |
| 3       | Interior values: 1 hour, 1 week                        |
| 4       | Authorization: stranger rejected; bounds-before-auth   |
| 5       | Missing offering → `OfferingNotFound`                  |
| 6       | Overwrite on reconfigure; `None` when unset            |
| 7       | Event emission; event on every reconfigure             |
| 8       | Admin can configure on behalf of issuer                |
| 9       | `previous_window == 0` sentinel on first call          |

Total: **16 tests**, hitting all four corners of the bounds matrix
(`min-1`, `min`, `min+1`, `max-1`, `max`, `max+1`, plus `0` and
`u64::MAX`) plus interior, auth, and event coverage.

---

## Cross-references

- [`RevoraError::TwapWindowOutOfBounds`](../src/lib.rs) (discriminant `76`)
- `MIN_TWAP_WINDOW_SECS`, `MAX_TWAP_WINDOW_SECS` — `pub const` in `src/lib.rs`
- `TwapConfig` — `#[contracttype] pub struct` in `src/lib.rs`
- `EVENT_TWAP_WINDOW_SET` — `symbol_short!("twap_set")` in `src/lib.rs`
- `DataKey2::TwapConfig(OfferingId)` — secondary storage key enum
- `tests/storage_layout_json.rs::storage_layout_json_matches_checked_in_docs`
  — drift guard for storage schema
