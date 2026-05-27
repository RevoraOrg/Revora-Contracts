# Testnet Mode Feature

## 🚨 CRITICAL SECURITY WARNING

**TESTNET MODE IS EXTREMELY DANGEROUS FOR PRODUCTION USE**

Testnet mode relaxes validations that protect investor funds. **Enabling testnet mode on production/mainnet contracts can lead to catastrophic fund loss.**

### Production Safety Requirements
- **NEVER enable testnet mode on mainnet/production contracts**
- **Always verify `is_testnet_mode()` returns `false` before production deployment**
- **Admin must ensure testnet mode is disabled before going live**
- **Integrators must check `is_testnet_mode()` in their client code**

### What Testnet Mode Does
When enabled, testnet mode **bypasses these critical safety checks**:
1. **Revenue Share Validation**: Allows `revenue_share_bps > 10000` (100%+ distributions)
2. **Concentration Enforcement**: Ignores holder concentration limits that prevent manipulation

**These relaxations can allow malicious actors to drain all funds from the contract.**

## Overview

The testnet mode feature provides a configuration flag that enables simplified behavior for testnet and development deployments. When enabled, certain strict validations are relaxed to facilitate testing and experimentation without compromising production safety.

## Purpose

Testnet mode is designed for:
- Non-production deployments (testnet, devnet, local development)
- Testing edge cases and boundary conditions
- Rapid prototyping and experimentation
- Integration testing with flexible parameters

## Behavior Changes

When testnet mode is enabled, the following behaviors are modified:

### 1. Revenue Share BPS Validation (register_offering)

**Normal Mode:**
- `revenue_share_bps` must be ≤ 10,000 (100%)
- Values > 10,000 return `InvalidRevenueShareBps` error

**Testnet Mode:**
- `revenue_share_bps` validation is skipped
- Any value is accepted, including > 10,000
- Useful for testing extreme scenarios

### 2. Concentration Enforcement (report_revenue)

**Normal Mode:**
- If concentration limit is set with `enforce=true`, `report_revenue` fails when reported concentration exceeds the limit
- Returns `ConcentrationLimitExceeded` error

**Testnet Mode:**
- Concentration enforcement is skipped
- `report_revenue` succeeds regardless of concentration
- Concentration warnings are still emitted via events

## Usage

### Setting Up Testnet Mode

1. **Set Admin** (one-time operation):
```rust
contract.set_admin(&admin_address);
```

2. **Enable Testnet Mode** (admin only):
```rust
contract.set_testnet_mode(&true);
```

3. **Verify Mode**:
```rust
let is_testnet = contract.is_testnet_mode();
```

4. **Disable Testnet Mode** (when moving to production):
```rust
contract.set_testnet_mode(&false);
```

### Example: Testing with High BPS

```rust
// Enable testnet mode
contract.set_admin(&admin);
contract.set_testnet_mode(&true);

// Register offering with > 100% revenue share (for testing)
contract.register_offering(&issuer, &token, &15_000); // 150%

// This would fail in normal mode but succeeds in testnet mode
```

### Example: Testing Concentration Scenarios

```rust
// Enable testnet mode
contract.set_admin(&admin);
contract.set_testnet_mode(&true);

// Set up concentration limit with enforcement
contract.register_offering(&issuer, &token, &5_000);
contract.set_concentration_limit(&issuer, &token, &5000, &true);

// Report high concentration
contract.report_concentration(&issuer, &token, &8000); // 80% > 50% limit

// Report revenue - succeeds in testnet mode, would fail in normal mode
contract.report_revenue(&issuer, &token, &1_000_000, &1);
```

## Security Considerations

### 🚨 FUND LOSS RISK
Testnet mode bypasses validations designed to protect investor funds. Enabling it on production contracts can result in:
- **Over-distribution**: `revenue_share_bps > 10000` allows distributions exceeding 100%
- **Concentration manipulation**: Bypassed enforcement allows unlimited holder concentration
- **Fund drainage**: Malicious issuers could extract all contract funds

### Admin-Only Access
- Only the contract admin can toggle testnet mode
- Requires `set_admin()` to be called first
- Admin authorization is enforced via `require_auth()`

### Production Safety
- **Testnet mode is disabled by default**
- Must be explicitly enabled by admin
- Can be toggled on/off at any time
- Mode changes emit events for auditability
- **CRITICAL**: Always verify `is_testnet_mode() == false` before production use
## Integrator Requirements

### Mandatory Production Checks
All integrators and frontends **MUST** implement these checks:

```rust
// ALWAYS check testnet mode before using the contract
let is_testnet = contract.is_testnet_mode();
if is_testnet {
    panic!("CRITICAL: Contract has testnet mode enabled - DO NOT USE FOR PRODUCTION");
}
```

### Client Library Integration
```rust
pub fn connect_to_revora_contract(contract_id: &str) -> Result<RevoraClient> {
    let client = RevoraClient::new(contract_id);
    
    // Mandatory safety check
    if client.is_testnet_mode()? {
        return Err(Error::TestnetModeEnabled);
    }
    
    Ok(client)
}
```

### Deployment Verification
Before deploying to production:
1. Deploy contract
2. Verify `is_testnet_mode()` returns `false`
3. Document the check in deployment scripts
4. Include in security audits
### Unaffected Operations

The following operations work identically in both modes:
- Blacklist management
- Pagination
- Audit summaries
- Claim operations
- Rounding modes
- All read-only queries

## Events

Testnet mode changes emit the `test_mode` event:

```
Topic: (test_mode, admin_address)
Payload: enabled (bool)
```

This allows off-chain systems to track when testnet mode is toggled.

## Testing

The feature includes comprehensive test coverage (95%+):

### Core Functionality Tests
- `testnet_mode_disabled_by_default` - Verifies default state
- `set_testnet_mode_requires_admin` - Admin authorization
- `testnet_mode_can_be_toggled` - Enable/disable cycles
- `set_testnet_mode_emits_event` - Event emission

### Validation Relaxation Tests
- `testnet_mode_allows_bps_over_10000` - BPS validation skip
- `testnet_mode_disabled_rejects_bps_over_10000` - Normal mode enforcement
- `testnet_mode_skips_concentration_enforcement` - Concentration skip
- `testnet_mode_disabled_enforces_concentration` - Normal mode enforcement

### Edge Cases
- `testnet_mode_toggle_after_offerings_exist` - Mode change with existing data
- `testnet_mode_affects_only_validation_not_storage` - Storage integrity
- `testnet_mode_multiple_offerings_with_varied_bps` - Multiple offerings

### Integration Tests
- `testnet_mode_normal_operations_unaffected` - Other operations work
- `testnet_mode_blacklist_operations_unaffected` - Blacklist unchanged
- `testnet_mode_pagination_unaffected` - Pagination unchanged

## Best Practices

### For Testnet Deployments

1. **Enable at deployment**: Set admin and enable testnet mode immediately after contract deployment
2. **Document clearly**: Mark testnet contracts in your documentation
3. **Monitor events**: Track `test_mode` events to verify configuration
4. **Test thoroughly**: Use testnet mode to test edge cases before production

### For Production Deployments

1. **Never enable**: Keep testnet mode disabled for production contracts
2. **Verify state**: Check `is_testnet_mode()` returns `false` before going live
3. **Admin security**: Protect admin keys to prevent unauthorized mode changes
4. **Audit trail**: Review event logs to ensure mode was never enabled

### Migration from Testnet to Production

1. Deploy new contract instance (testnet mode disabled by default)
2. Migrate data if needed
3. Verify `is_testnet_mode()` returns `false`
4. Do not reuse testnet contracts for production

## Implementation Details

### Storage

Testnet mode state is stored in persistent storage:
```rust
DataKey::TestnetMode -> bool
```

### Code Locations

- **Storage key**: `src/lib.rs` - `DataKey::TestnetMode`
- **Event symbol**: `src/lib.rs` - `EVENT_TESTNET_MODE`
- **Functions**: `src/lib.rs` - `set_testnet_mode()`, `is_testnet_mode()`
- **Modified flows**: `register_offering()` (reads `Self::is_testnet_mode(env.clone())` to gate BPS validation), `report_revenue()` (reads same flag to gate concentration enforcement)
- **Tests**: `src/test.rs` - Testnet mode section

## Limitations

- Testnet mode does NOT affect:
  - Token transfers
  - Claim calculations
  - Blacklist enforcement
  - Freeze functionality
  - Any other validation logic

- Mode changes are immediate (no delay or grace period)
- Existing offerings retain their parameters when mode is toggled

## Version History

- **v0.1.0** - Initial implementation (Issue #24)
  - Admin-only toggle
  - BPS validation relaxation
  - Concentration enforcement skip
  - Comprehensive test coverage

## Support

For questions or issues related to testnet mode:
1. Check test cases in `src/test.rs` for usage examples
2. Review event logs for mode change history
3. Verify admin configuration with `get_admin()`
