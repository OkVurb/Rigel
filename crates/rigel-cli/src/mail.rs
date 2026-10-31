//! `rigel mail` — the AgentMail inbox the agents in this project can read and send from.
//!
//! Every command goes through `rigel_core::agentmail`, the same client the window and the agent
//! tools use. The API key is never an argument: `rigel keys set agentmail` puts it in the OS
//! keychain, and `AGENTMAIL_API_KEY` is honoured for a shell or CI run. A message body is read
//! from stdin so it never appears in a process list, and no command prints the key.
//!
//! Mail that arrives is written by strangers. `read` prints the body as it is, and says so.

use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use clap::Subcommand;
use rigel_core::agentmail::{Mail, MailConfig, MailError};
use serde_json::json;

#[derive(Subcommand)]
pub enum MailCommand {
    /// The inbox, whether a key is stored, and where each is.
    Status {
        #[arg(long)]
        json: bool,
    },

    /// Save the inbox address beside the index.
    Configure {
        #[arg(long, value_name = "ADDRESS")]
        inbox: String,
        #[arg(long)]
        json: bool,
    },

    /// The newest messages, subjects and previews only.
    List {
        #[arg(long, default_value_t = 10, value_name = "N")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },

    /// One message, with its body.
    Read {
        #[arg(long, value_name = "ID")]
        message_id: String,
        #[arg(long)]
        json: bool,
    },

    /// Send one email. The body is read from stdin.
    Send {
        #[arg(long, value_name = "ADDRESS")]
        to: String,
        #[arg(long)]
        subject: String,
        #[arg(long)]
        json: bool,
    },

    /// Reply on a message's thread. The body is read from stdin.
    Reply {
        #[arg(long, value_name = "ID")]
        message_id: String,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(dir: &Path, command: MailCommand) -> ExitCode {
    match command {
        MailCommand::Status { json: as_json } => {
            let status = Mail::status_at(dir);
            if as_json {
                println!(
                    "{}",
                    json!({
                        "ok": true,
                        "configured": status.configured,
                        "inbox": status.inbox,
                        "key_present": status.key_present,
                        "config": MailConfig::path(dir),
                    })
                );
            } else {
                match status.inbox.as_deref() {
                    Some(inbox) => println!("inbox      {inbox}"),
                    None => println!("inbox      not configured"),
                }
                println!(
                    "api key    {}",
                    if status.key_present {
                        "stored in the OS keychain"
                    } else {
                        "missing — run `rigel keys set agentmail`"
                    }
                );
                println!("base       {}", Mail::base_url());
            }
            ExitCode::SUCCESS
        }

        MailCommand::Configure {
            inbox,
            json: as_json,
        } => match MailConfig::new(&inbox).and_then(|config| {
            let path = MailConfig::save(dir, &config)?;
            Ok((config, path))
        }) {
            Ok((config, path)) => {
                if as_json {
                    println!(
                        "{}",
                        json!({ "ok": true, "inbox": config.inbox, "config": path })
                    );
                } else {
                    println!("inbox      {}", config.inbox);
                    println!("config     {}", path.display());
                    println!("next       rigel keys set agentmail   (reads the key from stdin)");
                }
                ExitCode::SUCCESS
            }
            Err(error) => failure(error, as_json),
        },

        MailCommand::List {
            limit,
            json: as_json,
        } => {
            let mail = match Mail::open(dir) {
                Ok(mail) => mail,
                Err(error) => return failure(error, as_json),
            };
            match mail.list(limit).await {
                Ok(messages) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": true,
                                "inbox": mail.inbox(),
                                "count": messages.len(),
                                "messages": messages,
                            })
                        );
                    } else if messages.is_empty() {
                        println!("{} has no messages", mail.inbox());
                    } else {
                        for message in &messages {
                            println!(
                                "{}{}  {}",
                                if message.unread { "● " } else { "  " },
                                message.timestamp.as_deref().unwrap_or("—"),
                                message.subject
                            );
                            println!("    from {}", message.from);
                            println!("    id   {}", message.message_id);
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        MailCommand::Read {
            message_id,
            json: as_json,
        } => {
            let mail = match Mail::open(dir) {
                Ok(mail) => mail,
                Err(error) => return failure(error, as_json),
            };
            match mail.read(&message_id).await {
                Ok(message) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({ "ok": true, "untrusted": true, "message": message })
                        );
                    } else {
                        println!("from     {}", message.from);
                        println!("to       {}", message.to.join(", "));
                        println!("subject  {}", message.subject);
                        println!("date     {}", message.timestamp.as_deref().unwrap_or("—"));
                        println!("untrusted content written by a stranger, printed as received");
                        println!();
                        println!("{}", message.text);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        MailCommand::Send {
            to,
            subject,
            json: as_json,
        } => {
            let text = match read_body("body") {
                Ok(text) => text,
                Err(message) => return user_error(&message, as_json),
            };
            let mail = match Mail::open(dir) {
                Ok(mail) => mail,
                Err(error) => return failure(error, as_json),
            };
            match mail.send(&to, &subject, &text).await {
                Ok(sent) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": true,
                                "inbox": mail.inbox(),
                                "to": to,
                                "message_id": sent.message_id,
                                "thread_id": sent.thread_id,
                            })
                        );
                    } else {
                        println!("sent       {} → {to}", mail.inbox());
                        println!("subject    {subject}");
                        println!("id         {}", sent.message_id);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        MailCommand::Reply {
            message_id,
            json: as_json,
        } => {
            let text = match read_body("body") {
                Ok(text) => text,
                Err(message) => return user_error(&message, as_json),
            };
            let mail = match Mail::open(dir) {
                Ok(mail) => mail,
                Err(error) => return failure(error, as_json),
            };
            match mail.reply(&message_id, &text).await {
                Ok(sent) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": true,
                                "inbox": mail.inbox(),
                                "reply_to": message_id,
                                "message_id": sent.message_id,
                                "thread_id": sent.thread_id,
                            })
                        );
                    } else {
                        println!("replied    {} on {}", mail.inbox(), message_id);
                        println!("id         {}", sent.message_id);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }
    }
}

/// A body from stdin. Trailing newlines a pipe adds are dropped; everything else is kept,
/// because a mail body is prose and its line breaks are the author's.
fn read_body(label: &str) -> Result<String, String> {
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

fn failure(error: MailError, as_json: bool) -> ExitCode {
    let code = error.exit_code();
    if as_json {
        println!(
            "{}",
            json!({ "ok": false, "error": error.to_string(), "code": code })
        );
    } else {
        eprintln!("rigel: {error}");
    }
    ExitCode::from(code)
}
