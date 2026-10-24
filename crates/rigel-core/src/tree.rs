//! The tree hash a checkpoint records (I8).
//!
//! A resume is clean only when the files the session can touch are byte-identical to the
//! checkpoint. Git's index is not usable for that: it ignores unstaged work, and a session's
//! changes are exactly the unstaged ones. So Rigel hashes the content of every tracked file
//! itself, plus the set of paths. `.rigel/` is excluded — it holds the session log, not the
//! project.
//!
//! Git is only used to *enumerate* files (`git ls-files`), which is cheap and respects
//! `.gitignore`. Without git, the walker skips the usual build directories by name.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum TreeError {
    #[error("could not read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Stable across runs and machines: sorted `path\0content-sha` lines, hashed once.
pub fn tree_hash(root: &Path) -> Result<String, TreeError> {
    let files = tracked_files(root)?;
    let mut hasher = Sha256::new();
    for relative in files {
        let path = root.join(&relative);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(TreeError::Io {
                    path: path.clone(),
                    source,
                });
            }
        };
        hasher.update(relative.replace('\\', "/").as_bytes());
        hasher.update([0]);
        hasher.update(sha256_hex(&bytes).as_bytes());
        hasher.update(b"\n");
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn tracked_files(root: &Path) -> Result<Vec<String>, TreeError> {
    if let Some(mut files) = git_files(root)? {
        files.sort();
        files.dedup();
        return Ok(files);
    }
    let mut files = Vec::new();
    walk(root, root, &mut files, 0)?;
    files.sort();
    Ok(files)
}

fn git_files(root: &Path) -> Result<Option<Vec<String>>, TreeError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args([
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--exclude-standard",
    ]);
    crate::process::hide(&mut command);
    let output = command.output();

    let output = match output {
        Ok(output) if output.status.success() => output,
        // No git, not a repository, or a broken install: fall back to the walker.
        _ => return Ok(None),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    Ok(Some(
        text.split('\0')
            .filter(|path| !path.is_empty())
            .filter(|path| !path.starts_with(".rigel/"))
            .map(str::to_string)
            .collect(),
    ))
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>, depth: usize) -> Result<(), TreeError> {
    if depth > 24 {
        return Ok(());
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(TreeError::Io {
                path: dir.to_path_buf(),
                source,
            });
        }
    };
    for entry in entries {
        let entry = entry.map_err(|source| TreeError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if matches!(
            name.as_str(),
            ".git" | ".rigel" | "node_modules" | "target" | "dist" | ".next"
        ) {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| TreeError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            walk(root, &path, out, depth + 1)?;
        } else if file_type.is_file() {
            if let Ok(relative) = path.strip_prefix(root) {
                out.push(relative.to_string_lossy().into_owned());
            }
        }
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_changes_the_hash_and_a_revert_restores_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::write(tmp.path().join("a.txt"), "one\n").unwrap();
        fs::write(tmp.path().join("b.txt"), "two\n").unwrap();

        let first = tree_hash(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.txt"), "ONE\n").unwrap();
        let second = tree_hash(tmp.path()).unwrap();
        assert_ne!(first, second);

        fs::write(tmp.path().join("a.txt"), "one\n").unwrap();
        assert_eq!(tree_hash(tmp.path()).unwrap(), first);
    }

    #[test]
    fn the_session_directory_is_not_part_of_the_tree() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::write(tmp.path().join("a.txt"), "one\n").unwrap();
        let before = tree_hash(tmp.path()).unwrap();
        fs::create_dir_all(tmp.path().join(".rigel")).unwrap();
        fs::write(tmp.path().join(".rigel/rigel.db"), b"not a project file").unwrap();
        assert_eq!(tree_hash(tmp.path()).unwrap(), before);
    }
}
