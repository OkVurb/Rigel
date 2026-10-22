//! `rigel doctor` — a report, not a repair (P7.1).
//!
//! Every check answers a question the user could otherwise only ask by opening a database:
//! is the file sound, is the schema the one this build expects, are the blob rows and the blob
//! files the same set, are any claims stale, does the ledger agree with the turn sums, does
//! the keychain work, is the cached config stale, which harnesses answer, and is every hash
//! pin still the hash of what is on disk.
//!
//! Three rules:
//!
//! * **Never mutate.** A doctor that migrates, prunes or expires hides the problem it was
//!   asked to find. It opens an existing project database and leaves it alone.
//! * **Every row carries its evidence.** A count with no path is not actionable.
//! * **The exit code is the summary.** `0` all clear, `1` warnings, `2` failures — so a
//!   script can gate on it (P4.12).

use std::path::Path;
use std::process::ExitCode;

use rigel_adapters::AuthMode;
use rigel_core::{
    BudgetService, CLAIM_TTL, ClaimService, INDEX_MIGRATIONS, PROJECT_MIGRATIONS, Pin, STALE_AFTER,
    Store, ToolService, credentials, now_ms, skills::BlobStore,
};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    fn as_str(&self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone)]
struct Check {
    id: String,
    status: Status,
    detail: String,
    hint: Option<String>,
}

impl Check {
    fn ok(id: impl Into<String>, detail: impl Into<String>) -> Self {
        Check {
            id: id.into(),
            status: Status::Ok,
            detail: detail.into(),
            hint: None,
        }
    }

    fn warn(id: impl Into<String>, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Check {
            id: id.into(),
            status: Status::Warn,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }

    fn fail(id: impl Into<String>, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Check {
            id: id.into(),
            status: Status::Fail,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }
}

pub async fn run(project: &Path, data_dir: &Path, as_json: bool) -> ExitCode {
    let mut checks = Vec::new();

    /* ---------------- the index database ---------------- */
    let index_store = match Store::open_index(data_dir).await {
        Ok(store) => {
            checks.push(Check::ok("db.index", format!("{}", store.path().display())));
            Some(store)
        }
        Err(error) => {
            checks.push(Check::fail(
                "db.index",
                error.to_string(),
                "move index.db aside and re-index; nothing else can be checked without it",
            ));
            None
        }
    };

    if let Some(store) = &index_store {
        checks.push(integrity("db.index.integrity", store).await);
        checks.push(
            schema(
                "schema.index",
                store,
                INDEX_MIGRATIONS.iter().map(|m| m.version).max(),
            )
            .await,
        );
    }

    /* ---------------- blobs on disk vs rows ---------------- */
    if let Some(store) = &index_store {
        checks.push(blobs(data_dir, store).await);
    }

    /* ---------------- the project database ---------------- */
    let db_path = project.join(".rigel").join("rigel.db");
    let project_store = if db_path.exists() {
        match Store::open_project(project).await {
            Ok(store) => {
                checks.push(Check::ok("db.project", db_path.display().to_string()));
                Some(store)
            }
            Err(error) => {
                checks.push(Check::fail(
                    "db.project",
                    error.to_string(),
                    "run `rigel migrate --project .` from a shell with the project closed",
                ));
                None
            }
        }
    } else {
        checks.push(Check::ok(
            "db.project",
            format!("not created yet ({})", db_path.display()),
        ));
        None
    };

    if let Some(store) = &project_store {
        checks.push(integrity("db.project.integrity", store).await);
        checks.push(
            schema(
                "schema.project",
                store,
                PROJECT_MIGRATIONS.iter().map(|m| m.version).max(),
            )
            .await,
        );
        checks.push(claims(store, project).await);
        checks.push(budget(store).await);
    }

    /* ---------------- keychain ---------------- */
    checks.push(match credentials::available() {
        Ok(()) => Check::ok("keychain", "the OS credential store answers"),
        Err(error) => Check::warn(
            "keychain",
            format!("the OS credential store did not answer: {error}"),
            "keys from the environment still work; `rigel keys set <provider>` will report the same error",
        ),
    });

    /* ---------------- cached config ---------------- */
    checks.push(config(data_dir));
    checks.push(updates(data_dir));

    /* ---------------- harnesses and tools ---------------- */
    let harnesses =
        rigel_adapters::registry::probe_all(&credentials::KeychainCredentialStore).await;
    for info in &harnesses {
        let name = rigel_adapters::registry::display_name(info.harness);
        checks.push(harness_check(name, info));
    }

    let host = rigel_core::Host::current();
    if let Ok(tools) = ToolService::open(data_dir).await {
        checks.extend(tool_checks(&tools, &host).await);
    }

    if let Some(store) = &index_store {
        checks.push(installs(store).await);
    }

    /* ---------------- print ---------------- */
    let failures = checks.iter().filter(|c| c.status == Status::Fail).count();
    let warnings = checks.iter().filter(|c| c.status == Status::Warn).count();
    let exit = if failures > 0 {
        2
    } else if warnings > 0 {
        1
    } else {
        0
    };

    if as_json {
        println!(
            "{}",
            json!({
                "ok": failures == 0,
                "version": env!("CARGO_PKG_VERSION"),
                "server": "rigel",
                "project": project.display().to_string(),
                "data_dir": data_dir.display().to_string(),
                "exit_code": exit,
                "failures": failures,
                "warnings": warnings,
                "checks": checks.iter().map(|check| json!({
                    "id": check.id,
                    "status": check.status.as_str(),
                    "detail": check.detail,
                    "hint": check.hint,
                })).collect::<Vec<_>>(),
                "harnesses": harnesses,
            })
        );
    } else {
        println!("rigel {}", env!("CARGO_PKG_VERSION"));
        for check in &checks {
            println!(
                "  {:<6} {:<22} {}",
                check.status.as_str(),
                check.id,
                check.detail
            );
            if let Some(hint) = &check.hint {
                println!("         {:<22} → {hint}", "");
            }
        }
        println!(
            "\n  {} checks · {warnings} warnings · {failures} failures",
            checks.len()
        );
        if failures > 0 {
            println!("  exit 2");
        } else if warnings > 0 {
            println!("  exit 1");
        } else {
            println!("  exit 0");
        }
    }

    ExitCode::from(exit)
}

async fn integrity(id: &str, store: &Store) -> Check {
    match sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
        .fetch_all(store.pool())
        .await
    {
        Ok(rows) if rows.iter().all(|row| row == "ok") => {
            Check::ok(id, format!("{} rows, all ok", rows.len()))
        }
        Ok(rows) => Check::fail(
            id,
            format!("integrity_check said: {}", rows.join("; ")),
            "restore the newest backup or start the project again with a fresh .rigel directory",
        ),
        Err(error) => Check::fail(id, error.to_string(), "the database could not be read"),
    }
}

async fn schema(id: &str, store: &Store, expected: Option<i64>) -> Check {
    let applied: Option<i64> =
        match sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
            .fetch_one(store.pool())
            .await
        {
            Ok(version) => version,
            Err(error) => {
                return Check::fail(id, error.to_string(), "the migration table is unreadable");
            }
        };
    let expected = expected.unwrap_or(0);
    match applied {
        Some(version) if version == expected => Check::ok(id, format!("version {version}")),
        Some(version) => Check::warn(
            id,
            format!("version {version}, this build knows {expected}"),
            "a newer build wrote this database; update Rigel before opening it again",
        ),
        None => Check::fail(
            id,
            "no migrations recorded",
            "run `rigel migrate --project .` or delete the file if nothing is in it",
        ),
    }
}

async fn blobs(data_dir: &Path, store: &Store) -> Check {
    let store_blobs = BlobStore::new(data_dir);
    // A blob is live when a skill body or an HTTP cache row points at it. The `blobs` table
    // is a manifest of sizes since 0002, not the list of what is used.
    let rows: Vec<String> = match sqlx::query_scalar(
        "SELECT body_sha FROM skills WHERE body_sha IS NOT NULL
         UNION
         SELECT sha256 FROM http_cache WHERE sha256 IS NOT NULL",
    )
    .fetch_all(store.pool())
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            return Check::fail(
                "blobs",
                error.to_string(),
                "the blob references are unreadable",
            );
        }
    };
    let on_disk: Vec<String> = match std::fs::read_dir(store_blobs.root()) {
        Ok(entries) => entries
            .flatten()
            .filter(|entry| entry.path().is_file())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| !name.starts_with('.'))
            .collect(),
        Err(_) => Vec::new(),
    };

    let missing: Vec<&String> = rows.iter().filter(|sha| !on_disk.contains(sha)).collect();
    let orphans: Vec<&String> = on_disk.iter().filter(|sha| !rows.contains(sha)).collect();

    if !missing.is_empty() {
        return Check::fail(
            "blobs",
            format!(
                "{} rows point at a file that is gone (first: {})",
                missing.len(),
                &missing[0][..missing[0].len().min(12)]
            ),
            "re-index the source that provided those bodies; `rigel skills gc` will not help",
        );
    }
    if !orphans.is_empty() {
        return Check::warn(
            "blobs",
            format!("{} files are not referenced by a row", orphans.len()),
            "`rigel skills gc` deletes them",
        );
    }
    Check::ok("blobs", format!("{} files, all referenced", on_disk.len()))
}

async fn claims(store: &Store, project: &Path) -> Check {
    let service = ClaimService::new(store.pool().clone(), project);
    let held = match service.held(None).await {
        Ok(held) => held,
        Err(error) => {
            return Check::fail(
                "claims",
                error.to_string(),
                "the claim ledger is unreadable",
            );
        }
    };
    let stale = held.iter().filter(|claim| claim.stale).count();
    let now = now_ms();
    let expired = held
        .iter()
        .filter(|claim| now.saturating_sub(claim.heartbeat_at) > CLAIM_TTL.as_millis() as i64)
        .count();
    let stale_after = STALE_AFTER.as_secs();

    if expired > 0 {
        return Check::warn(
            "claims",
            format!("{expired} claims are past the 24 h expiry"),
            "`rigel claim list` shows them; expiry only happens when a session runs",
        );
    }
    if stale > 0 {
        return Check::warn(
            "claims",
            format!("{stale} claims have not heartbeat in {stale_after}s"),
            "nothing is released automatically; `rigel claim release <path>` if the agent is gone",
        );
    }
    Check::ok("claims", format!("{} active, none stale", held.len()))
}

async fn budget(store: &Store) -> Check {
    let sessions: Vec<String> = match sqlx::query_scalar(
        "SELECT id FROM sessions WHERE status IN ('active', 'idle') ORDER BY created_at DESC LIMIT 20",
    )
    .fetch_all(store.pool())
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            return Check::fail("budget", error.to_string(), "the session table is unreadable");
        }
    };
    if sessions.is_empty() {
        return Check::ok("budget", "no open sessions to reconcile");
    }

    let service = BudgetService::with_null_interrupt(store.pool().clone());
    let mut drift = Vec::new();
    for session in &sessions {
        match service.reconcile(session).await {
            Ok(report) if report.ok => {}
            Ok(report) => drift.push(format!(
                "{}: ledger {} µ$, turns {} µ$",
                &report.session_id[..report.session_id.len().min(8)],
                report.session_recorded_micros,
                report.session_summed_micros
            )),
            Err(error) => drift.push(format!("{}: {error}", &session[..session.len().min(8)])),
        }
    }

    if drift.is_empty() {
        Check::ok(
            "budget",
            format!(
                "{} open sessions reconcile with their turns",
                sessions.len()
            ),
        )
    } else {
        Check::warn(
            "budget",
            format!(
                "{} sessions do not reconcile ({})",
                drift.len(),
                drift.join(", ")
            ),
            "the cap uses the recorded number, so an under-count means the cap is loose",
        )
    }
}

/// An approved install that never recorded an exit is the shape a kill leaves behind.
async fn installs(store: &Store) -> Check {
    let unfinished: i64 = match sqlx::query_scalar(
        "SELECT COUNT(*) FROM install_ledger WHERE decision = 'user-approved' AND executed_at IS NULL",
    )
    .fetch_one(store.pool())
    .await
    {
        Ok(count) => count,
        Err(error) => {
            return Check::fail("installs", error.to_string(), "the install ledger is unreadable");
        }
    };
    if unfinished > 0 {
        return Check::warn(
            "installs",
            format!("{unfinished} approved installs never finished"),
            "the process died between the ledger row and the command; `rigel tools ledger` shows which",
        );
    }
    let queued: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM install_ledger WHERE decision = 'queued'")
            .fetch_one(store.pool())
            .await
            .unwrap_or(0);
    if queued > 0 {
        return Check::warn(
            "installs",
            format!("{queued} installs are queued"),
            "`rigel tools queued` lists them; approve one with `rigel tools install <tool> --yes`",
        );
    }
    Check::ok("installs", "no unfinished or queued installs")
}

/// The update switch is a row of its own: N5 says the fetch can be disabled, and a user
/// reading `doctor` should not have to open a JSON file to find out which way it is set.
fn updates(data_dir: &Path) -> Check {
    let settings = rigel_core::update::load_settings(data_dir);
    if !settings.enabled {
        return Check::ok("updates", "off — no request is made");
    }
    match rigel_core::update::public_key(data_dir) {
        Ok(_) => Check::ok(
            "updates",
            format!(
                "on · {}",
                settings
                    .manifest_url
                    .as_deref()
                    .unwrap_or("no manifest URL")
            ),
        ),
        Err(_) => Check::warn(
            "updates",
            "on, but no public key is configured",
            "put the publisher's 64 hex characters in update-pubkey.txt; without it a check cannot verify anything",
        ),
    }
}

fn config(data_dir: &Path) -> Check {
    let adapters = data_dir.join(rigel_adapters::registry::CACHE_FILE);
    if adapters.exists() {
        match std::fs::read_to_string(&adapters) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(value) => {
                    let probed = value.get("probed_at").and_then(|v| v.as_i64());
                    if let Some(probed) = probed {
                        // The cache stores seconds; the rest of Rigel stores milliseconds.
                        let probed = if probed < 100_000_000_000 {
                            probed * 1_000
                        } else {
                            probed
                        };
                        let age = now_ms() - probed;
                        if age > 24 * 60 * 60 * 1_000 {
                            return Check::warn(
                                "config",
                                format!(
                                    "{} was probed {} h ago",
                                    adapters.display(),
                                    age / 3_600_000
                                ),
                                "`rigel harness probe --refresh` updates it",
                            );
                        }
                    }
                    Check::ok("config", format!("{} parses", adapters.display()))
                }
                Err(error) => Check::fail(
                    "config",
                    format!("{} does not parse: {error}", adapters.display()),
                    "delete the file; the next probe writes it again",
                ),
            },
            Err(error) => Check::warn(
                "config",
                format!("{} could not be read: {error}", adapters.display()),
                "the next probe rewrites it",
            ),
        }
    } else {
        Check::ok("config", "no cached harness probe yet")
    }
}

fn harness_check(name: &str, info: &rigel_adapters::HarnessInfo) -> Check {
    let id = format!("harness:{name}");
    if !info.installed {
        return Check::warn(
            id,
            info.install_hint
                .clone()
                .unwrap_or_else(|| "not installed".to_string()),
            "a missing harness is not a failure: Rigel and the other harnesses still work",
        );
    }
    let version = info
        .version
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    match info.auth_mode {
        AuthMode::ApiKey => Check::ok(
            id,
            format!(
                "{version}{}",
                info.provider
                    .as_ref()
                    .map(|provider| format!(" · key from {provider}"))
                    .unwrap_or_default()
            ),
        ),
        AuthMode::Subscription => Check::warn(
            id,
            format!("{version} · no API key detected"),
            info.provider
                .as_ref()
                .map(|provider| format!("`rigel keys set {provider}`"))
                .unwrap_or_else(|| "set a provider key".to_string()),
        ),
        AuthMode::None => Check::warn(
            id,
            format!("{version} · no credentials"),
            "sign the harness in, or point it at a key",
        ),
    }
}

async fn tool_checks(tools: &ToolService, host: &rigel_core::Host) -> Vec<Check> {
    let rows = match tools.cached(host).await {
        Ok(rows) => rows,
        Err(error) => {
            return vec![Check::warn(
                "tools",
                error.to_string(),
                "`rigel tools list` re-probes everything",
            )];
        }
    };
    if rows.is_empty() {
        return vec![Check::warn(
            "tools",
            "never probed on this host",
            "`rigel tools list` writes the cache",
        )];
    }
    let present = rows.iter().filter(|row| row.present).count();
    let missing = rows.len() - present;
    let mut checks = Vec::new();

    for row in rows.iter().filter(|row| row.present) {
        if let Ok(Pin::Changed { from, to, path }) = tools.verify(&row.id, host).await {
            checks.push(Check::warn(
                format!("tools:{}", row.id),
                format!(
                    "{} changed since Rigel recorded it ({} → {})",
                    path.display(),
                    &from[..from.len().min(10)],
                    &to[..to.len().min(10)]
                ),
                "reinstall it from a source you trust, or accept the new bytes with `rigel tools restart`",
            ));
        }
    }

    if missing > 0 {
        checks.push(Check::warn(
            "tools",
            format!("{present} present, {missing} missing of {}", rows.len()),
            "`rigel tools requirements <skill>` says which ones a skill actually needs",
        ));
    } else {
        checks.push(Check::ok("tools", format!("all {} present", present)));
    }
    checks
}
