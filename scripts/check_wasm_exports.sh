#!/usr/bin/env bash
#
# Build the release contract wasm and assert that no `test_*` helper is
# exported. Issue #1308: `test_insert_period` / `test_set_last_claimed_idx`
# mutate a holder's claim cursor and period revenue with no authorization and no
# token transfer, so they must not exist in the deployed contract.
#
# Run locally with: ./scripts/check_wasm_exports.sh
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="wasm32-unknown-unknown"
WASM="target/${TARGET}/release/revora_contracts.wasm"

# Default feature set only: `testutils` would (intentionally) re-export the
# helpers, and the deployed artifact is built without it.
cargo build --release --target "${TARGET}" -p revora-contracts

if [ ! -f "${WASM}" ]; then
  echo "error: expected wasm artifact at ${WASM}" >&2
  exit 1
fi

python3 scripts/check_wasm_exports.py "${WASM}"
