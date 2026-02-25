# Implementation Prompt: Per-Offering Metadata Storage

## Objective
Implement a feature that allows issuers to attach off-chain metadata references (e.g., IPFS or HTTPS URIs) to offerings in the Revora revenue-sharing smart contract.

## Context
This is a Soroban smart contract (Stellar blockchain) written in Rust. The contract manages revenue-sharing offerings where issuers can register offerings, report revenue, and manage blacklists. You need to extend this functionality to support metadata storage.

## Requirements

### Functional Requirements
1. **Storage**: Support storing a short string or hash reference per offering (issuer, token pair)
2. **Authorization**: Only the issuer or IssuerAdmin can update metadata for their offerings
3. **Events**: Emit events when metadata is created or updated
4. **CRUD Operations**: Implement methods to set, update, and retrieve metadata

### Technical Constraints
- Must work within Soroban's storage model (persistent storage)
- Storage limits: Keep metadata references reasonably sized (suggest max 256 bytes)
- Must handle serialization properly using `#[contracttype]`
- Must respect the existing freeze and pause mechanisms
- Must verify offering exists before allowing metadata operations

## Implementation Steps

### 1. Fork and Branch
```bash
git checkout -b feature/offering-metadata-storage
```

### 2. Extend Data Model

Add to `src/lib.rs`:

**New DataKey variant:**
```rust
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    // ... existing variants ...
    /// Per (issuer, token): metadata reference (IPFS hash, HTTPS URI, etc.)
    OfferingMetadata(Address, Address),
}
```

**New event symbols:**
```rust
const EVENT_METADATA_SET: Symbol = symbol_short!("meta_set");
const EVENT_METADATA_UPDATED: Symbol = symbol_short!("meta_upd");
```

### 3. Implement Core Methods

Add these public methods to the `RevoraRevenueShareClient`:

**Set/Update Metadata:**
```rust
/// Set or update metadata reference for an offering.
/// Only callable by the current issuer of the offering.
/// Returns error if offering doesn't exist or caller is not authorized.
pub fn set_offering_metadata(
    env: Env,
    issuer: Address,
    token: Address,
    metadata: String,
) -> Result<(), RevoraError>
```

**Get Metadata:**
```rust
/// Retrieve metadata reference for an offering.
/// Returns None if no metadata has been set.
pub fn get_offering_metadata(
    env: Env,
    issuer: Address,
    token: Address,
) -> Option<String>
```

### 4. Implementation Details

**Authorization checks:**
- Verify the offering exists using `get_offering()`
- Verify caller is the current issuer using `get_current_issuer()`
- Call `require_not_frozen()` and `require_not_paused()`
- Call `issuer.require_auth()`

**Storage operations:**
- Use `env.storage().persistent()` for metadata storage
- Key: `DataKey::OfferingMetadata(issuer, token)`
- Value: `String` type

**Event emission:**
- Emit `EVENT_METADATA_SET` on first set (when no previous metadata exists)
- Emit `EVENT_METADATA_UPDATED` on updates (when metadata already exists)
- Include issuer, token, and new metadata value in event data

**Validation:**
- Check metadata length (suggest max 256 bytes)
- Return appropriate error if offering not found
- Handle empty string metadata (allow it for clearing metadata)

### 5. Testing Requirements

Add comprehensive tests to `src/test.rs`:

**Basic CRUD tests:**
- `test_set_offering_metadata_success` - Happy path for setting metadata
- `test_get_offering_metadata_returns_none_initially` - No metadata before set
- `test_update_offering_metadata_success` - Update existing metadata
- `test_get_offering_metadata_after_set` - Retrieve after setting

**Authorization tests:**
- `test_set_metadata_requires_auth` - Fails without auth
- `test_set_metadata_requires_issuer` - Only issuer can set
- `test_set_metadata_nonexistent_offering` - Fails for non-existent offering
- `test_set_metadata_respects_freeze` - Blocked when contract frozen
- `test_set_metadata_respects_pause` - Blocked when contract paused

**Edge cases:**
- `test_set_metadata_empty_string` - Allow empty metadata
- `test_set_metadata_max_length` - Test size limits
- `test_set_metadata_oversized_data` - Reject if too large
- `test_set_metadata_repeated_updates` - Multiple updates work correctly
- `test_metadata_scoped_per_offering` - Different offerings have separate metadata

**Event tests:**
- `test_metadata_set_emits_event` - Verify EVENT_METADATA_SET on first set
- `test_metadata_update_emits_event` - Verify EVENT_METADATA_UPDATED on update
- `test_metadata_events_include_correct_data` - Validate event structure

**Multi-offering tests:**
- `test_metadata_multiple_offerings_same_issuer` - Separate metadata per token
- `test_metadata_after_issuer_transfer` - Metadata persists after transfer

### 6. Test Coverage Target
- Minimum 95% test coverage
- Use `cargo tarpaulin` or similar to measure coverage
- All error paths must be tested
- All edge cases must be covered

### 7. Documentation

Add clear documentation:
- Inline comments explaining metadata constraints
- Document expected off-chain usage patterns (IPFS, HTTPS URIs)
- Add examples of valid metadata formats
- Document size limits and rationale

### 8. Validation

Before committing:
```bash
# Run all tests
cargo test

# Check for warnings
cargo clippy

# Format code
cargo fmt

# Verify test coverage
cargo tarpaulin --out Html
```

## Expected Commit Message
```
feat(contracts): add per-offering metadata storage

- Add OfferingMetadata DataKey for storing metadata references
- Implement set_offering_metadata() with issuer authorization
- Implement get_offering_metadata() for retrieval
- Add EVENT_METADATA_SET and EVENT_METADATA_UPDATED events
- Enforce 256-byte size limit on metadata strings
- Add comprehensive test suite with 95%+ coverage
- Support IPFS hashes, HTTPS URIs, and other reference formats
```

## Success Criteria
- [ ] All tests pass (`cargo test`)
- [ ] Test coverage ≥ 95%
- [ ] No clippy warnings
- [ ] Code properly formatted
- [ ] Events emit correctly
- [ ] Authorization checks work
- [ ] Freeze/pause mechanisms respected
- [ ] Edge cases handled
- [ ] Documentation complete

## Timeframe
96 hours from start

## Notes
- Metadata is stored as a simple string reference, not the actual content
- Off-chain systems will use these references to fetch actual metadata
- Consider common formats: IPFS CID (Qm...), HTTPS URLs, content hashes
- Metadata is optional - offerings can exist without metadata
- Metadata survives issuer transfers (tied to offering, not issuer)
