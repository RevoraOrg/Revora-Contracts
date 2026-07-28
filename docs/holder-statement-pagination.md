# Holder Statement Pagination

`get_holder_statement_page` adds cursor-based pagination for holder statement reads so long accrual histories can be fetched under Soroban gas limits.

## Behavior

- The cursor is a zero-based `PeriodEntry` index.
- Cursors are clamped to the holder's current `LastClaimedIdx`, so stale callers cannot page back into already-claimed rows.
- Rows are returned in deterministic `period_id` order.
- `limit` is capped to `MAX_PAGE_LIMIT`.
- A cursor past the last available row returns an empty page with `next_cursor = None`.
- If the first unprocessed period is still behind the claim delay barrier, the page stops there and returns that index as `next_cursor`.

## Security notes

- The page API is read-only and uses the same delay-boundary rules as claim previews, so it must not overstate what a holder can currently claim.
- Blacklisted holders and closed claim windows return an empty page rather than partially exposing inaccessible rows.
- Cursor stability holds for repeated queries against the same on-chain period set; adding new periods may extend later pages, but it does not reorder existing rows.
