# Offering Pagination Stability

## Overview

The Revora Revenue Share contract provides deterministic, stable pagination for all core entities to ensure that clients (e.g., front-ends, indexers) can reliably fetch large sets of data without hitting gas limits or skipping entries.

## Capability

### Core Paginated Getters

1.  **Offerings**: `get_offerings_page(issuer, namespace, start, limit)`
    *   **Ordering**: Registration order (insertion order).
    *   **Stability**: Once an offering is registered, its position in the issuer's list is fixed.
2.  **Issuers**: `get_issuers_page(start, limit)`
    *   **Ordering**: Global registration order.
    *   **Stability**: New issuers are appended to the global list.
3.  **Namespaces**: `get_namespaces_page(issuer, start, limit)`
    *   **Ordering**: Registration order for the specific issuer.
4.  **Periods**: `get_periods_page(issuer, namespace, token, start, limit)`
    *   **Ordering**: Deposit order.
5.  **Blacklist**: `get_blacklist_page(issuer, namespace, token, start, limit)`
    *   **Ordering**: Insertion order.
6.  **Whitelist**: `get_whitelist_page(issuer, namespace, token, start, limit)`
    *   **Ordering**: Lexicographical order by address (standard for Soroban Map keys).
7.  **Pending Periods**: `get_pending_periods_page(issuer, namespace, token, holder, start, limit)`
    *   **Ordering**: Deposit order, starting from the holder's next unclaimed period.

### Stability & Security

*   **Deterministic Ordering**: All paginated responses use stable storage structures (`Vec` or ordered `Map`) to ensure that the order is preserved across different blocks and calls.
*   **Production-Grade Limits**: All `limit` parameters are capped by `MAX_PAGE_LIMIT` (default: 20) to prevent denial-of-service or transaction failure due to high compute/storage costs.
*   **Cursor Behavior**: Functions return a `Option<u32>` as `next_cursor`. If `Some(cursor)` is returned, there are more entries. If `None`, the end of the list has been reached.

## Security Assumptions

*   **Read-Only Safety**: Paginated getters are read-only and do not mutate state. They can be safely called via `simulateTransaction` without gas costs for the user.
*   **Immutability**: Offerings, periods, and issuers are generally append-only. Whitelist/Blacklist can be modified, but their ordering mechanisms (Address keys for Whitelist, Order Vec for Blacklist) remain stable.
*   **Bounded Reads**: Pagination is strictly capped to `MAX_PAGE_LIMIT` (20) to prevent indexer crashes and unbounded memory consumption during iteration. A limit of `0` will default to `MAX_PAGE_LIMIT`. A limit `> 20` will be capped to `20`.

## Developer Notes

*   Always use the returned `next_cursor` for the next call to avoid missing items if the list grows between calls.
*   The `limit` parameter is a suggestion; the contract may return fewer items than requested if the end of the list is reached or if the limit exceeds the internal cap.
