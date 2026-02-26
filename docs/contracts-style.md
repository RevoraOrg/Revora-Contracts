# Contracts Linting and Style Guide

This document defines the minimum linting and code style expectations for the contracts crate.

## Linting baseline

The crate root (`src/lib.rs`) enforces:

- `#![deny(unsafe_code)]`
- `#![deny(clippy::dbg_macro, clippy::todo, clippy::unimplemented)]`

These lints are part of the contract safety baseline and should not be removed without maintainer approval.

## Required local checks

Run these commands before opening a PR:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Style expectations

- Keep formatting fully `rustfmt`-compatible; avoid manual alignment.
- Prefer `Result<_, RevoraError>` for recoverable contract failures.
- Use panics only for host-level auth/invariant failures that are intended to abort execution.
- Keep public entrypoints and storage types documented with concise `///` comments.
- If a lint must be suppressed, scope it to the smallest item and add a one-line justification above the attribute.
