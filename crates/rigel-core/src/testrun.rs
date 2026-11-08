//! Q268: one explicit test run for the open project.
//!
//! Rigel only starts a command it recognises from a manifest, never a string typed into the
//! renderer: Cargo, Go, a `package.json` test script, or pytest. The run is outside the agent
//! loop, its output is bounded, and the window gets both the counts and the captured log.

use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Serialize;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

const RUN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_OUTPUT: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestPlan {
    pub program: String,
    pub args: Vec<String>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestRun {
    pub command: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub passed: usize,
    pub failed: usize,
    pub output: String,
    pub truncated: bool,
    pub timed_out: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Error)]
pub enum TestRunError {
    #[error(
        "No test command found. Rigel looks for Cargo.toml, go.mod, a package.json test script, or pytest."
    )]
    NotFound,
    #[error("could not start `{command}`: {source}")]
    Start {
        command: String,
        source: std::io::Error,
    },
    #[error("the test runner could not be waited on: {0}")]
    Wait(String),
}

/// The first manifest Rigel recognises wins. A Tauri project is tested through Cargo; a plain
/// JavaScript project through its package manager's test script.
pub fn detect(project: &Path) -> Option<TestPlan> {
    if project.join("Cargo.toml").is_file() {
        return Some(plan("cargo", &["test"]));
    }
    if project.join("go.mod").is_file() {
        return Some(plan("go", &["test", "./..."]));
    }
    if let Some(plan) = package_test(project) {
        return Some(plan);
    }
    if project.join("pyproject.toml").is_file() || project.join("pytest.ini").is_file() {
        return Some(plan("python", &["-m", "pytest"]));
    }
    None
}

fn plan(program: &str, args: &[&str]) -> TestPlan {
    TestPlan {
        program: program.to_string(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        label: std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn package_test(project: &Path) -> Option<TestPlan> {
    let raw = fs::read_to_string(project.join("package.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let script = manifest.get("scripts")?.get("test")?.as_str()?;
    if script.trim().is_empty() {
        return None;
    }
    if project.join("pnpm-lock.yaml").is_file() {
        return Some(plan("pnpm", &["test"]));
    }
    if project.join("yarn.lock").is_file() {
        return Some(plan("yarn", &["test"]));
    }
    if project.join("bun.lock").is_file() || project.join("bun.lockb").is_file() {
        return Some(plan("bun", &["run", "test"]));
    }
    Some(plan("npm", &["test"]))
}

/// Run the detected command in the project root. A test runner that never exits is stopped at
/// the fifteen-minute deadline and its partial output is still returned.
pub async fn run(project: &Path) -> Result<TestRun, TestRunError> {
    let plan = detect(project).ok_or(TestRunError::NotFound)?;
    run_plan(project, &plan).await
}

async fn run_plan(project: &Path, plan: &TestPlan) -> Result<TestRun, TestRunError> {
    let mut command = Command::new(&plan.program);
    command
        .args(&plan.args)
        .current_dir(project)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::process::hide_tokio(&mut command);

    let mut child = command.spawn().map_err(|source| TestRunError::Start {
        command: plan.label.clone(),
        source,
    })?;
    let stdout = child
        .stdout
        .take()
        .expect("the test runner's stdout is piped");
    let stderr = child
        .stderr
        .take()
        .expect("the test runner's stderr is piped");
    let stdout = tokio::spawn(read_bounded(stdout));
    let stderr = tokio::spawn(read_bounded(stderr));

    let started = Instant::now();
    let waited = tokio::time::timeout(RUN_TIMEOUT, child.wait()).await;
    let timed_out = waited.is_err();
    let exit_code = match waited {
        Ok(Ok(status)) => status.code(),
        Ok(Err(error)) => return Err(TestRunError::Wait(error.to_string())),
        Err(_) => {
            let _ = child.kill().await;
            None
        }
    };

    let stdout = stdout.await.unwrap_or_else(|_| Captured::default());
    let stderr = stderr.await.unwrap_or_else(|_| Captured::default());
    let mut output = String::from_utf8_lossy(&stdout.bytes).into_owned();
    if !stderr.bytes.is_empty() {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&String::from_utf8_lossy(&stderr.bytes));
    }
    let truncated = stdout.truncated || stderr.truncated;
    if truncated {
        output.push_str("\n\n[Rigel kept the first 256 KB of output.]");
    }
    if timed_out {
        output.push_str("\n\n[Rigel stopped the test run after fifteen minutes.]");
    }
    let (passed, failed) = parse_counts(&output);
    Ok(TestRun {
        command: plan.label.clone(),
        ok: !timed_out && exit_code == Some(0),
        exit_code,
        passed,
        failed,
        output,
        truncated,
        timed_out,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn read_bounded<R: AsyncRead + Unpin>(mut source: R) -> Captured {
    let mut captured = Captured::default();
    let mut chunk = [0u8; 8 * 1024];
    loop {
        let read = match source.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let room = MAX_OUTPUT.saturating_sub(captured.bytes.len());
        if read > room {
            captured.truncated = true;
        }
        if room > 0 {
            captured.bytes.extend_from_slice(&chunk[..read.min(room)]);
        }
    }
    captured
}

/// Counts are read from summary lines, not from test names. Rust, Jest, Vitest, pytest and
/// Playwright all put a number before the words; Go only marks individual test results.
fn parse_counts(output: &str) -> (usize, usize) {
    let mut passed = 0;
    let mut failed = 0;
    let mut saw_summary = false;
    for line in output.lines() {
        let line_passed = count_before(line, "passed").or_else(|| count_before(line, "passing"));
        let line_failed = count_before(line, "failed").or_else(|| count_before(line, "failing"));
        if line_passed.is_some() || line_failed.is_some() {
            saw_summary = true;
            passed += line_passed.unwrap_or(0);
            failed += line_failed.unwrap_or(0);
        }
    }
    if saw_summary {
        return (passed, failed);
    }
    for line in output.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("--- PASS:") {
            passed += 1;
        } else if trimmed.starts_with("--- FAIL:") {
            failed += 1;
        }
    }
    (passed, failed)
}

fn count_before(line: &str, word: &str) -> Option<usize> {
    let lower = line.to_ascii_lowercase();
    // A status word such as "FAILED." has no number before it; keep looking for the summary
    // occurrence, where the count is immediately before the word.
    lower.match_indices(word).find_map(|(at, _)| {
        let prefix = &lower[..at];
        // `test_that_passed` and `bypassed` are names, not summaries.
        if prefix
            .chars()
            .next_back()
            .is_some_and(|character| character.is_alphanumeric() || character == '_')
        {
            return None;
        }
        prefix
            .split(|character: char| !character.is_ascii_digit())
            .rfind(|part| !part.is_empty())?
            .parse()
            .ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_projects_rigel_can_test() {
        let rust = tempfile::tempdir().unwrap();
        fs::write(rust.path().join("Cargo.toml"), "[package]").unwrap();
        assert_eq!(detect(rust.path()).unwrap().label, "cargo test");

        let go = tempfile::tempdir().unwrap();
        fs::write(go.path().join("go.mod"), "module example.com/demo").unwrap();
        assert_eq!(detect(go.path()).unwrap().label, "go test ./...");

        let node = tempfile::tempdir().unwrap();
        fs::write(
            node.path().join("package.json"),
            r#"{"scripts":{"test":"vitest run"}}"#,
        )
        .unwrap();
        assert_eq!(detect(node.path()).unwrap().label, "npm test");

        let python = tempfile::tempdir().unwrap();
        fs::write(python.path().join("pytest.ini"), "[pytest]").unwrap();
        assert_eq!(detect(python.path()).unwrap().label, "python -m pytest");
    }

    #[test]
    fn a_package_without_a_test_script_is_not_a_test_project() {
        let node = tempfile::tempdir().unwrap();
        fs::write(
            node.path().join("package.json"),
            r#"{"scripts":{"build":"tsc"}}"#,
        )
        .unwrap();
        assert!(detect(node.path()).is_none());
    }

    #[test]
    fn counts_come_from_the_summary_not_the_names() {
        assert_eq!(
            parse_counts("test result: FAILED. 18 passed; 1 failed; 0 ignored"),
            (18, 1)
        );
        assert_eq!(
            parse_counts("Tests: 1 failed, 18 passed, 19 total"),
            (18, 1)
        );
        assert_eq!(parse_counts("1 failed, 18 passed in 2.31s"), (18, 1));
        assert_eq!(parse_counts("19 passed (17.2s)"), (19, 0));
        assert_eq!(parse_counts("--- PASS: TestOne\n--- FAIL: TestTwo"), (1, 1));
        assert_eq!(parse_counts("10 test_that_passed"), (0, 0));
    }

    #[tokio::test]
    async fn a_plan_captures_output_and_a_nonzero_exit() {
        let project = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let plan = plan("cmd", &["/C", "echo 3 passed & echo 1 failed & exit /b 1"]);
        #[cfg(not(windows))]
        let plan = plan("sh", &["-c", "printf '3 passed\\n1 failed\\n'; exit 1"]);

        let run = run_plan(project.path(), &plan).await.unwrap();
        assert_eq!(run.passed, 3);
        assert_eq!(run.failed, 1);
        assert_eq!(run.exit_code, Some(1));
        assert!(!run.ok);
        assert!(run.output.contains("3 passed"));
    }
}
