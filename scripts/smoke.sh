#!/bin/sh
# Local native-binary checks only. No real providers, model calls, or user services.
set -eu
binary=${1:-target/debug/waking}
binary_dir=$(CDPATH= cd -- "$(dirname -- "$binary")" && pwd)
binary=$binary_dir/$(basename -- "$binary")
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/waking-smoke.XXXXXXXX")
trap 'rm -rf -- "$tmp"' EXIT HUP INT TERM
fail() { printf 'smoke: %s\n' "$*" >&2; exit 1; }
json_envelope() {
  awk '/"schema_version"[[:space:]]*:[[:space:]]*1/ {found=1} END {exit !found}' "$1" || fail "missing JSON envelope: $1"
}
reported=$("$binary" --version)
case "$reported" in 'waking '[0-9]*) ;; *) fail 'unexpected binary/version' ;; esac
version=v${reported#waking }
"$binary" --json schema > "$tmp/schema.json"
json_envelope "$tmp/schema.json"
"$binary" --data-dir "$tmp/data" --json setup > "$tmp/setup.json"
json_envelope "$tmp/setup.json"
cp "$tmp/data/config.json" "$tmp/config-before.json"
"$binary" --data-dir "$tmp/data" --json setup > "$tmp/setup-again.json"
cmp "$tmp/config-before.json" "$tmp/data/config.json" || fail 'setup changed existing configuration'
"$binary" --data-dir "$tmp/data" --json doctor > "$tmp/doctor.json" || :
json_envelope "$tmp/doctor.json"
for command in status bindings; do
  "$binary" --data-dir "$tmp/data" --json "$command" > "$tmp/$command.json"
  json_envelope "$tmp/$command.json"
done
for command in source host events jobs drafts; do
  "$binary" --data-dir "$tmp/data" --json "$command" list > "$tmp/$command.json"
  json_envelope "$tmp/$command.json"
done
"$binary" --data-dir "$tmp/data" --json backup --output "$tmp/backup.sqlite3" > "$tmp/backup.json"
json_envelope "$tmp/backup.json"
[ -s "$tmp/backup.sqlite3" ] || fail 'database backup is empty'
case "$(uname -s)" in Darwin) os=apple-darwin ;; Linux) os=unknown-linux-gnu ;; *) fail 'unsupported smoke platform' ;; esac
case "$(uname -m)" in x86_64|amd64) arch=x86_64 ;; arm64|aarch64) arch=aarch64 ;; *) fail 'unsupported smoke architecture' ;; esac
target=$arch-$os
sh "$root/scripts/package-release.sh" "$binary" "$version" "$target" "$tmp/release" > "$tmp/package.txt"
cat "$tmp/release"/*.sha256 > "$tmp/release/SHA256SUMS"
sh "$root/scripts/install.sh" --version "$version" --bin-dir "$tmp/bin with spaces" --from-dir "$tmp/release"
cmp "$binary" "$tmp/bin with spaces/waking" || fail 'installed binary differs'
sh "$root/scripts/install.sh" --version "$version" --bin-dir "$tmp/bin with spaces" --from-dir "$tmp/release"
cmp "$binary" "$tmp/bin with spaces/waking.previous" || fail 'upgrade backup differs'
printf 'corrupted archive\n' >> "$tmp/release/waking-$version-$target.tar.gz"
if sh "$root/scripts/install.sh" --version "$version" --bin-dir "$tmp/bin with spaces" --from-dir "$tmp/release" > "$tmp/rejected.log" 2>&1; then
  fail 'installer accepted corrupt archive'
fi
cmp "$binary" "$tmp/bin with spaces/waking" || fail 'failed install changed existing binary'
printf 'Native CLI, idempotent setup, package, install, backup, and checksum-rejection smoke passed.\n'
printf 'No real Codex, provider write, plugin installation, or service-manager check was run.\n'
