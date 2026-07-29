# feat: block cross-class transfers by default (#522)

## Summary

Adds class-restricted transfer rules to the `RevoraRevenueShare` contract, preventing unauthorized transfers between incompatible share classes (e.g., Class A → Class B). This enforces strict separation between share classes to satisfy legal covenants that require isolation of different investor tiers.

Cross-class transfer attempts revert with `RevoraError::ClassTransferBlocked` (discriminant **58**) and emit a `cls_block` event for off-chain compliance tooling.

## Motivation

Some offerings require strict legal separation between share classes to:
- Satisfy regulatory covenants that mandate isolation of different investor tiers
- Prevent accidental commingling of Class A (e.g., common) and Class B (e.g., preferred) shares
- Provide a deterministic on-chain audit trail when transfers are rejected for class incompatibility

Without this guard, an issuer would have no on-chain mechanism to prevent a Class A holder from transferring shares to a Class B holder, which could violate the offering's governing documents.

## Architecture

### Storage Layer (DataKey2)

Four new persistent storage keys are added to `DataKey2`:

| Key | Type | Purpose |
|-----|------|---------|
| `OfferingClasses(OfferingId)` | `Option<Vec<(ShareClass, ClassConfig)>>` | Configures which share classes exist for an offering and their BPS allocation |
| `HolderShareClass(OfferingId, Address, ShareClass)` | `u32` | Per-class share balance for a holder |
| `TransferRestrictions(OfferingId, Symbol)` | `TransferRestrictions` | Category-based transfer caps |
| `HolderCategory(OfferingId, Address)` | `Symbol` | Transfer category assignment for a holder |
| `CategoryHolderCount(OfferingId, Symbol)` | `u32` | Active count of holders in a transfer category |

### Guard Placement

The class-cross check (Guard 11) is inserted in **both** `transfer_with_attestation` implementations:

1. **Legacy function** (category-based, ~line 5503) — placed after the self-transfer check and before share lookups
2. **Modern function** (attestation-hash-based, ~line 6725) — placed after Guard 5 (offering freeze) and before Guard 6 (blacklist)

### Helper: `get_primary_class`

```
fn get_primary_class(env, offering_id, holder) -> Option<ShareClass>
```

Returns the first share class in which `holder` has a non-zero balance, or `None` if the holder has no class assignment. Used by both transfer guards to compare sender and recipient classes.

### Design Decisions

1. **Single-class check**: The guard compares "primary" classes (first non-zero class per holder). This is intentionally conservative — if Alice holds Class A (1,000) and Class B (500), and Bob holds only Class B, `get_primary_class(Alice) = Class A ≠ get_primary_class(Bob) = Class B`, so the transfer is blocked. This prevents accidental class mixing even for multi-class holders.

2. **Unassigned holders**: When either party has no class assignment (`get_primary_class` returns `None`), the check is **skipped**, ensuring backward compatibility for offerings that do not use classes. This means:
   - Unassigned → Class A: ✅ allowed
   - Class A → Unassigned: ✅ allowed
   - Both unassigned: ✅ allowed

3. **Event emission**: On rejection, a `cls_block` event is emitted with the topic tuple `(EVENT_CLASS_XFER_BLOCK, issuer, namespace, token)` and data `(from, to, from_class, to_class)`, enabling indexers to track compliance failures.

## Error Codes

| Discriminant | Variant | Description |
|---|---|---|
| 58 | `ClassTransferBlocked` | Cross-class share transfer rejected |

## Event Symbols

| Symbol | Name | Description |
|---|---|---|
| `cls_block` | `EVENT_CLASS_XFER_BLOCK` | Emitted when a cross-class transfer is blocked |

## Testing

### Test File: `src/test_class_transfer_lock.rs` (650 lines)

| # | Test Case | Expected |
|---|-----------|----------|
| 1 | Same class (A→A) | ✅ Succeeds |
| 2 | Same class (B→B) | ✅ Succeeds |
| 3 | Cross-class (A→B) | ❌ `ClassTransferBlocked` |
| 4 | Cross-class (B→A) | ❌ `ClassTransferBlocked` |
| 5 | Unassigned → Class A | ✅ Succeeds (backward compat) |
| 6 | Class A → Unassigned | ✅ Succeeds (backward compat) |
| 7 | Both unassigned | ✅ Succeeds (backward compat) |
| 8 | `cls_block` event emitted | 📡 Event present in log |
| 9 | Self-transfer bypass | ❌ `InvalidTransferParticipants` (Guard 3) |
| 10 | Zero-value transfer bypass | ❌ `InvalidShareBps` (Guard 10) |
| 11 | Custom class A → Custom class B | ❌ `ClassTransferBlocked` |
| 12 | Multi-class holder → different class | ❌ `ClassTransferBlocked` |

### Coverage

- All guard paths are exercised (positive and negative)
- Event emission verified for rejection scenarios
- Self-transfer and zero-value transfers confirmed to be caught by earlier guards
- Custom (non-standard) share classes covered
- Multi-class holder edge cases covered

## Security Considerations

1. **Reentrancy**: The class check is a pure storage read — no reentrancy risk.
2. **Authorization**: The guard fires before any state mutation, ensuring no partial state updates on rejection.
3. **Gas**: `get_primary_class` iterates over configured classes (typically 2–3), making it O(n) where n is negligible.
4. **Backward compatibility**: Offerings without class configuration are completely unaffected — `get_primary_class` returns `None`, and the check is skipped.
5. **Front-running**: The check is deterministic and stateless relative to the caller's identities — no front-running surface.

## Migration

No migration is required. The new `DataKey2` variants do not conflict with existing storage, and the class check only activates when `OfferingClasses` is explicitly configured for an offering.

---

Closes #522
