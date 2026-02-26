# Per-Offering Metadata - Quick Start Guide

## Overview
Attach off-chain metadata references (IPFS hashes, URLs, content hashes) to your revenue-sharing offerings.

## Basic Usage

### 1. Set Metadata
```rust
use soroban_sdk::String;

let metadata = String::from_str(&env, "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG");
client.set_offering_metadata(&issuer, &token, &metadata)?;
```

### 2. Get Metadata
```rust
let metadata = client.get_offering_metadata(&issuer, &token);
match metadata {
    Some(meta) => println!("Metadata: {}", meta),
    None => println!("No metadata set"),
}
```

### 3. Update Metadata
```rust
let new_metadata = String::from_str(&env, "https://example.com/new-metadata.json");
client.set_offering_metadata(&issuer, &token, &new_metadata)?;
```

## Supported Formats

### IPFS CID
```rust
"QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG"
```

### HTTPS URL
```rust
"https://api.example.com/metadata/token123.json"
```

### Content Hash
```rust
"0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
```

## Constraints

- **Max Length:** 256 bytes
- **Authorization:** Only current issuer can set/update
- **Offering:** Must exist before setting metadata
- **State:** Blocked when contract is frozen or paused

## Events

### Metadata Set (First Time)
```
Event: meta_set
Topics: [issuer, token]
Data: metadata_string
```

### Metadata Updated
```
Event: meta_upd
Topics: [issuer, token]
Data: metadata_string
```

## Error Codes

- `OfferingNotFound` - Offering doesn't exist or caller not issuer
- `MetadataTooLarge` - Metadata exceeds 256 bytes
- `ContractFrozen` - Contract is frozen
- Panic - Contract is paused or auth failed

## Common Patterns

### IPFS Workflow
```rust
// 1. Upload to IPFS
let cid = ipfs_client.add(metadata_json)?;

// 2. Store CID on-chain
let metadata = String::from_str(&env, &cid);
client.set_offering_metadata(&issuer, &token, &metadata)?;

// 3. Retrieve and fetch
let cid = client.get_offering_metadata(&issuer, &token).unwrap();
let metadata_json = ipfs_client.get(&cid)?;
```

### URL Workflow
```rust
// 1. Host metadata
// https://api.example.com/metadata/token123.json

// 2. Store URL on-chain
let url = String::from_str(&env, "https://api.example.com/metadata/token123.json");
client.set_offering_metadata(&issuer, &token, &url)?;

// 3. Retrieve and fetch
let url = client.get_offering_metadata(&issuer, &token).unwrap();
let metadata_json = http_client.get(&url)?;
```

## Best Practices

1. **Validate before storing** - Check format and length client-side
2. **Use IPFS for immutability** - Content-addressed storage
3. **Use URLs for flexibility** - Easy updates without on-chain changes
4. **Include version in metadata** - Support schema evolution
5. **Cache off-chain** - Reduce redundant fetches
6. **Listen for events** - Stay synced with metadata changes

## Example Metadata JSON

```json
{
  "version": "1.0",
  "name": "Example Token Offering",
  "description": "Revenue-sharing offering for Example Token",
  "image": "ipfs://QmImage123",
  "properties": {
    "category": "DeFi",
    "website": "https://example.com",
    "documentation": "https://docs.example.com"
  }
}
```

## Testing

```rust
#[test]
fn test_metadata_workflow() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // Register offering
    client.register_offering(&issuer, &token, &1000, &token);

    // Set metadata
    let metadata = String::from_str(&env, "ipfs://QmTest");
    client.set_offering_metadata(&issuer, &token, &metadata).unwrap();

    // Verify
    let retrieved = client.get_offering_metadata(&issuer, &token);
    assert_eq!(retrieved, Some(metadata));
}
```

## Troubleshooting

### "MetadataTooLarge" Error
- Check metadata length: `metadata.len() <= 256`
- Use shorter URLs or IPFS CIDs
- Store large data off-chain, reference on-chain

### "OfferingNotFound" Error
- Verify offering exists: `client.get_offering(issuer, token)`
- Check you're using the current issuer address
- Ensure offering was registered successfully

### Authorization Failures
- Verify caller is the current issuer
- Check issuer hasn't been transferred
- Ensure proper auth setup in tests (`env.mock_all_auths()`)

### Contract Frozen/Paused
- Check contract state: `client.is_frozen()`, `client.is_paused()`
- Wait for unpause or contact admin
- Read operations still work when frozen/paused
