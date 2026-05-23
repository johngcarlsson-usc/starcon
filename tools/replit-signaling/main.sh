#!/usr/bin/env bash
# Install matchbox_server on first run (cached in Replit's
# persistent disk for subsequent runs), then launch it bound
# to the port Replit assigns us.

set -euo pipefail

BIN="$HOME/.cargo/bin/matchbox_server"

if [ ! -x "$BIN" ]; then
    echo "First run: compiling matchbox_server (~5 min)..."
    cargo install --locked matchbox_server@0.14.0
fi

# Replit assigns an external port; the process should bind to
# 0.0.0.0 on whatever PORT is set (defaults to 3536, which the
# .replit file maps from external 80).
PORT="${PORT:-3536}"
exec "$BIN" "0.0.0.0:${PORT}"
