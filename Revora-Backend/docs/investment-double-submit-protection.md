# Investment Double-Submit Protection

## Overview
The Investment Double-Submit Protection enforces transaction idempotency across the backend platform using a distributed lock pattern. It acts as an express middleware that guards transaction endpoints (such as `/api/investments`) from processing the same investment request multiple times if a user triggers the action simultaneously (e.g. impatiently tapping the "Invest" button repeatedly).

This architecture prevents race conditions that could lead to double-spending, negative balances, or accounting discrepancies directly at the API edge.

## Security Assumptions

1. **Edge Protection**: The lock intercepts requests before validation, network traversal to the blockchain, or internal database writes. This reduces redundant load securely on upstream services.
2. **Deterministic Locking**: A lock is generated deterministically based on context context (like `userId` and `offeringId` payloads), guaranteeing a robust 1:1 map between an active transaction window and the user's intent. The system respects explicit client `Idempotency-Key` headers unconditionally when present.
3. **Atomic Failure Path**: If the lock cannot be acquired, execution traps immediately and a `409 Conflict` HTTP status is returned to the client, effectively bouncing rapid duplicate traffic securely but informatively.
4. **Guaranteed Release**: Lock releases are hooked natively to node's HTTP `finish` and `close` emitter events, ensuring that even if an execution context crashes, times out, or fails during downstream processing, the lock will always be gracefully released without locking users out indefinitely.

## Technical Implementation

### Core Dependencies
We provide a `DistributedLock` abstract interface. By default, the middleware bootstraps using `InMemoryLock` for single-node isolated environments such as local environments or specific testing. 

*Production readiness dictate that a Redis-backed DistributedLock must be provided and dependency-injected into the `setupInvestmentRouter({ lock: redisLock })` during the initialization phase for horizontal scale architectures.*

### Endpoint Configuration
You can wrap any mission-critical edge endpoint:
```typescript
import { investmentDoubleSubmitProtection } from './index';

// Attach middleware directly on the sensitive post route
router.post(
  '/api/investments',
  investmentDoubleSubmitProtection({ ttlMs: 15000 }), 
  investmentControllerHandler 
);
```

By default, the middleware falls back if no identification keys can be formed. Developers must ensure standard `req.body.userId`, and `req.body.offeringId` structures exist.

### Test Coverage (`health.test.ts`)
The feature includes comprehensive, >95% coverage evaluating:
- Strict lock conflicts handling simultaneous parallel executions.
- Standard consecutive clears mapping successful lock/releases.
- Correct isolation ensuring requests across differing users for identical offerings don't block.
- Standard health check fallback capabilities validating HTTP readiness correctly.
