# Storage Stress Automation

## Overview

The Storage Stress Automation capability provides a secure, deterministic, and isolated mechanism to test the ledger limits and evaluate the contract's capacity for scale within the Soroban execution environment. 

It allows the contract administrator to rapidly provision arbitrary limits of dummy storage records without negatively impacting existing production state or polluting the application namespaces. The records are placed under a special `StressData` namespace (internally `StressDataEntry` and `StressDataCount`), segregated entirely from real offerings and users.

## Security Assumptions

- **Restricted Access:** Only the initialized `Admin` account can invoke stress test functions (`automate_storage_stress` and `cleanup_storage_stress`). 
- **Gas Bounding:** `automate_storage_stress` enforces strict boundaries out-of-the-box (`MAX_RECORDS_PER_TX = 1000` and `MAX_PAYLOAD_SIZE = 10000`). This ensures transactions strictly remain within realistic operational parameters, mitigating DoS concerns related to infinite loops in an invocation.
- **State Segregation:** Generated stress data is mapped exclusively to `DataKey::StressDataEntry(Address, u32)` and tracked by `DataKey::StressDataCount(Address)`. These keys are not queryable through normal business endpoints and will never accidentally corrupt offering or holder information.

## How to Test

You can test this functionality with following sequence of operations. This requires you to load the contract as the admin account.

### Step 1: Provisioning Data
Call `automate_storage_stress` directly or locally with your configured testbed.

```bash
soroban contract invoke --id [CONTRACT_ID] --source [ADMIN_ACCOUNT] \
  -- automate_storage_stress \
  --caller [ADMIN_ACCOUNT] \
  --record_count 150 \
  --payload_size_bytes 100
```
This writes 150 unique records, each 100 bytes long, resolving instantly and proving successful execution limits. It returns the current cumulative storage stress index.

### Step 2: Hitting Protocol Limits (Optional)
Attempt to break the hard-coded limits. The contract safely rejects these requests, protecting the platform:
```bash
soroban contract invoke --id [CONTRACT_ID] --source [ADMIN_ACCOUNT] \
  -- automate_storage_stress \
  --caller [ADMIN_ACCOUNT] \
  --record_count 1500 \
  --payload_size_bytes 100
```
Expect an `Err(LimitReached)`.

### Step 3: Cleanup Operations
Storage space is highly valuable, so reclaiming it post-testing is essential. You can clean up generated data via pagination-like chunking to avoid CPU exhaustion.

```bash
soroban contract invoke --id [CONTRACT_ID] --source [ADMIN_ACCOUNT] \
  -- cleanup_storage_stress \
  --caller [ADMIN_ACCOUNT] \
  --max_remove 100
```
Returns the number of removed instances (would be 100). Re-run the command with `max_remove=50` to completely wipe out the generated test data.

## Metrics and Analytics
The stress tests emit the following predictable events, easily ingestible by indexers testing analytics pipelines:
- `stress`: `([Admin_Address]), ([Record_Count], [Payload_Size], [Cumulative_Count])`
- `clnstrss`: `([Admin_Address]), ([Removed_Count], [Remaining_Count])`
