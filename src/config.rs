use crate::model::ThreadTarget;
use crate::provider::SourceConfig;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostConfig {
    Codex {
        id: String,
        socket: PathBuf,
        /// The operator must actually ensure no other writer starts this thread.
        #[serde(default)]
        exclusive_writer: bool,
        /// Deployment prerequisite: the operator has restricted remote mutation
        /// tools and isolated bridge configuration from the executing agent.
        #[serde(default)]
        restricted_tools: bool,
        #[serde(default = "default_timeout")]
        timeout_seconds: u64,
    },
    Command {
        id: String,
        program: PathBuf,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(default = "default_timeout")]
        timeout_seconds: u64,
    },
}
impl HostConfig {
    pub fn id(&self) -> &str {
        match self {
            Self::Codex { id, .. } | Self::Command { id, .. } => id,
        }
    }
}
fn default_timeout() -> u64 {
    600
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    pub target: ThreadTarget,
    /// A dedicated router thread may only see these sources in one trust domain.
    pub source_ids: Vec<String>,
    #[serde(default = "default_candidates")]
    pub max_candidates: usize,
}
fn default_candidates() -> usize {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayDevice {
    pub id: String,
    pub token_env: String,
    pub source_ids: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema_version: u32,
    pub listen: String,
    pub paused: bool,
    pub sources: Vec<SourceConfig>,
    pub hosts: Vec<HostConfig>,
    pub router: Option<RouterConfig>,
    pub relay_devices: Vec<RelayDevice>,
    pub max_payload_bytes: usize,
    pub max_pending: usize,
    pub max_stored_events: usize,
    pub max_stored_raw_bytes: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            listen: "127.0.0.1:7433".into(),
            paused: false,
            sources: vec![],
            hosts: vec![],
            router: None,
            relay_devices: vec![],
            max_payload_bytes: 1024 * 1024,
            max_pending: 10000,
            max_stored_events: 50000,
            max_stored_raw_bytes: 512 * 1024 * 1024,
        }
    }
}

pub fn default_data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("WAKING_DATA_DIR") {
        return path.into();
    }
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(path).join("waking");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
        .join(".local/share/waking")
}

pub fn private_dir(path: &Path) -> Result<()> {
    if path.exists() && std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        bail!("data directory must not be a symlink");
    }
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn atomic_private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(&temp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(&temp, path)?;
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

impl Config {
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("config.json");
        let value: Self =
            serde_json::from_slice(&std::fs::read(&path).with_context(|| {
                format!("run waking --data-dir {} setup first", dir.display())
            })?)?;
        value.validate()?;
        Ok(value)
    }
    pub fn setup(dir: &Path) -> Result<Self> {
        private_dir(dir)?;
        if dir.join("config.json").exists() {
            return Self::load(dir);
        }
        let cfg = Self::default();
        cfg.save(dir)?;
        Ok(cfg)
    }
    pub fn save(&self, dir: &Path) -> Result<()> {
        self.validate()?;
        private_dir(dir)?;
        atomic_private_write(&dir.join("config.json"), &serde_json::to_vec_pretty(self)?)
    }
    pub fn source(&self, id: &str) -> Result<&SourceConfig> {
        self.sources
            .iter()
            .find(|s| s.id == id)
            .context("source not configured; run source add")
    }
    pub fn host(&self, id: &str) -> Result<&HostConfig> {
        self.hosts
            .iter()
            .find(|h| h.id() == id)
            .context("host not configured; run host add")
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("unsupported configuration schema {}", self.schema_version);
        }
        self.listen
            .parse::<std::net::SocketAddr>()
            .context("listen must be an IP address and port")?;
        if self.max_payload_bytes == 0
            || self.max_payload_bytes > 8 * 1024 * 1024
            || self.max_pending == 0
            || self.max_stored_events == 0
        {
            bail!("invalid admission limits");
        }
        let mut ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for s in &self.sources {
            s.validate()?;
            validate_id(&s.id)?;
            if !ids.insert(&s.id) {
                bail!("duplicate source id");
            }
            validate_env(&s.secret_env)?;
            if let Some(e) = &s.token_env {
                validate_env(e)?;
            }
        }
        ids.clear();
        for h in &self.hosts {
            validate_id(h.id())?;
            if !ids.insert(h.id()) {
                bail!("duplicate host id");
            }
            match h {
                HostConfig::Codex { socket, .. } if !socket.is_absolute() => {
                    bail!("host socket must be absolute")
                }
                HostConfig::Command { program, .. } if !program.is_absolute() => {
                    bail!("command executable must be absolute; no shell lookup")
                }
                _ => {}
            }
            match h {
                HostConfig::Command { cwd: Some(cwd), .. } if !cwd.is_absolute() => {
                    bail!("command working directory must be absolute");
                }
                HostConfig::Codex {
                    timeout_seconds, ..
                }
                | HostConfig::Command {
                    timeout_seconds, ..
                } if *timeout_seconds == 0 => {
                    bail!("host timeout must be greater than zero");
                }
                _ => {}
            }
        }
        ids.clear();
        for device in &self.relay_devices {
            validate_id(&device.id)?;
            validate_env(&device.token_env)?;
            if !ids.insert(&device.id) {
                bail!("duplicate relay device id");
            }
            if device.enabled && device.source_ids.is_empty() {
                bail!("an enabled relay device needs an explicit source scope");
            }
            for source in &device.source_ids {
                self.source(source)?;
            }
        }
        if let Some(r) = &self.router {
            self.host(&r.target.host_id)?;
            if r.source_ids.is_empty() || r.max_candidates == 0 || r.max_candidates > 10 {
                bail!("router needs a bounded source scope and 1..10 candidates");
            }
            for s in &r.source_ids {
                self.source(s)?;
            }
        }
        Ok(())
    }
}
pub fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 80
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        bail!("id must contain 1..80 letters, digits, underscores or hyphens");
    }
    Ok(())
}
pub fn validate_env(id: &str) -> Result<()> {
    if id.is_empty()
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || id.as_bytes()[0].is_ascii_digit()
    {
        bail!("secret must be an environment variable name, never a literal credential");
    }
    Ok(())
}
