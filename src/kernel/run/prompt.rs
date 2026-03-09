//! Prompt construction for a chat turn.

use std::fmt::Write;
use std::sync::Arc;

use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::skills::catalog::SkillCatalog;
use crate::llm::tool::ToolSchema;
use crate::storage::dal::learning::LearningRecord;

const LEARNINGS_LIMIT: u32 = 20;
const RECENT_ERRORS_LIMIT: u32 = 5;

// Per-layer max sizes (bytes). Prevents any single layer from bloating the prompt.
// Sized generously — modern models (Claude, GPT) support 128K–200K token contexts.
pub const MAX_IDENTITY_BYTES: usize = 8_192;
pub const MAX_SOUL_BYTES: usize = 16_384;
pub const MAX_SYSTEM_BYTES: usize = 65_536;
pub const MAX_SKILLS_BYTES: usize = 32_768;
pub const MAX_TOOLS_BYTES: usize = 32_768;
pub const MAX_LEARNINGS_BYTES: usize = 32_768;
pub const MAX_ERRORS_BYTES: usize = 8_192;
pub const MAX_RUNTIME_BYTES: usize = 4_096;

/// Truncate content to `max_bytes` on a char boundary.
/// Logs full content at info level for debugging; warns on truncation.
pub fn truncate_layer(layer: &str, content: &str, max_bytes: usize, source: &str) -> String {
    let original = content.len();

    if original <= max_bytes {
        tracing::info!(
            layer,
            size = original,
            max = max_bytes,
            source,
            "prompt layer loaded"
        );
        return content.to_string();
    }

    // Find a valid char boundary at or before max_bytes
    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = &content[..end];
    let dropped = original - end;
    tracing::warn!(
        layer,
        original_size = original,
        truncated_size = end,
        dropped_bytes = dropped,
        max = max_bytes,
        source,
        "prompt layer TRUNCATED"
    );
    format!("{truncated}\n[... truncated at {end}/{original} bytes ...]")
}

/// Builds the full system prompt for a chat turn.
///
/// Uses a builder pattern with `Arc` dependencies (no lifetimes).
///
/// Layer order:
///   Identity → Soul → System Prompt → Skills → Tools → Learnings → Recent Errors → Runtime
pub struct PromptBuilder {
    storage: Arc<AgentStore>,
    skills: Arc<dyn SkillCatalog>,

    identity: Option<String>,
    soul: Option<String>,
    runtime: Option<String>,
    learnings: Option<String>,
    recent_errors: Option<String>,
    tools: Option<Arc<Vec<ToolSchema>>>,
}

impl PromptBuilder {
    pub fn new(storage: Arc<AgentStore>, skills: Arc<dyn SkillCatalog>) -> Self {
        Self {
            storage,
            skills,
            identity: None,
            soul: None,
            runtime: None,
            learnings: None,
            recent_errors: None,
            tools: None,
        }
    }

    pub fn with_identity(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.is_empty() {
            self.identity = Some(s);
        }
        self
    }

    pub fn with_soul(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.is_empty() {
            self.soul = Some(s);
        }
        self
    }

    pub fn with_runtime(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.is_empty() {
            self.runtime = Some(s);
        }
        self
    }

    pub fn with_learnings(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.is_empty() {
            self.learnings = Some(s);
        }
        self
    }

    pub fn with_recent_errors(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.is_empty() {
            self.recent_errors = Some(s);
        }
        self
    }

    pub fn with_tools(mut self, tools: Arc<Vec<ToolSchema>>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Build the full system prompt.
    pub async fn build(&self, agent_id: &str, user_id: &str, session_id: &str) -> Result<String> {
        tracing::info!(
            log_kind = "server_log",
            stage = "prompt",
            status = "started",
            agent_id,
            user_id,
            session_id,
            "prompt build"
        );

        let config = self.storage.config_get(agent_id).await?;
        let has_config = config.is_some();
        tracing::info!(
            log_kind = "server_log",
            stage = "prompt",
            status = "config_loaded",
            agent_id,
            has_config,
            "prompt build"
        );

        let mut prompt = String::with_capacity(4096);

        // 1. Identity
        tracing::debug!("prompt step 1/8: loading identity layer");
        let identity = self
            .identity
            .as_deref()
            .or_else(|| {
                let c = config.as_ref()?;
                if c.identity.is_empty() { None } else { Some(c.identity.as_str()) }
            });
        if let Some(s) = identity.filter(|s| !s.is_empty()) {
            let src = if self.identity.is_some() {
                "injected"
            } else {
                "db"
            };
            let s = truncate_layer("identity", s, MAX_IDENTITY_BYTES, src);
            prompt.push_str(&s);
            prompt.push_str("\n\n");
        } else {
            tracing::debug!("prompt step 1/8: identity layer — skipped (empty)");
        }

        // 2. Soul
        tracing::debug!("prompt step 2/8: loading soul layer");
        let soul = self
            .soul
            .as_deref()
            .or_else(|| {
                let c = config.as_ref()?;
                if c.soul.is_empty() { None } else { Some(c.soul.as_str()) }
            });
        if let Some(s) = soul.filter(|s| !s.is_empty()) {
            let src = if self.soul.is_some() {
                "injected"
            } else {
                "db"
            };
            let s = truncate_layer("soul", s, MAX_SOUL_BYTES, src);
            prompt.push_str("## Soul\n\n");
            prompt.push_str(&s);
            prompt.push_str("\n\n");
        } else {
            tracing::debug!("prompt step 2/8: soul layer — skipped (empty)");
        }

        // 3. System Prompt (from DB)
        tracing::debug!("prompt step 3/8: loading system prompt layer");
        let system = config
            .as_ref()
            .map(|c| c.system_prompt.as_str())
            .unwrap_or("");
        if !system.is_empty() {
            let s = truncate_layer("system", system, MAX_SYSTEM_BYTES, "db");
            prompt.push_str(&s);
            prompt.push_str("\n\n");
        } else {
            tracing::debug!("prompt step 3/8: system prompt layer — skipped (empty)");
        }

        // 4. Skills (metadata only)
        tracing::debug!("prompt step 4/8: loading skills layer");
        self.append_skills(&mut prompt, agent_id, user_id);

        // 5. Tools (compact list)
        tracing::debug!("prompt step 5/8: loading tools layer");
        self.append_tools(&mut prompt);

        // 6. Learnings
        tracing::debug!("prompt step 6/8: loading learnings layer");
        self.append_learnings(&mut prompt, agent_id).await;

        // 7. Recent Errors
        tracing::debug!("prompt step 7/8: loading recent errors layer");
        self.append_recent_errors(&mut prompt, session_id).await;

        // 8. Runtime
        tracing::debug!("prompt step 8/8: loading runtime layer");
        self.append_runtime(&mut prompt);

        tracing::info!(
            log_kind = "server_log",
            stage = "prompt",
            status = "completed",
            agent_id,
            session_id,
            total_size = prompt.len(),
            "prompt build"
        );

        // Template substitution
        let state = self.storage.session_get_state(session_id).await?;
        Ok(substitute_template(&prompt, &state))
    }

    fn append_skills(&self, prompt: &mut String, agent_id: &str, user_id: &str) {
        let skills = self.skills.for_agent(agent_id, user_id);
        tracing::debug!(
            agent_id,
            user_id,
            total_skills = skills.len(),
            "skills: queried catalog for agent"
        );

        let non_exec: Vec<_> = skills.iter().filter(|s| !s.executable).collect();
        if non_exec.is_empty() {
            tracing::debug!("skills: no non-executable skills found — skipped");
            return;
        }

        tracing::debug!(
            count = non_exec.len(),
            "skills: loading non-executable skills into prompt"
        );

        let mut buf = String::new();
        buf.push_str("## Available Skills\n\n<available_skills>\n");
        for (i, s) in non_exec.iter().enumerate() {
            tracing::debug!(
                index = i,
                name = %s.name,
                description = %s.description,
                "skills: adding skill to prompt"
            );
            let _ = writeln!(buf, "<skill name=\"{}\">{}</skill>", s.name, s.description);
        }
        buf.push_str("</available_skills>\n\n");
        buf.push_str("Use `read_skill(name)` for full instructions.\n\n");

        let buf = truncate_layer("skills", &buf, MAX_SKILLS_BYTES, "catalog");
        prompt.push_str(&buf);
    }

    fn append_tools(&self, prompt: &mut String) {
        let tools = match &self.tools {
            Some(t) if !t.is_empty() => t,
            _ => {
                tracing::debug!("tools: no tools registered — skipped");
                return;
            }
        };

        tracing::debug!(count = tools.len(), "tools: loading tool list into prompt");

        let mut buf = String::new();
        buf.push_str("## Available Tools\n\n");
        for (i, t) in tools.iter().enumerate() {
            tracing::debug!(
                index = i,
                name = %t.function.name,
                description = %t.function.description,
                "tools: adding tool to prompt"
            );
            let _ = writeln!(buf, "- `{}`: {}", t.function.name, t.function.description);
        }
        buf.push_str(
            "\nCall tools when they would help accomplish the task.\
             \nAlways search memory before claiming you don't know something.\n\n",
        );

        let buf = truncate_layer("tools", &buf, MAX_TOOLS_BYTES, "registry");
        prompt.push_str(&buf);
    }

    async fn append_learnings(&self, prompt: &mut String, agent_id: &str) {
        let (text, src) = if let Some(ref s) = self.learnings {
            tracing::debug!(size = s.len(), "learnings: using injected text");
            (s.clone(), "injected")
        } else {
            tracing::debug!(agent_id, limit = LEARNINGS_LIMIT, "learnings: querying db");
            match self
                .storage
                .learning_list_by_agent(agent_id, LEARNINGS_LIMIT)
                .await
            {
                Ok(records) if !records.is_empty() => {
                    tracing::debug!(count = records.len(), "learnings: loaded from db");
                    for (i, r) in records.iter().enumerate() {
                        tracing::debug!(
                            index = i,
                            title = %r.title,
                            content = %r.content,
                            "learnings: record"
                        );
                    }
                    (format_learnings(&records), "db")
                }
                Ok(_) => {
                    tracing::debug!("learnings: no records found — skipped");
                    return;
                }
                Err(e) => {
                    tracing::debug!(error = %e, "learnings: db query failed — skipped");
                    return;
                }
            }
        };

        if !text.is_empty() {
            let mut buf = String::from("## Learnings\n\n");
            buf.push_str(&text);
            buf.push_str("\n\n");
            let buf = truncate_layer("learnings", &buf, MAX_LEARNINGS_BYTES, src);
            prompt.push_str(&buf);
        }
    }

    fn append_runtime(&self, prompt: &mut String) {
        let (buf, src) = if let Some(ref rt) = self.runtime {
            tracing::debug!(size = rt.len(), "runtime: using injected text");
            let mut b = String::from("## Runtime\n\n");
            b.push_str(rt);
            b.push_str("\n\n");
            (b, "injected")
        } else {
            let host = std::env::var("HOSTNAME")
                .or_else(|_| std::env::var("HOST"))
                .unwrap_or_else(|_| "unknown".into());
            let os = std::env::consts::OS;
            let arch = std::env::consts::ARCH;
            tracing::debug!(
                host = %host,
                os = %os,
                arch = %arch,
                "runtime: built from environment"
            );
            let b = format!("## Runtime\n\nHost: {} | OS: {} ({})\n\n", host, os, arch,);
            (b, "env")
        };

        let buf = truncate_layer("runtime", &buf, MAX_RUNTIME_BYTES, src);
        prompt.push_str(&buf);
    }

    async fn append_recent_errors(&self, prompt: &mut String, session_id: &str) {
        let (text, src) = if let Some(ref s) = self.recent_errors {
            tracing::debug!(size = s.len(), "recent_errors: using injected text");
            (s.clone(), "injected")
        } else {
            tracing::debug!(
                session_id,
                limit = RECENT_ERRORS_LIMIT,
                "recent_errors: querying db for failed spans"
            );
            match self
                .storage
                .recent_failed_spans(session_id, RECENT_ERRORS_LIMIT)
                .await
            {
                Ok(spans) if !spans.is_empty() => {
                    tracing::debug!(
                        count = spans.len(),
                        "recent_errors: loaded failed spans from db"
                    );
                    let mut out = String::new();
                    for (i, s) in spans.iter().enumerate() {
                        tracing::debug!(
                            index = i,
                            name = %s.name,
                            kind = %s.kind,
                            error_code = %s.error_code,
                            error_message = %s.error_message,
                            "recent_errors: failed span"
                        );
                        if s.error_message.is_empty() {
                            let _ = writeln!(out, "- `{}`: failed", s.name);
                        } else {
                            let _ = writeln!(out, "- `{}`: {}", s.name, s.error_message);
                        }
                    }
                    (out, "db")
                }
                Ok(_) => {
                    tracing::debug!("recent_errors: no failed spans found — skipped");
                    return;
                }
                Err(e) => {
                    tracing::debug!(error = %e, "recent_errors: db query failed — skipped");
                    return;
                }
            }
        };

        if !text.is_empty() {
            let mut buf = String::from("## Recent Errors\n\n");
            buf.push_str("The following operations failed recently in this session. Avoid repeating the same mistakes.\n\n");
            buf.push_str(&text);
            buf.push_str("\n\n");
            let buf = truncate_layer("recent_errors", &buf, MAX_ERRORS_BYTES, src);
            prompt.push_str(&buf);
        }
    }
}

pub fn format_learnings(records: &[LearningRecord]) -> String {
    let mut out = String::new();
    for r in records {
        if !r.title.is_empty() {
            let _ = writeln!(out, "- **{}**: {}", r.title, r.content);
        } else {
            let _ = writeln!(out, "- {}", r.content);
        }
    }
    out
}

/// Replace `{key}` placeholders with values from session state.
pub fn substitute_template(template: &str, state: &serde_json::Value) -> String {
    if !template.contains('{') || state.is_null() {
        return template.to_string();
    }
    let obj = match state.as_object() {
        Some(o) => o,
        None => return template.to_string(),
    };
    let mut result = template.to_string();
    for (key, value) in obj {
        let placeholder = format!("{{{key}}}");
        let replacement = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        result = result.replace(&placeholder, &replacement);
    }
    result
}
