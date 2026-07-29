# Deferred Distributions

Adds a `defer_until_close` flag to revenue reports.

### Lifecycle
1. **Queueing:** Deferred reports are stored in the `DeferredReports` mapping keyed by `period_id`.
2. **Security Barrier:** Any `claim` attempt against a period still in the deferred mapping will immediately panic with `DistributionDeferred`.
3. **Atomic Flush:** Calling `close_period` removes the block.

---

## Priority Queue (issue #551)

Deferred distributions that share the same `release_ts` are now ordered deterministically by an issuer-assigned **priority score**, with `queue_id` as the final tie-breaker.

### Motivation

Without explicit ordering, two entries with identical release timestamps would be flushed in an undefined order that could vary between nodes or re-runs. This breaks auditability and fairness contracts. The priority queue assigns a deterministic release order across all entries.

### Data Model

Each entry in the queue is a `DeferredQueueEntry` struct:

```rust
pub struct DeferredQueueEntry {
    pub release_ts:  u64,   // Unix timestamp (seconds) at/after which entry may be released.
    pub priority:    u32,   // Issuer-assigned urgency score; lower = higher priority (0 = highest).
    pub queue_id:    u32,   // Monotonically-increasing per-offering counter; final tie-breaker.
    pub payload_id:  u64,   // Issuer-defined identifier (e.g. period_id). Not interpreted by contract.
}
```

Entries are stored as `Vec<DeferredQueueEntry>` in `DataKey2::DeferredQueue(OfferingId)` and kept in **sorted order at all times**. The sort key is:

```
(release_ts ASC, priority ASC, queue_id ASC)
```

### Sort Key Semantics

| Key | Direction | Tie-break role |
|-----|-----------|----------------|
| `release_ts` | Ascending | Primary: entries with earlier release time come first |
| `priority` | Ascending | Secondary: lower score = higher urgency (0 is highest priority) |
| `queue_id` | Ascending | Final: monotonically-increasing per-offering insertion counter ensures strict determinism |

Because `queue_id` is unique within an offering and always ascending, the sort order is **strictly total** — no two entries ever compare equal. The ordering is identical across every node, re-run, and indexer.

### API

#### `enqueue_deferred(issuer, namespace, token, release_ts, priority, payload_id) → queue_id`

Inserts a new entry into the priority queue for the given offering.

- **Auth:** `issuer.require_auth()` — only the current offering issuer may enqueue.
- `release_ts`: Unix timestamp (seconds) at or after which the entry may be released.
- `priority`: Issuer-assigned score; **0 is highest priority**. Entries with the same `release_ts` are processed in ascending priority order.
- `payload_id`: Issuer-defined identifier (e.g. `period_id`). Not validated by the contract.
- **Returns:** The `queue_id` assigned to the new entry (monotonically increasing per offering).

Emits event `deferred_priority_set` (topic `def_pset`) with payload:
```
(queue_id: u32, release_ts: u64, priority: u32, payload_id: u64)
```

#### `get_deferred_queue(issuer, namespace, token) → Vec<DeferredQueueEntry>`

Read-only. Returns the full queue in release order `(release_ts, priority, queue_id)`.
No auth required. Returns an empty `Vec` if no entries have been enqueued.

### Tie-break Example

Three entries with the same `release_ts = 1000` and `priority = 5`:

| Insertion order | `queue_id` | `release_ts` | `priority` | Final position |
|-----------------|-----------|-------------|------------|----------------|
| First           | 0         | 1000        | 5          | 1st            |
| Second          | 1         | 1000        | 5          | 2nd            |
| Third           | 2         | 1000        | 5          | 3rd            |

Two entries with the same `release_ts = 2000` but different priorities:

| Insertion order | `queue_id` | `release_ts` | `priority` | Final position |
|-----------------|-----------|-------------|------------|----------------|
| First           | 0         | 2000        | 10         | 2nd (lower urgency) |
| Second          | 1         | 2000        | 0          | 1st (higher urgency) |

### Storage

Priority queue state is stored in `DataKey2::DeferredQueue(OfferingId)` as `Vec<DeferredQueueEntry>`.
The vector is maintained in sorted order; no post-hoc sorting is ever needed at flush time.

### Security Notes

- **Auth barrier:** Only the current issuer of the offering can call `enqueue_deferred`. Non-issuers receive `OfferingNotFound`.
- **Determinism:** The `queue_id` counter is derived from the current queue length before insertion, making it collision-free. The final `(release_ts, priority, queue_id)` triple is unique per entry, guaranteeing a strictly total order.
- **Priority manipulation:** Priority scores are issuer-controlled and not validated beyond their type (`u32`). Issuers bear responsibility for choosing meaningful scores consistent with their distribution policies.
- **No flush side effects:** `enqueue_deferred` does not transfer tokens or advance any accrual index. It only records the entry and emits the event.

### Event Reference

| Event name | Topic symbol | Data |
|-----------|-------------|------|
| `deferred_priority_set` | `def_pset` | `(queue_id: u32, release_ts: u64, priority: u32, payload_id: u64)` |

### Test Coverage

Tests are in `src/test_deferred_priority.rs` and cover:

- Monotonically increasing `queue_id` assignment
- Empty queue before first enqueue
- Event emission and payload correctness
- Primary sort by `release_ts` ascending
- Secondary sort by `priority` ascending (lower = higher urgency)
- Tertiary sort by `queue_id` ascending (tie-breaker for identical `release_ts` + `priority`)
- Complex multi-key mixed insertion order
- Boundary values (zero priority, u32::MAX priority, u64::MAX timestamps)
- Single-entry queue
- Authorization: rejects non-issuer callers (`OfferingNotFound`)
- Authorization: rejects unknown offerings
- Read-only `get_deferred_queue` requires no auth
- Multi-offering queue isolation
- Large queue (50 entries) with verified sorted invariant
