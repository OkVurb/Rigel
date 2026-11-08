//! Bring the control API back at logon without anyone running a command.
//!
//! The desktop supervisor covers a crashed server while the window is open. This covers the other
//! half: a machine that was restarted, where the Tailscale Serve mapping still points at
//! 127.0.0.1:20140 but nothing is listening.
//!
//! This is a per-user `Run` value, not a scheduled task: task creation under the task scheduler
//! needs elevation, and an API that only starts after an admin prompt is not automatic. `Run` is
//! per-user, needs no privilege, and shows up in Task Manager's startup list where it can be
//! turned off. `--hide-window` keeps the console from appearing.

use std::path::Path;
use std::process::ExitCode;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "RigelControlAPI";

pub fn apply(
    install: bool,
    uninstall: bool,
    project: &Path,
    host: &str,
    port: u16,
    public_url: Option<&str>,
    full_access: bool,
) -> ExitCode {
    if uninstall {
        return remove();
    }
    if !install {
        eprintln!("rigel: pass --install or --uninstall");
        return ExitCode::from(2);
    }
    install_entry(project, host, port, public_url, full_access)
}

#[cfg(windows)]
fn run_reg(args: &[&str]) -> (bool, String) {
    match std::process::Command::new("reg").args(args).output() {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            (output.status.success(), text.trim().to_string())
        }
        Err(error) => (false, error.to_string()),
    }
}

/// A verbatim (`\\?\`) path is what `canonicalize` returns, and it is not accepted everywhere a
/// plain path is — including by this command when it runs again at logon. Store the plain form.
#[cfg(windows)]
fn plain(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        text.strip_prefix(r"\\?\")
            .map(str::to_string)
            .unwrap_or(text)
    }
}

#[cfg(windows)]
fn install_entry(
    project: &Path,
    host: &str,
    port: u16,
    public_url: Option<&str>,
    full_access: bool,
) -> ExitCode {
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("rigel: cannot name this executable for the startup entry: {error}");
            return ExitCode::from(2);
        }
    };
    let mut command = format!(
        "\"{}\" serve --project \"{}\" --host {host} --port {port} --hide-window",
        executable.display(),
        plain(project)
    );
    if full_access {
        command.push_str(" --full-access");
    }
    if let Some(url) = public_url {
        command.push_str(&format!(" --public-url {url}"));
    }
    let (ok, text) = run_reg(&[
        "add", RUN_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d", &command, "/f",
    ]);
    if !ok {
        eprintln!("rigel: could not write the startup entry: {text}");
        return ExitCode::from(2);
    }
    println!("registered \"{VALUE_NAME}\" at logon:\n  {command}");
    println!("it starts the API hidden; a second start exits quietly when one is already up");
    ExitCode::SUCCESS
}

#[cfg(windows)]
fn remove() -> ExitCode {
    let (ok, text) = run_reg(&["delete", RUN_KEY, "/v", VALUE_NAME, "/f"]);
    if ok {
        println!("removed \"{VALUE_NAME}\"");
        ExitCode::SUCCESS
    } else {
        eprintln!("rigel: could not remove the startup entry: {text}");
        ExitCode::from(2)
    }
}

#[cfg(not(windows))]
fn install_entry(
    _project: &Path,
    _host: &str,
    _port: u16,
    _public_url: Option<&str>,
    _full_access: bool,
) -> ExitCode {
    eprintln!(
        "rigel: --install writes a Windows startup entry; on this platform run the desktop app, which supervises the API"
    );
    ExitCode::from(2)
}

#[cfg(not(windows))]
fn remove() -> ExitCode {
    eprintln!(
        "rigel: --uninstall removes a Windows startup entry; there is none to remove on this platform"
    );
    ExitCode::from(2)
}
