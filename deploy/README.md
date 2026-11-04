# Docker + Compose for a VPS relay

Run RightWake natively beside the local agent whose original conversation you want to continue. Use this Docker deployment for an online VPS relay: it accepts verified events while the local worker is offline, then delivers them over the worker's outbound connection. A cloud container does not import local Codex history, credentials, or worktrees.

The image includes RightWake and **bundled SQLite**. One persistent `/data` volume stores configuration and database state; there is no separate database service. It runs as UID/GID `10001` and publishes only a loopback host port for your HTTPS/WSS reverse proxy.

**[v0.2.0 Docker assets are available](https://github.com/Nunchakus888/right-wake/releases/tag/v0.2.0).** Both Linux amd64 and arm64 passed actual image build/run/load and Compose persistence checks. See [verification](../docs/releases/v0.2.0-verification.md) for exact jobs and public-download evidence, and [verification levels](#container-verification-levels) for their scope.

## Install the release image

Prerequisites: an available Linux amd64 or arm64 Docker daemon, Docker Compose v2, shell tools, `curl`, and `sha256sum` or `shasum`. RightWake does not install or reconfigure Docker. Ordinary native users do not need Docker.

On your VPS, use a new deployment directory:

```sh
mkdir right-wake-relay
cd right-wake-relay
curl --proto '=https' --tlsv1.2 --fail --location \
  --output install-container.sh \
  https://github.com/Nunchakus888/right-wake/releases/download/v0.2.0/install-container.sh
curl --proto '=https' --tlsv1.2 --fail --location \
  --output compose.yaml \
  https://github.com/Nunchakus888/right-wake/releases/download/v0.2.0/compose.yaml
# Inspect the installer and Compose configuration before running them.
sh install-container.sh --version v0.2.0
```

The installer selects the **Docker server's** platform, downloads its archive and `SHA256SUMS`, verifies the checksum, loads it with Docker, and checks the expected tag/platform/version metadata. The two image assets are:

- `right-wake-v0.2.0-container-linux-amd64.tar.gz`
- `right-wake-v0.2.0-container-linux-arm64.tar.gz`

Both contain the image tag `right-wake:0.2.0`, for their respective architectures. Distribution uses public GitHub Release assets, with no GHCR login. The Compose file uses `pull_policy: never`; it expects the image to have been explicitly loaded. The installer does not start containers, write configuration/secrets, download Compose, or remove existing images/volumes.

For an offline directory containing the matching image archive and release checksums:

```sh
sh install-container.sh --version v0.2.0 --from-dir /absolute/path/to/release-files
```

Checksums detect mismatches but are not an independent publisher signature. `SHA256SUMS` covers the four native archives and two image archives; review the separately downloaded installer and Compose file. See Docker's [image-load reference](https://docs.docker.com/reference/cli/docker/image/load/) and [pull policy](https://docs.docker.com/reference/compose-file/services/#pull_policy).

## Initialize and configure the relay

From the deployment directory, create a private environment file before the first Compose command. It may be empty for `setup`; actual event verification and device authentication require its configured secrets before startup.

```sh
umask 077
touch private.env
chmod 600 private.env
docker compose run --rm right-wake setup
docker compose run --rm right-wake --json schema
```

The service name is `right-wake`. All commands use the same Compose project's `right-wake-data` named volume mounted at `/data`. Docker derives the actual volume name from the project, so unrelated deployments do not share one global database. The image initializes that volume for UID/GID `10001`. An alternative bind mount needs deliberately configured ownership; do not make it world-writable.

For a relay receiving one Linear issue, configure the real organization and issue UUIDs:

```sh
RIGHT_WAKE_LINEAR_ORG='REPLACE_WITH_ORGANIZATION_UUID'
RIGHT_WAKE_LINEAR_ISSUE='REPLACE_WITH_ISSUE_UUID'
RIGHT_WAKE_LINEAR_RESOURCE="linear:https://linear.app:$RIGHT_WAKE_LINEAR_ORG:$RIGHT_WAKE_LINEAR_ISSUE"

docker compose run --rm right-wake source add \
  --id linear-work --kind linear \
  --instance https://linear.app --tenant "$RIGHT_WAKE_LINEAR_ORG" \
  --secret-env RIGHT_WAKE_LINEAR_WEBHOOK_SECRET \
  --trigger '@right-wake' --resource "$RIGHT_WAKE_LINEAR_RESOURCE"
docker compose run --rm right-wake relay device \
  --id mac-work --token-env RIGHT_WAKE_RELAY_DEVICE_TOKEN --source linear-work
```

Use your trusted editor or secret mechanism to populate `private.env` with the actual `RIGHT_WAKE_LINEAR_WEBHOOK_SECRET` and a device token of at least 32 characters under `RIGHT_WAKE_RELAY_DEVICE_TOKEN`. Source/device configuration stores these **variable names**, not their values. Transfer only the matching device token to the authorized local worker through your trusted channel. A receipt-only relay needs no provider write token, Codex login, or approval private key.

The [Linear guide](../docs/linear.md) explains webhook permissions and identifiers. Adding a source does not register a provider webhook. An existing Linear MCP connection also does not create this event subscription.

## Start, connect, and inspect

```sh
docker compose up -d
docker compose logs --tail 100 right-wake
docker compose exec right-wake right-wake --json status
docker compose exec right-wake right-wake --json events list
```

The supplied command is `relay serve --listen 0.0.0.0:7433`. It receives and queues events without starting model work. The container port is published as `127.0.0.1:7433:7433`; set `RIGHT_WAKE_PORT` for a different host port, retaining that value whenever Compose recreates the service. Configure your TLS proxy to forward only the intended `/hooks/<source_id>` and `/relay/<device_id>` paths. Preserve raw webhook bodies/signature headers and the WebSocket upgrade/device Authorization header. Do not expose a Codex control socket publicly.

The original local worker uses native `right-wake relay connect`, with its **own** persistent profile, source policy, supported host, and exact existing-thread binding. Follow [cloud relay plus local worker](../docs/deployment.md#cloud-relay-plus-local-worker). It initiates outbound WSS to the relay; creating a binding only in the cloud profile does not configure local execution.

`restart: unless-stopped` supervises the cloud relay. It does not make an offline laptop execute or install a local relay-client supervisor. A live public webhook, device connection, and actual model continuation still require verification in your deployment.

## Storage, upgrades, and removal

`/data` holds `config.json`, `state.sqlite3`, and any SQLite WAL/SHM sidecars, along with the durable event/job/draft/receipt state. Keep it on local persistent storage. Stop the service before making a filesystem backup and preserve the complete volume; restore into a separate volume for inspection before replacing live data. Keep a separate backup of operator-managed `private.env` under the appropriate secret controls.

To upgrade, load the new versioned image and review its matching Compose file, then recreate the service using the same deployment directory/project identity and volume. Preserve `private.env` and your reviewed configuration. Changing the project name can select a different empty volume. v0.2.0 uses the new `right-wake` namespace and a fresh profile; it does not migrate previous preview volumes. Database schema version 3 and bundled SQLite 3.54.1 remain in use; see [release notes](../docs/releases/v0.2.0.md).

```sh
docker compose stop
# Back up the persistent volume and preserve private.env before upgrading.
docker compose up -d
```

`docker compose down` removes the project's service containers and network while retaining the named volume. Do not add `--volumes` unless you explicitly intend to delete its data. See [Docker Compose removal semantics](https://docs.docker.com/reference/cli/docker/compose/down/). Uninstalling the relay does not remove the provider's webhook registration or the separate local worker/profile.

## Build from source

For development or a custom build, build the optional source Dockerfile from the repository root:

```sh
docker build --file deploy/Dockerfile --tag right-wake:0.2.0 .
mkdir "$HOME/right-wake-relay"
cp deploy/compose.yaml "$HOME/right-wake-relay/compose.yaml"
cd "$HOME/right-wake-relay"
```

Use a new deployment directory outside the source checkout, then follow [initialization and configuration](#initialize-and-configure-the-relay) to create its private environment file and profile. Compose selects relay mode even though the source Dockerfile's default command is the direct receiver. The release pipeline instead uses `Dockerfile.release` and the already-tested Linux native binary, avoiding another Rust build for the shipped image. Neither image contains an installed/authenticated Codex host.

## Container verification levels

| Check | Evidence and limit |
| --- | --- |
| Source container smoke | Run against the intended source commit; results belong in the current verification report. |
| v0.2.0 release image / Compose | [Release run 41768104125](https://github.com/Nunchakus888/right-wake/actions/runs/41768104125) passed on Linux amd64/arm64: actual image build/load, SQLite 3.54.1, non-root UID/GID `10001:10001`, preservation of two original jobs/two drafts, signed loopback receipt, and pending-event persistence across restart/recreation. The real image installer and 18 installer boundary checks also passed on each platform. See [verification](../docs/releases/v0.2.0-verification.md). |
| Public amd64 archive | The downloaded release archive matched `SHA256SUMS` and GitHub's asset digest. Its `right-wake:0.2.0` tag, `linux/amd64` platform, `10001:10001` user, entrypoint/data directory, and OCI version/title/source/revision labels matched the published source. This was archive inspection; runtime evidence comes from the Linux release jobs above. |
| Real deployment | Public HTTPS/WSS, provider receipt, offline-local replay, original-thread model continuation, and provider replies require separate authorized integration evidence. |

Repository validation commands are `sh scripts/container-smoke.sh` for the source build, and `sh scripts/compose-smoke.sh right-wake:0.2.0` for an already-built image. The test host needs Docker/Compose and Python 3; the runtime image does not need Python. The release packaging script reuses its native Linux binary, checks the image and Compose behavior, saves the archive, and exercises the real loader against that archive. A passing fixture does not establish a live Codex or provider integration; keep those results separate in [acceptance](../docs/acceptance.md). Earlier release evidence is preserved in the [archive](../docs/archive/README.md).
