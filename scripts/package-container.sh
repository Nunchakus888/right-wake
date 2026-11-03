#!/bin/sh
# Reuse the matrix's tested native binary; no cross-emulation or Rust rebuild.
set -eu
fail() { printf 'container package: %s\n' "$*" >&2; exit 1; }
[ "$#" -eq 4 ] || fail 'usage: package-container.sh BINARY VERSION TARGET OUTPUT_DIR'
binary=$1
version=$2
target=$3
output=$4
case "$target" in
  x86_64-unknown-linux-gnu) arch=amd64 ;;
  aarch64-unknown-linux-gnu) arch=arm64 ;;
  *) fail 'only native Linux release targets have container archives' ;;
esac
printf '%s\n' "$version" | awk '/^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$/ {ok=1} END {exit !ok}' || fail 'invalid release version'
[ -f "$binary" ] && [ -x "$binary" ] || fail 'native release binary is missing'
[ "$("$binary" --version)" = "waking ${version#v}" ] || fail 'binary version differs from release tag'
[ "$(docker version --format '{{.Server.Os}}/{{.Server.Arch}}')" = "linux/$arch" ] || fail 'Docker daemon must match this native Linux release target'
docker compose version >/dev/null || fail 'Docker Compose v2 is required for release verification'
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mkdir -p "$output"
output=$(CDPATH= cd -- "$output" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/waking-container-package.XXXXXXXX")
trap 'rm -rf -- "$tmp"' 0
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir "$tmp/context"
cp "$binary" "$tmp/context/waking"
cp "$root/LICENSE" "$tmp/context/LICENSE"
cp "$root/deploy/Dockerfile.release" "$tmp/context/Dockerfile"
image=waking:${version#v}
docker build --platform "linux/$arch" --build-arg "WAKING_VERSION=${version#v}" \
  --label "org.opencontainers.image.revision=$(git -C "$root" rev-parse HEAD)" \
  --tag "$image" "$tmp/context"
[ "$(docker image inspect --format '{{.Os}}/{{.Architecture}} {{.Config.User}}' "$image")" = "linux/$arch 10001:10001" ] || fail 'image platform or runtime user differs from the release contract'
[ "$(docker run --rm --network none "$image" --version)" = "waking ${version#v}" ] || fail 'container binary version mismatch'
sh "$root/scripts/container-smoke.sh" --image "$image"
sh "$root/scripts/compose-smoke.sh" "$image"
asset=waking-$version-container-linux-$arch.tar.gz
# Separate commands preserve docker-save errors on POSIX shells without pipefail.
docker image save --output "$tmp/image.tar" "$image"
gzip -n -c "$tmp/image.tar" > "$tmp/$asset"
mv "$tmp/$asset" "$output/$asset"
(cd "$output" && sha256sum "$asset") > "$output/$asset.sha256"
# Exercise the actual installer against the same saved image that will ship.
mkdir "$tmp/install"
cp "$output/$asset" "$tmp/install/$asset"
cp "$output/$asset.sha256" "$tmp/install/SHA256SUMS"
sh "$root/scripts/install-container.sh" --version "$version" --from-dir "$tmp/install"
printf 'Verified native container archive: %s\n' "$output/$asset"
