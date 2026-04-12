//! AskUser tool — structured multiple-choice questions for user interaction.
//!
//! The tool itself is pure logic: it validates parameters, calls the injected
//! `AskUserFn` callback, and formats the response for the LLM. All terminal
//! rendering lives in the app layer.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde::Serialize;

use crate::types::*;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single option presented to the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserOption {
    pub label: String,
    pub description: String,
}

/// A single question with its own header (tab label) and options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserQuestion {
    pub header: String,
    pub question: String,
    pub options: Vec<AskUserOption>,
}

/// Request sent to the host callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserRequest {
    pub questions: Vec<AskUserQuestion>,
}

/// A single answer for one question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskUserAnswer {
    pub header: String,
    pub question: String,
    pub answer: String,
}

/// Response returned by the host callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskUserResponse {
    /// User answered all questions.
    Answered(Vec<AskUserAnswer>),
    /// User cancelled / skipped.
    Skipped,
}

/// Host-provided callback that presents questions to the user and returns
/// their answers. The engine never touches the terminal — this is the bridge.
pub type AskUserFn = Arc<
    dyn Fn(AskUserRequest) -> BoxFuture<'static, Result<AskUserResponse, String>> + Send + Sync,
>;

// ---------------------------------------------------------------------------
// Tool
// ---------------------------------------------------------------------------

pub struct AskUserTool {
    ask_fn: AskUserFn,
}

impl AskUserTool {
    pub fn new(ask_fn: AskUserFn) -> Self {
        Self { ask_fn }
    }
}

#[async_trait]
impl AgentTool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }

    fn label(&self) -> &str {
        "Ask User"
    }

    fn description(&self) -> &str {
        "Ask the user multiple-choice questions to gather information, clarify ambiguity, \
         understand preferences, or make decisions.\n\
         \n\
         Use this tool when you need user input during planning:\n\
         1. Gather user preferences or requirements\n\
         2. Clarify ambiguous instructions\n\
         3. Get decisions on implementation choices\n\
         4. Offer choices about what direction to take\n\
         \n\
         Before asking, form your own best hypotheses from the code and task context. \
         Your options should reflect those hypotheses so the user can confirm or correct you, \
         rather than doing the thinking from scratch.\n\
         \n\
         Usage notes:\n\
         - You can ask 1-4 questions in a single call; batch related questions together\n\
         - Users can always select \"None of the above\" to provide custom text input\n\
         - If you recommend a specific option, make it the first option and add \"(Recommended)\" \
           at the end of the label\n\
         - Each option should have a concise label and a brief description explaining the tradeoff\n\
         - Provide 2-4 distinct options per question; do not include an \"Other\" option, \
           it is provided automatically\n\
         - Only ask when you genuinely need user input — do not ask what you can discover \
           by reading code\n\
         - Ask sparingly: prefer one well-structured call over multiple separate calls\n\
         \n\
         Plan mode note: Use this tool to clarify requirements or choose between approaches \
         BEFORE finalizing your plan. Do NOT use this tool to ask \"Is my plan ready?\" or \
         \"Should I proceed?\" — present the plan directly and let the user decide. Do not \
         reference \"the plan\" in your questions (e.g., \"Does the plan look good?\") because \
         the user sees your questions mid-stream, before the plan is complete."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 4,
                    "description": "Questions to ask the user (1-4 questions).",
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": {
                                "type": "string",
                                "description": "Clear, specific question ending with '?'"
                            },
                            "header": {
                                "type": "string",
                                "description": "Short tab label for this question. Examples: 'Auth method', 'Library', 'Approach'"
                            },
                            "options": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": {
                                            "type": "string",
                                            "description": "Concise choice (1-5 words). Recommended option ends with '(Recommended)'"
                                        },
                                        "description": {
                                            "type": "string",
                                            "description": "Brief explanation of tradeoffs"
                                        }
                                    },
                                    "required": ["label", "description"]
                                },
                                "minItems": 2,
                                "maxItems": 4,
                                "description": "Distinct options. No 'Other' — provided automatically."
                            }
                        },
                        "required": ["question", "header", "options"]
                    }
                }
            },
            "required": ["questions"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let request: AskUserRequest =
            serde_json::from_value(params).map_err(|e| ToolError::InvalidArgs(e.to_string()))?;

        validate_request(&request)?;

        let response = (self.ask_fn)(request).await.map_err(ToolError::Failed)?;

        let text = match &response {
            AskUserResponse::Answered(answers) => {
                let mut lines = vec!["User answered your questions:".to_string()];
                for a in answers {
                    lines.push(format!("- {} → {}", a.question, a.answer));
                }
                lines.join("\n")
            }
            AskUserResponse::Skipped => "User skipped. Proceed with your best judgment.".into(),
        };

        Ok(ToolResult {
            content: vec![Content::Text { text }],
            details: serde_json::Value::Null,
            retention: Retention::Normal,
        })
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate_request(request: &AskUserRequest) -> Result<(), ToolError> {
    if request.questions.is_empty() || request.questions.len() > 4 {
        return Err(ToolError::InvalidArgs(
            "questions must have 1-4 items".into(),
        ));
    }

    let mut seen_questions = HashSet::new();
    for (qi, q) in request.questions.iter().enumerate() {
        if q.question.trim().is_empty() {
            return Err(ToolError::InvalidArgs(format!(
                "questions[{qi}].question must not be empty"
            )));
        }
        if q.header.trim().is_empty() {
            return Err(ToolError::InvalidArgs(format!(
                "questions[{qi}].header must not be empty"
            )));
        }
        if !seen_questions.insert(&q.question) {
            return Err(ToolError::InvalidArgs(format!(
                "questions[{qi}].question is a duplicate"
            )));
        }
        if q.options.len() < 2 || q.options.len() > 4 {
            return Err(ToolError::InvalidArgs(format!(
                "questions[{qi}].options must have 2-4 items"
            )));
        }
        for (oi, o) in q.options.iter().enumerate() {
            if o.label.trim().is_empty() {
                return Err(ToolError::InvalidArgs(format!(
                    "questions[{qi}].options[{oi}].label must not be empty"
                )));
            }
            if o.description.trim().is_empty() {
                return Err(ToolError::InvalidArgs(format!(
                    "questions[{qi}].options[{oi}].description must not be empty"
                )));
            }
        }
    }
    Ok(())
}
