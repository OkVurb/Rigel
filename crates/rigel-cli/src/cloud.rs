//! `rigel cloud` — the Firebase project the user owns, from the terminal.
//!
//! Every command goes through `rigel_core::cloud`, the same client the window uses. The password
//! is read from stdin so it never appears in a process list, and no command prints a token: the
//! refresh token goes straight to the OS keychain. A push sends shelf metadata; a publish sends
//! one session's prompts and assistant text, and never its tool calls or project path.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Subcommand;
use rigel_core::SkillIndex;
use rigel_core::cloud::{AppCheckConfig, Cloud, CloudConfig, CloudError};
use serde_json::json;

#[derive(Subcommand)]
pub enum CloudCommand {
    /// The project, the account, and where each is stored.
    Status {
        #[arg(long)]
        json: bool,
    },

    /// Save the Firebase project id and web API key beside the index.
    Configure {
        #[arg(long, value_name = "ID")]
        project: String,
        /// The project's web API key. Read from stdin when omitted.
        #[arg(long, value_name = "KEY")]
        api_key: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// Sign in with email and password. The password is read from stdin.
    Login {
        #[arg(long, value_name = "EMAIL")]
        email: String,
        #[arg(long)]
        json: bool,
    },

    /// Create an account with email and password. The password is read from stdin.
    SignUp {
        #[arg(long, value_name = "EMAIL")]
        email: String,
        #[arg(long)]
        json: bool,
    },

    /// Email a password-reset link. The answer is the same for every address.
    Reset {
        #[arg(long, value_name = "EMAIL")]
        email: String,
        #[arg(long)]
        json: bool,
    },

    /// Email a verification link to the signed-in account. Send again to resend.
    Verify {
        /// Re-read the state from Firebase instead of sending another link.
        #[arg(long)]
        check: bool,
        #[arg(long)]
        json: bool,
    },

    /// Forget the local refresh token. Firebase keeps the account.
    Logout {
        #[arg(long)]
        json: bool,
    },

    /// Point App Check at an attestation endpoint, or turn it off.
    ///
    /// The endpoint is a service you run; it answers with a token the Firebase Admin SDK minted
    /// and the Unix second at which that token expires. Rigel attaches the token to every
    /// Firebase request. An endpoint that mints tokens for anyone is not attestation.
    AppCheck {
        /// The attestation URL. Omit to print the current setting.
        #[arg(long, value_name = "URL")]
        url: Option<String>,
        /// Stop sending App Check tokens.
        #[arg(long, conflicts_with = "url")]
        clear: bool,
        #[arg(long)]
        json: bool,
    },

    /// Mirror the local shelf's metadata into the user's Firestore project.
    Push {
        #[arg(long)]
        json: bool,
    },

    /// Bring down shelf rows this machine does not have yet.
    Pull {
        #[arg(long)]
        json: bool,
    },

    /// Push, then pull.
    Sync {
        #[arg(long)]
        json: bool,
    },

    /// Publish one session's turns so the phone can read them.
    Publish {
        /// The session id (see `rigel session list`).
        id: String,
        /// The project whose session database holds it.
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(dir: &Path, command: CloudCommand) -> ExitCode {
    match command {
        CloudCommand::Status { json: as_json } => match Cloud::status_at(dir) {
            Ok(status) => {
                if as_json {
                    println!(
                        "{}",
                        json!({
                            "ok": true,
                            "configured": status.configured,
                            "project_id": status.project_id,
                            "signed_in": status.signed_in,
                            "email": status.email,
                            "uid": status.uid,
                            "expires_at": status.expires_at,
                            "email_verified": status.email_verified,
                            "app_check": status.app_check,
                            "last_push_at": status.last_push_at,
                            "last_pull_at": status.last_pull_at,
                            "last_sync_at": status.last_sync_at,
                            "config": CloudConfig::path(dir),
                        })
                    );
                } else if status.configured {
                    println!("project    {}", status.project_id.as_deref().unwrap_or("—"));
                    match (&status.email, &status.uid) {
                        (Some(email), Some(uid)) => {
                            println!("account    {email} ({uid})");
                            println!("signed in  yes — the token refreshes on demand");
                            println!(
                                "email      {}",
                                if status.email_verified {
                                    "verified".to_string()
                                } else {
                                    "not verified — rigel cloud verify".to_string()
                                }
                            );
                        }
                        _ => {
                            println!("account    not signed in");
                            println!("           rigel cloud login --email <address>");
                        }
                    }
                    let pushed = status
                        .last_push_at
                        .map(rigel_core::skills::fmt_time)
                        .unwrap_or_else(|| "never".to_string());
                    let pulled = status
                        .last_pull_at
                        .map(rigel_core::skills::fmt_time)
                        .unwrap_or_else(|| "never".to_string());
                    println!("last sync  push {pushed} · pull {pulled}");
                    match &status.app_check {
                        Some(check) => println!(
                            "app check  {} · Firebase requests carry a token",
                            check.attest_url
                        ),
                        None => println!("app check  off"),
                    }
                    println!("config     {}", CloudConfig::path(dir).display());
                } else {
                    println!("project    not configured");
                    println!(
                        "           rigel cloud configure --project <id>  (reads the web API key \
                         from stdin)"
                    );
                    println!(
                        "           or set RIGEL_FIREBASE_PROJECT_ID and RIGEL_FIREBASE_API_KEY"
                    );
                    println!("config     {}", CloudConfig::path(dir).display());
                }
                ExitCode::SUCCESS
            }
            Err(error) => failure(error, as_json),
        },

        CloudCommand::Configure {
            project,
            api_key,
            json: as_json,
        } => {
            let key = match api_key {
                Some(key) => key.trim().to_string(),
                None => match read_secret("web API key") {
                    Ok(key) => key,
                    Err(message) => return user_error(&message, as_json),
                },
            };
            match CloudConfig::new(&project, key).and_then(|config| {
                let path = CloudConfig::save(dir, &config)?;
                Ok((config, path))
            }) {
                Ok((config, path)) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": true,
                                "project_id": config.project_id,
                                "config": path,
                            })
                        );
                    } else {
                        println!("project    {}", config.project_id);
                        println!("config     {}", path.display());
                        println!(
                            "the web API key is public by design; firestore.rules decides who \
                             may read a document"
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        CloudCommand::Login {
            email,
            json: as_json,
        } => {
            let password = match read_secret("password") {
                Ok(password) => password,
                Err(message) => return user_error(&message, as_json),
            };
            let cloud = match Cloud::open(dir) {
                Ok(cloud) => cloud,
                Err(error) => return failure(error, as_json),
            };
            match cloud.sign_in(&email, &password).await {
                Ok(status) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": true,
                                "project_id": status.project_id,
                                "email": status.email,
                                "uid": status.uid,
                                "signed_in": status.signed_in,
                            })
                        );
                    } else {
                        println!(
                            "signed in  {} ({})",
                            status.email.as_deref().unwrap_or("?"),
                            status.uid.as_deref().unwrap_or("?")
                        );
                        println!("token      stored in the OS keychain, never printed");
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        CloudCommand::SignUp {
            email,
            json: as_json,
        } => {
            let password = match read_secret("password") {
                Ok(password) => password,
                Err(message) => return user_error(&message, as_json),
            };
            let cloud = match Cloud::open(dir) {
                Ok(cloud) => cloud,
                Err(error) => return failure(error, as_json),
            };
            match cloud.sign_up(&email, &password).await {
                Ok(status) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": true,
                                "project_id": status.project_id,
                                "email": status.email,
                                "uid": status.uid,
                                "signed_in": status.signed_in,
                            })
                        );
                    } else {
                        println!(
                            "created    {} ({})",
                            status.email.as_deref().unwrap_or("?"),
                            status.uid.as_deref().unwrap_or("?")
                        );
                        println!("signed in  yes — the new session is in the OS keychain");
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        CloudCommand::Reset {
            email,
            json: as_json,
        } => {
            let cloud = match Cloud::open(dir) {
                Ok(cloud) => cloud,
                Err(error) => return failure(error, as_json),
            };
            match cloud.send_password_reset(&email).await {
                Ok(address) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": true,
                                "email": address,
                            })
                        );
                    } else {
                        println!(
                            "reset      if an account exists for {address}, Firebase has emailed a \
                             reset link"
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        CloudCommand::Verify {
            check,
            json: as_json,
        } => {
            let cloud = match Cloud::open(dir) {
                Ok(cloud) => cloud,
                Err(error) => return failure(error, as_json),
            };
            if check {
                match cloud.check_email_verification().await {
                    Ok(status) => {
                        if as_json {
                            println!(
                                "{}",
                                json!({
                                    "ok": true,
                                    "email": status.email,
                                    "email_verified": status.email_verified,
                                })
                            );
                        } else if status.email_verified {
                            println!("email      verified");
                        } else {
                            println!(
                                "email      not verified yet — open the link Firebase emailed, \
                                 then run this again"
                            );
                        }
                        ExitCode::SUCCESS
                    }
                    Err(error) => failure(error, as_json),
                }
            } else {
                match cloud.send_verification_email().await {
                    Ok(address) => {
                        if as_json {
                            println!(
                                "{}",
                                json!({
                                    "ok": true,
                                    "sent": true,
                                    "email": address,
                                })
                            );
                        } else {
                            println!("sent       a verification link to {address}");
                            println!("           open it, then: rigel cloud verify --check");
                        }
                        ExitCode::SUCCESS
                    }
                    Err(error) => failure(error, as_json),
                }
            }
        }

        CloudCommand::AppCheck {
            url,
            clear,
            json: as_json,
        } => {
            let mut config = match CloudConfig::load(dir) {
                Ok(Some(config)) => config,
                Ok(None) => return failure(CloudError::NotConfigured, as_json),
                Err(error) => return failure(error, as_json),
            };
            let changed = clear || url.is_some();
            if clear {
                config.app_check = None;
            }
            if let Some(url) = url {
                config.app_check = Some(AppCheckConfig {
                    attest_url: url.trim().to_string(),
                });
            }
            if changed {
                if let Err(error) = CloudConfig::save(dir, &config) {
                    return failure(error, as_json);
                }
            }
            match Cloud::status_at(dir) {
                Ok(status) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": true,
                                "app_check": status.app_check,
                            })
                        );
                    } else if let Some(check) = &status.app_check {
                        println!("app check  {}", check.attest_url);
                        println!("           Firebase requests carry a token from this endpoint");
                    } else {
                        println!("app check  off");
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        CloudCommand::Logout { json: as_json } => {
            let cloud = match Cloud::open(dir) {
                Ok(cloud) => cloud,
                Err(error) => return failure(error, as_json),
            };
            match cloud.sign_out() {
                Ok(status) => {
                    if as_json {
                        println!("{}", json!({ "ok": true, "signed_in": status.signed_in }));
                    } else {
                        println!("signed out; the local refresh token is gone");
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        CloudCommand::Push { json: as_json } => {
            let (cloud, index) = match open(dir).await {
                Ok(pair) => pair,
                Err(error) => return failure(error, as_json),
            };
            match cloud.push(&index).await {
                Ok(report) => {
                    if as_json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&report).unwrap_or_default()
                        );
                    } else {
                        println!(
                            "pushed     {} skill(s) in {} batch(es)",
                            report.pushed, report.batches
                        );
                        println!("project    {} · account {}", report.project_id, report.uid);
                        println!("bodies     stay local; only shelf metadata is sent");
                        if report.deleted > 0 {
                            println!(
                                "tombstones {} row(s) this machine removed were marked deleted \
                                 in the cloud",
                                report.deleted
                            );
                        }
                        if report.truncated {
                            println!(
                                "warning    shelf is larger than the mirror's cap; deletions \
                                 were not propagated"
                            );
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        CloudCommand::Pull { json: as_json } => {
            let (cloud, index) = match open(dir).await {
                Ok(pair) => pair,
                Err(error) => return failure(error, as_json),
            };
            match cloud.pull(&index).await {
                Ok(report) => {
                    print_pull(&report, as_json);
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        CloudCommand::Publish {
            id,
            project,
            json: as_json,
        } => {
            let cloud = match Cloud::open(dir) {
                Ok(cloud) => cloud,
                Err(error) => return failure(error, as_json),
            };
            let service = match rigel_core::SessionService::open(&project).await {
                Ok(service) => service,
                Err(error) => return failure(error.into(), as_json),
            };
            match cloud.publish_session(&service, &id).await {
                Ok(report) => {
                    if as_json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&report).unwrap_or_default()
                        );
                    } else {
                        println!(
                            "published  {} — {} turn(s) in {} batch(es)",
                            report.title, report.turns, report.batches
                        );
                        println!("project    {} · account {}", report.project_id, report.uid);
                        println!(
                            "sent       prompts and assistant text; tool calls, file changes and \
                             the project path stay local"
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        CloudCommand::Sync { json: as_json } => {
            let (cloud, index) = match open(dir).await {
                Ok(pair) => pair,
                Err(error) => return failure(error, as_json),
            };
            let pushed = match cloud.push(&index).await {
                Ok(report) => report,
                Err(error) => return failure(error, as_json),
            };
            let pulled = match cloud.pull(&index).await {
                Ok(report) => report,
                Err(error) => return failure(error, as_json),
            };
            if as_json {
                println!(
                    "{}",
                    json!({
                        "ok": true,
                        "pushed": pushed,
                        "pulled": pulled,
                    })
                );
            } else {
                println!(
                    "pushed     {} skill(s) in {} batch(es)",
                    pushed.pushed, pushed.batches
                );
                println!(
                    "pulled     {} received · {} written · {} skipped · {} tombstone(s), {} \
                     removed locally",
                    pulled.received,
                    pulled.written,
                    pulled.skipped,
                    pulled.tombstones,
                    pulled.deleted
                );
                if pushed.deleted > 0 {
                    println!(
                        "tombstones {} row(s) this machine removed were marked deleted in the \
                         cloud",
                        pushed.deleted
                    );
                }
                println!("project    {} · account {}", pushed.project_id, pushed.uid);
            }
            ExitCode::SUCCESS
        }
    }
}

async fn open(dir: &Path) -> Result<(Cloud, SkillIndex), CloudError> {
    let cloud = Cloud::open(dir)?;
    let index = SkillIndex::open(dir).await?;
    Ok((cloud, index))
}

fn print_pull(report: &rigel_core::cloud::PullReport, as_json: bool) {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).unwrap_or_default()
        );
    } else {
        println!(
            "pulled     {} received · {} written · {} skipped over {} page(s)",
            report.received, report.written, report.skipped, report.pages
        );
        println!(
            "tombstones {} seen · {} removed locally",
            report.tombstones, report.deleted
        );
        println!("project    {} · account {}", report.project_id, report.uid);
        println!("bodies     a pulled row keeps any local body; one is never overwritten");
    }
}

/// Read a secret from stdin, dropping only the newline a pipe or a paste adds. A password can
/// contain a leading or trailing space, so this does not trim like `rigel keys set` does.
fn read_secret(label: &str) -> Result<String, String> {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    let text = text.trim_end_matches(['\r', '\n']).to_string();
    if text.is_empty() {
        return Err(format!("no {label} on stdin"));
    }
    Ok(text)
}

fn user_error(message: &str, as_json: bool) -> ExitCode {
    if as_json {
        println!("{}", json!({ "ok": false, "error": message }));
    } else {
        eprintln!("rigel: {message}");
    }
    ExitCode::from(1)
}

fn failure(error: CloudError, as_json: bool) -> ExitCode {
    let code = error.exit_code();
    if as_json {
        println!(
            "{}",
            json!({
                "ok": false,
                "error": error.to_string(),
                "code": code,
            })
        );
    } else {
        eprintln!("rigel: {error}");
    }
    ExitCode::from(code)
}
