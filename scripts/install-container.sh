#!/bin/sh
# Verify and load a pinned release into the selected Docker daemon. Never start it.
set -eu

version=${RIGHT_WAKE_VERSION:-v0.3.0}
from_dir=
repository=Nunchakus888/right-wake
tmp=
fail() { printf 'right-wake container install: %s\n' "$*" >&2; exit 1; }
usage() {
  cat <<'USAGE'
Usage: install-container.sh [--version v0.3.0] [--from-dir DIR]

Load the checksummed Linux amd64 or arm64 image for the Docker server's platform.
The default release is pinned. Docker and its daemon must already be available.
--from-dir reads the image archive and SHA256SUMS from a local directory.
This command does not start containers, write config, or remove images/volumes.
USAGE
}
while [ "$#" -gt 0 ]; do
  case "$1" in
    --version) [ "$#" -ge 2 ] || fail 'missing --version value'; version=$2; shift 2 ;;
    --from-dir) [ "$#" -ge 2 ] || fail 'missing --from-dir value'; from_dir=$2; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) fail "unknown argument: $1" ;;
  esac
done
for tool in docker awk mktemp cp rm; do
  command -v "$tool" >/dev/null 2>&1 || fail "required tool is missing: $tool"
done
printf '%s\n' "$version" | awk '/^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$/ {ok=1} END {exit !ok}' || fail 'invalid version (expected vMAJOR.MINOR.PATCH)'
[ "${#version}" -le 129 ] || fail 'version exceeds the Docker tag limit'
image=right-wake:${version#v}
# Docker Desktop and remote contexts can differ from the client host's uname.
platform=$(docker version --format '{{.Server.Os}}/{{.Server.Arch}}') || fail 'Docker daemon is unavailable'
case "$platform" in
  linux/amd64) arch=amd64 ;;
  linux/arm64) arch=arm64 ;;
  *) fail "unsupported Docker server platform: $platform (expected linux/amd64 or linux/arm64)" ;;
esac
asset=right-wake-$version-container-linux-$arch.tar.gz
if command -v sha256sum >/dev/null 2>&1; then
  hash_file() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
  hash_file() { shasum -a 256 "$1" | awk '{print $1}'; }
else
  fail 'sha256sum or shasum is required'
fi
tmp=$(mktemp -d "${TMPDIR:-/tmp}/right-wake-container-install.XXXXXXXX")
cleanup() {
  [ -z "$tmp" ] || rm -rf -- "$tmp"
}
trap cleanup 0
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
if [ -n "$from_dir" ]; then
  [ -f "$from_dir/$asset" ] || fail "release image not found: $from_dir/$asset"
  [ -f "$from_dir/SHA256SUMS" ] || fail 'SHA256SUMS is missing from --from-dir'
  cp "$from_dir/$asset" "$tmp/$asset"
  cp "$from_dir/SHA256SUMS" "$tmp/SHA256SUMS"
else
  command -v curl >/dev/null 2>&1 || fail 'curl is required for release downloads'
  base=https://github.com/$repository/releases/download/$version
  printf 'Downloading RightWake %s container for %s\n' "$version" "$platform"
  curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --location --retry 3 --connect-timeout 10 --max-time 600 --output "$tmp/$asset" "$base/$asset" || fail 'image download failed; check the release exists or use --from-dir'
  curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --location --retry 3 --connect-timeout 10 --max-time 60 --output "$tmp/SHA256SUMS" "$base/SHA256SUMS" || fail 'checksum download failed'
fi
expected=$(awk -v name="$asset" '$2 == name || $2 == "*" name {print tolower($1)}' "$tmp/SHA256SUMS")
case "$expected" in ''|*[!0-9a-f]*) fail 'missing or ambiguous SHA256SUMS entry' ;; esac
[ "${#expected}" -eq 64 ] || fail 'invalid SHA256SUMS entry'
[ "$(hash_file "$tmp/$asset")" = "$expected" ] || fail 'checksum mismatch; image was not loaded'
metadata_format='{{.Os}}/{{.Architecture}} {{index .Config.Labels "org.opencontainers.image.version"}} {{index .Config.Labels "org.opencontainers.image.source"}}'
expected_metadata="$platform ${version#v} https://github.com/$repository"
if existing=$(docker image inspect --format "$metadata_format" "$image" 2>/dev/null); then
  [ "$existing" = "$expected_metadata" ] || fail "existing $image has different metadata; refusing to replace it"
fi
docker image load --input "$tmp/$asset" > "$tmp/load.log" || fail 'Docker could not load the verified archive'
# A pre-existing matching tag must not disguise an archive carrying other tags.
awk -v expected="Loaded image: $image" '$0 == expected {found=1} END {exit !found}' "$tmp/load.log" || fail "archive did not report the expected tag $image; inspect Docker images before retrying"
metadata=$(docker image inspect --format "$metadata_format" "$image") || fail 'loaded image tag is missing'
[ "$metadata" = "$expected_metadata" ] || fail 'loaded image metadata differs from the requested version/platform'
printf 'Loaded %s for %s. No container was started.\n' "$image" "$platform"
printf 'Next: prepare private.env and compose.yaml, then docker compose run --rm right-wake setup.\n'
