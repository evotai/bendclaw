use std::sync::Arc;

use crate::app::result::formats;
use crate::binding::recovery_gate;
use crate::binding::recovery_gate::CleanupPolicy;
use crate::binding::session_binding;
use crate::cli::pipeline;
use crate::request::AgentRequest;
use crate::request::OutputFormat;
use crate::storage::run_events::RunEventRepo;
use crate::storage::runs::RunRepo;
use crate::storage::sessions::SessionRepo;
use crate::types::Result;

#[derive(clap::Args, Debug)]
pub struct AgentArgs {
    /// Prompt text or @file path
    #[arg(long)]
    pub prompt: String,

    /// User ID
    #[arg(long, default_value = "cli")]
    pub user_id: String,

    /// Agent ID
    #[arg(long, default_value = "default")]
    pub agent_id: String,

    /// Resume an existing session
    #[arg(long)]
    pub session_id: Option<String>,

    /// Resume the latest session
    #[arg(long, default_value_t = false)]
    pub resume: bool,

    /// System prompt overlay
    #[arg(long)]
    pub system: Option<String>,

    /// Model override
    #[arg(long)]
    pub model: Option<String>,

    /// Tool selection
    #[arg(long)]
    pub tools: Option<String>,

    /// Maximum turns
    #[arg(long, default_value_t = 50)]
    pub max_turns: u32,

    /// Maximum duration in seconds
    #[arg(long, default_value_t = 600)]
    pub max_duration: u64,

    /// Output format: text, json, stream-json
    #[arg(long, default_value = "text")]
    pub format: String,
}

pub async fn execute(
    args: AgentArgs,
    session_repo: Arc<dyn SessionRepo>,
    run_repo: Arc<dyn RunRepo>,
    run_event_repo: Arc<dyn RunEventRepo>,
) -> Result<()> {
    let prompt = resolve_input(&args.prompt)?;
    let system = args.system.as_deref().map(resolve_input).transpose()?;

    let output_format = match args.format.as_str() {
        "json" => OutputFormat::Json,
        "stream-json" => OutputFormat::StreamJson,
        _ => OutputFormat::Text,
    };

    let request = AgentRequest {
        prompt,
        user_id: args.user_id.clone(),
        agent_id: args.agent_id.clone(),
        session_id: args.session_id.clone(),
        resume_session: args.resume,
        model: args.model,
        system_overlay: system,
        max_turns: Some(args.max_turns),
        max_duration_secs: Some(args.max_duration),
        output_format,
        tool_filter: args.tools,
    };

    // Recovery gate: targeted cleanup on --resume
    let policy = if request.resume_session {
        if let Some(ref sid) = request.session_id {
            CleanupPolicy::TargetedSession(sid.clone())
        } else {
            CleanupPolicy::Skip
        }
    } else {
        CleanupPolicy::Skip
    };
    recovery_gate::recovery_gate(&run_repo, &request.user_id, &request.agent_id, policy).await?;

    // Session binding
    let session = session_binding::bind_session(
        &session_repo,
        &request.user_id,
        &request.agent_id,
        request.session_id.as_deref(),
        request.resume_session,
    )
    .await?;

    // Run planning
    let plan = pipeline::build_run_plan(&request, &session);

    // Run execution
    let envelopes = pipeline::execute_run(&run_repo, &run_event_repo, &plan).await?;

    // Format output
    match request.output_format {
        OutputFormat::Json => {
            let json = formats::json::collect_json(&envelopes);
            println!(
                "{}",
                serde_json::to_string_pretty(&json).unwrap_or_default()
            );
        }
        OutputFormat::StreamJson => {
            for env in &envelopes {
                println!("{}", formats::stream_json::encode(env));
            }
        }
        OutputFormat::Sse => {
            for env in &envelopes {
                print!("{}", formats::sse::encode(env));
            }
        }
        OutputFormat::Text => {
            let text = formats::text::collect_text(&envelopes);
            if !text.is_empty() {
                println!("{text}");
            }
        }
    }

    Ok(())
}

fn resolve_input(input: &str) -> Result<String> {
    if let Some(path) = input.strip_prefix('@') {
        std::fs::read_to_string(path)
            .map_err(|e| crate::types::ErrorCode::internal(format!("read {path}: {e}")))
    } else {
        Ok(input.to_string())
    }
}
