# Deferred Distributions

Adds a `defer_until_close` flag to revenue reports. 

### Lifecycle
1. **Queueing:** Deferred reports are stored in the `DeferredReports` mapping keyed by `period_id`.
2. **Security Barrier:** Any `claim` attempt against a period still in the deferred mapping will immediately panic with `DistributionDeferred`.
3. **Atomic Flush:** Calling `close_period` removes the block.
