//! SkillRunner — runs skill scripts as subprocesses on the host.

use std::sync::Arc;
use std::time::Instant;

use serde_json::json;

use super::skill_executor::SkillExecutor;
use super::skill_executor::SkillOutput;
use super::usage_sink::UsageSink;
use crate::sessions::workspace::Workspace;
use crate::skills::definition::skill::Skill;
use crate::skills::diagnostics;
use crate::skills::sync::SkillIndex;
use crate::storage::pool::Pool;
use crate::types::ErrorCode;
use crate::types::Result;
use crate::variables::store::SharedVariableStore;
use crate::variables::store::VariableStore;

pub struct SkillRunner {
    catalog: Arc<SkillIndex>,
    usage_sink: Arc<dyn UsageSink>,
    workspace: Arc<Workspace>,
    pool: Pool,
    agent_id: String,
    user_id: String,
}

#[async_trait::async_trait]
impl SkillExecutor for SkillRunner {
    async fn execute(&self, skill_name: &str, args: &[String]) -> Result<SkillOutput> {
        self.execute_impl(skill_name, args).await
    }
}

impl SkillRunner {
    pub fn new(
        agent_id: &str,
        user_id: &str,
        catalog: Arc<SkillIndex>,
        usage_sink: Arc<dyn UsageSink>,
        workspace: Arc<Workspace>,
        pool: Pool,
    ) -> Self {
        Self {
            catalog,
            usage_sink,
            workspace,
            pool,
            agent_id: agent_id.to_string(),
            user_id: user_id.to_string(),
        }
    }

    async fn execute_impl(&self, skill_name: &str, args: &[String]) -> Result<SkillOutput> {
        let start = Instant::now();

        if skill_name == "skill_read" {
            let path = args
                .iter()
                .position(|a| a == "--path")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("");

            if path.is_empty() {
                return Err(ErrorCode::skill_exec(
                    "skill_read requires --path <skill-name>",
                ));
            }

            let content = self
                .catalog
                .read_skill(&self.user_id, path)
                .unwrap_or_else(|| format!("Skill not found: {path}"));

            return Ok(SkillOutput {
                data: Some(serde_json::Value::String(content)),
                error: None,
            });
        }

        let skill = self
            .catalog
            .resolve(&self.user_id, skill_name)
            .ok_or_else(|| ErrorCode::skill_not_found(format!("unknown skill: {skill_name}")))?;

        if !skill.is_visible_to(&self.user_id) {
            return Err(ErrorCode::skill_not_found(format!(
                "unknown skill: {skill_name}"
            )));
        }

        let host_script_path = self
            .catalog
            .host_script_path(&self.user_id, skill_name)
            .ok_or_else(|| ErrorCode::skill_exec(format!("skill '{skill_name}' has no script")))?;

        self.preflight_check(&skill).await?;

        let envelope = serde_json::to_string(&json!({
            "session": {
                "agent_id": self.agent_id,
                "user_id": self.user_id,
            },
            "timeout": 300,
        }))
        .map_err(|e| ErrorCode::skill_serde(format!("session serialize failed: {e}")))?;

        let script_path = host_script_path.to_string_lossy().to_string();
        let script_name = host_script_path
            .file_name()
            .and_then(|n: &std::ffi::OsStr| n.to_str())
            .unwrap_or("run.py");
        let program = interpreter_for(script_name)?;

        let mut cmd_args = vec![script_path];
        cmd_args.extend(args.iter().cloned());

        let mut command = self.workspace.command(&program);
        command
            .args(&cmd_args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|e| ErrorCode::skill_exec(format!("failed to spawn: {e}")))?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(envelope.as_bytes()).await;
            drop(stdin);
        }

        let output = child
            .wait_with_output()
            .await
            .map_err(|e| ErrorCode::skill_exec(format!("wait failed: {e}")))?;

        self.touch_used_secret_variables(&skill);

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let exit_code = output.status.code().unwrap_or(-1);

        if exit_code != 0 {
            let latency_ms = start.elapsed().as_millis() as u64;
            diagnostics::log_skill_failed(skill_name, latency_ms, exit_code, &stderr);
            return Ok(SkillOutput {
                data: None,
                error: Some(format!("exit code {exit_code}: {stderr}")),
            });
        }

        let latency_ms = start.elapsed().as_millis() as u64;
        diagnostics::log_skill_completed(skill_name, latency_ms, exit_code, stdout.len());

        self.usage_sink
            .touch_used(skill.skill_id(), self.agent_id.clone());

        match serde_json::from_str::<SkillOutput>(&stdout) {
            Ok(out) => Ok(out),
            Err(_) => {
                let trimmed = stdout.trim();
                if trimmed.is_empty() {
                    Ok(SkillOutput {
                        data: Some(serde_json::Value::String("OK".into())),
                        error: None,
                    })
                } else {
                    Ok(SkillOutput {
                        data: Some(serde_json::Value::String(trimmed.to_string())),
                        error: None,
                    })
                }
            }
        }
    }

    async fn preflight_check(&self, skill: &Skill) -> Result<()> {
        let requires = match &skill.requires {
            Some(r) if !r.bins.is_empty() || !r.env.is_empty() => r,
            _ => return Ok(()),
        };

        for bin in &requires.bins {
            let output = tokio::process::Command::new("which")
                .arg(bin)
                .output()
                .await
                .map_err(|e| ErrorCode::skill_requirements(format!("which failed: {e}")))?;
            if !output.status.success() {
                return Err(ErrorCode::skill_requirements(format!(
                    "skill '{}' requires '{}' but it is not installed",
                    skill.name, bin
                )));
            }
        }

        for var in &requires.env {
            if !self.workspace.has_variable(var) {
                return Err(ErrorCode::skill_requirements(format!(
                    "skill '{}' requires env var '{}' but it is not set",
                    skill.name, var
                )));
            }
        }

        Ok(())
    }

    fn touch_used_secret_variables(&self, skill: &Skill) {
        let Some(requires) = &skill.requires else {
            return;
        };
        let ids = self
            .workspace
            .secret_variable_ids_for_keys(requires.env.iter().map(|s| s.as_str()));
        if ids.is_empty() {
            return;
        }
        let pool = self.pool.clone();
        let user_id = self.user_id.clone();
        crate::types::spawn_fire_and_forget("variable_touch_last_used", async move {
            let store = SharedVariableStore::new(pool);
            let _ = store.touch_last_used_many(&ids, &user_id).await;
        });
    }
}

fn interpreter_for(script_name: &str) -> Result<String> {
    if script_name.ends_with(".sh") {
        Ok("bash".to_string())
    } else if script_name.ends_with(".py") {
        Ok("python3".to_string())
    } else {
        Err(ErrorCode::skill_exec(format!(
            "unsupported script extension: '{script_name}' (only .sh and .py are supported)"
        )))
    }
}
