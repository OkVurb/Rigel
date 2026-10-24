//! Child processes that must never flash a console window.
//!
//! A packaged Tauri app is a GUI process with no console. Windows gives every
//! console-subsystem child of such a process a new console window, so a turn that runs `git`,
//! `cmd /C …` or a tool probe would pop a black rectangle over the window for as long as the
//! command lives. `CREATE_NO_WINDOW` starts the child without a console window; piped stdio,
//! exit codes and timeouts are unchanged.

/// `CREATE_NO_WINDOW` from the Windows process-creation flags.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Mark a standard-library command as hidden. No-op away from Windows.
pub fn hide(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// The same flag for a Tokio command, which is what turn tools and probes use.
pub fn hide_tokio(command: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}
