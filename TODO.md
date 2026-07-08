# TODO

## feat/multisig-administered-offerings (#453)

1. Inspect `src/lib.rs` around existing offering registration (`register_offering`) and multisig-related storage/API.
2. Implement new entrypoint `register_offering_multisig` in `src/lib.rs`.
   - Inputs: same as `register_offering` plus `multisig: Address`.
   - Auth: require_auth resolution expectation documented against multisig.
   - Fail-fast: cross-contract call `multisig.get_threshold()`; fail cleanly if call fails or method missing.
   - Persist: store canonical issuer principal as `issuer` after validation.
   - Events/indexer mapping: ensure `off_reg`/offering register event includes `is_multisig_admin=true` (or equivalent documented mapping).
3. Add any required storage keys/flags in `src/lib.rs` to mark multisig-administered offerings.
4. Add tests to `src/test_multisig_gas.rs` (or new test file if needed):
   - success: multisig implements `get_threshold()`; registration succeeds.
   - failure: multisig does not implement `get_threshold()`; registration fails cleanly.
5. Validate edge cases: duplicate offering idempotency; ensure no partial writes on failed multisig validation.
6. Run `cargo test --all` and `cargo clippy --all-targets --all-features -- -D warnings`.
7. Update documentation/comments where needed for indexers and security notes.

