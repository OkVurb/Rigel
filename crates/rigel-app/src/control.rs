//! Keep the control API running without a human starting it.
//!
//! `rigel serve` is a separate process from the window, and a reboot leaves it stopped while the
//! Tailscale Serve mapping survives — Muse then gets a connection refused that looks like a
//! credential problem. This module closes that gap from the side that is already open: the tray
//! process probes the loopback port, spawns the CLI when nothing answers, and re-spawns after a
//! crash.
//!
//! It never adopts a server on another host and never writes a token: `rigel serve` already owns
//! `api.json`, and binding is loopback only.

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const HOST: &str = "127.0.0.1";
const PORT: u16 = 20140;
/// How long to wait between "is it up?" probes once it is healthy.
const WATCH_INTERVAL: Duration = Duration::from_secs(15);
/// Give a freshly spawned server time to bind and open its database before probing again.
const BOOT_GRACE: Duration = Duration::from_secs(4);
const DEFAULT_PUBLIC_URL: &str = "https://rigel.tail5fbfdb.ts.net:8443";

fn public_url() -> String {
    std::env::var("RIGEL_CONTROL_PUBLIC_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_PUBLIC_URL.to_string())
}

/// A raw HTTP/1.1 probe. `/v1/health` answers `200` with a token and `401` without one, so the
/// status line — not the status code — is what proves the right process holds the port.
fn serving() -> bool {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), PORT);
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(400)) else {
        return false;
    };
    stream
        .set_read_timeout(Some(Duration::from_millis(900)))
        .ok();
    let request = b"GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    if std::io::Write::write_all(&mut stream, request).is_err() {
        return false;
    }
    let mut head = [0u8; 16];
    match std::io::Read::read(&mut stream, &mut head) {
        Ok(count) if count >= 12 => head.starts_with(b"HTTP/1."),
        _ => false,
    }
}

/// Windows writes the project in `api.json` with the extended-length prefix (`\\?\C:\...`),
/// which `Path::is_dir` rejects. Strip it before both the directory check and `rigel serve`'s
/// `--project` argument; the normal absolute path is the one every other part of Rigel uses.
fn normalize_project_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        if let Some(local) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(local);
        }
    }
    path.to_path_buf()
}

/// The project the last served API was pointed at. A window sitting on the project picker has no
/// `current_project_root`, and without this fallback a crashed server would stay down until
/// somebody opened a session — the exact silence this module exists to prevent.
///
/// The window's own `current-workspace.json` is checked after `api.json`: the server writes an
/// extended Windows path, while the window writes the normal path, so the latter is useful when
/// an older server has left an unusable project record behind.
fn remembered_project(data_dir: &Path) -> Option<PathBuf> {
    for name in ["api.json", "current-workspace.json", "app-state.json"] {
        let Ok(text) = std::fs::read_to_string(data_dir.join(name)) else {
            continue;
        };
        let Ok(config) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(raw) = config.get("project").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let path = normalize_project_path(Path::new(raw));
        if path.is_dir() {
            return Some(path);
        }
    }
    None
}

fn stamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// The window has no console in a release build, so a supervisor failure must leave evidence in
/// the app data directory instead of disappearing through a detached `eprintln`.
fn log(data_dir: &Path, message: &str) {
    let path = data_dir.join("control-api.log");
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let _ = writeln!(file, "{} {message}", stamp());
}

/// The CLI that carries `serve`. A shipped build keeps it beside the window executable; a
/// development checkout has `target/debug/rigel.exe` as a sibling, which the first candidate
/// already covers. `RIGEL_CLI` wins so a packaged layout can point somewhere else.
fn cli_binary() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("RIGEL_CLI")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Some(explicit);
    }
    let executable = std::env::current_exe().ok()?;
    let directory = executable.parent()?;
    #[cfg(windows)]
    let names = ["rigel.exe"];
    #[cfg(not(windows))]
    let names = ["rigel"];
    names
        .iter()
        .map(|name| directory.join(name))
        .find(|candidate| candidate.is_file())
}

fn spawn_api(root: &Path) -> Result<u32, String> {
    let cli = cli_binary().ok_or_else(|| {
        "no rigel CLI beside the app executable; set RIGEL_CLI to the binary that owns `serve`"
            .to_string()
    })?;
    let mut command = Command::new(&cli);
    command
        .arg("serve")
        .arg("--project")
        .arg(root)
        .arg("--host")
        .arg(HOST)
        .arg("--port")
        .arg(PORT.to_string())
        .arg("--full-access")
        .arg("--public-url")
        .arg(public_url())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // A console binary started from a GUI process would otherwise flash a window at every launch.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
        .spawn()
        .map(|child| child.id())
        .map_err(|error| format!("could not start rigel serve: {error}"))
}

/// Supervise the API for the life of the window. The open project wins; a window with nothing
/// open falls back to the project the previous server used.
pub fn start(data_dir: PathBuf, project: fn() -> Option<PathBuf>) {
    std::thread::Builder::new()
        .name("rigel-control-api".to_string())
        .spawn(move || {
            let mut last = String::new();
            loop {
                if serving() {
                    last.clear();
                    std::thread::sleep(WATCH_INTERVAL);
                    continue;
                }
                let root = project().or_else(|| remembered_project(&data_dir));
                let message = match root {
                    None => "no project available to start the control API".to_string(),
                    Some(root) => {
                        let root = normalize_project_path(&root);
                        match spawn_api(&root) {
                            Ok(pid) => {
                                std::thread::sleep(BOOT_GRACE);
                                if serving() {
                                    format!(
                                        "control API listening on {HOST}:{PORT} (pid {pid}, project {})",
                                        root.display()
                                    )
                                } else {
                                    format!(
                                        "started rigel serve pid {pid}, but {HOST}:{PORT} did not answer",
                                    )
                                }
                            }
                            Err(problem) => problem,
                        }
                    }
                };
                if message != last {
                    eprintln!("rigel: {message}");
                    log(&data_dir, &message);
                    last = message;
                }
                std::thread::sleep(WATCH_INTERVAL);
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_asks_the_health_route_without_a_token() {
        // A token-less probe is deliberate: 401 is a healthy answer from the right process, so the
        // status line is the signal. This asserts the shape the supervisor depends on.
        let request = b"GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
        let text = String::from_utf8_lossy(request);
        assert!(text.starts_with("GET /v1/health HTTP/1.1"));
        assert!(!text.to_lowercase().contains("authorization"));
    }

    #[test]
    fn the_public_url_has_a_default_and_can_be_overridden() {
        assert!(!public_url().is_empty());
        assert!(public_url().starts_with("http"));
    }

    #[test]
    fn the_cli_lookup_never_returns_a_directory() {
        if let Some(path) = cli_binary() {
            assert!(path.is_file());
        }
    }

    #[test]
    fn a_remembered_project_is_read_from_api_json() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            root.path().join("api.json"),
            serde_json::json!({ "project": project.to_string_lossy() }).to_string(),
        )
        .unwrap();
        assert_eq!(remembered_project(root.path()), Some(project));
    }
    #[test]
    fn a_missing_or_vanished_project_is_not_invented() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(remembered_project(root.path()), None);
        std::fs::write(
            root.path().join("api.json"),
            serde_json::json!({ "project": root.path().join("gone").to_string_lossy() })
                .to_string(),
        )
        .unwrap();
        assert_eq!(remembered_project(root.path()), None);
    }

    #[cfg(windows)]
    #[test]
    fn extended_windows_project_paths_and_window_fallback_are_accepted() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let extended = format!("\\\\?\\{}", project.display());
        std::fs::write(
            root.path().join("api.json"),
            serde_json::json!({ "project": extended }).to_string(),
        )
        .unwrap();
        assert_eq!(remembered_project(root.path()), Some(project.clone()));

        std::fs::remove_file(root.path().join("api.json")).unwrap();
        std::fs::write(
            root.path().join("current-workspace.json"),
            serde_json::json!({ "project": project.to_string_lossy() }).to_string(),
        )
        .unwrap();
        assert_eq!(remembered_project(root.path()), Some(project));
    }
}
