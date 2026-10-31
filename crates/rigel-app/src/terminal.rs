//! A real terminal in the window: one PTY per tab, owned by Rust.
//!
//! The renderer is xterm.js, but nothing about the shell lives there: `portable-pty` gives the
//! child a console (hidden on Windows), Rust keeps the master handle, and output streams over
//! `rigel://terminal` as base64 so escape sequences and non-UTF-8 bytes survive the event
//! boundary. Closing the tab kills the shell; closing the window does the same for all of them.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

struct Terminal {
    /// Writes block until the shell drains them; keep that behind a per-terminal lock so a slow
    /// shell never stops another terminal from resizing or closing while the table is held.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
}

fn terminals() -> &'static Mutex<HashMap<String, Terminal>> {
    static CELL: OnceLock<Mutex<HashMap<String, Terminal>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Clone, Serialize)]
pub struct TerminalInfo {
    pub id: String,
    pub shell: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize)]
struct TerminalOutput {
    id: String,
    /// Base64-encoded bytes from the PTY.
    data: String,
    /// The reader ended: the shell exited or the tab was closed.
    exit: bool,
}

fn next_id() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!(
        "term-{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

/// What a terminal opens: PowerShell on Windows, the user's shell elsewhere. A non-empty
/// override from Settings wins, which is how a machine with two shells (pwsh beside Windows
/// PowerShell, zsh beside bash) picks one without changing its environment.
fn shell_choice(override_path: Option<&str>) -> (String, Vec<String>) {
    if let Some(path) = override_path.map(str::trim).filter(|path| !path.is_empty()) {
        return (path.to_string(), Vec::new());
    }
    #[cfg(windows)]
    {
        ("powershell.exe".to_string(), vec!["-NoLogo".to_string()])
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        (shell, Vec::new())
    }
}

fn project_dir(project: Option<String>) -> std::path::PathBuf {
    project
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_dir())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// The PTY half, split from the Tauri command so a test can drive it without an app handle.
fn start_terminal(
    project: Option<String>,
    cols: u16,
    rows: u16,
    shell: Option<&str>,
) -> Result<(TerminalInfo, Terminal, Box<dyn Read + Send>), String> {
    let cwd = project_dir(project);
    let (program, args) = shell_choice(shell);
    let mut command = CommandBuilder::new(&program);
    for arg in &args {
        command.arg(arg);
    }
    command.cwd(&cwd);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");

    let size = PtySize {
        rows: rows.clamp(2, 200),
        cols: cols.clamp(2, 500),
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = native_pty_system()
        .openpty(size)
        .map_err(|error| format!("could not open a terminal: {error}"))?;
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| format!("could not start {program}: {error}"))?;
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| format!("could not read the terminal: {error}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| format!("could not write to the terminal: {error}"))?;

    let info = TerminalInfo {
        id: next_id(),
        shell: program.clone(),
        cwd: cwd.display().to_string(),
    };
    Ok((
        info,
        Terminal {
            writer: Arc::new(Mutex::new(writer)),
            master: pair.master,
            child,
        },
        reader,
    ))
}

/// Open a PTY in the project root and start streaming it to the window.
#[tauri::command]
pub(crate) fn terminal_open(
    app: AppHandle,
    project: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
    shell: Option<String>,
) -> Result<TerminalInfo, String> {
    let (info, mut terminal, mut reader) = start_terminal(
        project,
        cols.unwrap_or(100),
        rows.unwrap_or(30),
        shell.as_deref(),
    )?;
    let id = info.id.clone();

    let event_id = id.clone();
    if let Err(error) = std::thread::Builder::new()
        .name(format!("rigel-pty-{id}"))
        .spawn(move || {
            let mut buffer = [0u8; 8 * 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        let payload = TerminalOutput {
                            id: event_id.clone(),
                            data: base64::engine::general_purpose::STANDARD.encode(&buffer[..read]),
                            exit: false,
                        };
                        let _ = app.emit("rigel://terminal", payload);
                    }
                }
            }
            let _ = app.emit(
                "rigel://terminal",
                TerminalOutput {
                    id: event_id,
                    data: String::new(),
                    exit: true,
                },
            );
        })
    {
        let _ = terminal.child.kill();
        return Err(format!("could not start the terminal reader: {error}"));
    }

    let mut guard = match terminals().lock() {
        Ok(guard) => guard,
        Err(_) => {
            let _ = terminal.child.kill();
            return Err("the terminal table is poisoned".to_string());
        }
    };
    guard.insert(id, terminal);
    Ok(info)
}

#[tauri::command]
pub(crate) fn terminal_write(id: String, data: String) -> Result<(), String> {
    let writer = {
        let guard = terminals()
            .lock()
            .map_err(|_| "the terminal table is poisoned".to_string())?;
        let terminal = guard
            .get(&id)
            .ok_or_else(|| "that terminal is closed".to_string())?;
        Arc::clone(&terminal.writer)
    };
    let mut writer = writer
        .lock()
        .map_err(|_| "that terminal's writer is poisoned".to_string())?;
    writer
        .write_all(data.as_bytes())
        .and_then(|()| writer.flush())
        .map_err(|error| format!("could not write to the terminal: {error}"))
}

#[tauri::command]
pub(crate) fn terminal_resize(id: String, cols: u16, rows: u16) -> Result<(), String> {
    let guard = terminals()
        .lock()
        .map_err(|_| "the terminal table is poisoned".to_string())?;
    let terminal = guard
        .get(&id)
        .ok_or_else(|| "that terminal is closed".to_string())?;
    terminal
        .master
        .resize(PtySize {
            rows: rows.clamp(2, 200),
            cols: cols.clamp(2, 500),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| format!("could not resize the terminal: {error}"))
}

#[tauri::command]
pub(crate) fn terminal_close(id: String) -> Result<(), String> {
    let terminal = terminals()
        .lock()
        .map_err(|_| "the terminal table is poisoned".to_string())?
        .remove(&id);
    if let Some(mut terminal) = terminal {
        let _ = terminal.child.kill();
    }
    Ok(())
}

/// Close every shell when the window exits. Called from the same place that stops chat agents.
pub fn close_all() {
    if let Ok(mut guard) = terminals().lock() {
        for (_, mut terminal) in guard.drain() {
            let _ = terminal.child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{Duration, Instant};

    /// D-settings: the shell override wins over the OS default, and an empty one does not.
    #[test]
    fn a_shell_override_replaces_the_default_shell() {
        #[cfg(windows)]
        let (program, args) = shell_choice(Some(" pwsh.exe "));
        #[cfg(not(windows))]
        let (program, args) = shell_choice(Some(" /usr/bin/fish "));
        assert!(
            program.contains("pwsh") || program.contains("fish"),
            "{program}"
        );
        assert!(args.is_empty(), "an override is spawned as given");
        let (default, _) = shell_choice(Some("   "));
        assert!(
            !default.trim().is_empty(),
            "a blank override means the default"
        );
    }

    #[test]
    fn a_shell_runs_commands_in_the_project_directory() {
        let project = tempfile::tempdir().expect("temp project");
        let cwd = project.path().display().to_string();
        let (info, mut terminal, mut reader) =
            start_terminal(Some(cwd.clone()), 80, 24, None).expect("a PTY starts");

        assert_eq!(info.cwd, cwd);
        assert!(!info.shell.is_empty());

        let (send, receive) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 2_048];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        let _ = send.send(String::from_utf8_lossy(&buffer[..read]).into_owned());
                    }
                }
            }
        });
        // PowerShell asks the terminal for its cursor position before drawing the prompt. xterm
        // answers this in the app; the test has to answer it or the shell waits forever. POSIX
        // shells do not send the query, so they just get a moment to start.
        let mut seen = String::new();
        #[cfg(windows)]
        {
            let started = Instant::now() + Duration::from_secs(15);
            while Instant::now() < started && !seen.contains("\u{1b}[6n") {
                if let Ok(chunk) = receive.recv_timeout(Duration::from_millis(200)) {
                    seen.push_str(&chunk);
                }
            }
            assert!(
                seen.contains("\u{1b}[6n"),
                "the shell never asked for a cursor: {seen:?}"
            );
            let mut writer = terminal.writer.lock().expect("the terminal writer");
            writer
                .write_all(b"\x1b[1;1R")
                .and_then(|()| writer.flush())
                .expect("the shell accepts the cursor reply");
        }
        #[cfg(not(windows))]
        std::thread::sleep(Duration::from_millis(300));
        std::thread::sleep(Duration::from_millis(500));
        #[cfg(windows)]
        let create = b"Set-Content -Path rigel-pty.txt -Value ok\r\n" as &[u8];
        #[cfg(not(windows))]
        let create = b"touch rigel-pty.txt\n" as &[u8];
        let mut writer = terminal.writer.lock().expect("the terminal writer");
        writer
            .write_all(create)
            .and_then(|()| writer.flush())
            .expect("the shell accepts input");

        let marker = project.path().join("rigel-pty.txt");
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut seen = String::new();
        while Instant::now() < deadline && !marker.is_file() {
            if let Ok(chunk) = receive.recv_timeout(Duration::from_millis(200)) {
                seen.push_str(&chunk);
            }
        }
        let _ = terminal.child.kill();
        assert!(
            marker.is_file(),
            "the shell never wrote into the project directory; output: {seen:?}"
        );
    }
}
