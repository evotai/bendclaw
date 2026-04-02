//! Pure prompt assembly — no DB, no async.

use std::fmt::Write;

use super::prompt_model::*;
use super::tool_prompt;
use crate::kernel::run::default_identity;
use crate::kernel::run::runtime_context;

/// Build a complete system prompt from pre-fetched inputs. Pure, sync, no DB.
pub fn build_prompt(inputs: PromptInputs) -> String {
    let mut prompt = String::with_capacity(4096);
    let cfg = inputs.seed.cached_config.as_ref();

    // 1. Identity
    let identity = cfg
        .map(|c| c.identity.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_identity::default_identity());
    let src = if cfg.is_some_and(|c| !c.identity.is_empty()) {
        "db"
    } else {
        "default"
    };
    prompt.push_str(&truncate_layer(
        "identity",
        identity,
        MAX_IDENTITY_BYTES,
        src,
    ));
    prompt.push_str("\n\n");

    // 2. Soul
    if let Some(soul) = cfg.map(|c| c.soul.as_str()).filter(|s| !s.is_empty()) {
        prompt.push_str("## Soul\n\n");
        prompt.push_str(&truncate_layer("soul", soul, MAX_SOUL_BYTES, "db"));
        prompt.push_str("\n\n");
    }

    // 3. System Prompt
    if let Some(sys) = cfg
        .map(|c| c.system_prompt.as_str())
        .filter(|s| !s.is_empty())
    {
        prompt.push_str(&truncate_layer("system", sys, MAX_SYSTEM_BYTES, "db"));
        prompt.push_str("\n\n");
    }
    if let Some(ref o) = inputs.system_overlay {
        if !o.is_empty() {
            prompt.push_str(o);
            prompt.push_str("\n\n");
        }
    }

    // 4. Skills
    append_skill_prompts(&mut prompt, &inputs.seed.skill_prompts);
    if let Some(ref o) = inputs.skill_overlay {
        if !o.is_empty() {
            prompt.push_str(o);
            prompt.push_str("\n\n");
        }
    }

    // 5. Tools + Cluster + Directive
    tool_prompt::render_tools_section(&mut prompt, &inputs.tools);
    if let Some(ref info) = inputs.cluster_info {
        if !info.is_empty() {
            prompt.push_str(&truncate_layer("cluster", info, MAX_CLUSTER_BYTES, "cache"));
        }
    }
    if let Some(ref text) = inputs.seed.directive_prompt {
        if !text.is_empty() {
            let mut buf = String::from("## Directive\n\n");
            buf.push_str(text);
            buf.push_str("\n\n");
            prompt.push_str(&truncate_layer(
                "directive",
                &buf,
                MAX_DIRECTIVE_BYTES,
                "platform",
            ));
        }
    }

    // 6. Variables
    append_variables_section(&mut prompt, &inputs.seed.variables);

    // 7. Recent errors
    if let Some(ref errors) = inputs.recent_errors {
        if !errors.is_empty() {
            let mut buf = String::from("## Recent Errors\n\n");
            buf.push_str("The following operations failed recently in this session. Avoid repeating the same mistakes.\n\n");
            buf.push_str(errors);
            buf.push_str("\n\n");
            prompt.push_str(&truncate_layer(
                "recent_errors",
                &buf,
                MAX_ERRORS_BYTES,
                "prefetched",
            ));
        }
    }

    // 8. Runtime
    let rt = if let Some(ref override_rt) = inputs.runtime_override {
        format!("## Runtime\n\n{override_rt}")
    } else {
        runtime_context::build_runtime_context(
            inputs.channel_type.as_deref(),
            inputs.channel_chat_id.as_deref(),
            Some(&inputs.cwd),
        )
    };
    prompt.push_str(&truncate_layer("runtime", &rt, MAX_RUNTIME_BYTES, "env"));

    // 9. Memory recall
    if let Some(ref recall) = inputs.memory_recall {
        if !recall.is_empty() {
            prompt.push_str(recall);
            prompt.push_str("\n\n");
        }
    }

    // Template substitution
    let state = inputs
        .session_state
        .as_ref()
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    substitute_template(&prompt, &state)
}

fn append_skill_prompts(prompt: &mut String, skills: &[SkillPromptEntry]) {
    if skills.is_empty() {
        return;
    }
    let mut buf = String::new();
    buf.push_str("## Available Skills\n\n<available_skills>\n");
    for s in skills {
        let _ = writeln!(
            buf,
            "<skill name=\"{}\">{}</skill>",
            s.display_name, s.description
        );
    }
    buf.push_str("</available_skills>\n\n");
    buf.push_str("Use `read_skill(name)` for full instructions.\n\n");
    prompt.push_str(&truncate_layer("skills", &buf, MAX_SKILLS_BYTES, "catalog"));
}

fn append_variables_section(prompt: &mut String, variables: &[PromptVariable]) {
    if variables.is_empty() {
        return;
    }
    let mut buf = String::from("## Variables\n\n");
    buf.push_str(
        "The following variables are available as environment variables in shell commands.\n\n",
    );
    for v in variables {
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
    prompt.push_str(&truncate_layer(
        "variables",
        &buf,
        MAX_VARIABLES_BYTES,
        "snapshot",
    ));
}
