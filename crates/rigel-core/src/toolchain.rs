//! Dependencies and trust (Phase 6).
//!
//! Three separate jobs, kept separate on purpose:
//!
//! * **Detection** answers *what is on this machine*. It probes a small curated registry of
//!   tools on a named host (`windows`, `wsl:<distro>`, `darwin`, `linux`) and caches the answer
//!   in `tool_status`, so a shelf render does not spawn twenty processes.
//! * **Inference** answers *what does this skill need*, and always names the line that said so.
//!   A requirement with no source line is a guess, and guesses do not start installs.
//! * **Trust** answers *how much do we know about this package*, as arithmetic the user can
//!   read. It is advisory: no score runs an install by itself (D12, N6). Approval is separate
//!   and always required; unattended mode only ever queues.
//!
//! `§11.6` never got written, so the weights live in [`WEIGHTS`] with a reason on each one.
//! Changing a weight changes what `rigel trust explain` prints, and the test at the bottom of
//! this file pins the arithmetic so a silent drift is a failing test rather than a new number.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;

use crate::migrate::now_ms;
use crate::storage::{StorageError, Store};

/// A probe answer older than this is refreshed on the next `detect` without `--refresh`.
pub const CACHE_TTL_MS: i64 = 6 * 60 * 60 * 1_000;

/// Where a tool is looked for. `wsl:<distro>` is a host in its own right: a tool installed in
/// Ubuntu is not installed in Windows, and pretending otherwise is how a shelf lies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Host {
    Windows,
    Wsl(String),
    Darwin,
    Linux,
}

impl Host {
    pub fn current() -> Self {
        if let Some(distro) = std::env::var("WSL_DISTRO_NAME")
            .ok()
            .filter(|s| !s.trim().is_empty())
        {
            return Host::Wsl(distro.trim().to_string());
        }
        match std::env::consts::OS {
            "windows" => Host::Windows,
            "macos" => Host::Darwin,
            _ => Host::Linux,
        }
    }

    /// The string written to `tool_status.host`.
    pub fn key(&self) -> String {
        match self {
            Host::Windows => "windows".to_string(),
            Host::Wsl(distro) => format!("wsl:{distro}"),
            Host::Darwin => "darwin".to_string(),
            Host::Linux => "linux".to_string(),
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "windows" => Host::Windows,
            "darwin" | "macos" => Host::Darwin,
            "linux" => Host::Linux,
            other => match other.strip_prefix("wsl:") {
                Some(distro) if !distro.is_empty() => Host::Wsl(distro.to_string()),
                _ => Host::current(),
            },
        }
    }
}

/* ------------------------------------------------------------------ *
 * the curated registry
 * ------------------------------------------------------------------ */

/// Package ids per manager. `None` means the manager has no package for this tool, not that
/// the tool is uninstallable there.
#[derive(Debug, Clone, Copy, Default)]
pub struct Package {
    pub winget: Option<&'static str>,
    pub choco: Option<&'static str>,
    pub scoop: Option<&'static str>,
    pub brew: Option<&'static str>,
    pub apt: Option<&'static str>,
    pub cargo: Option<&'static str>,
    pub npm: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolSpec {
    /// Stable id, used in `tool_status` and on the command line.
    pub id: &'static str,
    pub name: &'static str,
    /// The executable that has to exist on `PATH`.
    pub binary: &'static str,
    /// Every spelling a skill may use for this tool.
    pub aliases: &'static [&'static str],
    pub category: &'static str,
    pub docs: &'static str,
    /// Who publishes it. Used by the trust arithmetic, never fetched.
    pub publisher: &'static str,
    /// Rough install count, from the publisher's own numbers. Zero means "not known".
    pub installs: u64,
    pub package: Package,
}

const fn pkg(
    winget: Option<&'static str>,
    choco: Option<&'static str>,
    scoop: Option<&'static str>,
    brew: Option<&'static str>,
    apt: Option<&'static str>,
    cargo: Option<&'static str>,
    npm: Option<&'static str>,
) -> Package {
    Package {
        winget,
        choco,
        scoop,
        brew,
        apt,
        cargo,
        npm,
    }
}

/// A dozen tools, chosen because skills actually ask for them. This is a curated list, not a
/// marketplace: a package outside it can still be explained by `rigel trust explain`, but it
/// will never score as trusted.
pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        id: "ripgrep",
        name: "ripgrep",
        binary: "rg",
        aliases: &["ripgrep", "rg"],
        category: "search",
        docs: "https://github.com/BurntSushi/ripgrep",
        publisher: "BurntSushi",
        installs: 40_000_000,
        package: pkg(
            Some("BurntSushi.ripgrep.MSVC"),
            Some("ripgrep"),
            Some("ripgrep"),
            Some("ripgrep"),
            Some("ripgrep"),
            Some("ripgrep"),
            None,
        ),
    },
    ToolSpec {
        id: "git",
        name: "git",
        binary: "git",
        aliases: &["git"],
        category: "vcs",
        docs: "https://git-scm.com",
        publisher: "Software Freedom Conservancy",
        installs: 100_000_000,
        package: pkg(
            Some("Git.Git"),
            Some("git"),
            Some("git"),
            Some("git"),
            Some("git"),
            None,
            None,
        ),
    },
    ToolSpec {
        id: "jq",
        name: "jq",
        binary: "jq",
        aliases: &["jq"],
        category: "data",
        docs: "https://jqlang.github.io/jq/",
        publisher: "jqlang",
        installs: 30_000_000,
        package: pkg(
            Some("jqlang.jq"),
            Some("jq"),
            Some("jq"),
            Some("jq"),
            Some("jq"),
            None,
            None,
        ),
    },
    ToolSpec {
        id: "node",
        name: "Node.js",
        binary: "node",
        aliases: &["node", "nodejs", "npm"],
        category: "runtime",
        docs: "https://nodejs.org",
        publisher: "OpenJS Foundation",
        installs: 90_000_000,
        package: pkg(
            Some("OpenJS.NodeJS.LTS"),
            Some("nodejs-lts"),
            Some("nodejs-lts"),
            Some("node"),
            Some("nodejs"),
            None,
            None,
        ),
    },
    ToolSpec {
        id: "python",
        name: "Python",
        binary: "python",
        aliases: &["python", "python3", "py"],
        category: "runtime",
        docs: "https://www.python.org",
        publisher: "Python Software Foundation",
        installs: 80_000_000,
        package: pkg(
            Some("Python.Python.3.12"),
            Some("python"),
            Some("python"),
            Some("python@3.12"),
            Some("python3"),
            None,
            None,
        ),
    },
    ToolSpec {
        id: "gh",
        name: "GitHub CLI",
        binary: "gh",
        aliases: &["gh", "github-cli"],
        category: "vcs",
        docs: "https://cli.github.com",
        publisher: "GitHub",
        installs: 15_000_000,
        package: pkg(
            Some("GitHub.cli"),
            Some("gh"),
            Some("gh"),
            Some("gh"),
            Some("gh"),
            None,
            None,
        ),
    },
    ToolSpec {
        id: "ffmpeg",
        name: "FFmpeg",
        binary: "ffmpeg",
        aliases: &["ffmpeg"],
        category: "media",
        docs: "https://ffmpeg.org",
        publisher: "FFmpeg",
        installs: 20_000_000,
        package: pkg(
            Some("Gyan.FFmpeg"),
            Some("ffmpeg"),
            Some("ffmpeg"),
            Some("ffmpeg"),
            Some("ffmpeg"),
            None,
            None,
        ),
    },
    ToolSpec {
        id: "make",
        name: "GNU Make",
        binary: "make",
        aliases: &["make", "gmake"],
        category: "build",
        docs: "https://www.gnu.org/software/make/",
        publisher: "GNU Project",
        installs: 50_000_000,
        package: pkg(
            Some("GnuWin32.Make"),
            Some("make"),
            Some("make"),
            Some("make"),
            Some("make"),
            None,
            None,
        ),
    },
    ToolSpec {
        id: "pandoc",
        name: "Pandoc",
        binary: "pandoc",
        aliases: &["pandoc"],
        category: "documents",
        docs: "https://pandoc.org",
        publisher: "John MacFarlane",
        installs: 8_000_000,
        package: pkg(
            Some("JohnMacFarlane.Pandoc"),
            Some("pandoc"),
            Some("pandoc"),
            Some("pandoc"),
            Some("pandoc"),
            None,
            None,
        ),
    },
    ToolSpec {
        id: "docker",
        name: "Docker",
        binary: "docker",
        aliases: &["docker"],
        category: "containers",
        docs: "https://docs.docker.com",
        publisher: "Docker Inc.",
        installs: 25_000_000,
        package: pkg(
            Some("Docker.DockerDesktop"),
            Some("docker-desktop"),
            None,
            None,
            Some("docker.io"),
            None,
            None,
        ),
    },
    ToolSpec {
        id: "uv",
        name: "uv",
        binary: "uv",
        aliases: &["uv"],
        category: "python",
        docs: "https://docs.astral.sh/uv/",
        publisher: "Astral",
        installs: 5_000_000,
        package: pkg(
            Some("astral-sh.uv"),
            Some("uv"),
            Some("uv"),
            Some("uv"),
            None,
            Some("uv"),
            None,
        ),
    },
    ToolSpec {
        id: "rust",
        name: "Rust toolchain",
        binary: "cargo",
        aliases: &["cargo", "rustc", "rustup"],
        category: "build",
        docs: "https://rustup.rs",
        publisher: "Rust Project",
        installs: 10_000_000,
        package: pkg(
            Some("Rustlang.Rustup"),
            Some("rustup.install"),
            Some("rustup"),
            Some("rustup"),
            Some("cargo"),
            None,
            None,
        ),
    },
];

pub fn tool_by_id(id: &str) -> Option<&'static ToolSpec> {
    let id = id.trim().to_ascii_lowercase();
    TOOLS.iter().find(|spec| spec.id == id)
}

/// Resolve an alias, a binary name or a `Bash(rg:*)` style token to a registry entry.
pub fn tool_by_alias(name: &str) -> Option<&'static ToolSpec> {
    let token = name
        .trim()
        .trim_matches(|c: char| c == '`' || c == '"' || c == '\'');
    let inner = token
        .split_once('(')
        .map(|(_, rest)| rest.trim_end_matches([')', '*']))
        .map(|rest| rest.split([':', ')']).next().unwrap_or(rest));
    let bare = token
        .split(['(', ':', ' ', '\t', '/'])
        .next()
        .unwrap_or(token)
        .to_ascii_lowercase();
    let candidates: Vec<String> = [
        Some(bare),
        inner.map(str::trim).map(str::to_ascii_lowercase),
    ]
    .into_iter()
    .flatten()
    .filter(|token| !token.is_empty())
    .collect();
    TOOLS.iter().find(|spec| {
        candidates.iter().any(|bare| {
            spec.id == bare
                || spec.binary.eq_ignore_ascii_case(bare)
                || spec.aliases.iter().any(|alias| *alias == bare)
        })
    })
}

/* ------------------------------------------------------------------ *
 * probing
 * ------------------------------------------------------------------ */

/// Everything detection needs from the machine, behind a trait so the arithmetic and the
/// parser can be tested without spawning anything.
#[async_trait]
pub trait Environment: Send + Sync {
    /// Directories to search, in order. The real implementation reads `PATH`.
    fn path_entries(&self) -> Vec<PathBuf>;
    /// First line of `<exe> --version`, trimmed.
    async fn version(&self, exe: &Path, args: &[&str]) -> Option<String>;
    /// SHA-256 of the file, streamed; `None` when it cannot be read.
    fn sha256(&self, exe: &Path) -> Option<String>;
    /// Whether a package manager's own binary is available.
    fn manager_available(&self, manager: Manager) -> bool;
}

pub struct SystemEnvironment;

impl SystemEnvironment {
    /// Locate `binary` on `PATH`, honouring `PATHEXT` on Windows.
    pub fn locate(binary: &str, entries: &[PathBuf]) -> Option<PathBuf> {
        let mut names: Vec<String> = Vec::new();
        if Path::new(binary).extension().is_some() {
            names.push(binary.to_string());
        } else {
            #[cfg(windows)]
            {
                let extensions =
                    std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string());
                for extension in extensions.split(';').filter(|e| !e.is_empty()) {
                    names.push(format!("{binary}{}", extension.to_ascii_lowercase()));
                }
                names.push(binary.to_string());
            }
            #[cfg(not(windows))]
            {
                names.push(binary.to_string());
            }
        }
        for dir in entries {
            for name in &names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }
}

#[async_trait]
impl Environment for SystemEnvironment {
    fn path_entries(&self) -> Vec<PathBuf> {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|path| !path.as_os_str().is_empty())
            .collect()
    }

    async fn version(&self, exe: &Path, args: &[&str]) -> Option<String> {
        // A version probe is a promise the tool keeps quickly or not at all. Two seconds is
        // the same budget the harness probe uses.
        let mut probe = tokio::process::Command::new(exe);
        probe.args(args);
        crate::process::hide_tokio(&mut probe);
        let output = tokio::time::timeout(Duration::from_secs(2), probe.output())
            .await
            .ok()?
            .ok()?;
        let text = if output.stdout.is_empty() {
            String::from_utf8_lossy(&output.stderr).to_string()
        } else {
            String::from_utf8_lossy(&output.stdout).to_string()
        };
        text.lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| clamp(line, 120))
    }

    fn sha256(&self, exe: &Path) -> Option<String> {
        use std::io::Read;
        let mut file = std::fs::File::open(exe).ok()?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 64 * 1_024];
        loop {
            let read = file.read(&mut buffer).ok()?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Some(format!("{:x}", hasher.finalize()))
    }

    fn manager_available(&self, manager: Manager) -> bool {
        let entries = self.path_entries();
        SystemEnvironment::locate(manager.binary(), &entries).is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Manager {
    Winget,
    Chocolatey,
    Scoop,
    Brew,
    Apt,
    Cargo,
    Npm,
}

impl Manager {
    pub fn binary(&self) -> &'static str {
        match self {
            Manager::Winget => "winget",
            Manager::Chocolatey => "choco",
            Manager::Scoop => "scoop",
            Manager::Brew => "brew",
            Manager::Apt => "apt-get",
            Manager::Cargo => "cargo",
            Manager::Npm => "npm",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Manager::Winget => "winget",
            Manager::Chocolatey => "chocolatey",
            Manager::Scoop => "scoop",
            Manager::Brew => "homebrew",
            Manager::Apt => "apt",
            Manager::Cargo => "cargo",
            Manager::Npm => "npm",
        }
    }

    /// Managers a host can plausibly use, most-preferred first.
    pub fn preferred(host: &Host) -> &'static [Manager] {
        match host {
            Host::Windows => &[
                Manager::Winget,
                Manager::Chocolatey,
                Manager::Scoop,
                Manager::Cargo,
                Manager::Npm,
            ],
            Host::Wsl(_) | Host::Linux => {
                &[Manager::Apt, Manager::Cargo, Manager::Npm, Manager::Brew]
            }
            Host::Darwin => &[Manager::Brew, Manager::Cargo, Manager::Npm],
        }
    }

    /// What the manager itself verifies before its installer runs. This is the honest half of
    /// P6.7: Rigel does not re-implement a signature check it cannot see.
    pub fn verification(&self) -> &'static str {
        match self {
            Manager::Winget => {
                "winget compares the installer against its manifest's SHA-256 before running it"
            }
            Manager::Chocolatey => {
                "Chocolatey checks the checksum a package declares, when it declares one"
            }
            Manager::Scoop => "Scoop hashes the download against its bucket manifest",
            Manager::Brew => "Homebrew checks each formula's pinned SHA-256",
            Manager::Apt => "apt verifies the repository's signature before unpacking",
            Manager::Cargo => "cargo verifies the crates.io registry checksum before building",
            Manager::Npm => "npm verifies the tarball integrity from the registry metadata",
        }
    }

    /// apt needs root, and Rigel never runs an elevation tool. The command is shown; the user
    /// runs it.
    pub fn requires_root(&self) -> bool {
        matches!(self, Manager::Apt)
    }

    pub fn command(&self, package: &str) -> String {
        match self {
            Manager::Winget => format!(
                "winget install --id {package} -e --accept-source-agreements --accept-package-agreements"
            ),
            Manager::Chocolatey => format!("choco install {package} -y --no-progress"),
            Manager::Scoop => format!("scoop install {package}"),
            Manager::Brew => format!("brew install {package}"),
            Manager::Apt => format!("apt-get install -y {package}"),
            Manager::Cargo => format!("cargo install {package}"),
            Manager::Npm => format!("npm install --global {package}"),
        }
    }

    pub fn package_for(&self, spec: &ToolSpec) -> Option<&'static str> {
        match self {
            Manager::Winget => spec.package.winget,
            Manager::Chocolatey => spec.package.choco,
            Manager::Scoop => spec.package.scoop,
            Manager::Brew => spec.package.brew,
            Manager::Apt => spec.package.apt,
            Manager::Cargo => spec.package.cargo,
            Manager::Npm => spec.package.npm,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolStatus {
    pub id: String,
    pub name: String,
    pub host: String,
    pub present: bool,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub sha256: Option<String>,
    pub checked_at: i64,
    pub docs: String,
    pub category: String,
}

impl ToolStatus {
    pub fn missing(spec: &ToolSpec, host: &Host) -> Self {
        ToolStatus {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            host: host.key(),
            present: false,
            version: None,
            path: None,
            sha256: None,
            checked_at: now_ms(),
            docs: spec.docs.to_string(),
            category: spec.category.to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{0} is not in the curated tool registry")]
    UnknownTool(String),

    #[error("{tool} has no package for {manager} on this machine")]
    NoPackage { tool: String, manager: &'static str },

    #[error("no supported package manager found for {0} on this host")]
    NoManager(String),

    #[error(
        "the trust score for {package} is {score:.0}/100 ({band}); Rigel will not run it \
         without --allow-low-trust"
    )]
    Blocked {
        package: String,
        score: f64,
        band: &'static str,
    },

    #[error(
        "{manager} needs administrator rights; Rigel never runs sudo. Run `{command}` yourself"
    )]
    NeedsRoot {
        manager: &'static str,
        command: String,
    },

    #[error("install {0} did not run: the command failed to start")]
    Spawn(String),

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/* ------------------------------------------------------------------ *
 * tool service: detect, cache, verify, restart
 * ------------------------------------------------------------------ */

pub struct ToolService {
    store: Store,
    env: Arc<dyn Environment>,
}

impl ToolService {
    pub async fn open(data_dir: &Path) -> Result<Self, ToolError> {
        Ok(Self {
            store: Store::open_index(data_dir).await?,
            env: Arc::new(SystemEnvironment),
        })
    }

    pub fn with_environment(mut self, env: Arc<dyn Environment>) -> Self {
        self.env = env;
        self
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn environment(&self) -> Arc<dyn Environment> {
        self.env.clone()
    }

    /// Probe every registry tool, reusing a cache row younger than [`CACHE_TTL_MS`] unless
    /// `refresh`. A missing tool is always re-probed: installing it is the common case, and a
    /// stale "absent" is the answer that wastes the user's time.
    pub async fn detect(&self, host: &Host, refresh: bool) -> Result<Vec<ToolStatus>, ToolError> {
        let cached = self.cached(host).await?;
        let mut out = Vec::with_capacity(TOOLS.len());
        for spec in TOOLS {
            if !refresh {
                if let Some(row) = cached.iter().find(|row| row.id == spec.id) {
                    let fresh = now_ms() - row.checked_at < CACHE_TTL_MS;
                    if fresh && row.present {
                        out.push(row.clone());
                        continue;
                    }
                }
            }
            let status = self.probe(spec, host).await;
            self.save(&status).await?;
            out.push(status);
        }
        Ok(out)
    }

    pub async fn cached(&self, host: &Host) -> Result<Vec<ToolStatus>, ToolError> {
        let rows = sqlx::query(
            "SELECT tool_id, version, path, sha256, checked_at, present
             FROM tool_status WHERE host = ? ORDER BY tool_id",
        )
        .bind(host.key())
        .fetch_all(self.store.pool())
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.get("tool_id");
            let Some(spec) = tool_by_id(&id) else {
                continue;
            };
            out.push(ToolStatus {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                host: host.key(),
                present: row.get::<i64, _>("present") != 0,
                version: row.get("version"),
                path: row.get::<Option<String>, _>("path").map(PathBuf::from),
                sha256: row.get("sha256"),
                checked_at: row.get("checked_at"),
                docs: spec.docs.to_string(),
                category: spec.category.to_string(),
            });
        }
        Ok(out)
    }

    pub async fn status(&self, id: &str, host: &Host) -> Result<Option<ToolStatus>, ToolError> {
        let Some(spec) = tool_by_id(id) else {
            return Err(ToolError::UnknownTool(id.to_string()));
        };
        Ok(self
            .cached(host)
            .await?
            .into_iter()
            .find(|row| row.id == spec.id))
    }

    /// One probe, without touching the cache.
    pub async fn probe(&self, spec: &ToolSpec, host: &Host) -> ToolStatus {
        let located = SystemEnvironment::locate(spec.binary, &self.env.path_entries());
        let Some(path) = located else {
            return ToolStatus::missing(spec, host);
        };
        let version = self.env.version(&path, &["--version"]).await;
        let sha256 = self.env.sha256(&path);
        ToolStatus {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            host: host.key(),
            present: true,
            version: version.or_else(|| Some("unknown".to_string())),
            path: Some(path),
            sha256,
            checked_at: now_ms(),
            docs: spec.docs.to_string(),
            category: spec.category.to_string(),
        }
    }

    pub async fn save(&self, status: &ToolStatus) -> Result<(), ToolError> {
        sqlx::query(
            "INSERT INTO tool_status (tool_id, host, version, checked_at, path, sha256, present)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(tool_id, host) DO UPDATE SET
               version = excluded.version,
               checked_at = excluded.checked_at,
               path = excluded.path,
               sha256 = excluded.sha256,
               present = excluded.present",
        )
        .bind(&status.id)
        .bind(&status.host)
        .bind(status.version.as_deref())
        .bind(status.checked_at)
        .bind(
            status
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
        )
        .bind(status.sha256.as_deref())
        .bind(i64::from(status.present))
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    /// Compare the binary's current hash with the one recorded when it was detected or
    /// installed. This is the half of P6.7 the package manager cannot do for us.
    pub async fn verify(&self, id: &str, host: &Host) -> Result<Pin, ToolError> {
        let Some(spec) = tool_by_id(id) else {
            return Err(ToolError::UnknownTool(id.to_string()));
        };
        let recorded = self.status(id, host).await?;
        let located = SystemEnvironment::locate(spec.binary, &self.env.path_entries());
        let Some(path) = located else {
            return Ok(Pin::Missing);
        };
        let current = self.env.sha256(&path);
        let (Some(current), Some(previous)) = (
            current,
            recorded.as_ref().and_then(|row| row.sha256.clone()),
        ) else {
            return Ok(Pin::Unpinned);
        };
        if current == previous {
            return Ok(Pin::Pinned { sha256: current });
        }
        // Record the observation before returning, so `doctor` reports the change once and
        // the next verify compares against what the machine actually has.
        let mut fresh = self.probe(spec, host).await;
        fresh.sha256 = Some(current.clone());
        self.save(&fresh).await?;
        Ok(Pin::Changed {
            from: previous,
            to: current,
            path,
        })
    }

    /// After an install: re-probe, drop the cached versions for this host and hand the agent a
    /// note it can act on. A new shell sees the registry PATH; this process already did.
    pub async fn restart(&self, host: &Host) -> Result<Vec<ToolStatus>, ToolError> {
        self.detect(host, true).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Pin {
    /// The binary matches the hash Rigel recorded.
    Pinned { sha256: String },
    /// The binary changed since it was recorded. Reported, never silently accepted.
    Changed {
        from: String,
        to: String,
        path: PathBuf,
    },
    /// Present, but nothing was recorded to compare against.
    Unpinned,
    /// Not on `PATH` any more.
    Missing,
}

/* ------------------------------------------------------------------ *
 * requirement inference
 * ------------------------------------------------------------------ */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// The frontmatter's `allowed-tools` list.
    AllowedTools,
    /// A command in the body, inside a fence or written as one.
    Body,
}

impl SourceKind {
    pub fn label(&self) -> &'static str {
        match self {
            SourceKind::AllowedTools => "allowed-tools",
            SourceKind::Body => "body",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequirementSource {
    pub kind: SourceKind,
    /// 1-based line in the original document.
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Requirement {
    pub tool: String,
    pub name: String,
    pub binary: String,
    pub docs: String,
    pub source: RequirementSource,
}

/// What a skill needs, each with the line that said so.
///
/// `allowed-tools` wins over a body mention: it is the author's declaration, and the body is
/// usually an example of the same tool in use.
pub fn requirements(src: &str) -> Vec<Requirement> {
    let mut found: Vec<Requirement> = Vec::new();
    let header_end = header_end_line(src);
    let lines: Vec<&str> = src.lines().collect();

    // allowed-tools, including its multi-line list form.
    let mut in_allowed = false;
    for (index, raw) in lines.iter().enumerate() {
        if index >= header_end {
            break;
        }
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("allowed-tools:") {
            in_allowed = true;
            push_requirements(rest, index + 1, line, SourceKind::AllowedTools, &mut found);
            continue;
        }
        if in_allowed {
            if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                push_requirements(
                    trimmed,
                    index + 1,
                    line,
                    SourceKind::AllowedTools,
                    &mut found,
                );
                continue;
            }
            if !trimmed.starts_with([' ', '\t']) && !trimmed.is_empty() && trimmed.contains(':') {
                in_allowed = false;
            }
        }
    }

    // Body: a fenced command, a `$` line, or an alias followed by a flag.
    let mut fenced = false;
    for (index, raw) in lines.iter().enumerate() {
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if index < header_end {
            continue;
        }
        let looks_like_command = fenced
            || trimmed.starts_with("$ ")
            || trimmed.starts_with("> ")
            || trimmed.starts_with("sudo ");
        for spec in TOOLS {
            let mention = spec.aliases.iter().any(|alias| {
                word_positions(line, alias).iter().any(|position| {
                    looks_like_command || looks_like_flag(line, *position + alias.len())
                })
            }) || backticked(line, spec);
            if mention {
                push_requirement(spec, index + 1, line, SourceKind::Body, &mut found);
            }
        }
    }

    found
}

fn push_requirements(
    text: &str,
    line_no: usize,
    raw: &str,
    kind: SourceKind,
    found: &mut Vec<Requirement>,
) {
    // `Bash(rg:*)` is one token but three characters of syntax around the tool's name, so the
    // match is by whole word anywhere in the list rather than by splitting into tokens.
    for spec in TOOLS {
        let mentioned = spec
            .aliases
            .iter()
            .any(|alias| !word_positions(text, alias).is_empty());
        if mentioned {
            push_requirement(spec, line_no, raw, kind, found);
        }
    }
}

fn push_requirement(
    spec: &'static ToolSpec,
    line_no: usize,
    raw: &str,
    kind: SourceKind,
    found: &mut Vec<Requirement>,
) {
    // The declared requirement is the one worth showing; a body example of an already
    // declared tool would only add noise.
    if found.iter().any(|row| row.tool == spec.id) {
        return;
    }
    found.push(Requirement {
        tool: spec.id.to_string(),
        name: spec.name.to_string(),
        binary: spec.binary.to_string(),
        docs: spec.docs.to_string(),
        source: RequirementSource {
            kind,
            line: line_no,
            text: clamp(raw.trim(), 200),
        },
    });
}

/// 1-based line where the frontmatter's closing `---` sits, or 0 when there is no header.
fn header_end_line(src: &str) -> usize {
    let mut lines = src.lines();
    if lines.next().map(|line| line.trim_end_matches('\r')) != Some("---") {
        return 0;
    }
    for (index, line) in src.lines().enumerate().skip(1) {
        if line.trim_end_matches('\r').trim() == "---" {
            return index + 2; // the body starts after this line
        }
    }
    0
}

fn word_positions(haystack: &str, needle: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    if needle.is_empty() {
        return positions;
    }
    let haystack_lower = haystack.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let mut start = 0;
    while let Some(found) = haystack_lower[start..].find(&needle) {
        let at = start + found;
        let before = haystack_lower[..at].chars().next_back();
        let after = haystack_lower[at + needle.len()..].chars().next();
        let boundary = |c: Option<char>| {
            c.map(|c| !(c.is_alphanumeric() || c == '_' || c == '-'))
                .unwrap_or(true)
        };
        if boundary(before) && boundary(after) {
            positions.push(at);
        }
        start = at + needle.len();
    }
    positions
}

/// `rg -n …` reads as a command; `the rg tool` does not.
fn looks_like_flag(line: &str, after: usize) -> bool {
    line[after..].trim_start().starts_with('-')
}

fn backticked(line: &str, spec: &ToolSpec) -> bool {
    spec.aliases
        .iter()
        .any(|alias| line.contains(&format!("`{alias}`")))
}

/* ------------------------------------------------------------------ *
 * trust arithmetic
 * ------------------------------------------------------------------ */

/// The weights §11.6 was meant to define. Each sums to 100.
pub const WEIGHTS: &[(&str, f64, &str)] = &[
    (
        "curated",
        38.0,
        "the package is in Rigel's own reviewed registry",
    ),
    (
        "publisher",
        18.0,
        "the registry's publisher matches the package's owner",
    ),
    (
        "manager",
        18.0,
        "the system package manager verifies what it installs",
    ),
    (
        "adoption",
        16.0,
        "how many installs the publisher reports, on a log scale",
    ),
    (
        "command",
        10.0,
        "the install command itself contains nothing that escalates or pipes",
    ),
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrustFactor {
    pub key: &'static str,
    pub label: &'static str,
    pub weight: f64,
    /// 0..1.
    pub ratio: f64,
    pub points: f64,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrustReport {
    pub package: String,
    pub tool: Option<String>,
    pub score: f64,
    pub band: &'static str,
    pub factors: Vec<TrustFactor>,
    pub arithmetic: Vec<String>,
}

impl TrustReport {
    pub fn trusted(&self) -> bool {
        self.score >= 75.0
    }
}

/// Resolve what a user typed (`ripgrep`, `BurntSushi.ripgrep.MSVC`) to a registry entry and a
/// package id for this host, then score it.
pub fn trust_for(package: &str, host: &Host) -> TrustReport {
    let spec = tool_by_alias(package).or_else(|| {
        TOOLS.iter().find(|spec| {
            [
                spec.package.winget,
                spec.package.choco,
                spec.package.scoop,
                spec.package.brew,
                spec.package.apt,
                spec.package.cargo,
                spec.package.npm,
            ]
            .iter()
            .flatten()
            .any(|id| id.eq_ignore_ascii_case(package.trim()))
        })
    });

    // The manager a `plan` would pick, when one is available in the registry.
    let manager = Manager::preferred(host)
        .iter()
        .copied()
        .find(|manager| spec.is_some_and(|spec| manager.package_for(spec).is_some()));

    score(package, spec, manager)
}

fn score(package: &str, spec: Option<&'static ToolSpec>, manager: Option<Manager>) -> TrustReport {
    let command = match (spec, manager) {
        (Some(spec), Some(manager)) => manager
            .package_for(spec)
            .map(|id| manager.command(id))
            .unwrap_or_else(|| format!("install {package}")),
        _ => format!("install {package}"),
    };

    let curated_ratio = if spec.is_some() { 1.0 } else { 0.0 };
    let publisher_ratio = match (spec, manager) {
        (Some(spec), Some(manager)) => {
            let id = manager.package_for(spec).unwrap_or_default();
            if id
                .to_ascii_lowercase()
                .contains(&spec.publisher.to_ascii_lowercase())
                || spec.publisher.split_whitespace().count() > 1
                || matches!(manager, Manager::Winget | Manager::Brew | Manager::Apt)
            {
                0.9
            } else {
                0.5
            }
        }
        _ => 0.0,
    };
    let manager_ratio = match manager {
        Some(
            Manager::Winget | Manager::Chocolatey | Manager::Scoop | Manager::Brew | Manager::Apt,
        ) => 1.0,
        Some(Manager::Cargo | Manager::Npm) => 0.6,
        None => 0.0,
    };
    let adoption_ratio = spec
        .map(|spec| (spec.installs.max(1) as f64).log10() / 7.0)
        .map(|ratio| ratio.clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let command_ratio = command_penalty(&command);

    let evidence = [
        (
            "curated",
            curated_ratio,
            spec.map(|spec| format!("{} is in the registry as {}", spec.name, spec.id))
                .unwrap_or_else(|| format!("{package} is not in the curated registry")),
        ),
        (
            "publisher",
            publisher_ratio,
            spec.map(|spec| format!("published by {}; matched against {command}", spec.publisher))
                .unwrap_or_else(|| "publisher is unknown offline".to_string()),
        ),
        (
            "manager",
            manager_ratio,
            manager
                .map(|manager| format!("{}: {}", manager.label(), manager.verification()))
                .unwrap_or_else(|| "no package manager can install this here".to_string()),
        ),
        (
            "adoption",
            adoption_ratio,
            spec.map(|spec| format!("{} installs reported", spec.installs))
                .unwrap_or_else(|| "no install count".to_string()),
        ),
        (
            "command",
            command_ratio,
            if command_ratio >= 1.0 {
                "no pipes, redirection, elevation or force flags".to_string()
            } else {
                format!("the command has a pattern Rigel penalises: {command}")
            },
        ),
    ];

    let mut factors = Vec::new();
    let mut arithmetic = Vec::new();
    let mut total = 0.0;
    for (key, weight, label) in WEIGHTS {
        let (_, ratio, why) = evidence
            .iter()
            .find(|(name, _, _)| name == key)
            .cloned()
            .expect("every weight has evidence");
        let points = weight * ratio;
        total += points;
        factors.push(TrustFactor {
            key,
            label,
            weight: *weight,
            ratio,
            points,
            evidence: why.clone(),
        });
        arithmetic.push(format!("{key}: {weight:.0} × {ratio:.2} = {points:.1}"));
    }

    let band = if total >= 75.0 {
        "trusted"
    } else if total >= 50.0 {
        "caution"
    } else {
        "blocked"
    };

    TrustReport {
        package: package.trim().to_string(),
        tool: spec.map(|spec| spec.id.to_string()),
        score: total,
        band,
        factors,
        arithmetic,
    }
}

fn command_penalty(command: &str) -> f64 {
    let lower = command.to_ascii_lowercase();
    let mut ratio: f64 = 1.0;
    for (pattern, penalty) in [
        ("|", 0.5),
        (">", 0.4),
        ("sudo", 0.6),
        ("--force", 0.4),
        ("--ignore-security-hash", 0.8),
        ("http://", 0.5),
        ("https://", 0.4),
        ("&&", 0.3),
        (";", 0.3),
    ] {
        if lower.contains(pattern) {
            ratio -= penalty;
        }
    }
    ratio.clamp(0.0, 1.0)
}

/* ------------------------------------------------------------------ *
 * install plans and the ledger
 * ------------------------------------------------------------------ */

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstallPlan {
    pub tool: String,
    pub name: String,
    pub host: String,
    pub manager: Manager,
    pub package: String,
    /// The exact text the user approves and, if approved, the shell runs.
    pub command: String,
    pub trust: TrustReport,
    pub verification: &'static str,
    pub network: bool,
    pub requires_root: bool,
    pub already_present: Option<String>,
}

impl ToolService {
    /// Build the command without running anything. The command is the plan: what is shown is
    /// what runs (N6).
    pub async fn plan(&self, target: &str, host: &Host) -> Result<InstallPlan, ToolError> {
        let spec =
            tool_by_alias(target).ok_or_else(|| ToolError::UnknownTool(target.to_string()))?;
        let manager = Manager::preferred(host)
            .iter()
            .copied()
            .find(|manager| {
                manager.package_for(spec).is_some() && self.env.manager_available(*manager)
            })
            .ok_or_else(|| ToolError::NoManager(spec.name.to_string()))?;
        let package = manager
            .package_for(spec)
            .ok_or(ToolError::NoPackage {
                tool: spec.id.to_string(),
                manager: manager.label(),
            })?
            .to_string();
        let command = manager.command(&package);
        let trust = trust_for(&package, host);
        // A plan is usually the first thing a fresh process asks, so an empty cache is
        // filled for this one tool instead of being read as "not installed".
        let present = match self.status(spec.id, host).await? {
            Some(row) => Some(row),
            None => {
                let row = self.probe(spec, host).await;
                self.save(&row).await?;
                Some(row)
            }
        };
        Ok(InstallPlan {
            tool: spec.id.to_string(),
            name: spec.name.to_string(),
            host: host.key(),
            manager,
            package,
            command,
            trust,
            verification: manager.verification(),
            network: true,
            requires_root: manager.requires_root(),
            already_present: present
                .filter(|row| row.present)
                .and_then(|row| row.version),
        })
    }

    pub async fn queued(&self) -> Result<Vec<InstallRecord>, ToolError> {
        self.records("queued").await
    }

    pub async fn ledger(&self, limit: usize) -> Result<Vec<InstallRecord>, ToolError> {
        let rows = sqlx::query(
            "SELECT id, name, package_id, command, trust_score, decision, executed_at,
                    exit_code, host, duration_ms, verification, error
             FROM install_ledger ORDER BY rowid DESC LIMIT ?",
        )
        .bind(limit as i64)
        .fetch_all(self.store.pool())
        .await?;
        Ok(rows.into_iter().map(InstallRecord::from_row).collect())
    }

    async fn records(&self, decision: &str) -> Result<Vec<InstallRecord>, ToolError> {
        let rows = sqlx::query(
            "SELECT id, name, package_id, command, trust_score, decision, executed_at,
                    exit_code, host, duration_ms, verification, error
             FROM install_ledger WHERE decision = ? ORDER BY rowid",
        )
        .bind(decision)
        .fetch_all(self.store.pool())
        .await?;
        Ok(rows.into_iter().map(InstallRecord::from_row).collect())
    }

    /// Record the decision before anything runs, so a killed process still leaves a row.
    async fn open_row(
        &self,
        plan: &InstallPlan,
        decision: Decision,
        error: Option<&str>,
    ) -> Result<String, ToolError> {
        let id = uuid::Uuid::new_v4().to_string();
        let evidence = serde_json::json!({
            "trust": plan.trust,
            "manager": plan.manager.label(),
            "requires_root": plan.requires_root,
        });
        sqlx::query(
            "INSERT INTO install_ledger
               (id, name, package_id, command, trust_score, evidence_json, decision, host,
                verification, error)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&plan.name)
        .bind(&plan.package)
        .bind(&plan.command)
        .bind(plan.trust.score)
        .bind(evidence.to_string())
        .bind(decision.as_str())
        .bind(&plan.host)
        .bind(plan.verification)
        .bind(error)
        .execute(self.store.pool())
        .await?;
        Ok(id)
    }

    /// Run an approved install and record what happened. `unattended` queues instead: D13
    /// bans a network install when nobody is there to approve the exact command.
    pub async fn install(
        &self,
        plan: &InstallPlan,
        runner: Arc<dyn Runner>,
        host: &Host,
        unattended: bool,
        allow_low_trust: bool,
    ) -> Result<InstallRecord, ToolError> {
        if unattended {
            let id = self.open_row(plan, Decision::Queued, None).await?;
            return self
                .record(&id)
                .await?
                .ok_or_else(|| ToolError::Spawn(plan.name.clone()));
        }

        if plan.trust.score < 50.0 && !allow_low_trust {
            self.open_row(
                plan,
                Decision::Denied,
                Some("trust score below the block threshold"),
            )
            .await?;
            return Err(ToolError::Blocked {
                package: plan.package.clone(),
                score: plan.trust.score,
                band: plan.trust.band,
            });
        }

        if plan.requires_root {
            let id = self
                .open_row(
                    plan,
                    Decision::Denied,
                    Some("needs administrator rights; Rigel never runs sudo"),
                )
                .await?;
            let _ = id;
            return Err(ToolError::NeedsRoot {
                manager: plan.manager.label(),
                command: plan.command.clone(),
            });
        }

        // A command that disagrees with the plan is a bug, not something to run.
        debug_assert_eq!(plan.command, plan.manager.command(&plan.package));

        let id = self.open_row(plan, Decision::UserApproved, None).await?;
        let outcome = runner
            .run(&plan.command, Path::new("."), Duration::from_secs(600))
            .await;

        if !outcome.spawned {
            sqlx::query("UPDATE install_ledger SET error = ? WHERE id = ?")
                .bind("the command could not be started")
                .bind(&id)
                .execute(self.store.pool())
                .await?;
            return Err(ToolError::Spawn(plan.name.clone()));
        }

        let error = if outcome.exit_code == Some(0) {
            None
        } else {
            Some(clamp(
                outcome
                    .stderr
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("the install command failed"),
                400,
            ))
        };

        sqlx::query(
            "UPDATE install_ledger SET executed_at = ?, exit_code = ?, duration_ms = ?, error = ?
             WHERE id = ?",
        )
        .bind(now_ms())
        .bind(outcome.exit_code)
        .bind(outcome.duration_ms as i64)
        .bind(error.as_deref())
        .bind(&id)
        .execute(self.store.pool())
        .await?;

        if error.is_some() {
            return self
                .record(&id)
                .await?
                .ok_or_else(|| ToolError::Spawn(plan.name.clone()));
        }

        // P6.6: the shell that started Rigel cannot see a new PATH entry, so re-probe now and
        // give the agent the line it should run to confirm.
        let _ = self.restart(host).await;
        let _ = self.verify(&plan.tool, host).await;
        self.record(&id)
            .await?
            .ok_or_else(|| ToolError::Spawn(plan.name.clone()))
    }

    /// Record a refusal. `install` is never called on this path, so nothing ran.
    pub async fn deny(&self, plan: &InstallPlan, reason: &str) -> Result<InstallRecord, ToolError> {
        let id = self.open_row(plan, Decision::Denied, Some(reason)).await?;
        self.record(&id)
            .await?
            .ok_or_else(|| ToolError::Spawn(plan.name.clone()))
    }

    pub async fn record(&self, id: &str) -> Result<Option<InstallRecord>, ToolError> {
        let row = sqlx::query(
            "SELECT id, name, package_id, command, trust_score, decision, executed_at,
                    exit_code, host, duration_ms, verification, error
             FROM install_ledger WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.store.pool())
        .await?;
        Ok(row.map(InstallRecord::from_row))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Decision {
    Auto,
    UserApproved,
    Denied,
    Queued,
}

impl Decision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Decision::Auto => "auto",
            Decision::UserApproved => "user-approved",
            Decision::Denied => "denied",
            Decision::Queued => "queued",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstallRecord {
    pub id: String,
    pub name: String,
    pub package: String,
    pub command: String,
    pub trust_score: f64,
    pub decision: String,
    pub host: Option<String>,
    pub executed_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub verification: Option<String>,
    pub error: Option<String>,
}

impl InstallRecord {
    fn from_row(row: sqlx::sqlite::SqliteRow) -> Self {
        InstallRecord {
            id: row.get("id"),
            name: row.get("name"),
            package: row
                .get::<Option<String>, _>("package_id")
                .unwrap_or_default(),
            command: row.get("command"),
            trust_score: row.get("trust_score"),
            decision: row.get("decision"),
            host: row.get("host"),
            executed_at: row.get("executed_at"),
            exit_code: row.get("exit_code"),
            duration_ms: row.get::<Option<i64>, _>("duration_ms").map(|ms| ms as u64),
            verification: row.get("verification"),
            error: row.get("error"),
        }
    }

    /// What the agent is told after an install. The wording matters: the tool is on disk, but
    /// the process that asked for it is not guaranteed to see it until it probes again.
    pub fn recheck(&self) -> String {
        if self.decision == "queued" {
            return format!(
                "{} is queued, not installed. It runs only when someone approves `{}`.",
                self.name, self.command
            );
        }
        match self.exit_code {
            Some(0) => format!(
                "{} is installed. Re-check with `rigel tools which {}` in a new shell before using it.",
                self.name, self.name
            ),
            _ => format!(
                "{} did not install (exit {}). The command was `{}`.",
                self.name,
                self.exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                self.command
            ),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunOutcome {
    pub spawned: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

/// Runs the approved command string through the platform shell, exactly as typed.
#[async_trait]
pub trait Runner: Send + Sync {
    async fn run(&self, command: &str, cwd: &Path, timeout: Duration) -> RunOutcome;
}

pub struct SystemRunner;

#[async_trait]
impl Runner for SystemRunner {
    async fn run(&self, command: &str, cwd: &Path, timeout: Duration) -> RunOutcome {
        #[cfg(windows)]
        let mut process = {
            let mut process = tokio::process::Command::new("cmd");
            process.arg("/C").arg(command);
            process
        };
        #[cfg(not(windows))]
        let mut process = {
            let mut process = tokio::process::Command::new("/bin/sh");
            process.arg("-c").arg(command);
            process
        };

        process
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        crate::process::hide_tokio(&mut process);

        let started = Instant::now();
        let child = match process.spawn() {
            Ok(child) => child,
            Err(_) => return RunOutcome::default(),
        };
        match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(output)) => RunOutcome {
                spawned: true,
                exit_code: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                duration_ms: started.elapsed().as_millis() as u64,
            },
            Ok(Err(error)) => RunOutcome {
                spawned: true,
                exit_code: None,
                stdout: String::new(),
                stderr: error.to_string(),
                duration_ms: started.elapsed().as_millis() as u64,
            },
            Err(_) => RunOutcome {
                spawned: true,
                exit_code: None,
                stdout: String::new(),
                stderr: format!("the install timed out after {}s", timeout.as_secs()),
                duration_ms: started.elapsed().as_millis() as u64,
            },
        }
    }
}

fn clamp(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let kept: String = trimmed.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

/* ------------------------------------------------------------------ *
 * tests
 * ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    const SKILL: &str = "---\nname: rg-audit\ndescription: Audit a tree.\nallowed-tools: [Bash(rg:*), Read]\n---\n# Audit\n\nRun the audit:\n\n```bash\nrg -n \"TODO\" src/\n```\n\nThen read `SKILL.md` with the rg tool's docs at https://example.com.\n";

    #[test]
    fn allowed_tools_win_and_name_their_line() {
        let found = requirements(SKILL);
        let rg = found
            .iter()
            .find(|row| row.tool == "ripgrep")
            .expect("ripgrep is declared");
        assert_eq!(rg.source.kind, SourceKind::AllowedTools);
        assert_eq!(rg.source.line, 4);
        assert!(rg.source.text.contains("allowed-tools"));
    }

    #[test]
    fn a_body_command_names_the_line_it_is_on() {
        let body =
            "# Audit\n\nUse ripgrep here, in prose.\n\n```sh\nrg --files-with-matches TODO\n```\n";
        let found = requirements(body);
        let rg = found.iter().find(|row| row.tool == "ripgrep").unwrap();
        assert_eq!(rg.source.kind, SourceKind::Body);
        assert_eq!(rg.source.line, 6);
        assert!(rg.source.text.contains("--files-with-matches"));
    }

    #[test]
    fn prose_alone_is_not_a_requirement() {
        let body = "# Notes\n\nWe like ripgrep because it is fast.\n";
        assert!(
            requirements(body).is_empty(),
            "a preference in prose must not queue an install"
        );
    }

    #[test]
    fn the_arithmetic_is_pinned_and_readable() {
        let report = trust_for("ripgrep", &Host::Windows);
        assert_eq!(report.factors.len(), 5);
        let sum: f64 = report.factors.iter().map(|factor| factor.points).sum();
        assert!((sum - report.score).abs() < 1e-9);
        assert_eq!(report.arithmetic.len(), 5);
        assert!(
            report.score >= 75.0,
            "a curated tool from a system manager should be trusted: {}",
            report.score
        );

        // An unknown package keeps only the ten points for a command with nothing dangerous
        // in it, which is not enough to reach the block threshold's floor.
        let unknown = trust_for("left-pad-9000", &Host::Windows);
        assert_eq!(unknown.band, "blocked");
        assert_eq!(unknown.score, 10.0);
        assert!(!unknown.trusted());
    }

    #[test]
    fn a_piped_command_loses_points() {
        let base = score("ripgrep", tool_by_id("ripgrep"), Some(Manager::Winget));
        let piped = score("curl https://example.test/x | sh", None, None);
        assert!(piped.score < base.score);
        assert_eq!(piped.band, "blocked");
    }

    #[test]
    fn every_registry_entry_has_a_package_somewhere() {
        for spec in TOOLS {
            assert!(
                spec.package
                    .winget
                    .or(spec.package.brew)
                    .or(spec.package.choco)
                    .or(spec.package.scoop)
                    .or(spec.package.apt)
                    .or(spec.package.cargo)
                    .or(spec.package.npm)
                    .is_some(),
                "{} has nowhere to come from",
                spec.id
            );
        }
    }

    #[test]
    fn host_keys_round_trip() {
        for host in [
            Host::Windows,
            Host::Wsl("Ubuntu-24.04".into()),
            Host::Darwin,
            Host::Linux,
        ] {
            assert_eq!(Host::parse(&host.key()), host);
        }
    }
}
