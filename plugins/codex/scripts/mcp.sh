#!/bin/sh
# The cached plugin is self-contained; the native binary is installed separately.
set -eu
if [ -n "${RIGHT_WAKE_BIN:-}" ]; then
  binary=$RIGHT_WAKE_BIN
elif [ -x "$HOME/.local/bin/right-wake" ]; then
  binary=$HOME/.local/bin/right-wake
elif command -v right-wake >/dev/null 2>&1; then
  binary=$(command -v right-wake)
else
  printf 'RightWake binary not found. Run the right-wake-setup skill or follow the repository installation guide.\n' >&2
  exit 127
fi
[ -f "$binary" ] && [ -x "$binary" ] || { printf 'RIGHT_WAKE_BIN must identify an executable native binary.\n' >&2; exit 127; }
if [ -n "${RIGHT_WAKE_DATA_DIR:-}" ]; then
  exec "$binary" --data-dir "$RIGHT_WAKE_DATA_DIR" mcp
fi
exec "$binary" mcp
