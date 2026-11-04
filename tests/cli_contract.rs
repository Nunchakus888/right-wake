use serde_json::{json, Value};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_right-wake"))
}

#[test]
fn cli_identity_and_state_selection_use_one_namespace() {
    let version = bin().arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap(),
        format!("right-wake {}\n", env!("CARGO_PKG_VERSION"))
    );
    let help = bin().arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout)
        .unwrap()
        .contains("Wake the right Agent thread."));

    let output = bin().args(["--json", "schema"]).output().unwrap();
    assert!(output.status.success());
    let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(schema["data"]["product"], "right-wake");
    assert_eq!(schema["data"]["cli"]["name"], "right-wake");

    // Exercise actual CLI parsing and disk writes in child processes, without
    // changing the test process's shared environment or the operator's profile.
    let sandbox = tempfile::tempdir().unwrap();
    let home = sandbox.path().join("home");
    let xdg = sandbox.path().join("xdg data");
    let env_dir = sandbox.path().join("explicit environment");
    let flag_dir = sandbox.path().join("explicit argument");
    let base = || {
        let mut command = bin();
        command.env_clear().env("HOME", &home);
        command
    };
    let check_setup = |command: &mut Command, expected: &Path| {
        let output = command.args(["--json", "setup"]).output().unwrap();
        assert!(
            output.status.success(),
            "{} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let setup: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(setup["data"]["data_dir"], expected.to_str().unwrap());
        assert!(expected.join("config.json").is_file());
        assert!(expected.join("state.sqlite3").is_file());
        assert!(setup["data"]["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action.as_str().unwrap().starts_with("right-wake ")));
    };
    check_setup(&mut base(), &home.join(".local/share/right-wake"));
    check_setup(base().env("XDG_DATA_HOME", &xdg), &xdg.join("right-wake"));
    check_setup(
        base()
            .env("XDG_DATA_HOME", &xdg)
            .env("RIGHT_WAKE_DATA_DIR", &env_dir),
        &env_dir,
    );
    check_setup(
        base()
            .env("RIGHT_WAKE_DATA_DIR", &env_dir)
            .arg("--data-dir")
            .arg(&flag_dir),
        &flag_dir,
    );
}

#[test]
fn signed_demo_continues_two_existing_mock_threads_and_deduplicates() {
    let dir = tempfile::tempdir().unwrap();
    let output = bin()
        .args(["--json", "--data-dir", dir.path().to_str().unwrap(), "demo"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} / {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["data"]["processed"], 2);
    assert_eq!(report["data"]["idle_model_calls"], 0);
    assert_eq!(report["data"]["duplicate_state"], "duplicate");
    assert_eq!(report["data"]["external_replies_sent"], 0);
    let drafts = report["data"]["drafts"].as_array().unwrap();
    assert_eq!(drafts.len(), 2);
    assert!(drafts
        .iter()
        .any(|d| d["body"].as_str().unwrap().contains("existing outbox")));
    assert!(drafts
        .iter()
        .any(|d| d["body"].as_str().unwrap().contains("installation native")));
}

#[test]
fn stdio_mcp_negotiates_and_returns_agent_readable_results_without_stdout_noise() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = bin()
        .args(["--data-dir", dir.path().to_str().unwrap(), "mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"contract-test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"right_wake_setup","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"right_wake_status","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"right_wake_approve","arguments":{}}}),
    ];
    let mut stdin = child.stdin.take().unwrap();
    for message in messages {
        writeln!(stdin, "{message}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies.len(), 5);
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "right-wake");
    let tools = replies[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 12);
    assert!(tools
        .iter()
        .all(|tool| tool["name"].as_str().unwrap().starts_with("right_wake_")));
    assert_eq!(replies[2]["result"]["structuredContent"]["ok"], true);
    assert_eq!(
        replies[3]["result"]["structuredContent"]["data"]["bindings"],
        0
    );
    assert_eq!(replies[4]["result"]["isError"], true);
}

#[test]
fn invalid_cli_arguments_are_json_with_nonzero_exit() {
    let output = bin()
        .args(["--json", "bind", "--thread", "unknown"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["code"], "invalid_arguments");
}

#[test]
fn personal_linear_scope_bootstrap_and_queue_settings_are_real_cli_contracts() {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| -> Value {
        let output = bin()
            .args(["--json", "--data-dir", dir.path().to_str().unwrap()])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].clone()
    };
    run(&["setup"]);
    run(&[
        "source",
        "add",
        "--id",
        "personal",
        "--kind",
        "linear",
        "--tenant",
        "workspace",
        "--secret-env",
        "RIGHT_WAKE_SYNTHETIC_SECRET",
        "--linear-assignee-id",
        "me",
    ]);
    assert_eq!(run(&["source", "list"])[0]["linear_assignee_id"], "me");
    let snapshot = dir.path().join("baseline.json");
    let issue = |id: &str| json!({"issue_id":id,"assignee":{"state":"assigned","user_id":"me"},"updated_at_ms":1_793_695_800_000_i64});
    std::fs::write(
        &snapshot,
        json!({"schema_version":1,"organization_id":"workspace","issues":[issue("a"),issue("b")]})
            .to_string(),
    )
    .unwrap();
    let import = || {
        run(&[
            "source",
            "linear-import",
            "--id",
            "personal",
            "--file",
            snapshot.to_str().unwrap(),
        ])
    };
    assert_eq!(import()["changed"], 2);
    assert_eq!(import()["changed"], 0);
    std::fs::write(
        &snapshot,
        json!({"schema_version":1,"organization_id":"workspace","issues":[issue("a")]}).to_string(),
    )
    .unwrap();
    let partial = import();
    assert_eq!(partial["absence_unassigns"], false);
    assert_eq!(partial["model_invoked"], false);
    assert_eq!(
        run(&["source", "linear-state", "--id", "personal"])["issues"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let socket = dir.path().join("existing-host.sock");
    run(&[
        "host",
        "add",
        "--id",
        "local",
        "--socket",
        socket.to_str().unwrap(),
        "--delivery",
        "queue",
    ]);
    let doctor = run(&["doctor"]);
    assert_eq!(doctor["hosts"][0]["delivery"], "queue");
    assert_eq!(doctor["hosts"][0]["exclusive_writer_required"], false);
    assert_eq!(doctor["hosts"][0]["restricted_tools_declared"], false);
    assert_eq!(doctor["hosts"][0]["dispatch_context_tool_required"], true);
    assert_eq!(doctor["model_turns_requested_by_doctor"], 0);
    assert!(doctor["sources"][0]["public_webhook_verified"].is_null());
    let assignment = &doctor["sources"][0]["linear_assignment_state"];
    assert_eq!(assignment["tracked_issues_in_view"], 2);
    assert_eq!(assignment["assigned_to_user_in_view"], 2);
    assert_eq!(assignment["waiting_assignment"], 0);
    assert!(assignment["issues"].is_null());

    std::fs::write(&snapshot, json!({"schema_version":1,"organization_id":"another-workspace","issues":[issue("outside")]}).to_string()).unwrap();
    let rejected = bin()
        .args([
            "--json",
            "--data-dir",
            dir.path().to_str().unwrap(),
            "source",
            "linear-import",
            "--id",
            "personal",
            "--file",
            snapshot.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert_eq!(
        run(&["source", "linear-state", "--id", "personal"])["issues"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(run(&["schema"])["tools"].as_array().unwrap().len(), 12);
}
