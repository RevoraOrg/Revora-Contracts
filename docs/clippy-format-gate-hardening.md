# Clippy / Format Gate Hardening

## Purpose

Make contribution barriers explicit and auditable. Every check that runs in CI must
also be runnable locally with a single command, and failures must not hide behind
optional developer workflows.

This document is the authoritative reference for:
- Which lint gates are enforced and why
- The exact local commands a reviewer or contributor must run
- How CI wires those same checks
- Security assumptions and risk notes

---

## Local Commands (run before every PR)

These are the **exact** commands CI runs. If they pass locally they will pass in CI.

```bash
# 1. Format check — must produce no diff
cargo fmt --all -- --check

# 2. Clippy — every warning is a hard error
cargo clippy --all-targets --all-features -- -D warnings

# 3. Build
cargo build --release

# 4. Tests — single-threaded for deterministic Soroban output
cargo test -- --test-threads=1
```

> **Why `--all-targets`?** Includes `lib`, `tests`, `benches`, and `examples`.
> Lint suppressions in test files are visible and reviewable.
>
> **Why `--all-features`?** Exercises feature-gated code paths that would otherwise
> be invisible to clippy.
>
> **Why `-D warnings`?** Warnings are not errors by default. Without this flag a
> contributor can push code with active warnings that silently accumulate. In a
> financial contract, every warning is a potential audit finding.

---

## Crate-Level Deny Gates (`src/lib.rs`)

The following `#![deny(...)]` attributes are set at the crate root. They produce
**compile errors** locally and in CI — not just warnings.

| Lint | Rationale |
|------|-----------|
| `unsafe_code` | No unsafe code is permitted in a Soroban WASM contract. |
| `clippy::dbg_macro` | Debug output must never reach production WASM. |
| `clippy::todo` | Incomplete code paths are a security risk; all paths must be explicit. |
| `clippy::unimplemented` | Same rationale as `todo`. |
| `clippy::panic` | Panics in `no_std` WASM abort the host; every failure must return a typed `RevoraError`. |
| `clippy::unwrap_used` | `unwrap()` hides error paths; use `.ok_or(RevoraError::...)` or explicit match. |
| `clippy::expect_used` | Same rationale as `unwrap_used`. |
| `clippy::wildcard_imports` | Explicit imports keep the public API surface auditable. |
| `clippy::manual_let_else` | Prefer let-else for early-return clarity in guard code. |

### Intentional Per-Function Suppressions

`#[allow(clippy::too_many_arguments)]` is used on specific public entry points where
the Soroban ABI requires all parameters to be explicit (e.g. `report_revenue`,
`deposit_revenue_with_snapshot`, `meta_set_holder_share`). Each use is:
- Reviewed per-function, not suppressed globally
- Documented with a comment explaining why
- Not a blanket suppression of the lint

---

## Test File Suppressions Policy

Broad suppressions like `#![allow(warnings)]` or
`#![allow(dead_code, unused_variables, unused_imports)]` are **not permitted** in
test files. They hide real issues and make CI output untrustworthy.

### Permitted pattern

```rust
// Suppress only the specific lint with a comment explaining why.
#![allow(dead_code)] // helper functions not used by every test in this module
```

### Prohibited pattern

```rust
#![allow(warnings)]                              // ← hides everything, prohibited
#![allow(dead_code, unused_variables, unused_imports)] // ← too broad, prohibited
```

### Current status

| File | Before | After |
|------|--------|-------|
| `src/test_utils.rs` | `#![allow(warnings)]` | `#![allow(dead_code)]` with comment |
| `src/chunking_tests.rs` | `#![allow(dead_code, unused_variables, unused_imports)]` | `#![allow(dead_code)]` with comment |

---

## CI Workflow (`ci.yml`)

The CI is split into three sequential jobs to make failures fast and readable:

```
fmt  →  clippy  →  test
```

| Job | Command | Failure mode |
|-----|---------|-------------|
| `fmt` | `cargo fmt --all -- --check` | Hard fail — no auto-commit |
| `clippy` | `cargo clippy --all-targets --all-features -- -D warnings` | Hard fail |
| `test` | `cargo test -- --test-threads=1` | Hard fail |

### Key changes from previous CI

| Before | After | Reason |
|--------|-------|--------|
| `cargo fmt --check \|\| (cargo fmt && auto-commit)` | `cargo fmt --all -- --check` (hard fail) | Auto-commit silently hid formatting failures from PR authors |
| `cargo clippy --all-targets -- -D warnings` | `cargo clippy --all-targets --all-features -- -D warnings` | `--all-features` was missing; feature-gated paths were not linted |
| Single `test` job (fmt + clippy + test combined) | Three separate jobs with `needs:` dependency | Faster feedback; fmt/clippy failures don't waste test runner time |
| No `RUSTFLAGS=-D warnings` env | `RUSTFLAGS="-D warnings"` set globally | Ensures `rustc` warnings (not just clippy) are also hard errors |

---

## Security Assumptions

1. **Lint gates are not a substitute for code review.** They catch mechanical issues;
   logic errors require human review.

2. **`-D warnings` in CI matches `#![deny(...)]` in source.** If a lint is denied
   in source but not in CI (or vice versa), the gates diverge and one can be bypassed.
   This document and the CI file must be kept in sync.

3. **Per-function `#[allow(...)]` suppressions are an audit surface.** Every
   suppression in production code (`src/lib.rs`, `src/vesting.rs`) must have a
   comment explaining why it is safe. Reviewers should treat unexplained suppressions
   as a red flag.

4. **Test file suppressions do not affect production WASM.** However, they can hide
   real bugs in test logic (e.g. unused variables that should be assertions). Targeted
   suppressions with comments are required.

5. **`unsafe_code` deny is enforced at the crate root.** Soroban contracts must not
   use unsafe code. Any attempt to add `unsafe` will fail to compile.

---

## Risk Notes

- **Divergence risk**: If a developer adds a new `#[allow(...)]` in production code
  without a comment, it will pass CI (the allow overrides the deny). Code review is
  the last line of defence here.

- **`--all-features` assumption**: This project currently has no Cargo features
  defined. If features are added in the future, the CI command already covers them.
  No CI change will be needed.

- **`--test-threads=1`**: Soroban's test environment is not thread-safe. Parallel
  test execution can produce non-deterministic failures. This flag must not be removed.

---

## Reviewer Checklist

Before approving any PR that touches `src/lib.rs` or test files:

- [ ] `cargo fmt --all -- --check` passes with no diff
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes with zero diagnostics
- [ ] No new `#![allow(warnings)]` or broad suppression added to any file
- [ ] Any new `#[allow(...)]` in production code has an explanatory comment
- [ ] `cargo test -- --test-threads=1` passes
