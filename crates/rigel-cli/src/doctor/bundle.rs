//! The archive `rigel doctor --support-bundle` writes (roadmap 12).
//!
//! It carries the doctor report, a version/platform manifest, the redacted settings, the crash
//! tail on disk and the in-memory diagnostic ring — the material a maintainer actually reads —
//! plus `omitted.txt`, which names every other entry in the data directory and why it is not
//! inside. Nothing leaves the machine: the file is written where the user asked, and every text
//! passes the shared redactor before it is added.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rigel_core::diagnostics::{self, Secret, ring};
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;

/// Keep the archive small; a longer crash log already has the relevant stack at its end.
const MAX_LOG_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub struct Written {
    pub path: PathBuf,
    pub included: Vec<String>,
    pub omitted: Vec<String>,
}

/// Write a bundle using the secrets Rigel can currently resolve.
pub fn write(doctor: &Value, data_dir: &Path, project: &Path, dest: &Path) -> io::Result<Written> {
    write_with_secrets(doctor, data_dir, project, dest, &diagnostics::secrets())
}

/// The testable form: the caller supplies the secret set so a seeded value can prove redaction.
pub fn write_with_secrets(
    doctor: &Value,
    data_dir: &Path,
    project: &Path,
    dest: &Path,
    secrets: &[Secret],
) -> io::Result<Written> {
    let mut included = Vec::new();
    let mut omitted = Vec::new();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();

    let events = ring::shared()
        .lock()
        .map(|ring| ring.lines())
        .unwrap_or_default();
    let schema: Vec<&Value> = doctor
        .get("checks")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter(|row| {
                    row.get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| id.starts_with("schema."))
                })
                .collect()
        })
        .unwrap_or_default();
    let manifest = json!({
        "rigel": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "project": project.display().to_string(),
        "data_dir": data_dir.display().to_string(),
        "generated_ms": now_ms(),
        "diagnostic_ring_events": events.len(),
        "schema": schema,
    });
    add(
        &mut entries,
        &mut included,
        "manifest.json",
        serde_json::to_vec_pretty(&manifest).map_err(io::Error::other)?,
    );

    let doctor_text = redact(
        &serde_json::to_string_pretty(doctor).map_err(io::Error::other)?,
        secrets,
    );
    add(
        &mut entries,
        &mut included,
        "doctor.json",
        doctor_text.into_bytes(),
    );

    let mut settings_seen = false;
    for name in ["settings.toml", "settings.json"] {
        let path = data_dir.join(name);
        if !path.exists() {
            continue;
        }
        settings_seen = true;
        match fs::read_to_string(&path) {
            Ok(text) => add(
                &mut entries,
                &mut included,
                name,
                redact(&text, secrets).into_bytes(),
            ),
            Err(error) => omitted.push(format!("{name}: unreadable ({error})")),
        }
    }
    if !settings_seen {
        omitted.push("settings.toml, settings.json: not present in the data directory".to_string());
    }

    match fs::read(data_dir.join("crash.log")) {
        Ok(bytes) => {
            let start = bytes.len().saturating_sub(MAX_LOG_BYTES);
            let text = String::from_utf8_lossy(&bytes[start..]).into_owned();
            add(
                &mut entries,
                &mut included,
                "logs/crash.log",
                redact(&text, secrets).into_bytes(),
            );
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            omitted.push("crash.log: not present (nothing has crashed)".to_string())
        }
        Err(error) => omitted.push(format!("crash.log: unreadable ({error})")),
    }

    // The ring redacts token shapes on the way in; the caller's resolved secrets are applied
    // again here so a key that is secret only in this process cannot ride out in a line.
    let mut event_text = events
        .iter()
        .map(|line| redact(line, secrets))
        .collect::<Vec<_>>()
        .join("\n");
    if !event_text.is_empty() {
        event_text.push('\n');
    }
    add(
        &mut entries,
        &mut included,
        "logs/events.log",
        event_text.into_bytes(),
    );

    // Every other top-level entry is named, so an omission is auditable instead of silent.
    let mut top: Vec<String> = fs::read_dir(data_dir)
        .map(|dir| {
            dir.flatten()
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    top.sort();
    for name in top {
        if matches!(
            name.as_str(),
            "crash.log" | "settings.toml" | "settings.json"
        ) {
            continue;
        }
        let reason = match name.as_str() {
            "index.db" | "index.db-wal" | "index.db-shm" => {
                "index database; session rows stay on the machine"
            }
            "blobs" => "skill blob store; a binary cache, not a log",
            "diagnostics" => "earlier bundles; this archive replaces them",
            "exports" => "session exports; may hold unrelated sessions",
            "updates" => "downloaded installers; too large and not diagnostic",
            _ if name.ends_with(".db")
                || name.ends_with(".db-wal")
                || name.ends_with(".db-shm") =>
            {
                "database; not a log"
            }
            _ => "not on the diagnostics allowlist",
        };
        omitted.push(format!("{name}: {reason}"));
    }
    let project_db = project.join(".rigel").join("rigel.db");
    if project_db.exists() {
        omitted.push(format!(
            "{}: project database; not a log",
            project_db.display()
        ));
    }

    let readme = format!(
        "Rigel support bundle\n\n\
         Rigel {version} on {os}/{arch}, generated {ms} ms since the epoch.\n\n\
         manifest.json   version, platform, schema state and the ring size\n\
         doctor.json     the same report `rigel doctor --json` prints\n\
         settings.*      the data directory's settings, redacted\n\
         logs/crash.log  the newest crash record, redacted\n\
         logs/events.log the in-memory diagnostic ring, already redacted\n\
         omitted.txt     every entry that was left out, and why\n\n\
         Nothing was sent anywhere. Attach this file to an issue if you want help.\n",
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        ms = now_ms(),
    );
    add(
        &mut entries,
        &mut included,
        "README.txt",
        readme.into_bytes(),
    );

    let mut omitted_text =
        String::from("Every entry the bundle looked at but did not include, and why.\n\n");
    for line in &omitted {
        omitted_text.push_str("- ");
        omitted_text.push_str(line);
        omitted_text.push('\n');
    }
    add(
        &mut entries,
        &mut included,
        "omitted.txt",
        omitted_text.into_bytes(),
    );

    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(dest)?;
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in &entries {
        writer
            .start_file(name.as_str(), options)
            .map_err(zip_error)?;
        writer.write_all(bytes)?;
    }
    writer.finish().map_err(zip_error)?;

    Ok(Written {
        path: dest.to_path_buf(),
        included,
        omitted,
    })
}

fn add(
    entries: &mut Vec<(String, Vec<u8>)>,
    included: &mut Vec<String>,
    name: &str,
    bytes: Vec<u8>,
) {
    included.push(name.to_string());
    entries.push((name.to_string(), bytes));
}

fn redact(text: &str, secrets: &[Secret]) -> String {
    diagnostics::redact_text(text, secrets)
}

fn zip_error(error: zip::result::ZipError) -> io::Error {
    io::Error::other(error)
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    fn entries(path: &Path) -> Vec<(String, Vec<u8>)> {
        let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut out = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let name = entry.name().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            out.push((name, bytes));
        }
        out
    }

    #[test]
    fn a_bundle_carries_the_ring_manifest_and_redacted_material() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(project.join(".rigel")).unwrap();
        fs::write(project.join(".rigel").join("rigel.db"), b"database").unwrap();

        let data = tmp.path().join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("index.db"), b"database").unwrap();
        let secret = Secret::new("hunter2-seeded-secret");
        fs::write(
            data.join("settings.toml"),
            format!("api_key = \"{}\"\n", secret.expose()),
        )
        .unwrap();
        fs::write(
            data.join("crash.log"),
            "panic with sk-proj-AbCdEfGhIjKlMnOpQrStUvWx",
        )
        .unwrap();
        ring::shared_record(
            ring::Level::Error,
            Some("turn"),
            "ring event with hunter2-seeded-secret",
        );

        let doctor = json!({
            "ok": true,
            "checks": [{"id": "schema.index", "status": "ok", "detail": "version 3"}],
        });
        let dest = tmp.path().join("out.zip");
        let written = write_with_secrets(
            &doctor,
            &data,
            &project,
            &dest,
            std::slice::from_ref(&secret),
        )
        .unwrap();

        assert!(
            written
                .included
                .iter()
                .any(|name| name == "logs/events.log")
        );
        assert!(written.omitted.iter().any(|line| line.contains("index.db")));
        assert!(written.omitted.iter().any(|line| line.contains("rigel.db")));

        let unpacked = entries(&dest);
        let names: Vec<&str> = unpacked.iter().map(|(name, _)| name.as_str()).collect();
        for expected in [
            "README.txt",
            "manifest.json",
            "doctor.json",
            "settings.toml",
            "logs/crash.log",
            "logs/events.log",
            "omitted.txt",
        ] {
            assert!(names.contains(&expected), "missing {expected}: {names:?}");
        }

        for (name, bytes) in &unpacked {
            let text = String::from_utf8_lossy(bytes);
            assert!(
                !text.contains("hunter2-seeded-secret"),
                "{name} kept the seeded secret"
            );
            assert!(
                !text.contains("sk-proj-AbCdEfGhIjKlMnOpQrStUvWx"),
                "{name} kept the shaped secret"
            );
        }

        let find = |wanted: &str| -> String {
            unpacked
                .iter()
                .find(|(name, _)| name == wanted)
                .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
                .unwrap_or_default()
        };
        let events = find("logs/events.log");
        assert!(events.contains("ring event"), "{events}");
        assert!(events.contains("<redacted>"), "{events}");

        let manifest = find("manifest.json");
        assert!(manifest.contains("\"rigel\""), "{manifest}");
        assert!(manifest.contains("schema.index"), "{manifest}");

        let omitted = find("omitted.txt");
        assert!(omitted.contains("index.db"), "{omitted}");
        assert!(omitted.contains("rigel.db"), "{omitted}");
    }
}
