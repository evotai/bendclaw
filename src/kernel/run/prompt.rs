//! Prompt construction for a chat turn.

use std::fmt::Write;
use std::sync::Arc;

use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::cluster::ClusterService;
use crate::kernel::recall::RecallStore;
use crate::kernel::run::default_identity;
use crate::kernel::run::runtime_context;
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
/// Warns on truncation.
pub fn truncate_layer(layer: &str, content: &str, max_bytes: usize, source: &str) -> String {
    let original = content.len();

    if original <= max_bytes {
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
        tracing::debug!(
            stage = "prompt",
            status = "started",
            agent_id,
            user_id,
            session_id,
            "prompt build"
        );

        // Phase 1: Fire all independent DB queries in parallel.
        let (config, recall_hints, variables_text, errors_text, state) =
            self.fetch_all(agent_id, session_id).await;

        let config = config?;
        let has_config = config.is_some();
        tracing::debug!(
            stage = "prompt",
            status = "config_loaded",
            agent_id,
            has_config,
            "prompt build"
        );

        // Phase 2: Assemble prompt (CPU-only, no I/O).
        let mut prompt = String::with_capacity(4096);

        // 1. Identity (with default fallback)
        let identity = self
            .identity
            .as_deref()
            .or_else(|| {
                let c = config.as_ref()?;
                if c.identity.is_empty() {
                    None
                } else {
                    Some(c.identity.as_str())
                }
            })
            .unwrap_or_else(|| default_identity::default_identity());
        let src = if self.identity.is_some() {
            "injected"
        } else if config.as_ref().is_some_and(|c| !c.identity.is_empty()) {
            "db"
        } else {
            "default"
        };
        let s = truncate_layer("identity", identity, MAX_IDENTITY_BYTES, src);
        prompt.push_str(&s);
        prompt.push_str("\n\n");

        // 2. Soul
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
        }

        // 3. System Prompt (from DB)
        let system = config
            .as_ref()
            .map(|c| c.system_prompt.as_str())
            .unwrap_or("");
        if !system.is_empty() {
            let s = truncate_layer("system", system, MAX_SYSTEM_BYTES, "db");
            prompt.push_str(&s);
            prompt.push_str("\n\n");
        }

        // 4. Skills (metadata only, sync)
        self.append_skills(&mut prompt, agent_id);

        // 5. Tools (compact list, sync)
        self.append_tools(&mut prompt);

        // 5b. Cluster info (reads cached peers, no DB)
        self.append_cluster_info(&mut prompt).await;

        // 5c. Directive (platform-driven behavior, sync)
        self.append_directive(&mut prompt);

        // 6-8. Append pre-fetched async layers
        prompt.push_str(&recall_hints);
        prompt.push_str(&variables_text);
        prompt.push_str(&errors_text);

        // 9. Runtime (sync)
        self.append_runtime(&mut prompt, session_id);

        tracing::info!(
            stage = "prompt",
            status = "completed",
            agent_id,
            session_id,
            total_size = prompt.len(),
            "prompt build"
        );
        tracing::debug!(
            stage = "prompt",
            agent_id,
            session_id,
            content = %prompt,
            "prompt full content"
        );

        // Template substitution (state was fetched in parallel)
        let state = state?;
        Ok(substitute_template(&prompt, &state))
    }

    /// Fetch all independent data sources in parallel.
    /// Returns (config, recall_hints_text, variables_text, errors_text, session_state).
    async fn fetch_all(
        &self,
        agent_id: &str,
        session_id: &str,
    ) -> (
        Result<Option<crate::storage::dal::agent_config::record::AgentConfigRecord>>,
        String,
        String,
        String,
        Result<serde_json::Value>,
    ) {
        let config_fut = self.storage.config_get(agent_id);
        let recall_fut = self.build_recall_hints(agent_id);
        let vars_fut = self.build_variables_text();
        let errors_fut = self.build_errors_text(session_id);
        let state_fut = self.storage.session_get_state(session_id);

        tokio::join!(config_fut, recall_fut, vars_fut, errors_fut, state_fut)
    }

    fn append_skills(&self, prompt: &mut String, agent_id: &str) {
        let skills = self.skills.for_agent(agent_id);

        let non_exec: Vec<_> = skills.iter().filter(|s| !s.executable).collect();
        if non_exec.is_empty() {
            return;
        }

        let mut buf = String::new();
        buf.push_str("## Available Skills\n\n<available_skills>\n");
        for s in &non_exec {
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
            _ => return,
        };

        let mut buf = String::new();
        buf.push_str("## Available Tools\n\n");
        for t in tools.iter() {
            let _ = writeln!(buf, "- `{}`: {}", t.function.name, t.function.description);
        }
        buf.push_str(
            "\nCall tools when they would help accomplish the task.\
             \nUse memory_write for user or session preferences.\
             \nUse learning_write for reusable agent-level lessons.\
             \nSearch memory, knowledge, or learning when prior context may help.\n",
        );

        let has_claude = tools.iter().any(|t| t.function.name == "claude_code");
        let has_codex = tools.iter().any(|t| t.function.name == "codex_exec");
        if has_claude || has_codex {
            buf.push_str("\n### Coding-agent orchestration\n\n");
            buf.push_str("Coding-agent tools can continue work across multiple rounds in the same agent session. You decide the higher-level workflow: when to implement, review, switch agents, or stop.\n");
            if has_claude && has_codex {
                buf.push_str(
                    "You may use one coding agent to implement and the other to review.\n",
                );
            }
            buf.push_str("It is normal to do multiple fix/review rounds. Decide whether to continue or stop based on the remaining issues and the task goal.\n\n");
        } else {
            buf.push('\n');
        }

        buf.push('\n');

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
                let meta = n.meta();
                let _ = writeln!(
                    buf,
                    "| {} | {} | {}/{} | {} |",
                    n.node_id, n.endpoint, meta.current_load, meta.max_load, meta.status
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

    /// Build recall hints text. Returns formatted section string (may be empty).
    async fn build_recall_hints(&self, _agent_id: &str) -> String {
        // If text was injected directly, use it (backwards compat for tests)
        if let Some(ref s) = self.learnings {
            if s.is_empty() {
                return String::new();
            }
            let mut buf = String::from("## Learnings\n\n");
            buf.push_str(s);
            buf.push_str("\n\n");
            return truncate_layer("learnings", &buf, MAX_LEARNINGS_BYTES, "injected");
        }

        // If a RecallStore is available, build recall hints from it
        if let Some(ref recall) = self.recall {
            let mut buf = String::new();

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
                return truncate_layer("recall_hints", &section, MAX_RECALL_BYTES, "recall_store");
            }
            return String::new();
        }

        // Fallback: load old-style learnings from DB
        match self.storage.learning_list(LEARNINGS_LIMIT).await {
            Ok(records) if !records.is_empty() => {
                let text = format_learnings(&records);
                if text.is_empty() {
                    return String::new();
                }
                let mut buf = String::from("## Learnings\n\n");
                buf.push_str(&text);
                buf.push_str("\n\n");
                truncate_layer("learnings", &buf, MAX_LEARNINGS_BYTES, "db")
            }
            Ok(_) => String::new(),
            Err(e) => {
                tracing::warn!(error = %e, "learnings: db query failed — skipped");
                String::new()
            }
        }
    }

    /// Build variables text. Returns formatted section string (may be empty).
    async fn build_variables_text(&self) -> String {
        let records: Vec<&VariableRecord>;
        let fetched;

        if let Some(ref vars) = self.variables {
            if vars.is_empty() {
                return String::new();
            }
            records = vars.iter().collect();
        } else {
            match self.storage.variable_list().await {
                Ok(r) if !r.is_empty() => {
                    fetched = r;
                    records = fetched.iter().collect();
                }
                Ok(_) => return String::new(),
                Err(e) => {
                    tracing::warn!(error = %e, "variables: db query failed — skipped");
                    return String::new();
                }
            }
        }

        let src = if self.variables.is_some() {
            "snapshot"
        } else {
            "db"
        };
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
        truncate_layer("variables", &buf, MAX_VARIABLES_BYTES, src)
    }

    fn append_runtime(&self, prompt: &mut String, session_id: &str) {
        let (buf, src) = if let Some(ref rt) = self.runtime {
            let mut b = String::from("## Runtime\n\n");
            b.push_str(rt);
            b.push_str("\n\n");
            (b, "injected")
        } else {
            let ch_ctx =
                crate::kernel::channel::context::ChannelContext::from_session_key(session_id);
            let (ch_type, ch_chat) = match &ch_ctx {
                Some(c) => (Some(c.channel_type.as_str()), Some(c.chat_id.as_str())),
                None => (None, None),
            };
            (
                runtime_context::build_runtime_context(ch_type, ch_chat),
                "env",
            )
        };

        let buf = truncate_layer("runtime", &buf, MAX_RUNTIME_BYTES, src);
        prompt.push_str(&buf);
    }

    /// Build recent errors text. Returns formatted section string (may be empty).
    async fn build_errors_text(&self, session_id: &str) -> String {
        let (text, src) = if let Some(ref s) = self.recent_errors {
            (s.clone(), "injected")
        } else {
            match self
                .storage
                .recent_failed_spans(session_id, RECENT_ERRORS_LIMIT)
                .await
            {
                Ok(spans) if !spans.is_empty() => {
                    let mut out = String::new();
                    for s in &spans {
                        if s.error_message.is_empty() {
                            let _ = writeln!(out, "- `{}`: failed", s.name);
                        } else {
                            let _ = writeln!(out, "- `{}`: {}", s.name, s.error_message);
                        }
                    }
                    (out, "db")
                }
                Ok(_) => return String::new(),
                Err(e) => {
                    tracing::warn!(error = %e, "recent_errors: db query failed — skipped");
                    return String::new();
                }
            }
        };

        if text.is_empty() {
            return String::new();
        }
        let mut buf = String::from("## Recent Errors\n\n");
        buf.push_str("The following operations failed recently in this session. Avoid repeating the same mistakes.\n\n");
        buf.push_str(&text);
        buf.push_str("\n\n");
        truncate_layer("recent_errors", &buf, MAX_ERRORS_BYTES, src)
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
