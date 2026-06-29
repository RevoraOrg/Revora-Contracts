# Structured Error Coverage Expansion

## Overview

Every `RevoraError` variant has a fixed numeric discriminant, a precise trigger
condition, and a dedicated test in `src/structured_error_tests.rs`.

## Security assumptions

| Assumption | Enforcement |
|---|---|
| Auth failures (wrong signer) are host panics, not `RevoraError` | `require_auth()` panics; use `try_*` client methods to catch contract errors |
| Discriminants are immutable | Enforced by `sec_all_discriminants_unique_and_contiguous` test |
| All typed errors are reachable without auth panics | One test per variant in `structured_error_tests.rs` |
| `NotInitialized` surfaces before any admin-gated operation | `require_admin()` returns `Err(NotInitialized)` when `DataKey::Admin` absent |
| `UnauthorizedTransferAccept` is a typed error, not a panic | `accept_issuer_transfer` checks `caller == new_issuer` before `require_auth` |

## Error reference

| # | Variant | Discriminant | Trigger | Condition |
|---|---|---|---|---|
| 1 | `InvalidRevenueShareBps` | 1 | `register_offering` | `revenue_share_bps > 10_000` |
| 2 | `LimitReached` | 2 | `set_admin`, fees, multisig | Admin already set; fee > 5 000 bps |
| 3 | `ConcentrationLimitExceeded` | 3 | `report_revenue` | `enforce=true` and concentration > `max_bps` |
| 4 | `OfferingNotFound` | 4 | Any issuer-gated entrypoint | Offering not registered |
| 5 | `PeriodAlreadyDeposited` | 5 | `deposit_revenue` | `period_id` already deposited |
| 6 | `NoPendingClaims` | 6 | `claim` | `share_bps == 0` or all periods claimed |
| 7 | `HolderBlacklisted` | 7 | `claim` | Holder is blacklisted |
| 8 | `InvalidShareBps` | 8 | `set_holder_share` | `share_bps > 10_000` |
| 9 | `PaymentTokenMismatch` | 9 | `deposit_revenue` | `payment_token` != token locked by the first successful deposit |
| 10 | `ContractFrozen` | 10 | All state-mutating entrypoints | `DataKey::Frozen` is `true` |
| 11 | `ClaimDelayNotElapsed` | 11 | `claim` | Deposit time + delay > `now` |
| 12 | `SnapshotNotEnabled` | 12 | `deposit_revenue_with_snapshot` | Snapshots not enabled |
| 13 | `OutdatedSnapshot` | 13 | `deposit_revenue_with_snapshot` | `snapshot_reference <= last_snapshot_ref` |
| 14 | `PayoutAssetMismatch` | 14 | `report_revenue` | `payout_asset` != offering's registered asset |
| 15 | `IssuerTransferPending` | 15 | `propose_issuer_transfer` | Transfer already pending |
| 16 | `NoTransferPending` | 16 | `cancel/accept_issuer_transfer` | No pending transfer |
| 17 | `UnauthorizedTransferAccept` | 17 | `accept_issuer_transfer` | `caller != nominated_new_issuer` |
| 18 | `MetadataTooLarge` | 18 | `set_offering_metadata` | `metadata.len() > 256` |
| 19 | `NotAuthorized` | 19 | `meta_set_holder_share` | Signer is not the configured delegate |
| 20 | `NotInitialized` | 20 | `set_testnet_mode` | `DataKey::Admin` absent |
| 21 | `InvalidAmount` | 21 | `report_revenue`, `deposit_revenue` | `amount < 0` or `<= 0` |
| 22 | `InvalidPeriodId` | 22 | `deposit_revenue` | `period_id == 0` |
| 23 | `SupplyCapExceeded` | 23 | `deposit_revenue` | Cumulative deposits > `supply_cap` |
| 24 | `MetadataInvalidFormat` | 24 | `set_offering_metadata` | No recognised scheme prefix |
| 25 | `ReportingWindowClosed` | 25 | `report_revenue` | Outside reporting window |
| 26 | `ClaimWindowClosed` | 26 | `claim` | Outside claiming window |
| 27 | `SignatureExpired` | 27 | `meta_set_holder_share` | `expiry < ledger.timestamp()` |
| 28 | `SignatureReplay` | 28 | `meta_set_holder_share` | Nonce already consumed |
| 29 | `SignerKeyNotRegistered` | 29 | `meta_set_holder_share` | No ed25519 key registered |
| 30 | `ShareSumExceeded` | 30 | `set_holder_share` | Aggregate > 10 000 bps |

## Breaking change

`accept_issuer_transfer` now takes `caller: Address` as the first parameter.
Pass the accepting address explicitly. This enables `UnauthorizedTransferAccept`
as a typed error instead of a host panic.

## Run tests

```bash
cargo test structured_error
```
