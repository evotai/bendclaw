//! Prompt construction for a chat turn.

use std::fmt::Write;
use std::sync::Arc;

use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::cluster::ClusterService;
use crate::kernel::recall::RecallStore;
use crate::kernel::skills::store::SkillStore;
use crate::llm::tool::ToolSchema;
use crate::storage::dal::learning::LearningRecord;
use crate::storage::dal::variable::record::VariableRecord;

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
pub const MAX_RECALL_BYTES: usize = 32_768;
pub const MAX_ERRORS_BYTES: usize = 8_192;
pub const MAX_VARIABLES_BYTES: usize = 16_384;
pub const MAX_RUNTIME_BYTES: usize = 4_096;
pub const MAX_CLUSTER_BYTES: usize = 8_192;
pub const MAX_DIRECTIVE_BYTES: usize = 4_096;

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
///   Identity → Soul → System Prompt → Skills → Tools → Learnings → Variables → Recent Errors → Runtime
pub struct PromptBuilder {
    storage: Arc<AgentStore>,
    skills: Arc<SkillStore>,

    identity: Option<String>,
    soul: Option<String>,
    runtime: Option<String>,
    learnings: Option<String>,
    recent_errors: Option<String>,
    tools: Option<Arc<Vec<ToolSchema>>>,
    variables: Option<Vec<VariableRecord>>,
    recall: Option<Arc<RecallStore>>,
    cluster_client: Option<Arc<ClusterService>>,
    directive_prompt: Option<String>,
}

impl PromptBuilder {
    pub fn new(storage: Arc<AgentStore>, skills: Arc<SkillStore>) -> Self {
        Self {
            storage,
            skills,
            identity: None,
            soul: None,
            runtime: None,
            learnings: None,
            recent_errors: None,
            tools: None,
            variables: None,
            recall: None,
            cluster_client: None,
            directive_prompt: None,
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

    pub fn with_variables(mut self, vars: Vec<VariableRecord>) -> Self {
        if !vars.is_empty() {
            self.variables = Some(vars);
        }
        self
    }

    pub fn with_recall(mut self, store: Arc<RecallStore>) -> Self {
        self.recall = Some(store);
        self
    }

    pub fn with_cluster_client(mut self, client: Arc<ClusterService>) -> Self {
        self.cluster_client = Some(client);
        self
    }

    pub fn with_directive_prompt(mut self, prompt: Option<String>) -> Self {
        self.directive_prompt = prompt;
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
        tracing::debug!("prompt step 1/9: loading identity layer");
        let identity = self.identity.as_deref().or_else(|| {
            let c = config.as_ref()?;
            if c.identity.is_empty() {
                None
            } else {
                Some(c.identity.as_str())
            }
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
        tracing::debug!("prompt step 2/9: loading soul layer");
        let soul = self.soul.as_deref().or_else(|| {
            let c = config.as_ref()?;
            if c.soul.is_empty() {
                None
            } else {
                Some(c.soul.as_str())
            }
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
        tracing::debug!("prompt step 3/9: loading system prompt layer");
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
        tracing::debug!("prompt step 4/9: loading skills layer");
        self.append_skills(&mut prompt, agent_id);

        // 5. Tools (compact list)
        tracing::debug!("prompt step 5/10: loading tools layer");
        self.append_tools(&mut prompt);

        // 5b. Cluster info (between Tools and Recall)
        tracing::debug!("prompt step 5b/10: loading cluster layer");
        self.append_cluster_info(&mut prompt).await;

        // 5c. Directive (platform-driven behavior)
        tracing::debug!("prompt step 5c/10: loading directive layer");
        self.append_directive(&mut prompt);

        // 6. Learnings / Recall Hints
        tracing::debug!("prompt step 6/10: loading learnings layer");
        self.append_recall_hints(&mut prompt, agent_id).await;

        // 7. Variables
        tracing::debug!("prompt step 7/9: loading variables layer");
        self.append_variables(&mut prompt).await;

        // 8. Recent Errors
        tracing::debug!("prompt step 8/9: loading recent errors layer");
        self.append_recent_errors(&mut prompt, session_id).await;

        // 9. Runtime
        tracing::debug!("prompt step 9/9: loading runtime layer");
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

    fn append_skills(&self, prompt: &mut String, agent_id: &str) {
        let skills = self.skills.for_agent(agent_id);
        tracing::debug!(
            agent_id,
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
             \nUse memory_write for user or session preferences.\
             \nUse learning_write for reusable agent-level lessons.\
             \nSearch memory, knowledge, or learning when prior context may help.\n\n",
        );

        let buf = truncate_layer("tools", &buf, MAX_TOOLS_BYTES, "registry");
        prompt.push_str(&buf);
    }

    async fn append_cluster_info(&self, prompt: &mut String) {
        let cluster_service = match &self.cluster_client {
            Some(c) => c,
            None => return,
        };

        // Read cached peer snapshot — never blocks on network
        let nodes = cluster_service.cached_peers();

        let mut buf = String::from("## Cluster\n\n");
        buf.push_str("You are part of a distributed cluster. You can dispatch subtasks to peer nodes for parallel execution.\n\n");

        if nodes.is_empty() {
            buf.push_str("No peer nodes currently available.\n\n");
        } else {
            buf.push_str("| Node ID | Endpoint | Load | Status |\n");
            buf.push_str("|---------|----------|------|--------|\n");
            for n in &nodes {
                let _ = writeln!(
                    buf,
                    "| {} | {} | {}/{} | {} |",
                    n.instance_id, n.endpoint, n.current_load, n.max_load, n.status
                );
            }
            buf.push('\n');
        }

        buf.push_str("Tools:\n");
        buf.push_str("- `cluster_nodes`: Refresh the list of available peer nodes\n");
        buf.push_str("- `cluster_dispatch(node_id, agent_id, task)`: Send a subtask to a peer node by its node_id\n");
        buf.push_str("- `cluster_collect(dispatch_ids, timeout_secs)`: Wait for and collect results from dispatched subtasks\n\n");

        let buf = truncate_layer("cluster", &buf, MAX_CLUSTER_BYTES, "cache");
        prompt.push_str(&buf);
    }

    fn append_directive(&self, prompt: &mut String) {
        let text = match &self.directive_prompt {
            Some(s) if !s.is_empty() => s,
            _ => return,
        };
        let mut buf = String::from("## Directive\n\n");
        buf.push_str(text);
        buf.push_str("\n\n");
        let buf = truncate_layer("directive", &buf, MAX_DIRECTIVE_BYTES, "platform");
        prompt.push_str(&buf);
    }

    async fn append_recall_hints(&self, prompt: &mut String, agent_id: &str) {
        // If text was injected directly, use it (backwards compat for tests)
        if let Some(ref s) = self.learnings {
            tracing::debug!(size = s.len(), "learnings: using injected text");
            if !s.is_empty() {
                let mut buf = String::from("## Learnings\n\n");
                buf.push_str(s);
                buf.push_str("\n\n");
                let buf = truncate_layer("learnings", &buf, MAX_LEARNINGS_BYTES, "injected");
                prompt.push_str(&buf);
            }
            return;
        }

        // If a RecallStore is available, build recall hints from it
        if let Some(ref recall) = self.recall {
            let mut buf = String::new();

            // Agent learnings (all kinds, limit 10, high-confidence only)
            match recall.learnings().list(10).await {
                Ok(records) if !records.is_empty() => {
                    let filtered: Vec<_> = records
                        .iter()
                        .filter(|r| r.confidence >= 0.7 && r.status == "active")
                        .collect();
                    if !filtered.is_empty() {
                        buf.push_str("### Learnings\n\n");
                        for r in &filtered {
                            let _ = writeln!(buf, "- [{}] **{}**: {}", r.kind, r.title, r.content);
                        }
                        buf.push('\n');
                    }
                }
                Err(e) => tracing::warn!(error = %e, "recall: learnings query failed, skipping"),
                _ => {}
            }

            // Active knowledge entries (limit 5, high-confidence only, metadata-only summaries)
            match recall.knowledge().list_active(5).await {
                Ok(records) if !records.is_empty() => {
                    let filtered: Vec<_> = records.iter().filter(|r| r.confidence >= 0.7).collect();
                    if !filtered.is_empty() {
                        buf.push_str("### Known Context\n\n");
                        for r in &filtered {
                            let _ = writeln!(buf, "- [{}] {} ({})", r.kind, r.title, r.locator);
                        }
                        buf.push('\n');
                    }
                }
                Err(e) => tracing::warn!(error = %e, "recall: knowledge query failed, skipping"),
                _ => {}
            }

            if !buf.is_empty() {
                let mut section = String::from("## Recall Hints\n\n");
                section.push_str(&buf);
                let section =
                    truncate_layer("recall_hints", &section, MAX_RECALL_BYTES, "recall_store");
                prompt.push_str(&section);
            }
            return;
        }

        // Fallback: load old-style learnings from DB
        tracing::debug!(agent_id, limit = LEARNINGS_LIMIT, "learnings: querying db");
        match self.storage.learning_list(LEARNINGS_LIMIT).await {
            Ok(records) if !records.is_empty() => {
                tracing::debug!(count = records.len(), "learnings: loaded from db");
                let text = format_learnings(&records);
                if !text.is_empty() {
                    let mut buf = String::from("## Learnings\n\n");
                    buf.push_str(&text);
                    buf.push_str("\n\n");
                    let buf = truncate_layer("learnings", &buf, MAX_LEARNINGS_BYTES, "db");
                    prompt.push_str(&buf);
                }
            }
            Ok(_) => {
                tracing::debug!("learnings: no records found — skipped");
            }
            Err(e) => {
                tracing::debug!(error = %e, "learnings: db query failed — skipped");
            }
        }
    }

    async fn append_variables(&self, prompt: &mut String) {
        // If a snapshot was provided at session creation, use it instead of
        // querying the database live.  This keeps the prompt stable within a
        // session even when variables are changed externally.
        if let Some(ref vars) = self.variables {
            if vars.is_empty() {
                tracing::info!("variables: snapshot empty — skipped");
                return;
            }
            tracing::info!(count = vars.len(), "variables: using snapshot");
            let mut buf = String::from("## Variables\n\n");
            buf.push_str(
                "The following variables are available as environment variables in shell commands.\n\n",
            );
            for v in vars {
                if v.secret {
                    let _ = writeln!(
                        buf,
                        "- `{}`: [SECRET] (available as env var `${}`)",
                        v.key, v.key
                    );
                } else {
                    let _ = writeln!(buf, "- `{}` = `{}`", v.key, v.value);
                }
            }
            buf.push('\n');
            let buf = truncate_layer("variables", &buf, MAX_VARIABLES_BYTES, "snapshot");
            prompt.push_str(&buf);
            return;
        }

        tracing::info!("variables: querying db");
        let records = match self.storage.variable_list().await {
            Ok(r) if !r.is_empty() => r,
            Ok(_) => {
                tracing::info!("variables: no records found — skipped");
                return;
            }
            Err(e) => {
                tracing::info!(error = %e, "variables: db query failed — skipped");
                return;
            }
        };

        tracing::info!(count = records.len(), "variables: loaded from db");

        let mut buf = String::from("## Variables\n\n");
        buf.push_str(
            "The following variables are available as environment variables in shell commands.\n\n",
        );
        for v in &records {
            if v.secret {
                let _ = writeln!(
                    buf,
                    "- `{}`: [SECRET] (available as env var `${}`)",
                    v.key, v.key
                );
            } else {
                let _ = writeln!(buf, "- `{}` = `{}`", v.key, v.value);
            }
        }
        buf.push('\n');

        let buf = truncate_layer("variables", &buf, MAX_VARIABLES_BYTES, "db");
        prompt.push_str(&buf);
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
