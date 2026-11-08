//! `rigel route explain`: the decision the window's inspector shows, without the window.

use std::process::ExitCode;

use clap::Subcommand;
use rigel_core::agent::routing;

#[derive(Subcommand)]
pub enum RouteCommand {
    /// Show the provider and model a turn would use, and why.
    Explain {
        /// The provider the picker would request. Omit it to send only a model, as the picker
        /// does when the saved door can serve a bare id.
        #[arg(long, value_name = "ID")]
        provider: Option<String>,
        /// The model the picker would request.
        #[arg(long, value_name = "ID")]
        model: Option<String>,
        /// The provider saved in Settings, when it differs from `--provider`.
        #[arg(long, value_name = "ID")]
        saved_provider: Option<String>,
        /// The model saved in Settings, when it differs from `--model`.
        #[arg(long, value_name = "ID")]
        saved_model: Option<String>,
        /// Print the explanation as JSON instead of labelled lines.
        #[arg(long)]
        json: bool,
    },
}

pub fn run(command: RouteCommand) -> ExitCode {
    let RouteCommand::Explain {
        provider,
        model,
        saved_provider,
        saved_model,
        json,
    } = command;

    let selected_provider = saved_provider
        .clone()
        .or_else(|| provider.clone())
        .unwrap_or_default();
    let selected_model = saved_model
        .clone()
        .or_else(|| model.clone())
        .unwrap_or_default();
    let explanation = match routing::explain(
        &selected_provider,
        &selected_model,
        provider.as_deref(),
        model.as_deref(),
    ) {
        Ok(explanation) => explanation,
        Err(error) => {
            eprintln!("rigel: {error}");
            return ExitCode::from(2);
        }
    };

    if json {
        match serde_json::to_string_pretty(&explanation) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("rigel: {error}");
                ExitCode::from(2)
            }
        }
    } else {
        println!(
            "saved:     {} / {}",
            explanation.saved.provider, explanation.saved.model
        );
        if let Some(requested) = &explanation.requested {
            println!("requested: {} / {}", requested.provider, requested.model);
        }
        println!(
            "resolved:  {} / {}",
            explanation.resolved.provider, explanation.resolved.model
        );
        println!("reason:    {}", explanation.reason.as_str());
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_json_payload_has_the_four_fields_the_inspector_uses() {
        let explanation = routing::explain(
            "vertex",
            "google/gemini-4-argon",
            Some("groq"),
            Some("openai/gpt-oss-120b"),
        )
        .unwrap();
        let value = serde_json::to_value(&explanation).unwrap();
        for field in ["saved", "requested", "resolved", "reason"] {
            assert!(value.get(field).is_some(), "missing `{field}`");
        }
        assert_eq!(value["reason"], "picker");
        assert_eq!(value["resolved"]["provider"], "groq");
        assert_eq!(value["saved"]["provider"], "vertex");
        assert_eq!(value["requested"]["model"], "openai/gpt-oss-120b");
    }

    #[test]
    fn a_bare_id_repair_says_so_in_the_payload() {
        let explanation = routing::explain(
            "vertex",
            "google/gemini-4-argon",
            None,
            Some("deepseek-chat"),
        )
        .unwrap();
        let value = serde_json::to_value(&explanation).unwrap();
        assert_eq!(value["reason"], "bare-id-repair");
        assert_eq!(value["resolved"]["provider"], "deepseek");
    }
}
