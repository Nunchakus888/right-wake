#!/bin/sh
set -eu
if [ "$#" -ne 4 ]; then
  printf 'Usage: package-release.sh BINARY VERSION TARGET OUTPUT_DIR\n' >&2
  exit 2
fi
binary=$1
version=$2
target=$3
output=$4
case "$version" in v[0-9]*) ;; *) printf 'Invalid release version\n' >&2; exit 2 ;; esac
case "$version:$target" in *[!A-Za-z0-9._+:-]*) printf 'Invalid release identifier\n' >&2; exit 2 ;; esac
[ -f "$binary" ] && [ -x "$binary" ] || { printf 'Binary is missing\n' >&2; exit 1; }
[ "$("$binary" --version)" = "right-wake ${version#v}" ] || { printf 'Binary version differs from tag\n' >&2; exit 1; }
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mkdir -p "$output"
output=$(CDPATH= cd -- "$output" && pwd)
temp=$(mktemp -d "${TMPDIR:-/tmp}/right-wake-package.XXXXXXXX")
trap 'rm -rf -- "$temp"' EXIT HUP INT TERM
cp "$binary" "$temp/right-wake"
cp "$root/LICENSE" "$temp/LICENSE"
cp "$root/README.md" "$temp/README.md"
asset=right-wake-$version-$target.tar.gz
tar -czf "$output/$asset" -C "$temp" right-wake LICENSE README.md
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$output" && sha256sum "$asset") > "$output/$asset.sha256"
else
  (cd "$output" && shasum -a 256 "$asset") > "$output/$asset.sha256"
fi
printf '%s\n' "$output/$asset"
