#!/bin/sh
# Install a pinned, checksummed native release. No service or plugin changes.
set -eu

version=${RIGHT_WAKE_VERSION:-v0.3.1}
bin_dir=${RIGHT_WAKE_INSTALL_DIR:-"$HOME/.local/bin"}
from_dir=
repository=Nunchakus888/right-wake
tmp=
staged=

fail() { printf 'right-wake install: %s\n' "$*" >&2; exit 1; }
usage() {
  cat <<'USAGE'
Usage: install.sh [--version v0.3.1] [--bin-dir DIR] [--from-dir DIR]

Install the matching native binary for macOS or glibc Linux, arm64 or x86_64.
The default release is pinned; no daemon starts and no Codex config is changed.
--from-dir reads the release archive and SHA256SUMS from a local directory.
An existing RightWake executable is backed up as right-wake.previous.
USAGE
}
while [ "$#" -gt 0 ]; do
  case "$1" in
    --version) [ "$#" -ge 2 ] || fail 'missing --version value'; version=$2; shift 2 ;;
    --bin-dir) [ "$#" -ge 2 ] || fail 'missing --bin-dir value'; bin_dir=$2; shift 2 ;;
    --from-dir) [ "$#" -ge 2 ] || fail 'missing --from-dir value'; from_dir=$2; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) fail "unknown argument: $1" ;;
  esac
done
case "$version" in v[0-9]*) ;; *) fail 'version must start with v and a digit' ;; esac
case "$version" in *[!A-Za-z0-9.+-]*) fail 'invalid version' ;; esac
case "$bin_dir" in /*) ;; *) fail '--bin-dir must be an absolute path' ;; esac

case "$(uname -s)" in
  Darwin) os=apple-darwin ;;
  Linux) os=unknown-linux-gnu
    command -v getconf >/dev/null 2>&1 || fail 'glibc detection requires getconf; use a source build on other Linux systems'
    getconf GNU_LIBC_VERSION >/dev/null 2>&1 || fail 'this release targets glibc Linux; musl/Alpine needs a source build'
    ;;
  *) fail 'supported release systems: macOS and glibc Linux' ;;
esac
case "$(uname -m)" in
  x86_64|amd64) arch=x86_64 ;;
  aarch64|arm64) arch=aarch64 ;;
  *) fail 'supported architectures: x86_64 and arm64' ;;
esac
target=$arch-$os
asset=right-wake-$version-$target.tar.gz
for tool in tar awk mktemp chmod mkdir mv cp; do
  command -v "$tool" >/dev/null 2>&1 || fail "required system tool is missing: $tool"
done
if command -v sha256sum >/dev/null 2>&1; then
  hash_file() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
  hash_file() { shasum -a 256 "$1" | awk '{print $1}'; }
else
  fail 'sha256sum or shasum is required'
fi
tmp=$(mktemp -d "${TMPDIR:-/tmp}/right-wake-install.XXXXXXXX")
cleanup() {
  [ -z "$staged" ] || rm -f -- "$staged"
  [ -z "$tmp" ] || rm -rf -- "$tmp"
}
trap cleanup EXIT HUP INT TERM
if [ -n "$from_dir" ]; then
  [ -f "$from_dir/$asset" ] || fail "release archive not found: $from_dir/$asset"
  [ -f "$from_dir/SHA256SUMS" ] || fail 'SHA256SUMS is missing from --from-dir'
  cp "$from_dir/$asset" "$tmp/$asset"
  cp "$from_dir/SHA256SUMS" "$tmp/SHA256SUMS"
else
  command -v curl >/dev/null 2>&1 || fail 'curl is required for release downloads'
  base=https://github.com/$repository/releases/download/$version
  printf 'Downloading RightWake %s for %s\n' "$version" "$target"
  curl --proto '=https' --tlsv1.2 --fail --location --retry 3 --connect-timeout 10 --max-time 300 --output "$tmp/$asset" "$base/$asset" || fail 'download failed; check the release exists or use --from-dir'
  curl --proto '=https' --tlsv1.2 --fail --location --retry 3 --connect-timeout 10 --max-time 60 --output "$tmp/SHA256SUMS" "$base/SHA256SUMS" || fail 'checksum download failed'
fi
expected=$(awk -v name="$asset" '$2 == name || $2 == "*" name {print $1}' "$tmp/SHA256SUMS")
case "$expected" in ''|*[!0-9a-fA-F]*) fail 'missing or ambiguous SHA256SUMS entry' ;; esac
[ "${#expected}" -eq 64 ] || fail 'invalid SHA256SUMS entry'
actual=$(hash_file "$tmp/$asset")
[ "$actual" = "$expected" ] || fail 'checksum mismatch; installation aborted'
tar -tzf "$tmp/$asset" > "$tmp/entries" || fail 'invalid release archive'
awk '$0 != "right-wake" && $0 != "LICENSE" && $0 != "README.md" {bad=1} $0 == "right-wake" {count++} END {exit (bad || count != 1)}' "$tmp/entries" || fail 'unexpected archive contents'
# Stream the one regular payload to a file; never extract paths or links.
tar -xOzf "$tmp/$asset" right-wake > "$tmp/right-wake" || fail 'cannot read release binary'
chmod 755 "$tmp/right-wake"
reported=$("$tmp/right-wake" --version) || fail 'binary cannot run on this machine'
[ "$reported" = "right-wake ${version#v}" ] || fail "version mismatch: $reported"
mkdir -p "$bin_dir"
destination=$bin_dir/right-wake
[ ! -L "$destination" ] || fail 'existing right-wake is a symlink; update it with its original package manager'
if [ -e "$destination" ]; then
  [ -f "$destination" ] && [ -x "$destination" ] || fail 'destination exists and is not an executable file'
  old_version=$("$destination" --version 2>/dev/null) || fail 'existing executable is not identifiable; choose another --bin-dir'
  case "$old_version" in 'right-wake '*) ;; *) fail 'refusing to replace an unrelated executable' ;; esac
  [ ! -L "$destination.previous" ] || fail 'backup path is a symlink'
  if [ -e "$destination.previous" ]; then
    [ -f "$destination.previous" ] || fail 'backup path exists and is not a regular file'
  fi
  cp -p "$destination" "$destination.previous"
fi
staged=$(mktemp "$bin_dir/.right-wake-install.XXXXXXXX")
cp "$tmp/right-wake" "$staged"
chmod 755 "$staged"
mv -f "$staged" "$destination"
staged=
printf 'Installed %s at %s\n' "$reported" "$destination"
printf 'Next: %s setup\n' "$destination"
case ":${PATH:-}:" in *":$bin_dir:"*) ;; *) printf 'Add %s to your PATH to use right-wake by name.\n' "$bin_dir" ;; esac
