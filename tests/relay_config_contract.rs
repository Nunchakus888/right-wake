//! Persisting a relay reference must not migrate or reset the local profile.
use right_wake::{cli::acquire_worker_lock, config::Config, db::Db};
use serde_json::{json, Value};
use std::{path::Path, process::Command};

fn invoke(dir: &Path, args: &[&str]) -> (bool, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_right-wake"))
        .arg("--json")
        .arg("--data-dir")
        .arg(dir)
        .args(args)
        // Never inherit actual relay credentials in this fixture.
        .env_remove("RIGHT_WAKE_FIXTURE_DEVICE_TOKEN")
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), value)
}

fn run(dir: &Path, args: &[&str]) -> Value {
    let (success, result) = invoke(dir, args);
    assert!(success, "{result}");
    result["data"].clone()
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    run(dir.path(), &["setup"]);
    run(
        dir.path(),
        &[
            "source",
            "add",
            "--id",
            "personal",
            "--kind",
            "linear",
            "--tenant",
            "org",
            "--secret-env",
            "RIGHT_WAKE_FIXTURE_PROVIDER_SECRET",
            "--linear-assignee-id",
            "me",
        ],
    );
    dir
}

fn configure(url: &str) -> [&str; 8] {
    [
        "relay",
        "configure",
        "--url",
        url,
        "--token-env",
        "RIGHT_WAKE_FIXTURE_DEVICE_TOKEN",
        "--source",
        "personal",
    ]
}

#[test]
fn saved_connection_survives_process_restart_without_resetting_existing_state() {
    let dir = fixture();
    run(dir.path(), &["pause"]);
    run(
        dir.path(),
        &[
            "host",
            "add",
            "--id",
            "original",
            "--socket",
            "/tmp/right-wake-fixture.sock",
        ],
    );
    run(
        dir.path(),
        &[
            "bind",
            "--source",
            "personal",
            "--resource",
            "linear:https://linear.app:org:issue",
            "--host",
            "original",
            "--thread",
            "11111111-1111-4111-8111-111111111111",
            "--expected-version",
            "0",
        ],
    );
    let device = [
        "relay",
        "device",
        "--id",
        "existing-device",
        "--token-env",
        "RIGHT_WAKE_FIXTURE_DEVICE_TOKEN",
        "--source",
        "personal",
    ];
    assert_eq!(run(dir.path(), &device)["changed"], true);
    assert_eq!(run(dir.path(), &device)["changed"], false);
    let config_before = Config::load(dir.path()).unwrap();
    let bindings_before = run(dir.path(), &["bindings"]);
    let state_before = Db::open(dir.path()).unwrap().status().unwrap();
    let baseline = b"{\n  \"format\": \"unknown-existing-onboarding\", \"baseline\": [117]\n}\n";
    let secret = b"RIGHT_WAKE_FIXTURE_PROVIDER_SECRET=not-a-live-secret\n";
    std::fs::write(dir.path().join("linear-onboarding.json"), baseline).unwrap();
    std::fs::write(dir.path().join("private.env"), secret).unwrap();

    // Simulate a 0.3.0 profile: the new field is absent, not preinitialized.
    let path = dir.path().join("config.json");
    let mut legacy: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("relay_connection");
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let endpoint = "wss://relay.example.com/relay/existing-device";
    let result = run(dir.path(), &configure(endpoint));
    assert_eq!(result["changed"], true);
    assert_eq!(result["execution_resumed"], false);
    assert_eq!(result["service_started"], false);
    assert_eq!(result["webhook_registered"], false);
    let saved_bytes = std::fs::read(&path).unwrap();
    // Each call is a new native process, exercising durable readback.
    assert_eq!(run(dir.path(), &configure(endpoint))["changed"], false);
    assert_eq!(std::fs::read(&path).unwrap(), saved_bytes);
    let show = run(dir.path(), &["relay", "show"]);
    assert_eq!(show["connection"]["url"], endpoint);
    assert_eq!(show["devices"][0]["id"], "existing-device");
    assert!(show["connected"].is_null());
    let config_after = Config::load(dir.path()).unwrap();
    assert!(config_after.paused);
    assert_eq!(config_after.listen, config_before.listen);
    assert_eq!(json!(config_after.sources), json!(config_before.sources));
    assert_eq!(json!(config_after.hosts), json!(config_before.hosts));
    assert_eq!(config_after.relay_devices, config_before.relay_devices);
    assert_eq!(run(dir.path(), &["bindings"]), bindings_before);
    assert_eq!(
        Db::open(dir.path()).unwrap().status().unwrap(),
        state_before
    );
    assert_eq!(
        std::fs::read(dir.path().join("linear-onboarding.json")).unwrap(),
        baseline
    );
    assert_eq!(
        std::fs::read(dir.path().join("private.env")).unwrap(),
        secret
    );
    let doctor = run(dir.path(), &["doctor"]);
    assert_eq!(doctor["relay"]["connection"]["url"], endpoint);
    assert_eq!(doctor["relay"]["token_available"], false);
    assert_eq!(doctor["hosts"][0]["tool_isolation_verified"], false);
    assert_eq!(
        doctor["hosts"][0]["original_host_ownership_verified"],
        false
    );
    assert_eq!(
        doctor["sources"][0]["linear_assignment_state"]["receive_scope_verified"],
        false
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn saved_relay_rejects_unsafe_endpoints_and_incomplete_or_unknown_scopes() {
    let dir = fixture();
    let endpoint = "wss://relay.example.com/relay/mac";
    run(dir.path(), &configure(endpoint));
    let before = std::fs::read(dir.path().join("config.json")).unwrap();
    for url in [
        "ws://relay.example.com/relay/mac",
        "ws://127.0.0.1:7433/relay/mac",
        "https://relay.example.com/relay/mac",
        "wss://user:secret@relay.example.com/relay/mac",
        "wss://relay.example.com/relay/mac?token=secret",
        "wss://relay.example.com/relay/mac#fragment",
        "wss://relay.example.com/control",
        "wss://relay.example.com/relay/",
    ] {
        assert!(!invoke(dir.path(), &configure(url)).0, "accepted {url}");
        assert_eq!(
            std::fs::read(dir.path().join("config.json")).unwrap(),
            before
        );
    }
    let mut unknown = configure(endpoint);
    unknown[7] = "other-source";
    assert!(!invoke(dir.path(), &unknown).0);
    let mut repeated = configure(endpoint).to_vec();
    repeated.extend(["--source", "personal"]);
    assert!(!invoke(dir.path(), &repeated).0);
    for args in [
        vec!["relay", "connect", "--url", endpoint],
        vec![
            "relay",
            "connect",
            "--token-env",
            "RIGHT_WAKE_FIXTURE_DEVICE_TOKEN",
        ],
        vec!["relay", "connect", "--source", "personal"],
        vec!["relay", "connect", "--allow-loopback"],
    ] {
        let (success, error) = invoke(dir.path(), &args);
        assert!(!success);
        assert_eq!(error["error"]["code"], "invalid_arguments");
    }
    assert_eq!(
        std::fs::read(dir.path().join("config.json")).unwrap(),
        before
    );
    // No token: fails locally before any network call or daemon can start.
    let (success, error) = invoke(dir.path(), &["relay", "connect"]);
    assert!(!success, "{error}");
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("token"));
}

#[test]
fn changing_a_live_connection_is_blocked_but_idempotent_reuse_is_safe() {
    let dir = fixture();
    let endpoint = "wss://relay.example.com/relay/mac";
    run(dir.path(), &configure(endpoint));
    let before = std::fs::read(dir.path().join("config.json")).unwrap();
    let _worker = acquire_worker_lock(dir.path()).unwrap();
    assert_eq!(run(dir.path(), &configure(endpoint))["changed"], false);
    let (success, error) = invoke(
        dir.path(),
        &configure("wss://relay.example.com/relay/other"),
    );
    assert!(!success);
    assert_eq!(error["error"]["code"], "relay_connection_in_use");
    assert_eq!(
        std::fs::read(dir.path().join("config.json")).unwrap(),
        before
    );
}

#[test]
fn paired_device_reuse_compares_the_source_set_without_rewriting_credentials() {
    let dir = fixture();
    run(
        dir.path(),
        &[
            "source",
            "add",
            "--id",
            "second",
            "--kind",
            "linear",
            "--tenant",
            "org",
            "--secret-env",
            "RIGHT_WAKE_SECOND_FIXTURE_SECRET",
        ],
    );
    let mut args = vec![
        "relay",
        "device",
        "--id",
        "paired",
        "--token-env",
        "RIGHT_WAKE_FIXTURE_DEVICE_TOKEN",
        "--source",
        "second",
        "--source",
        "personal",
    ];
    assert_eq!(run(dir.path(), &args)["changed"], true);
    let before = std::fs::read(dir.path().join("config.json")).unwrap();
    args.swap(7, 9);
    assert_eq!(run(dir.path(), &args)["changed"], false);
    assert_eq!(
        std::fs::read(dir.path().join("config.json")).unwrap(),
        before
    );
    args[9] = args[7];
    assert!(!invoke(dir.path(), &args).0);
    assert_eq!(
        std::fs::read(dir.path().join("config.json")).unwrap(),
        before
    );
}
