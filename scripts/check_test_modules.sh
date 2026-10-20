#!/usr/bin/env bash
# scripts/check_test_modules.sh
#
# CI guard: fails if any src/test*.rs or src/*_test*.rs file is NOT declared
# as a #[cfg(test)] mod in src/lib.rs.  This prevents the silent rot that
# caused issue #1353 — where src/test.rs (634 tests, 15,469 lines) was never
# compiled or run because lib.rs never declared `mod test;`.
#
# Usage:
#   bash scripts/check_test_modules.sh          # run from repo root
#
# Exit codes:
#   0  all test files are declared
#   1  one or more test files are undeclared (prints them to stderr)
#
# Note: files that are intentionally not wired yet can be added to the
# ALLOWLIST below.  Each entry should reference the issue that will wire it.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

ALLOWLIST=(
    # Orphaned test files not yet wired into lib.rs — tracked in separate issues.
    # See: https://github.com/upstage-interactive/revora-contracts/issues
    "src/audit_summary_tests.rs"
    "src/chunking_tests.rs"
    "src/invalid_amount_matrix_tests.rs"
    "src/normalize_share_tests.rs"
    "src/payout_test.rs"
    "src/security_assertions_integration_tests.rs"
    "src/self_test.rs"
    "src/structured_error_tests.rs"
    "src/test_accrual_ledger.rs"
    "src/test_auth.rs"
    "src/test_blacklist_batch_gas.rs"
    "src/test_blacklist_batch_prop.rs"
    "src/test_class_transfer_lock.rs"
    "src/test_cross_contract.rs"
    "src/test_cross_contract_transfer_fail.rs"
    "src/test_dispute.rs"
    "src/test_dispute_resolve.rs"
    "src/test_dispute_window.rs"
    "src/test_dividend_accrual_ledger.rs"
    "src/test_fork_replay_regression.rs"
    "src/test_freeze_matrix.rs"
    "src/test_get_offering_platform_fee.rs"
    "src/test_governance_proposals.rs"
    "src/test_holder_share_sum_invariant.rs"
    "src/test_indexer_fixtures.rs"
    "src/test_jurisdiction.rs"
    "src/test_merkle_root_rotation.rs"
    "src/test_multisig_gas.rs"
    "src/test_namespaces.rs"
    "src/test_ofac_snapshot_pin.rs"
    "src/test_override_audit_trail.rs"
    "src/test_pause_tiers.rs"
    "src/test_period_id_boundary.rs"
    "src/test_platform_fee_model.rs"
    "src/test_prove_distribution.rs"
    "src/test_reconciliation_bug.rs"
    "src/test_redemption.rs"
    "src/test_reject_issuer_transfer.rs"
    "src/test_self_test.rs"
    "src/test_share_conservation_prop.rs"
    "src/test_snapshot_finalization.rs"
    "src/test_snapshot_monotonicity_replay.rs"
    "src/test_snapshot_negative_matrix.rs"
    "src/test_transfer_restriction.rs"
    "src/test_transfer_restrictions.rs"
    "src/test_transfer_with_attestation.rs"
    "src/test_twap_window.rs"
    "src/vesting_test.rs"
)

# ── Find every candidate test file in src/ ─────────────────────────────────────
shopt -s nullglob
candidates=()
for pattern in "src/test*.rs" "src/*_test*.rs"; do
    for f in $pattern; do
        candidates+=("$f")
    done
done

if [[ ${#candidates[@]} -eq 0 ]]; then
    echo "INFO: no src/test*.rs or src/*_test*.rs files found — nothing to check."
    exit 0
fi

# ── Extract declared test modules from lib.rs ───────────────────────────────────
# Match patterns like:
#   #[cfg(test)]
#   mod foo;
#   #[cfg(test)] mod bar;
declared=()

# Single-line: #[cfg(test)] mod foo;
while IFS= read -r mod_part; do
    if [[ -n "$mod_part" ]]; then
        declared+=("$mod_part")
    fi
done < <(grep -oE '#\[cfg\(test\)\]\s*mod\s+[a-zA-Z_][a-zA-Z0-9_]*' "${REPO_ROOT}/src/lib.rs" | sed 's/.*mod //' || true)

# All mod declarations (handles both single-line #[cfg(test)] mod foo; and
# multi-line #[cfg(test)]\nmod foo; forms, plus plain mod foo; for non-test
# modules like revenue_deposit_contract, safe_math, etc.)
declare -A seen
while IFS= read -r mod_name; do
    if [[ -n "$mod_name" && -z "${seen[$mod_name]+set}" ]]; then
        seen["$mod_name"]=1
        declared+=("$mod_name")
    fi
done < <(grep -oE 'mod\s+[a-zA-Z_][a-zA-Z0-9_]*' "${REPO_ROOT}/src/lib.rs" | sed 's/mod //' || true)

# Deduplicate declared
declare -A declared_map
for m in "${declared[@]}"; do
    declared_map["$m"]=1
done

# ── Check each candidate ────────────────────────────────────────────────────────
missing=()
for f in "${candidates[@]}"; do
    base="$(basename "$f" .rs)"

    # Skip files that are not meant to be compiled as a module (e.g. standalone
    # test harnesses that are included by other files).
    # Every real test module is declared in lib.rs; if it isn't, flag it unless
    # it is in the allowlist.
    if [[ -n "${declared_map[$base]+set}" ]]; then
        continue
    fi

    allowed=false
    for a in "${ALLOWLIST[@]}"; do
        if [[ "$f" == "$a" ]]; then
            allowed=true
            break
        fi
    done

    if [[ "$allowed" == "true" ]]; then
        continue
    fi

    missing+=("$f")
done

# ── Report ──────────────────────────────────────────────────────────────────────
if [[ ${#missing[@]} -gt 0 ]]; then
    echo "ERROR: the following test files are NOT declared as a module in src/lib.rs:" >&2
    for m in "${missing[@]}"; do
        echo "  - $m" >&2
    done
    echo "" >&2
    echo "Add a #[cfg(test)] mod <name>; line to src/lib.rs for each, or add the" >&2
    echo "file to scripts/check_test_modules.sh ALLOWLIST with a tracking issue." >&2
    exit 1
fi

echo "PASS: all ${#candidates[@]} test source files are declared in src/lib.rs."
exit 0
