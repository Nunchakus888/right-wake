#!/bin/sh
# The cached plugin is self-contained; the native binary is installed separately.
set -eu
if [ -n "${WAKING_BIN:-}" ]; then
  binary=$WAKING_BIN
elif [ -x "$HOME/.local/bin/waking" ]; then
  binary=$HOME/.local/bin/waking
elif command -v waking >/dev/null 2>&1; then
  binary=$(command -v waking)
else
  printf 'Waking binary not found. Run the waking-setup skill or follow the repository installation guide.\n' >&2
  exit 127
fi
[ -f "$binary" ] && [ -x "$binary" ] || { printf 'WAKING_BIN must identify an executable native binary.\n' >&2; exit 127; }
if [ -n "${WAKING_DATA_DIR:-}" ]; then
  exec "$binary" --data-dir "$WAKING_DATA_DIR" mcp
fi
exec "$binary" mcp
