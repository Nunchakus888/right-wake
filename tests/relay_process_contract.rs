//! Real child-process recovery with synthetic provider input and loopback HTTP/WS.
//! This does not verify public TLS/DNS, a service manager, machine boot, Linear,
//! or an actual Codex host. Local execution stays paused for the whole fixture.
//! An explicit RIGHT_WAKE_TEST_RELAY_BIN can select a trusted older relay binary
//! for protocol compatibility checks; the local worker is always this build.
use hmac::{Hmac, Mac};
use right_wake::{
    config::{atomic_private_write, Config, RelayDevice},
    db::{Db, LinearIssueSnapshot, LinearStateImport},
    model::{now_ms, LinearAssignee, ThreadTarget},
};
use serde_json::{json, Value};
use sha2::Sha256;
use std::{
    fs::File,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{process::Child, time::timeout};

const SOURCE: &str = "synthetic-personal";
const DEVICE: &str = "synthetic-device";
const RESOURCE: &str = "linear:https://linear.app:synthetic-org:synthetic-issue";
const WEBHOOK_ENV: &str = "RIGHT_WAKE_PROCESS_TEST_WEBHOOK";
const DEVICE_ENV: &str = "RIGHT_WAKE_PROCESS_TEST_DEVICE";
const WEBHOOK_SECRET: &str = "synthetic-webhook-secret-never-a-real-credential";
const DEVICE_TOKEN: &str = "synthetic-device-token-never-a-real-credential";
const DEADLINE: Duration = Duration::from_secs(15);

struct Resident {
    child: Child,
    log: PathBuf,
}

impl Resident {
    fn start(dir: &Path, name: &str, args: &[&str]) -> Self {
        let log = dir.join(format!("{name}.log"));
        let output = File::create(&log).unwrap();
        let binary = if args.starts_with(&["relay", "serve"]) {
            std::env::var_os("RIGHT_WAKE_TEST_RELAY_BIN")
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_right-wake").into())
        } else {
            env!("CARGO_BIN_EXE_right-wake").into()
        };
        let child = tokio::process::Command::new(binary)
            .args(["--json", "--data-dir"])
            .arg(dir)
            .arg("--env-file")
            .arg(dir.join("private.env"))
            .args(args)
            // Child processes must never inherit operator credentials.
            .env_clear()
            .env("RUST_LOG", "warn")
            .stdin(Stdio::null())
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        Self { child, log }
    }

    fn assert_alive(&mut self) {
        assert!(
            self.child.try_wait().unwrap().is_none(),
            "native fixture exited: {}",
            std::fs::read_to_string(&self.log).unwrap_or_default()
        );
    }

    async fn stop(mut self) {
        if self.child.try_wait().unwrap().is_none() {
            // A forced process stop exercises recovery after an interruption;
            // kill() also reaps the child. This is not service-manager evidence.
            timeout(Duration::from_secs(5), self.child.kill())
                .await
                .expect("native fixture did not stop within its deadline")
                .unwrap();
        }
    }
}

fn profile(dir: &Path, address: &str, cloud: bool) -> Db {
    let cfg = Config {
        listen: address.into(),
        paused: true,
        sources: vec![serde_json::from_value(json!({
            "id":SOURCE, "kind":"linear", "tenant_id":"synthetic-org",
            "secret_env":WEBHOOK_ENV, "linear_assignee_id":"synthetic-user",
            "allowed_resources":[RESOURCE]
        }))
        .unwrap()],
        relay_devices: if cloud {
            vec![RelayDevice {
                id: DEVICE.into(),
                token_env: DEVICE_ENV.into(),
                source_ids: vec![SOURCE.into()],
                enabled: true,
            }]
        } else {
            vec![]
        },
        ..Config::default()
    };
    cfg.save(dir).unwrap();
    let private = if cloud {
        format!("{WEBHOOK_ENV}={WEBHOOK_SECRET}\n{DEVICE_ENV}={DEVICE_TOKEN}\n")
    } else {
        // The local worker receives no provider signing key.
        format!("{DEVICE_ENV}={DEVICE_TOKEN}\n")
    };
    atomic_private_write(&dir.join("private.env"), private.as_bytes()).unwrap();
    let db = Db::open(dir).unwrap();
    db.import_linear_state(
        &cfg.sources[0],
        &LinearStateImport {
            schema_version: 1,
            organization_id: "synthetic-org".into(),
            issues: vec![LinearIssueSnapshot {
                issue_id: "synthetic-issue".into(),
                assignee: LinearAssignee::Assigned {
                    user_id: "synthetic-user".into(),
                },
                updated_at_ms: now_ms() - 1000,
            }],
        },
    )
    .unwrap();
    db
}

fn comment(id: &str) -> Vec<u8> {
    let at = chrono::Utc::now().to_rfc3339();
    serde_json::to_vec_pretty(&json!({
        "type":"Comment", "action":"create", "organizationId":"synthetic-org",
        "webhookTimestamp":now_ms(), "actor":{"id":"synthetic-author"},
        "data":{"id":id, "issueId":"synthetic-issue", "body":"Continue the fixture's existing work.",
            "createdAt":at, "updatedAt":at}
    }))
    .unwrap()
}

async fn post(
    client: &reqwest::Client,
    endpoint: &str,
    body: &[u8],
    delivery: &str,
    valid: bool,
) -> (reqwest::StatusCode, Value) {
    let mut mac = Hmac::<Sha256>::new_from_slice(WEBHOOK_SECRET.as_bytes()).unwrap();
    mac.update(body);
    let signature = if valid {
        hex::encode(mac.finalize().into_bytes())
    } else {
        "malformed-signature".into()
    };
    let response = client
        .post(endpoint)
        .header("Content-Type", "application/json")
        .header("Linear-Signature", signature)
        .header("Linear-Delivery", delivery)
        .body(body.to_vec())
        .send()
        .await
        .unwrap();
    (response.status(), response.json().await.unwrap())
}

async fn ready(client: &reqwest::Client, url: &str, resident: &mut Resident) {
    timeout(DEADLINE, async {
        loop {
            resident.assert_alive();
            if let Ok(response) = client.get(url).send().await {
                if response.status().is_success() {
                    let health: Value = response.json().await.unwrap();
                    assert_eq!(health["data"]["service"], "right-wake");
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("relay did not become healthy at its original loopback URL");
}

async fn received(cloud: &Db, local: &Db, id: &str, worker: &mut Resident) {
    timeout(DEADLINE, async {
        loop {
            worker.assert_alive();
            if local.event(id).is_ok()
                && cloud.relay_phase(DEVICE, id).unwrap().as_deref() == Some("received")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("durable local receipt and cloud ACK did not converge");
}

#[tokio::test]
#[ignore = "native_socket: spawns real relay/worker processes and requires loopback TCP sockets"]
async fn offline_receipt_process_restarts_reconnect_and_dedup_preserve_paused_work() {
    let cloud_dir = tempfile::tempdir().unwrap();
    let local_dir = tempfile::tempdir().unwrap();
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap().to_string();
    let health = format!("http://{address}/health");
    let endpoint = format!("http://{address}/hooks/{SOURCE}");
    let worker_url = format!("ws://{address}/relay/{DEVICE}");
    let cloud_db = profile(cloud_dir.path(), &address, true);
    let local_db = profile(local_dir.path(), &address, false);
    let local_config = std::fs::read(local_dir.path().join("config.json")).unwrap();
    let cloud_config = std::fs::read(cloud_dir.path().join("config.json")).unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();
    let serve = ["relay", "serve", "--listen", address.as_str()];
    let connect = [
        "relay",
        "connect",
        "--url",
        &worker_url,
        "--token-env",
        DEVICE_ENV,
        "--source",
        SOURCE,
        "--allow-loopback",
    ];
    drop(reservation);

    let mut cloud = Resident::start(cloud_dir.path(), "cloud-first", &serve);
    ready(&client, &health, &mut cloud).await;
    let first = comment("comment-first");
    assert_eq!(
        post(&client, &endpoint, &first, "bad-signature", false)
            .await
            .0,
        reqwest::StatusCode::UNAUTHORIZED
    );
    assert!(cloud_db.events(None, 10).unwrap().is_empty());
    let (status, receipt) = post(&client, &endpoint, &first, "first-delivery", true).await;
    assert_eq!(status, reqwest::StatusCode::OK);
    let first_id = receipt["data"]["event_id"].as_str().unwrap().to_owned();
    assert!(cloud_db.event(&first_id).is_ok());
    assert!(local_db.events(None, 10).unwrap().is_empty());

    // The cloud process restarts after HTTP success, before any device receipt.
    cloud.stop().await;
    let mut cloud = Resident::start(cloud_dir.path(), "cloud-second", &serve);
    ready(&client, &health, &mut cloud).await;
    assert!(cloud_db.event(&first_id).is_ok());
    let mut worker = Resident::start(local_dir.path(), "worker-first", &connect);
    received(&cloud_db, &local_db, &first_id, &mut worker).await;
    let event = local_db.event(&first_id).unwrap();
    let binding = local_db
        .bind(
            SOURCE,
            RESOURCE,
            &ThreadTarget {
                host_id: "synthetic-unconnected-host".into(),
                thread_id: "synthetic-existing-thread".into(),
                cwd: None,
            },
            "explicit",
            None,
        )
        .unwrap();
    // One explicit fixture job lets us inspect durable queue identity without
    // executing a host or treating a mock as original-thread acceptance.
    let job = local_db.enqueue(&event, &binding).unwrap().unwrap();

    worker.stop().await;
    cloud.stop().await;
    let mut cloud = Resident::start(cloud_dir.path(), "cloud-third", &serve);
    ready(&client, &health, &mut cloud).await;
    let second = comment("comment-second");
    let (status, receipt) = post(&client, &endpoint, &second, "second-delivery", true).await;
    assert_eq!(status, reqwest::StatusCode::OK);
    let second_id = receipt["data"]["event_id"].as_str().unwrap().to_owned();
    assert_eq!(cloud_db.events(None, 10).unwrap().len(), 2);
    assert_eq!(local_db.events(None, 10).unwrap().len(), 1);
    let mut worker = Resident::start(local_dir.path(), "worker-second", &connect);
    received(&cloud_db, &local_db, &second_id, &mut worker).await;

    // Only the cloud process restarts. The same live worker must reconnect.
    let worker_pid = worker.child.id();
    cloud.stop().await;
    let mut cloud = Resident::start(cloud_dir.path(), "cloud-fourth", &serve);
    ready(&client, &health, &mut cloud).await;
    let third = comment("comment-third");
    let (status, receipt) = post(&client, &endpoint, &third, "third-delivery", true).await;
    assert_eq!(status, reqwest::StatusCode::OK);
    let third_id = receipt["data"]["event_id"].as_str().unwrap().to_owned();
    received(&cloud_db, &local_db, &third_id, &mut worker).await;
    assert_eq!(worker.child.id(), worker_pid);
    let (status, duplicate) = post(&client, &endpoint, &third, "third-redelivery", true).await;
    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(duplicate["data"]["state"], "duplicate");
    assert_eq!(duplicate["data"]["event_id"], third_id);

    assert_eq!(cloud_db.events(None, 10).unwrap().len(), 3);
    assert_eq!(local_db.events(None, 10).unwrap().len(), 3);
    assert!(cloud_db.jobs(None, 10).unwrap().is_empty());
    let jobs = local_db.jobs(None, 10).unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, job.id);
    assert_eq!(jobs[0].state, "queued");
    assert!(jobs[0].dispatch_id.is_none());
    assert!(local_db.drafts().unwrap().is_empty());
    assert!(local_db.enqueue(&event, &binding).unwrap().is_none());
    assert_eq!(
        std::fs::read(local_dir.path().join("config.json")).unwrap(),
        local_config
    );
    assert_eq!(
        std::fs::read(cloud_dir.path().join("config.json")).unwrap(),
        cloud_config
    );
    worker.stop().await;
    cloud.stop().await;
}
