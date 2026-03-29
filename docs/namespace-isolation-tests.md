# Namespace Isolation in Revora

Revora supports multi-tenant deployments through the use of **Namespaces**. This document details the security assumptions, implementation details, and verification strategy for namespace isolation.

## Security Overview

The goal of namespace isolation is to ensure that different issuers or the same issuer across different logical groups (namespaces) cannot access or modify each other's data.

### Key Concepts

- **TenantId**: Composed of `(issuer: Address, namespace: Symbol)`. Used to group offerings.
- **OfferingId**: Composed of `(issuer: Address, namespace: Symbol, token: Address)`. Used for per-offering storage keys.

## Isolation Mechanism

Isolation is achieved at the storage level by incorporating the `namespace` and the original `issuer` address into all persistent storage keys.

### Storage Keys

All data related to an offering is keyed by its `OfferingId`:
- Blacklists and Whitelists
- Concentration limits and reported values
- Revenue reports and period indices
- Configuration (rounding modes, thresholds, windows)

Because Soroban storage keys are XDR-encoded, the unique combination of `(Issuer, Namespace, Token)` ensures distinct storage locations with no possibility of collision.

### Authorization & Tenure

1. **Existence Verification**: Most operations now explicitly verify that an offering exists under the provided `(Issuer, Namespace, Token)` triple before allowing mutations.
2. **Current Issuer Check**: The contract tracks the current owner of an offering. Authorization (`require_auth`) is enforced against the **current issuer**, even if the offering was originally created by a different address in the same namespace.
3. **Admin Override**: The system admin has authority across all namespaces for emergency actions (e.g., managing blacklists), as defined in the contract security policy.

## Security Assumptions

- **Namespace Uniqueness**: Issuers are responsible for managing their own namespaces. Different issuers using the same namespace name (e.g., "default") are still isolated because their underlying `Address` is different.
- **Issuer Trust**: It is assumed that the `issuer` address is secure. Namespace isolation protects against cross-issuer leakage, not against a compromised issuer account.
- **Storage Integrity**: We rely on the Soroban host environment to ensure that distinct storage keys do not collide.

## Verification Strategy

Namespace isolation is verified through comprehensive unit tests in `src/test_namespaces.rs` and `src/test.rs`.

### Test Scenarios

- **Isolated Storage**: Verifying that registering identical tokens in different namespaces results in separate revenue pools and blacklists.
- **Unauthorized Cross-Access**: Attempting to modify a namespace's state using an unauthorized issuer address.
- **Transfer Integrity**: Ensuring that transferring an offering to a new issuer maintains its namespace-scoped data while updating owner-based access controls.
- **Ghost State Prevention**: Ensuring that operations on non-existent namespaces/offerings fail with `OfferingNotFound`.
- **Duplicate Registration Rejection**: Explicitly rejecting duplicate `(Issuer, Namespace, Token)` offerings to prevent state clobbering.
- **Aggregation Boundary Testing**: Confirmed that global iteration across all tokens only aggregates data from the correct requested namespace/issuer.

### Abuse & Failure Paths Documented

The implementation hardens these isolation boundaries by specifically covering the following abuse paths:
1. **Malicious Override Attempts**: An issuer attempting to create a collision by submitting a duplicate namespace string. Prevented because the system securely anchors the `Address` type directly with the `Symbol`.
2. **Ghost-Namespace Attacks**: Trying to call setter operations (e.g. `set_claim_delay`) on a namespace `Symbol` that has no registered tokens to blindly manipulate storage. The contract securely fails with `OfferingNotFound` due to strict `exists` validation before mutations.
3. **Cross-Tenant Authorization Bypass**: A valid authenticated issuer attempts to pass an `OfferingId` belonging to another issuer. The contract explicitly verifies the `current_issuer` in storage matches the `caller` parameter before any data mutation, preventing horizontal privilege escalation.

### Invalid Inputs coverage

Namespace isolation enforces strict validation around inputs:
- `Namespace`: Tested against exceeding string limits or character exceptions via basic Soroban `Symbol` type constraints.
- `OfferingId`: Any mismatch in the tuple `(issuer, namespace, token)` during retrieval reliably produces an `Option::None` short-circuiting to an explicit `Err(OfferingNotFound)`.
