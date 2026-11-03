# Self-hosted container

This is an optional source-distributed deployment recipe. The container build and live restart/recovery have not been validated merely by checking in this Dockerfile. It is not part of the ordinary native-user prerequisites and no image registry publication is implied.

From the repository root:

```sh
docker build --file deploy/Dockerfile --tag waking:0.1.0 .
docker volume create waking-data
docker run --rm --volume waking-data:/data waking:0.1.0 setup
docker run --rm --volume waking-data:/data waking:0.1.0 --json schema
```

Configure the source and execution host using the CLI commands from the installed schema, mounting the same volume each time. Use secret references in configuration. Supply the referenced variables to the runtime through your container platform's secret mechanism; do not bake credentials into an image or commit them to this repository.

Run the receiver behind your own TLS reverse proxy:

```sh
docker run --detach --name waking \
  --restart unless-stopped \
  --volume waking-data:/data \
  --publish 127.0.0.1:7433:7433 \
  --env-file /absolute/path/to/waking.env \
  waking:0.1.0
```

The process runs as UID/GID 10001. A named Docker volume initialized from the image keeps `/data` writable by that identity. If using a bind mount, prepare its ownership explicitly; do not make it world-writable. The receiver is bound to the container interface but the example publishes only to host loopback. Your reverse proxy must forward only the intended webhook/relay routes over HTTPS and enforce its own request limits. Do not expose a Codex control socket to the public network.

The data volume contains configuration and durable database state. Stop the receiver before making a filesystem backup; copy the complete data directory so SQLite sidecar files are not omitted. Restore into a separate volume and inspect it before replacing a live deployment. Removing the container does not remove the named data volume.

A cloud container does not inherit a laptop's Codex chats or login. For local continuity, use the separately configured relay plus local executor path. Relay source/device isolation, offline backlog, authenticated acknowledgement, restart, and capacity tests remain explicit items in [the acceptance ledger](../docs/acceptance.md).
