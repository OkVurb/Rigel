//! Skill script sandbox (roadmap 67).
//!
//! A skill script runs with a cleared environment, only the variables its contract
//! declares, a working directory inside the project, a hard timeout and captured output.
//! Nothing the app holds — provider keys, `RIGEL_*` settings, the user's shell history —
//! reaches the child unless the contract names it.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// The only inherited variables. `PATH` is needed to resolve an interpreter; the Windows
/// loader needs `SystemRoot` and `COMSPEC`; the rest make a Unix child behave.
pub const ENVIRONMENT_ALLOWLIST: &[&str] = &[
    "PATH",
    "Path",
    "SystemRoot",
    "SYSTEMROOT",
    "COMSPEC",
    "PATHEXT",
    "TEMP",
    "TMP",
    "HOME",
    "USERPROFILE",
    "LANG",
    "LC_ALL",
];

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Script {
    pub program: String,
    pub args: Vec<String>,
    /// Declared by the skill's contract. These override the inherited allowlist.
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub max_output: usize,
}

impl Script {
    pub fn new(root: impl Into<PathBuf>, program: impl Into<String>) -> Self {
        let root = root.into();
        Self {
            program: program.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: root,
            timeout: DEFAULT_TIMEOUT,
            max_output: MAX_OUTPUT_BYTES,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The exact command line, for `--dry-run` before anything starts.
    pub fn command_line(&self) -> String {
        let mut parts = vec![self.program.clone()];
        parts.extend(self.args.iter().cloned());
        parts.join(" ")
    }

    /// The environment the child receives, after the allowlist and the declared values.
    pub fn environment(&self, inherited: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        let mut environment = BTreeMap::new();
        for (name, value) in inherited {
            if ENVIRONMENT_ALLOWLIST.contains(&name.as_str()) {
                environment.insert(name.clone(), value.clone());
            }
        }
        for (name, value) in &self.env {
            environment.insert(name.clone(), value.clone());
        }
        environment
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub truncated: bool,
    pub duration_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("the script cwd {cwd} is outside the project root {root}")]
    CwdOutsideRoot { cwd: PathBuf, root: PathBuf },

    #[error("could not start {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not read the script output: {0}")]
    Io(#[from] std::io::Error),

    #[error("the script has no program")]
    NoProgram,
}

/// Run one script. The child is killed at the deadline and its partial output is kept.
pub fn run(
    root: &Path,
    script: &Script,
    inherited: &BTreeMap<String, String>,
) -> Result<Output, RunError> {
    if script.program.trim().is_empty() {
        return Err(RunError::NoProgram);
    }
    let root = root.canonicalize()?;
    let cwd = script.cwd.canonicalize()?;
    if !cwd.starts_with(&root) {
        return Err(RunError::CwdOutsideRoot {
            cwd: script.cwd.clone(),
            root,
        });
    }

    let started = Instant::now();
    let mut command = Command::new(&script.program);
    command
        .args(&script.args)
        .current_dir(&cwd)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in script.environment(inherited) {
        command.env(name, value);
    }

    let mut child = command.spawn().map_err(|source| RunError::Spawn {
        program: script.program.clone(),
        source,
    })?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());

    let deadline = started + script.timeout;
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            timed_out = true;
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };

    let stdout = stdout.recv().unwrap_or_default();
    let stderr = stderr.recv().unwrap_or_default();
    let (stdout, stdout_truncated) = clamp(&stdout, script.max_output);
    let (stderr, stderr_truncated) = clamp(&stderr, script.max_output);

    Ok(Output {
        exit_code: status.and_then(|status| status.code()),
        stdout,
        stderr,
        timed_out,
        truncated: stdout_truncated || stderr_truncated,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

fn drain<R: Read + Send + 'static>(reader: Option<R>) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    if let Some(mut reader) = reader {
        thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = reader.read_to_end(&mut buffer);
            let _ = sender.send(buffer);
        });
    }
    receiver
}

fn clamp(bytes: &[u8], limit: usize) -> (String, bool) {
    let truncated = bytes.len() > limit;
    let slice = &bytes[..bytes.len().min(limit)];
    (String::from_utf8_lossy(slice).into_owned(), truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    fn inherited() -> BTreeMap<String, String> {
        ["PATH", "SystemRoot", "COMSPEC", "PATHEXT", "TEMP", "TMP"]
            .iter()
            .filter_map(|name| {
                std::env::var(name)
                    .ok()
                    .map(|value| (name.to_string(), value))
            })
            .collect()
    }

    #[test]
    fn the_exact_command_line_is_available_before_the_run() {
        let script = Script::new(".", "python")
            .arg("scripts/run.py")
            .arg("--check");
        assert_eq!(script.command_line(), "python scripts/run.py --check");
    }

    #[test]
    fn a_cwd_outside_the_project_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let script = Script::new(root.path(), "echo");
        let script = script.cwd(outside.path());
        let result = run(root.path(), &script, &BTreeMap::new());
        assert!(matches!(result, Err(RunError::CwdOutsideRoot { .. })));
    }

    #[cfg(windows)]
    #[test]
    fn the_child_environment_is_cleared_and_only_declared_values_appear() {
        let root = tempfile::tempdir().unwrap();
        let mut inherited = inherited();
        inherited.insert("RIGEL_TEST_SECRET".to_string(), "must-not-leak".to_string());
        inherited.insert("OPENAI_API_KEY".to_string(), "sk-must-not-leak".to_string());
        let script = Script::new(root.path(), "cmd")
            .arg("/C")
            .arg("set")
            .env("RIGEL_SCRIPT_FLAG", "1");
        let output = run(root.path(), &script, &inherited).unwrap();
        assert_eq!(output.exit_code, Some(0));
        assert!(output.stdout.contains("RIGEL_SCRIPT_FLAG=1"));
        assert!(!output.stdout.contains("RIGEL_TEST_SECRET"));
        assert!(!output.stdout.contains("OPENAI_API_KEY"));
        assert!(!output.stdout.contains("must-not-leak"));
    }

    #[cfg(windows)]
    #[test]
    fn a_script_that_outlives_its_deadline_is_killed_and_keeps_partial_output() {
        let root = tempfile::tempdir().unwrap();
        let script = Script::new(root.path(), "powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg("Start-Sleep -Seconds 10")
            .timeout(Duration::from_millis(250));
        let output = run(root.path(), &script, &inherited()).unwrap();
        assert!(output.timed_out, "{output:?}");
        assert!(output.duration_ms < 3_000);
        assert!(output.exit_code.is_none());
    }
}
