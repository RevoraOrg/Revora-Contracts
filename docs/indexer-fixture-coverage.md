# Indexer Fixture Coverage

Issue: #187

## Summary

This change adds deterministic, contract-native fixture topics for indexer validation.

Implemented in:
- src/lib.rs
- src/test_indexer_fixtures.rs

## Why

Indexer integrations can fail due to schema drift, event ordering assumptions, or incorrect period handling. These fixtures provide a stable reference that can be consumed by CI and by downstream indexers.

## New API

`get_indexer_fixture_topics(issuer, namespace, token, period_id) -> Vec<EventIndexTopicV2>`

Returns canonical v2 topics in stable order:
1. `offer` (period `0`)
2. `rv_init` (period `period_id`)
3. `rv_ovr` (period `period_id`)
4. `rv_rej` (period `period_id`)
5. `rv_rep` (period `period_id`)
6. `claim` (period `0`)

All fixtures are versioned with `version = 2`.

## Security and Reliability Notes

- Fixtures are read-only and do not mutate state.
- Payload is deterministic for a given input tuple.
- Canonical ordering prevents nondeterministic fixture regressions across environments.

## Tests

Added deterministic tests:
- `fixture_topics_have_stable_order_and_shape`
- `fixture_topics_bind_to_requested_identity`

These validate event ordering, event type identity, version pinning, and issuer/namespace/token binding.
