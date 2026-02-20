use soroban_sdk::{contracttype, Address};

/// Storage keys for all persistent data.
///
/// - `Blacklist(token)` → `Map<Address, bool>` — per-offering blacklist
/// - `Status(token)`    → `OfferingStatus`     — current lifecycle state
/// - `Issuer(token)`    → `Address`            — stored at registration
#[contracttype]
pub enum DataKey {
    Blacklist(Address),
    Status(Address),
    Issuer(Address),
}

/// Offering lifecycle states.
///
/// Allowed transitions:
/// ```
///  Active ──pause──▶ Paused ──resume──▶ Active
///  Active ──close──▶ Closed  (terminal)
///  Paused ──close──▶ Closed  (terminal)
///  Closed ──*──────▶ ❌ panics — no recovery
/// ```
#[contracttype]
#[derive(Clone, PartialEq, Debug)]
pub enum OfferingStatus {
    Active,
    Paused,
    Closed,
}