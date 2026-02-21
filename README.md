# Revora Contracts — Event Schema Versioning

This repository contains the `RevoraRevenueShare` Soroban contract. This document describes the event formats and versioning scheme used for emitted events so off-chain consumers can remain compatible across upgrades.

## Versioning strategy
- Event versions are included as an extra topic element (not in the event data payload). Adding a version topic is backward-compatible because existing consumers that only inspect the event payload remain unchanged. Consumers that filter by topics should include the version when they want to pin to a specific schema.

## Current event formats (v1)

- `offer_reg` (offering registration)
  - Topic: `("offer_reg", issuer_address, "offer_v1")`
  - Payload: `(token_address, revenue_share_bps)`

- `rev_rep` (revenue reported)
  - Topic: `("rev_rep", issuer_address, token_address, "rev_v1")`
  - Payload: `(amount, period_id, blacklist)` — where `blacklist` is an array of addresses.

## Version history
- v1 (current)
  - `offer_v1` — initial schema for offering registration events
  - `rev_v1` — initial schema for revenue report events

## Compatibility notes
- Adding the version as a topic keeps the payload shape unchanged for v1 consumers.
- Future schema changes should introduce a new version symbol (e.g. `offer_v2`) and may change payload structure. Consumers should explicitly check the version topic and handle known versions; unknown newer versions should be ignored or handled defensively.

## Tests and guarantees
- Tests validate that the version symbols are present in event topics.
- The design aims to allow future non-breaking upgrades by adding versioned topics first, then moving to new payloads under new version symbols.
