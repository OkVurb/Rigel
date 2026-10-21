//! A model that reads its answers from a JSON script.
//!
//! This exists so the native loop, the budget kill switch, the patch pipeline and resume can
//! be tested end to end without a network key — and so the Phase 3 exit test is
//! reproducible. It is not a mock of the coordinator: it speaks the same [`Model`] trait,
//! emits the same deltas, and its usage is charged by the same `BudgetService`.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::model::{ChatRequest, Delta, DeltaSink, Model, ModelError, RawUsage};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Script {
    #[serde(default)]
    pub steps: Vec<ScriptStep>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ScriptStep {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ScriptToolCall>,
    #[serde(default)]
    pub usage: Option<ScriptUsage>,
    /// Simulate the provider failing instead of answering.
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ScriptUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    /// Reported as if it came from the provider, so the budget sees a provider cost.
    #[serde(default)]
    pub cost_usd: Option<f64>,
}

pub struct ScriptedModel {
    steps: Mutex<VecDeque<ScriptStep>>,
    id: String,
}

impl ScriptedModel {
    pub fn new(script: Script) -> Self {
        Self {
            steps: Mutex::new(script.steps.into()),
            id: "script".to_string(),
        }
    }

    pub fn from_json(json: &str) -> Result<Self, ModelError> {
        let script: Script =
            serde_json::from_str(json).map_err(|error| ModelError::Parse(error.to_string()))?;
        Ok(Self::new(script))
    }

    pub fn from_path(path: &std::path::Path) -> Result<Self, ModelError> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| ModelError::Other(format!("{}: {error}", path.display())))?;
        Self::from_json(&text)
    }

    pub fn remaining(&self) -> usize {
        self.steps.lock().map(|steps| steps.len()).unwrap_or(0)
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }
}

#[async_trait]
impl Model for ScriptedModel {
    fn id(&self) -> String {
        self.id.clone()
    }

    async fn chat(
        &self,
        _request: ChatRequest,
        cancel: &CancellationToken,
        on_delta: &mut DeltaSink<'_>,
    ) -> Result<(), ModelError> {
        if cancel.is_cancelled() {
            return Err(ModelError::Cancelled);
        }
        let step = self
            .steps
            .lock()
            .map_err(|_| ModelError::Other("script lock poisoned".to_string()))?
            .pop_front()
            .ok_or(ModelError::ScriptExhausted)?;

        if let Some(error) = step.error {
            return Err(ModelError::Other(error));
        }

        if let Some(reasoning) = step.reasoning {
            // Stream it in two pieces so the delta path is exercised, not bypassed.
            for piece in split_in_two(&reasoning) {
                if !piece.is_empty() {
                    on_delta(Delta::Reasoning(piece.to_string()));
                }
            }
        }
        if let Some(text) = step.text {
            for piece in split_in_two(&text) {
                if !piece.is_empty() {
                    on_delta(Delta::Text(piece.to_string()));
                }
            }
        }

        for (index, call) in step.tool_calls.iter().enumerate() {
            let arguments =
                serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".to_string());
            on_delta(Delta::ToolCall {
                index: index as u32,
                id: Some(format!("call_{index}")),
                name: Some(call.name.clone()),
                arguments,
            });
        }

        if let Some(usage) = step.usage {
            on_delta(Delta::Usage(RawUsage {
                input_tokens: usage.input_tokens,
                cached_input_tokens: usage.cached_input_tokens,
                output_tokens: usage.output_tokens,
                reasoning_tokens: usage.reasoning_tokens,
                cost_usd: usage.cost_usd,
            }));
        }

        on_delta(Delta::Finish(if step.tool_calls.is_empty() {
            "stop".to_string()
        } else {
            "tool_calls".to_string()
        }));
        Ok(())
    }
}

/// Split on a character boundary, never a byte one.
fn split_in_two(text: &str) -> [&str; 2] {
    let midpoint = text
        .char_indices()
        .nth(text.chars().count() / 2)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    let (left, right) = text.split_at(midpoint);
    [left, right]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_script_emits_its_steps_in_order_and_then_stops() {
        let model = ScriptedModel::from_json(
            r#"{"steps":[{"text":"one","usage":{"cost_usd":0.00001}},{"text":"two"}]}"#,
        )
        .unwrap();
        let cancel = CancellationToken::new();
        let request = ChatRequest {
            model: "script".into(),
            messages: vec![],
            tools: vec![],
            max_tokens: None,
            temperature: 0.0,
        };

        for expected in ["one", "two"] {
            let mut text = String::new();
            let mut sink = |delta: Delta| {
                if let Delta::Text(piece) = delta {
                    text.push_str(&piece);
                }
            };
            model
                .chat(request.clone(), &cancel, &mut sink)
                .await
                .unwrap();
            assert_eq!(text, expected);
        }

        let mut sink = |_: Delta| {};
        let error = model.chat(request, &cancel, &mut sink).await.unwrap_err();
        assert!(matches!(error, ModelError::ScriptExhausted));
    }
}
