# Issuer-Only Blacklist Governance

## Overview

The Revora-Contracts smart contract implements **issuer-only blacklist governance**, ensuring that only the current issuer of a revenue-share offering can manage the blacklist for that offering's token. This provides issuers with complete control over compliance and investor management, preventing unauthorized modifications to blacklist status.

## Security Assumptions

- **Issuer Authority:** Only the current issuer (after any transfers) can add or remove addresses from the offering's blacklist.
- **No Admin Override:** The global admin cannot modify blacklists, even in emergency situations.
- **Immutable Enforcement:** Blacklist checks are enforced at claim time; blacklisted holders cannot claim revenue.

## Implementation Details

### Authorization Check

Both `blacklist_add` and `blacklist_remove` functions perform the following authorization:

1. Retrieve the current issuer using `get_current_issuer()`
2. Verify `caller == current_issuer`
3. Return `RevoraError::NotAuthorized` if the check fails

### Code Changes

```rust
// In src/lib.rs
let current_issuer = Self::get_current_issuer(&env, issuer.clone(), namespace.clone(), token.clone())
    .ok_or(RevoraError::OfferingNotFound)?;

// Verify auth: caller must be the current issuer
if caller != current_issuer {
    return Err(RevoraError::NotAuthorized);
}
```

### Test Coverage

Comprehensive tests ensure the feature works correctly:

- `blacklist_add_requires_issuer_auth`: Verifies only issuer can add to blacklist
- `blacklist_remove_requires_issuer_auth`: Verifies only issuer can remove from blacklist
- Existing auth tests ensure `require_auth` is still enforced
- Integration tests with issuer transfers verify current issuer is used

## Migration Notes

This change restricts blacklist management from "any authenticated address" to "only the current issuer". Existing off-chain systems that relied on non-issuer addresses managing blacklists must be updated to route blacklist operations through the issuer.

## Security Benefits

1. **Controlled Compliance:** Issuers maintain exclusive control over investor access.
2. **Prevented Griefing:** Malicious actors cannot arbitrarily blacklist legitimate investors.
3. **Auditability:** All blacklist changes are attributable to the issuer.
4. **Transfer Safety:** Issuer transfers properly update blacklist management authority.

## Usage Example

```rust
// Only the issuer can manage the blacklist
contract.blacklist_add(issuer, issuer, namespace, token, investor_address);
contract.blacklist_remove(issuer, issuer, namespace, token, investor_address);
```

Non-issuer attempts will fail with `RevoraError::NotAuthorized`.