//! Content-addressed storage for skill bodies.
//!
//! §6.1 puts bodies at `<data dir>\blobs\<sha256>` and P2.3 keeps them out of SQLite: a
//! 3,000-skill index would otherwise put megabytes of prose through the page cache for every
//! search, when the search only ever looks at `name` and `description`.
//!
//! Order matters — §6.4: **the blob is written before its row**. A crash between the two
//! leaves an orphan blob, which `gc` collects. The other order would leave a row pointing at
//! a body that never existed.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    /// `<data dir>/blobs`, created on first write rather than on open: a read-only index
    /// (the shelf over a copy) must still be usable.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            root: data_dir.join("blobs"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_for(&self, sha: &str) -> PathBuf {
        self.root.join(sha)
    }

    /// Store `bytes` and return the hex sha256 that addresses them.
    pub fn put(&self, bytes: &[u8]) -> io::Result<String> {
        let sha = sha256_hex(bytes);
        let path = self.path_for(&sha);
        if path.exists() {
            return Ok(sha);
        }

        fs::create_dir_all(&self.root)?;

        // Same-directory temp file, then rename: a reader never sees a half-written blob.
        let tmp = self.root.join(format!(".{sha}.{}.tmp", std::process::id()));
        {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        Ok(sha)
    }

    pub fn get(&self, sha: &str) -> io::Result<Option<Vec<u8>>> {
        match fs::read(self.path_for(sha)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn get_text(&self, sha: &str) -> io::Result<Option<String>> {
        match self.get(sha)? {
            Some(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            None => Ok(None),
        }
    }

    pub fn exists(&self, sha: &str) -> bool {
        self.path_for(sha).exists()
    }

    /// Delete every blob not in `live`. Returns how many were removed.
    pub fn gc(&self, live: &HashSet<String>) -> io::Result<usize> {
        if !self.root.exists() {
            return Ok(0);
        }
        let mut removed = 0;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || live.contains(&name) {
                continue;
            }
            // Only touch things that look like sha256 names; anything else was put there by
            // someone else and is not ours to delete.
            if name.len() == 64 && name.chars().all(|c| c.is_ascii_hexdigit()) {
                fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
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
    use tempfile::TempDir;

    #[test]
    fn identical_bodies_are_stored_once_under_their_hash() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path());

        let a = store.put(b"# skill\n").unwrap();
        let b = store.put(b"# skill\n").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_eq!(store.get(&a).unwrap().unwrap(), b"# skill\n");

        let files = fs::read_dir(store.root()).unwrap().count();
        assert_eq!(files, 1, "the second write is a no-op");
    }

    #[test]
    fn a_missing_blob_is_none_not_an_error() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path());
        assert!(store.get(&"0".repeat(64)).unwrap().is_none());
    }

    #[test]
    fn gc_keeps_live_blobs_and_ignores_foreign_files() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path());
        let keep = store.put(b"keep").unwrap();
        let drop = store.put(b"drop").unwrap();
        fs::write(store.root().join("notes.txt"), b"mine").unwrap();
        fs::write(store.root().join(format!(".{keep}.tmp")), b"partial").unwrap();

        let live: HashSet<String> = [keep.clone()].into_iter().collect();
        assert_eq!(store.gc(&live).unwrap(), 1);
        assert!(store.exists(&keep));
        assert!(!store.exists(&drop));
        assert!(
            store.root().join("notes.txt").exists(),
            "not ours to delete"
        );
        assert!(store.root().join(format!(".{keep}.tmp")).exists());
    }
}
