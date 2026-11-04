//! User-scoped service definitions. No shell, credential interpolation, or data removal.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One managed service per profile; changing its role is an explicit install.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ServiceMode {
    #[default]
    Serve,
    RelayConnect,
    RelayServe,
}

impl ServiceMode {
    fn name(self) -> &'static str {
        match self {
            Self::Serve => "serve",
            Self::RelayConnect => "relay_connect",
            Self::RelayServe => "relay_serve",
        }
    }

    fn command(self) -> &'static [&'static str] {
        match self {
            Self::Serve => &["serve"],
            Self::RelayConnect => &["relay", "connect"],
            Self::RelayServe => &["relay", "serve"],
        }
    }
}

#[cfg(target_os = "linux")]
const UNIT: &str = "right-wake.service";
#[cfg(any(test, target_os = "macos"))]
const LABEL: &str = "io.github.nunchakus888.right-wake";
const MAX_OUTPUT: u64 = 64 * 1024;

struct ManagerResult {
    success: bool,
    code: Option<i32>,
    output: String,
}

fn run(program: &Path, args: &[&str]) -> Result<ManagerResult> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| {
            format!(
                "cannot run {}; use a supported logged-in user session",
                program.display()
            )
        })?;
    let stdout = child
        .stdout
        .take()
        .context("service manager stdout missing")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(MAX_OUTPUT + 1).read_to_end(&mut bytes);
        let _ = tx.send((result, bytes));
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("service manager timed out; inspect this user's service manager before retrying");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let (result, bytes) = rx
        .recv_timeout(Duration::from_secs(1))
        .context("service manager output did not close")?;
    result.context("cannot read service manager output")?;
    if bytes.len() as u64 > MAX_OUTPUT {
        bail!("service manager output exceeded its limit");
    }
    Ok(ManagerResult {
        success: status.success(),
        code: status.code(),
        output: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

fn require(program: &Path, args: &[&str]) -> Result<()> {
    let result = run(program, args)?;
    if !result.success {
        // Do not echo manager output: it can include the user's environment.
        bail!("{} action {} failed (exit {:?}); inspect the logged-in user's service manager; configuration and data are retained", program.display(), args.first().unwrap_or(&"unknown"), result.code);
    }
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf> {
    let value = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let value = if value.exists() {
        value.canonicalize()?
    } else {
        value
    };
    text_path(&value)?;
    Ok(value)
}

fn text_path(path: &Path) -> Result<&str> {
    let value = path.to_str().context("service paths must be valid UTF-8")?;
    if !path.is_absolute() || value.chars().any(char::is_control) {
        bail!("service paths must be absolute and contain no control characters");
    }
    Ok(value)
}

fn marker(data_dir: &Path) -> Result<String> {
    Ok(format!(
        "right-wake-managed-service-v1:{}",
        hex::encode(text_path(data_dir)?.as_bytes())
    ))
}

fn check_owned(path: &Path, data_dir: &Path) -> Result<bool> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.len() > MAX_OUTPUT {
        bail!("refusing to modify a service definition that is not a regular RightWake file");
    }
    let body = std::fs::read_to_string(path)?;
    let mark = marker(data_dir)?;
    if !body
        .lines()
        .any(|line| line == format!("# {mark}") || line == format!("<!-- {mark} -->"))
    {
        bail!("service definition belongs to another application or RightWake data directory; leave it intact and uninstall through its original configuration");
    }
    Ok(true)
}

fn write_definition(path: &Path, data_dir: &Path, body: &str) -> Result<bool> {
    let existed = check_owned(path, data_dir)?;
    if existed && std::fs::read_to_string(path)? == body {
        return Ok(false);
    }
    std::fs::create_dir_all(path.parent().context("service path has no parent")?)?;
    crate::config::atomic_private_write(path, body.as_bytes())?;
    Ok(true)
}

fn remove_definition(path: &Path, data_dir: &Path) -> Result<bool> {
    if !check_owned(path, data_dir)? {
        return Ok(false);
    }
    std::fs::remove_file(path)?;
    Ok(true)
}

fn definition_mode(body: &str) -> Result<ServiceMode> {
    let mut found = None;
    for line in body.lines() {
        let name = line
            .strip_prefix("# right-wake-service-mode-v1:")
            .or_else(|| {
                line.strip_prefix("<!-- right-wake-service-mode-v1:")?
                    .strip_suffix(" -->")
            });
        if let Some(name) = name {
            let mode = match name {
                "serve" => ServiceMode::Serve,
                "relay_connect" => ServiceMode::RelayConnect,
                "relay_serve" => ServiceMode::RelayServe,
                _ => bail!("unrecognized RightWake service mode; leave the definition intact and inspect it"),
            };
            if found.replace(mode).is_some() {
                bail!(
                    "duplicate RightWake service mode; leave the definition intact and inspect it"
                );
            }
        }
    }
    // The only command emitted before mode metadata existed was `serve`.
    Ok(found.unwrap_or_default())
}

fn installed_mode(path: &Path, data_dir: &Path) -> Result<Option<ServiceMode>> {
    if !check_owned(path, data_dir)? {
        return Ok(None);
    }
    Ok(Some(definition_mode(&std::fs::read_to_string(path)?)?))
}

fn validate_mode(mode: ServiceMode, data_dir: &Path) -> Result<()> {
    if mode == ServiceMode::RelayConnect
        && crate::config::Config::load(data_dir)?
            .relay_connection
            .is_none()
    {
        bail!("relay_connection_required: save the fixed endpoint and existing device scope with relay configure before installing or starting the relay-connect service");
    }
    Ok(())
}

struct InstallPlan {
    mode: ServiceMode,
    mode_changed: bool,
    // Keep a mode-switch lock until the new definition is written and enabled.
    _lock: Option<std::fs::File>,
}

fn prepare_install(
    path: &Path,
    data_dir: &Path,
    env_file: Option<&Path>,
    requested_mode: Option<ServiceMode>,
) -> Result<InstallPlan> {
    let installed = installed_mode(path, data_dir)?;
    let mode = requested_mode.or(installed).unwrap_or_default();
    if installed.is_some()
        && env_file.is_none()
        && std::fs::read_to_string(path)?.contains("--env-file")
    {
        bail!("service_env_required: existing definition uses a private environment file; pass its existing --env-file path explicitly to retain it; the definition is unchanged");
    }
    validate_mode(mode, data_dir)?;
    let mode_changed = installed.is_some_and(|old| old != mode);
    let lock = if mode_changed {
        Some(crate::cli::acquire_worker_lock(data_dir).context(
            "service_mode_in_use: stop the existing RightWake service or foreground worker before changing service mode; configuration and definition are unchanged",
        )?)
    } else {
        None
    };
    Ok(InstallPlan {
        mode,
        mode_changed,
        _lock: lock,
    })
}

fn arguments(
    executable: &Path,
    data_dir: &Path,
    env_file: Option<&Path>,
    mode: ServiceMode,
) -> Result<Vec<String>> {
    let mut args = vec![
        text_path(executable)?.to_owned(),
        "--data-dir".into(),
        text_path(data_dir)?.to_owned(),
    ];
    if let Some(path) = env_file {
        args.extend(["--env-file".into(), text_path(path)?.to_owned()]);
    }
    args.extend(mode.command().iter().map(|value| (*value).to_owned()));
    Ok(args)
}

#[cfg(any(test, target_os = "linux"))]
fn systemd_quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    )
}

#[cfg(any(test, target_os = "linux"))]
fn unit(
    executable: &Path,
    data_dir: &Path,
    env_file: Option<&Path>,
    mode: ServiceMode,
) -> Result<String> {
    if text_path(executable)?.contains(['\'', '"', '\\']) {
        bail!("systemd cannot execute a path containing quotes or backslashes; install RightWake at a supported absolute path (spaces and Unicode are supported)");
    }
    let args = arguments(executable, data_dir, env_file, mode)?
        .iter()
        .map(|arg| systemd_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    // The ':' executable prefix disables systemd's $VARIABLE expansion.
    Ok(format!("# {}\n# right-wake-service-mode-v1:{}\n[Unit]\nDescription=RightWake work event bridge\n\n[Service]\nType=exec\nExecStart=:{args}\nRestart=on-failure\nRestartSec=5\nTimeoutStopSec=30\nUMask=0077\nNoNewPrivileges=true\n\n[Install]\nWantedBy=default.target\n", marker(data_dir)?, mode.name()))
}

#[cfg(any(test, target_os = "macos"))]
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(any(test, target_os = "macos"))]
fn plist(
    executable: &Path,
    data_dir: &Path,
    env_file: Option<&Path>,
    mode: ServiceMode,
) -> Result<String> {
    let args = arguments(executable, data_dir, env_file, mode)?
        .iter()
        .map(|arg| format!("    <string>{}</string>", xml(arg)))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<!-- {} -->\n<!-- right-wake-service-mode-v1:{} -->\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>{LABEL}</string>\n  <key>ProgramArguments</key>\n  <array>\n{args}\n  </array>\n  <key>RunAtLoad</key><true/>\n  <key>KeepAlive</key><true/>\n  <key>ThrottleInterval</key><integer>10</integer>\n  <key>Umask</key><integer>63</integer>\n</dict>\n</plist>\n", marker(data_dir)?, mode.name()))
}

/// Manage only this user's RightWake service. `install` enables future login
/// startup; `start` explicitly starts it now. Existing database files are kept.
pub fn manage(
    action: &str,
    data_dir: &Path,
    executable: &Path,
    env_file: Option<&Path>,
    requested_mode: Option<ServiceMode>,
) -> Result<Value> {
    if !["install", "start", "stop", "status", "uninstall"].contains(&action) {
        bail!("service action must be install, start, stop, status, or uninstall");
    }
    if requested_mode.is_some() && action != "install" {
        bail!("service mode can only be selected by an explicit service install");
    }
    let data_dir = absolute(data_dir)?;
    let executable = absolute(executable)?;
    let env_file = env_file.map(absolute).transpose()?;
    if action == "install" {
        if !executable.is_file() {
            bail!("service executable does not exist");
        }
        if let Some(path) = &env_file {
            if !path.is_file() {
                bail!("service environment file does not exist");
            }
        }
    }
    let home =
        PathBuf::from(std::env::var_os("HOME").context("HOME is required for a user service")?);
    #[cfg(target_os = "linux")]
    {
        linux(
            action,
            &data_dir,
            &executable,
            env_file.as_deref(),
            &home,
            requested_mode,
        )
    }
    #[cfg(target_os = "macos")]
    {
        macos(
            action,
            &data_dir,
            &executable,
            env_file.as_deref(),
            &home,
            requested_mode,
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (data_dir, executable, env_file, home, requested_mode);
        bail!("user service management supports Linux systemd and macOS launchd")
    }
}

#[cfg(target_os = "linux")]
fn linux(
    action: &str,
    data_dir: &Path,
    executable: &Path,
    env_file: Option<&Path>,
    home: &Path,
    requested_mode: Option<ServiceMode>,
) -> Result<Value> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let path = absolute(&config)?.join("systemd/user").join(UNIT);
    let manager = if Path::new("/usr/bin/systemctl").is_file() {
        Path::new("/usr/bin/systemctl")
    } else {
        Path::new("/bin/systemctl")
    };
    let mode = installed_mode(&path, data_dir)?;
    let installed = mode.is_some();
    if action == "status" {
        let active = if installed {
            Some(run(manager, &["--user", "is-active", "--quiet", UNIT])?.success)
        } else {
            None
        };
        return Ok(
            json!({"manager":"systemd", "installed":installed, "mode":mode, "running":active, "definition":path, "startup_condition":"user session; pre-login startup requires operator-configured lingering", "data_preserved":true}),
        );
    }
    if action == "install" {
        let plan = prepare_install(&path, data_dir, env_file, requested_mode)?;
        if plan.mode_changed {
            let state = run(manager, &["--user", "is-active", UNIT])?;
            if !matches!(state.output.trim(), "inactive" | "failed") {
                bail!("service_mode_in_use: stop the existing RightWake user service before changing mode; definition is unchanged");
            }
        }
        let changed = write_definition(
            &path,
            data_dir,
            &unit(executable, data_dir, env_file, plan.mode)?,
        )?;
        require(manager, &["--user", "daemon-reload"])?;
        require(manager, &["--user", "enable", UNIT])?;
        return Ok(
            json!({"manager":"systemd", "action":action, "mode":plan.mode, "mode_changed":plan.mode_changed, "definition":path, "changed":changed, "started":false, "next_action":"run right-wake service start; after updating a running service, stop then start to load the new definition", "data_preserved":true}),
        );
    }
    if !installed {
        if action == "uninstall" || action == "stop" {
            return Ok(
                json!({"manager":"systemd", "action":action, "installed":false, "data_preserved":true}),
            );
        }
        bail!("RightWake user service is not installed; run service install first");
    }
    match action {
        "start" => {
            validate_mode(mode.context("installed service mode missing")?, data_dir)?;
            require(manager, &["--user", "start", UNIT])?;
        }
        "stop" => require(manager, &["--user", "stop", UNIT])?,
        "uninstall" => {
            require(manager, &["--user", "disable", "--now", UNIT])?;
            remove_definition(&path, data_dir)?;
            require(manager, &["--user", "daemon-reload"])?;
        }
        _ => unreachable!(),
    }
    Ok(
        json!({"manager":"systemd", "action":action, "mode":mode, "definition":path, "data_preserved":true}),
    )
}

#[cfg(target_os = "macos")]
fn macos(
    action: &str,
    data_dir: &Path,
    executable: &Path,
    env_file: Option<&Path>,
    home: &Path,
    requested_mode: Option<ServiceMode>,
) -> Result<Value> {
    let path = absolute(home)?
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"));
    let manager = Path::new("/bin/launchctl");
    let uid = run(Path::new("/usr/bin/id"), &["-u"])?;
    if !uid.success
        || !uid.output.trim().bytes().all(|byte| byte.is_ascii_digit())
        || uid.output.trim().is_empty()
    {
        bail!("cannot resolve this user's launchd domain");
    }
    let domain = format!("gui/{}", uid.output.trim());
    let target = format!("{domain}/{LABEL}");
    let mode = installed_mode(&path, data_dir)?;
    let installed = mode.is_some();
    if action == "status" {
        let state = if installed {
            Some(run(manager, &["print", &target])?)
        } else {
            None
        };
        return Ok(
            json!({"manager":"launchd", "installed":installed, "mode":mode, "loaded":state.as_ref().map(|s|s.success), "running":state.as_ref().map(|s|s.success && s.output.lines().any(|line|line.trim()=="state = running")), "definition":path, "startup_condition":"logged-in GUI user; unavailable before login or while the computer is asleep/offline", "data_preserved":true}),
        );
    }
    if action == "install" {
        let plan = prepare_install(&path, data_dir, env_file, requested_mode)?;
        if plan.mode_changed && run(manager, &["print", &target])?.success {
            bail!("service_mode_in_use: stop the existing RightWake user service before changing mode; definition is unchanged");
        }
        let changed = write_definition(
            &path,
            data_dir,
            &plist(executable, data_dir, env_file, plan.mode)?,
        )?;
        // Enable the registered label; bootstrap/start is a separate action.
        require(manager, &["enable", &target])?;
        return Ok(
            json!({"manager":"launchd", "action":action, "mode":plan.mode, "mode_changed":plan.mode_changed, "definition":path, "changed":changed, "started":false, "next_action":"run right-wake service start; if updating a running service, stop then start to load the new definition", "data_preserved":true}),
        );
    }
    if !installed {
        if action == "uninstall" || action == "stop" {
            return Ok(
                json!({"manager":"launchd", "action":action, "installed":false, "data_preserved":true}),
            );
        }
        bail!("RightWake user service is not installed; run service install first");
    }
    let loaded = run(manager, &["print", &target])?;
    match action {
        "start" => {
            validate_mode(mode.context("installed service mode missing")?, data_dir)?;
            require(manager, &["enable", &target])?;
            if !loaded.success {
                require(manager, &["bootstrap", &domain, text_path(&path)?])?;
            } else if !loaded
                .output
                .lines()
                .any(|line| line.trim() == "state = running")
            {
                require(manager, &["kickstart", &target])?;
            }
        }
        "stop" | "uninstall" => {
            if loaded.success {
                require(manager, &["bootout", &target])?;
            }
            if action == "uninstall" {
                require(manager, &["disable", &target])?;
                remove_definition(&path, data_dir)?;
            }
        }
        _ => unreachable!(),
    }
    Ok(
        json!({"manager":"launchd", "action":action, "mode":mode, "definition":path, "data_preserved":true}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_paths_cannot_expand_environment_or_specifiers() {
        let body = unit(
            Path::new("/apps/a $HOME %t/right-wake"),
            Path::new("/data/a b\"c\\d"),
            Some(Path::new("/private/token.env")),
            ServiceMode::Serve,
        )
        .unwrap();
        assert!(body.contains("ExecStart=:\"/apps/a $HOME %%t/right-wake\""));
        assert!(body.contains("\"--data-dir\" \"/data/a b\\\"c\\\\d\""));
        assert!(body.contains("\"--env-file\" \"/private/token.env\""));
        assert!(!body.contains("Environment="));
        assert!(!body.contains("sh -c"));
        assert!(unit(
            Path::new("/apps/right-wake\nExecStart=bad"),
            Path::new("/data"),
            None,
            ServiceMode::Serve,
        )
        .is_err());
        assert!(unit(
            Path::new("/apps/a\"b/right-wake"),
            Path::new("/data"),
            None,
            ServiceMode::Serve
        )
        .is_err());
        assert!(unit(
            Path::new("/apps/a\\b/right-wake"),
            Path::new("/data"),
            None,
            ServiceMode::Serve
        )
        .is_err());
    }

    #[test]
    fn plist_preserves_argument_boundaries_and_escapes_xml() {
        let body = plist(
            Path::new("/apps/a & b/<right-wake>"),
            Path::new("/data/a'\"b"),
            Some(Path::new("/private/keys.env")),
            ServiceMode::Serve,
        )
        .unwrap();
        assert!(body.contains("<string>/apps/a &amp; b/&lt;right-wake&gt;</string>"));
        assert!(body.contains("<string>/data/a&apos;&quot;b</string>"));
        assert!(
            body.contains("<string>--env-file</string>\n    <string>/private/keys.env</string>")
        );
        assert!(!body.contains("EnvironmentVariables"));
        assert!(!body.contains("<string>sh</string>"));
    }

    #[test]
    fn repeated_install_and_removal_preserve_user_data() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir(&data).unwrap();
        std::fs::write(data.join("state.sqlite"), b"keep this data").unwrap();
        let definition = dir.path().join("service/right-wake.service");
        let body = unit(
            Path::new("/usr/local/bin/right-wake"),
            &data,
            None,
            ServiceMode::Serve,
        )
        .unwrap();
        assert!(write_definition(&definition, &data, &body).unwrap());
        assert!(!write_definition(&definition, &data, &body).unwrap());
        assert!(remove_definition(&definition, &data).unwrap());
        assert!(!remove_definition(&definition, &data).unwrap());
        assert_eq!(
            std::fs::read(data.join("state.sqlite")).unwrap(),
            b"keep this data"
        );
    }

    #[test]
    fn refuses_unowned_definition_or_another_profile() {
        let dir = tempfile::tempdir().unwrap();
        let definition = dir.path().join("right-wake.service");
        std::fs::write(&definition, "[Service]\nExecStart=/another/app\n").unwrap();
        assert!(remove_definition(&definition, Path::new("/data")).is_err());
        let body = unit(
            Path::new("/apps/right-wake"),
            Path::new("/data/one"),
            None,
            ServiceMode::Serve,
        )
        .unwrap();
        std::fs::write(&definition, body).unwrap();
        assert!(check_owned(&definition, Path::new("/data/two")).is_err());
        assert!(definition.exists());
    }

    #[test]
    fn relay_services_keep_only_saved_profile_and_private_file_references() {
        let executable = Path::new("/apps/right-wake");
        let data = Path::new("/private/my profile");
        let env_file = Some(Path::new("/private/keys.env"));
        for mode in [
            ServiceMode::Serve,
            ServiceMode::RelayConnect,
            ServiceMode::RelayServe,
        ] {
            let systemd = unit(executable, data, env_file, mode).unwrap();
            let launchd = plist(executable, data, env_file, mode).unwrap();
            assert_eq!(definition_mode(&systemd).unwrap(), mode);
            assert_eq!(definition_mode(&launchd).unwrap(), mode);
            let args = arguments(executable, data, env_file, mode).unwrap();
            assert_eq!(args[args.len() - mode.command().len()..], *mode.command());
            assert!(systemd.contains("Restart=on-failure"));
            assert!(launchd.contains("<key>RunAtLoad</key><true/>"));
            assert!(launchd.contains("<key>KeepAlive</key><true/>"));
            for body in [systemd, launchd] {
                assert!(!body.contains("wss://"));
                assert!(!body.contains("--token-env"));
                assert!(!body.contains("--source"));
                assert!(!body.contains("--allow-loopback"));
                assert!(!body.contains("Bearer"));
            }
        }
    }

    #[test]
    fn reinstall_preserves_saved_mode_and_never_drops_the_private_file() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let config = crate::config::Config {
            paused: true,
            sources: vec![serde_json::from_value(json!({
                "id":"source", "kind":"linear", "tenant_id":"test-org", "secret_env":"TEST_ONLY"
            }))
            .unwrap()],
            relay_connection: Some(crate::config::RelayConnection {
                url: "wss://relay.example.com/relay/test-device".into(),
                token_env: "TEST_DEVICE_TOKEN".into(),
                source_ids: vec!["source".into()],
            }),
            ..crate::config::Config::default()
        };
        config.save(&data).unwrap();
        let original_config = std::fs::read(data.join("config.json")).unwrap();
        let definition = dir.path().join("right-wake.service");
        let env_path = data.join("private.env");
        let original = unit(
            Path::new("/apps/right-wake"),
            &data,
            Some(&env_path),
            ServiceMode::RelayConnect,
        )
        .unwrap();
        write_definition(&definition, &data, &original).unwrap();
        let plan = prepare_install(&definition, &data, Some(&env_path), None).unwrap();
        assert_eq!(plan.mode, ServiceMode::RelayConnect);
        assert!(!plan.mode_changed);
        let error = prepare_install(&definition, &data, None, None)
            .err()
            .unwrap();
        assert!(error.to_string().starts_with("service_env_required:"));
        assert_eq!(std::fs::read_to_string(&definition).unwrap(), original);
        assert_eq!(
            std::fs::read(data.join("config.json")).unwrap(),
            original_config
        );
    }

    #[test]
    fn explicit_mode_change_requires_the_existing_worker_to_stop_and_keeps_profile() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir(&data).unwrap();
        let preserved = [
            ("config.json", b"original profile".as_slice()),
            ("state.sqlite3", b"bindings and drafts".as_slice()),
            ("private.env", b"original private values".as_slice()),
            (
                "linear-onboarding.json",
                b"original onboarding baseline".as_slice(),
            ),
        ];
        for (name, bytes) in preserved {
            std::fs::write(data.join(name), bytes).unwrap();
        }
        let definition = dir.path().join("right-wake.service");
        let executable = Path::new("/apps/right-wake");
        let original = unit(executable, &data, None, ServiceMode::Serve).unwrap();
        write_definition(&definition, &data, &original).unwrap();
        let worker = crate::cli::acquire_worker_lock(&data).unwrap();
        let error = prepare_install(&definition, &data, None, Some(ServiceMode::RelayServe))
            .err()
            .unwrap();
        assert!(error.to_string().starts_with("service_mode_in_use:"));
        assert_eq!(std::fs::read_to_string(&definition).unwrap(), original);
        drop(worker);
        let plan =
            prepare_install(&definition, &data, None, Some(ServiceMode::RelayServe)).unwrap();
        assert!(plan.mode_changed);
        assert_eq!(plan.mode, ServiceMode::RelayServe);
        assert!(crate::cli::acquire_worker_lock(&data).is_err());
        let updated = unit(executable, &data, None, plan.mode).unwrap();
        assert!(write_definition(&definition, &data, &updated).unwrap());
        drop(plan);
        assert!(crate::cli::acquire_worker_lock(&data).is_ok());
        assert_eq!(
            installed_mode(&definition, &data).unwrap(),
            Some(ServiceMode::RelayServe)
        );
        assert!(remove_definition(&definition, &data).unwrap());
        for (name, bytes) in preserved {
            assert_eq!(std::fs::read(data.join(name)).unwrap(), bytes);
        }
    }

    #[test]
    fn legacy_mode_is_serve_and_unknown_or_duplicate_modes_are_rejected() {
        let legacy = "# right-wake-managed-service-v1:2f64617461\n[Service]\nExecStart=/apps/right-wake --data-dir /data serve\n";
        assert_eq!(definition_mode(legacy).unwrap(), ServiceMode::Serve);
        assert!(definition_mode("# right-wake-service-mode-v1:tunnel").is_err());
        assert!(definition_mode(
            "# right-wake-service-mode-v1:serve\n# right-wake-service-mode-v1:relay_serve\n"
        )
        .is_err());
    }

    #[test]
    fn relay_worker_requires_a_saved_connection_before_installation() {
        let dir = tempfile::tempdir().unwrap();
        crate::config::Config::setup(dir.path()).unwrap();
        let definition = dir.path().join("right-wake.service");
        let error = prepare_install(
            &definition,
            dir.path(),
            None,
            Some(ServiceMode::RelayConnect),
        )
        .err()
        .unwrap();
        assert!(error.to_string().starts_with("relay_connection_required:"));
        assert!(!definition.exists());
    }
}
