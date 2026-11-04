//! User-scoped service definitions. No shell, credential interpolation, or data removal.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
const UNIT: &str = "right-wake.service";
#[cfg(any(test, target_os = "macos"))]
const LABEL: &str = "io.github.nunchakus888.right-wake";
const MAX_OUTPUT: u64 = 64 * 1024;

struct ManagerResult {
    success: bool,
    code: Option<i32>,
    #[cfg(target_os = "macos")]
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
        #[cfg(target_os = "macos")]
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

fn arguments(executable: &Path, data_dir: &Path, env_file: Option<&Path>) -> Result<Vec<String>> {
    let mut args = vec![
        text_path(executable)?.to_owned(),
        "--data-dir".into(),
        text_path(data_dir)?.to_owned(),
    ];
    if let Some(path) = env_file {
        args.extend(["--env-file".into(), text_path(path)?.to_owned()]);
    }
    args.push("serve".into());
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
fn unit(executable: &Path, data_dir: &Path, env_file: Option<&Path>) -> Result<String> {
    if text_path(executable)?.contains(['\'', '"', '\\']) {
        bail!("systemd cannot execute a path containing quotes or backslashes; install RightWake at a supported absolute path (spaces and Unicode are supported)");
    }
    let args = arguments(executable, data_dir, env_file)?
        .iter()
        .map(|arg| systemd_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    // The ':' executable prefix disables systemd's $VARIABLE expansion.
    Ok(format!("# {}\n[Unit]\nDescription=RightWake work event bridge\n\n[Service]\nType=exec\nExecStart=:{args}\nRestart=on-failure\nRestartSec=5\nTimeoutStopSec=30\nUMask=0077\nNoNewPrivileges=true\n\n[Install]\nWantedBy=default.target\n", marker(data_dir)?))
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
fn plist(executable: &Path, data_dir: &Path, env_file: Option<&Path>) -> Result<String> {
    let args = arguments(executable, data_dir, env_file)?
        .iter()
        .map(|arg| format!("    <string>{}</string>", xml(arg)))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<!-- {} -->\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>{LABEL}</string>\n  <key>ProgramArguments</key>\n  <array>\n{args}\n  </array>\n  <key>RunAtLoad</key><true/>\n  <key>KeepAlive</key><true/>\n  <key>ThrottleInterval</key><integer>10</integer>\n  <key>Umask</key><integer>63</integer>\n</dict>\n</plist>\n", marker(data_dir)?))
}

/// Manage only this user's RightWake service. `install` enables future login
/// startup; `start` explicitly starts it now. Existing database files are kept.
pub fn manage(
    action: &str,
    data_dir: &Path,
    executable: &Path,
    env_file: Option<&Path>,
) -> Result<Value> {
    if !["install", "start", "stop", "status", "uninstall"].contains(&action) {
        bail!("service action must be install, start, stop, status, or uninstall");
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
        linux(action, &data_dir, &executable, env_file.as_deref(), &home)
    }
    #[cfg(target_os = "macos")]
    {
        macos(action, &data_dir, &executable, env_file.as_deref(), &home)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (data_dir, executable, env_file, home);
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
    let installed = check_owned(&path, data_dir)?;
    if action == "status" {
        let active = if installed {
            Some(run(manager, &["--user", "is-active", "--quiet", UNIT])?.success)
        } else {
            None
        };
        return Ok(
            json!({"manager":"systemd", "installed":installed, "running":active, "definition":path, "data_preserved":true}),
        );
    }
    if action == "install" {
        let changed = write_definition(&path, data_dir, &unit(executable, data_dir, env_file)?)?;
        require(manager, &["--user", "daemon-reload"])?;
        require(manager, &["--user", "enable", UNIT])?;
        return Ok(
            json!({"manager":"systemd", "action":action, "definition":path, "changed":changed, "started":false, "next_action":"run right-wake service start", "data_preserved":true}),
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
        "start" => require(manager, &["--user", "start", UNIT])?,
        "stop" => require(manager, &["--user", "stop", UNIT])?,
        "uninstall" => {
            require(manager, &["--user", "disable", "--now", UNIT])?;
            remove_definition(&path, data_dir)?;
            require(manager, &["--user", "daemon-reload"])?;
        }
        _ => unreachable!(),
    }
    Ok(json!({"manager":"systemd", "action":action, "definition":path, "data_preserved":true}))
}

#[cfg(target_os = "macos")]
fn macos(
    action: &str,
    data_dir: &Path,
    executable: &Path,
    env_file: Option<&Path>,
    home: &Path,
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
    let installed = check_owned(&path, data_dir)?;
    if action == "status" {
        let state = if installed {
            Some(run(manager, &["print", &target])?)
        } else {
            None
        };
        return Ok(
            json!({"manager":"launchd", "installed":installed, "loaded":state.as_ref().map(|s|s.success), "running":state.as_ref().map(|s|s.success && s.output.lines().any(|line|line.trim()=="state = running")), "definition":path, "data_preserved":true}),
        );
    }
    if action == "install" {
        let changed = write_definition(&path, data_dir, &plist(executable, data_dir, env_file)?)?;
        // Enable the registered label; bootstrap/start is a separate action.
        require(manager, &["enable", &target])?;
        return Ok(
            json!({"manager":"launchd", "action":action, "definition":path, "changed":changed, "started":false, "next_action":"run right-wake service start; if updating a running service, stop then start to load the new definition", "data_preserved":true}),
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
    Ok(json!({"manager":"launchd", "action":action, "definition":path, "data_preserved":true}))
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
            None
        )
        .is_err());
        assert!(unit(Path::new("/apps/a\"b/right-wake"), Path::new("/data"), None).is_err());
        assert!(unit(Path::new("/apps/a\\b/right-wake"), Path::new("/data"), None).is_err());
    }

    #[test]
    fn plist_preserves_argument_boundaries_and_escapes_xml() {
        let body = plist(
            Path::new("/apps/a & b/<right-wake>"),
            Path::new("/data/a'\"b"),
            Some(Path::new("/private/keys.env")),
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
        let body = unit(Path::new("/usr/local/bin/right-wake"), &data, None).unwrap();
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
        let body = unit(Path::new("/apps/right-wake"), Path::new("/data/one"), None).unwrap();
        std::fs::write(&definition, body).unwrap();
        assert!(check_owned(&definition, Path::new("/data/two")).is_err());
        assert!(definition.exists());
    }
}
