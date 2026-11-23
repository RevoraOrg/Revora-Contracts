# Issuer quorum authentication

`register_offering` lets the primary issuer declare co-issuers and a `quorum`
(the number of distinct issuer signatures required on privileged calls). This
document describes what that threshold means on-chain.

## Rule

A privileged issuer entrypoint (deposits, snapshot operations, offering
configuration, issuer-transfer proposals, …) requires a `signers` vector. The
contract then:

1. **Deduplicates** `signers`, so repeating an address never counts twice.
2. Rejects any signer that is neither the primary issuer nor a co-issuer with
   `NotAuthorized`.
3. Calls `require_auth()` on each remaining signer.
4. Requires the **primary issuer to be among the signers**. The primary is the
   offering's root of authority, so co-issuers can never reach quorum on their
   own.
5. Requires at least `quorum` distinct signers, otherwise
   `QuorumNotReached` (code 93).

The `signers` argument is the last parameter of every affected entrypoint:

```rust
// 2-of-3 offering: primary + one co-issuer is sufficient.
client.deposit_revenue(
    &primary,
    &namespace,
    &token,
    &payment_token,
    &amount,
    &period_id,
    &Vec::from_array(&env, [primary.clone(), co_issuer.clone()]),
);
```

Passing only `[primary]` on a quorum-2 offering fails with
`QuorumNotReached`; passing `[primary, co_issuer, co_issuer]` succeeds because
the duplicate is collapsed.

## Registration validation

`register_offering` rejects configurations that would make the threshold
ambiguous or gameable:

| Condition | Error |
| --- | --- |
| `quorum == 0` or `quorum > 1 + co_issuers.len()` | `LimitReached` (existing) |
| a co-issuer is repeated | `InvalidCoIssuers` (94) |
| the primary issuer also appears in `co_issuers` | `InvalidCoIssuers` (94) |

These checks ensure one key can never occupy two slots, which would otherwise
let a single address satisfy a higher quorum than intended.

## Scope

The rule covers issuer-authenticated entrypoints that already called
`require_issuer_quorum_auth`. It does not change holder-facing operations,
`accept_issuer_transfer`, `set_holder_share_class`, or the separate
contract-level multisig (`init_multisig` / `approve_action` / `check_quorum`).
Single-issuer offerings (`quorum == 1`) keep working unchanged: callers pass
`[primary]`.

## Tests

`tests/issuer_quorum_auth.rs` covers the 2-of-3 happy path, primary-only
rejection, co-issuers-only rejection, duplicate-signer collapsing (including
attempts to inflate past quorum), non-issuer rejection, both registration
validation failures, and the single-issuer case.
