TODO: #477 proptest harness for blacklist_add_many/remove_many order-independence

1. Update src/test_blacklist_batch_prop.rs:
   - Scope event collection to only events emitted by blacklist_add_many/remove_many (use before/after event indices).
   - Enforce empty-input no-op semantics: if addrs empty => no bl_add events; if rem_addrs empty => no bl_rem events.
   - Strengthen duplicate/overlap coverage in vectors.
   - Assert final blacklist state equality as deduped set (sorted unique addresses).
   - Assert emitted blacklist events equality as order-independent multiset.
2. Run cargo test --all.
3. Ensure all lint/format is clean.
4. Commit as: test: add proptest for blacklist batch order-independence.
5. Push branch and open PR.

