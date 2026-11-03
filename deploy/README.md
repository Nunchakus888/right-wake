# Self-hosted container

This is an optional source-distributed deployment recipe. Ordinary native installs do not require Docker, and no image registry publication is implied. The separate [Container smoke workflow](../.github/workflows/container.yml) builds and runs this Dockerfile; its actual CI run result is the evidence for container validation. Checking in or statically checking the recipe alone does not prove that the image builds or starts.

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


## Container verification levels

Run the same container smoke locally from the repository root:

```sh
sh scripts/container-smoke.sh
```

The test host needs Docker with an accessible daemon and Python 3 to parse CLI JSON output. The runtime container needs neither Python nor curl; the test uses its installed `waking` binary through `docker exec`. The image build downloads its base images and build dependencies. All subsequent test containers use `--network none`, publish no ports, and receive no real provider or model credentials.

The [smoke script](../scripts/container-smoke.sh) creates uniquely named, labeled temporary resources and checks these steps:

1. Build `deploy/Dockerfile` and initialize a private named volume with `setup`.
2. Run `waking --json demo` inside that volume, verify the explicit synthetic executor result, and read its exact `/data/demo-…` state directory.
3. Start the real foreground receiver in a background container using that state. Check `daemon_running`, the two durable completed queue records, and the two unsent drafts through `docker exec`.
4. Restart the same container and compare the original event/job/draft identities, bodies, versions, and states.
5. Stop and remove that container, attach the same named volume to a newly created container, and compare the persisted records again.
6. Remove only this run's labeled containers, volume, and image tag. Failure cleanup follows the same ownership checks and prints available receiver logs; it never runs a Docker prune operation.

The workflow runs on Linux for changes to `deploy/**`, the container workflow, or the smoke script, and can also be started manually. It is separate from native CI and release builds. The script's finite readiness checks are test startup checks; they do not add a timer or periodic model call to Waking.

| Level | Evidence | What it establishes |
| --- | --- | --- |
| Static checks | Shell syntax, workflow structure, and script review | The recipe is internally consistent; Docker build and runtime remain unverified at this level. |
| Native tests | Existing Rust and native CLI test jobs | Tested application contracts on their reported native platform. They do not substitute for running this Docker image. |
| Container smoke | A successful **Container smoke** run for a specific commit | Actual Linux image build, non-root setup, synthetic demo, real receiver startup, restart, and named-volume persistence across container replacement. |
| Deployment integration | Separately recorded tests against configured live systems | Actual Codex session continuation, provider permissions and replies, public HTTPS ingress, and cloud relay delivery require their own evidence. |

A passing container smoke proves persistence for the demo's two completed queue records and two unsent drafts. It does not establish pending-backlog replay, real model execution, a platform comment round trip, a publicly reachable webhook, or public relay end-to-end behavior. Keep those results separate in [the acceptance ledger](../docs/acceptance.md).
