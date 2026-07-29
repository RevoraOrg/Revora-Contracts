#!/usr/bin/env bash
# scripts/check_event_sunset.sh
#
# CI guard: verifies that indexer/event_sunset.json is in sync with
# docs/EVENT_SUNSET.yaml and that every entry passes structural validation.
#
# Usage:
#   bash scripts/check_event_sunset.sh          # run from repo root
#
# Exit codes:
#   0  all checks pass
#   1  Python not found or generator failed validation
#   2  JSON is out of sync with YAML (run: python3 scripts/gen_event_sunset.py)
#
# This script is intentionally minimal: it delegates all business logic to
# scripts/gen_event_sunset.py and only provides a human-friendly wrapper.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# ── Locate Python ──────────────────────────────────────────────────────────────
PYTHON=""
for candidate in python3 python; do
    if command -v "${candidate}" &>/dev/null; then
        PYTHON="${candidate}"
        break
    fi
done

if [[ -z "${PYTHON}" ]]; then
    echo "ERROR: python3 (or python) not found in PATH." >&2
    echo "       Install Python 3.8+ to run this check." >&2
    exit 1
fi

# ── Run generator in --check mode ─────────────────────────────────────────────
echo "==> Checking event sunset table drift..."
echo "    Source : ${REPO_ROOT}/docs/EVENT_SUNSET.yaml"
echo "    Output : ${REPO_ROOT}/indexer/event_sunset.json"

if "${PYTHON}" "${REPO_ROOT}/scripts/gen_event_sunset.py" --check; then
    echo ""
    echo "✓  event_sunset.json is in sync with EVENT_SUNSET.yaml"
    exit 0
else
    status=$?
    echo ""
    echo "✗  Check failed (exit ${status})."
    echo ""
    echo "   To fix: run the generator and commit the result:"
    echo "     python3 scripts/gen_event_sunset.py"
    echo "     git add indexer/event_sunset.json"
    echo "     git commit -m 'chore: regenerate event_sunset.json'"
    exit "${status}"
fi
