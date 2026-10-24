//! `rigel update` — the switch, the manifest and the signature check (P7.7).
//!
//! Nothing here runs on a timer. `status` prints the switch, `check` verifies the latest GitHub
//! release manifest (or a file/URL the user points at), and `verify` compares a downloaded
//! artifact with the hash the signed manifest names. A check is allowed by default; `disable`
//! stops it, and no command runs on a timer.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Subcommand;
use rigel_core::update::{
    self, PUBKEY_FILE, UpdateManifest, fetch_manifest, load_settings, manifest_url, parse_manifest,
    save_settings, settings_path, verify_artifact, verify_manifest,
};
use serde_json::json;

#[derive(Subcommand)]
pub enum UpdateCommand {
    /// The switch, the key and this build's version.
    Status {
        #[arg(long)]
        json: bool,
    },

    /// Turn checks on, optionally remembering where the manifest lives.
    Enable {
        #[arg(long)]
        manifest: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// Turn checks off, so no check runs at launch.
    Disable {
        #[arg(long)]
        json: bool,
    },

    /// Verify a manifest and say whether it is newer than this build.
    Check {
        /// A local manifest file.
        #[arg(long, value_name = "FILE")]
        manifest: Option<PathBuf>,
        /// A manifest over https. The only network call any update command makes.
        #[arg(long, value_name = "URL")]
        url: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// Compare a downloaded artifact with a signed manifest's hash.
    Verify {
        artifact: PathBuf,
        #[arg(long, value_name = "FILE")]
        manifest: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(dir: &Path, command: UpdateCommand) -> ExitCode {
    match command {
        UpdateCommand::Status { json: as_json } => {
            let settings = load_settings(dir);
            let key = update::public_key(dir);
            let key_state = match &key {
                Ok(key) => format!("{}…{}", &hex(key)[..8], &hex(key)[56..]),
                Err(_) => "missing".to_string(),
            };
            if as_json {
                println!(
                    "{}",
                    json!({
                        "ok": true,
                        "version": env!("CARGO_PKG_VERSION"),
                        "enabled": settings.enabled,
                        "auto_install": settings.auto_install,
                        "manifest_url": manifest_url(&settings),
                        "settings": settings_path(dir),
                        "key": key_state,
                        "key_file": dir.join(PUBKEY_FILE),
                    })
                );
            } else {
                println!("version      {}", env!("CARGO_PKG_VERSION"));
                println!(
                    "updates      {}",
                    if settings.enabled { "on" } else { "off" }
                );
                println!(
                    "auto-install {}",
                    if settings.auto_install { "on" } else { "off" }
                );
                println!("manifest     {}", manifest_url(&settings));
                println!("public key   {key_state}");
                println!("settings     {}", settings_path(dir).display());
                println!(
                    "             {}",
                    if settings.enabled {
                        "the app checks at launch; the CLI only when `update check` asks"
                    } else {
                        "no request is made while updates are off"
                    }
                );
            }
            ExitCode::SUCCESS
        }

        UpdateCommand::Enable {
            manifest,
            json: as_json,
        } => {
            let mut settings = load_settings(dir);
            settings.enabled = true;
            if manifest.is_some() {
                settings.manifest_url = manifest;
            }
            match save_settings(dir, &settings) {
                Ok(()) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({ "ok": true, "enabled": true, "manifest_url": settings.manifest_url })
                        );
                    } else {
                        println!("updates on");
                        if let Some(url) = &settings.manifest_url {
                            println!("  manifest {url}");
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        UpdateCommand::Disable { json: as_json } => {
            let mut settings = load_settings(dir);
            settings.enabled = false;
            match save_settings(dir, &settings) {
                Ok(()) => {
                    if as_json {
                        println!("{}", json!({ "ok": true, "enabled": false }));
                    } else {
                        println!("updates off");
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        UpdateCommand::Check {
            manifest,
            url,
            json: as_json,
        } => {
            let parsed: Result<UpdateManifest, update::UpdateError> = match (manifest, url) {
                (Some(path), _) => std::fs::read_to_string(&path)
                    .map_err(update::UpdateError::from)
                    .and_then(|text| parse_manifest(&text)),
                (None, Some(url)) => fetch_manifest(&url).await,
                (None, None) => {
                    let settings = load_settings(dir);
                    if !settings.enabled {
                        return failure(update::UpdateError::Disabled, as_json);
                    }
                    fetch_manifest(manifest_url(&settings)).await
                }
            };

            let manifest = match parsed {
                Ok(manifest) => manifest,
                Err(error) => return failure(error, as_json),
            };

            match update::check(dir, &manifest, env!("CARGO_PKG_VERSION")) {
                Ok(result) => {
                    if as_json {
                        println!("{}", json!({ "ok": true, "update": result }));
                    } else {
                        println!(
                            "signed by the configured key · {} → {}",
                            result.current, result.offered
                        );
                        if result.available {
                            println!("  available  {}", result.url);
                            println!("  sha256     {}", result.sha256);
                            if let Some(notes) = &result.notes {
                                println!("  notes      {notes}");
                            }
                        } else {
                            println!("  this build is current");
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        UpdateCommand::Verify {
            artifact,
            manifest,
            json: as_json,
        } => {
            let parsed = match std::fs::read_to_string(&manifest)
                .map_err(update::UpdateError::from)
                .and_then(|text| parse_manifest(&text))
            {
                Ok(parsed) => parsed,
                Err(error) => return failure(error, as_json),
            };
            let key = match update::public_key(dir) {
                Ok(key) => key,
                Err(error) => return failure(error, as_json),
            };
            if let Err(error) = verify_manifest(&parsed, &key) {
                return failure(error, as_json);
            }
            match verify_artifact(&artifact, &parsed) {
                Ok(()) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": true,
                                "artifact": artifact,
                                "sha256": parsed.sha256,
                                "version": parsed.version,
                                "signed": true,
                            })
                        );
                    } else {
                        println!("{} matches the signed manifest", artifact.display());
                        println!("  version {} · sha256 {}", parsed.version, parsed.sha256);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }
    }
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn failure(error: update::UpdateError, as_json: bool) -> ExitCode {
    if as_json {
        println!("{}", json!({ "ok": false, "error": error.to_string() }));
    }
    eprintln!("rigel: {error}");
    ExitCode::from(1)
}
