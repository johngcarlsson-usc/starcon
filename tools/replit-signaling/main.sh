#!/usr/bin/env bash
# Workspace-only entrypoint. Compiles matchbox_server on first
# run (cached in `./bin/` for subsequent runs and for the
# deployment snapshot), then launches it bound to the port
# Replit assigns us.
#
# For the production deployment, see the `[deployment]` block
# in `.replit` — that uses a separate `build` step so the
# compile doesn't fight Replit's port-open health-check
# timeout.

set -euo pipefail

BIN="./bin/matchbox_server"

if [ ! -x "$BIN" ]; then
    echo "First run: compiling matchbox_server (~5 min)..."
    cargo install --locked --root . matchbox_server@0.14.0
fi

# Bind to whatever port Replit assigns via $PORT (defaults to
# 3536 for local dev — matches `.replit`'s `[[ports]]` hint).
PORT="${PORT:-3536}"
echo "Starting matchbox_server on 0.0.0.0:${PORT}"
exec "$BIN" "0.0.0.0:${PORT}"
