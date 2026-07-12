#!/usr/bin/env bash
# Publish the rabbitui name-reservation placeholders to crates.io.
# Requires `cargo login` to have been run. Crates are independent, so order
# doesn't matter; the sleep stays under crates.io's new-crate rate limit.
set -euo pipefail
cd "$(dirname "$0")"

# -agent deliberately last (also dodges the 5-new-crates burst limit).
CRATES=(rabbitui rabbitui-core rabbitui-ratatui rabbitui-testing rabbitui-widgets rabbitui-agent)

for c in "${CRATES[@]}"; do
    # --allow-dirty: this is a jj workspace; git sees the files as untracked.
    cargo publish -p "$c" --allow-dirty "$@"
    sleep 15
done
