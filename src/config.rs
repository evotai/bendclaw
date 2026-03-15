//! Service configuration — three-layer merge: file < env < CLI args.
//!
//! ## Environment variables
//!
//! | Env var                     | TOML path              | Default        |
//! |-----------------------------|------------------------|----------------|
//! | `BENDCLAW_STORAGE_DATABEND_API_BASE_URL` | `storage.databend_api_base_url` | **required** |
//! | `BENDCLAW_STORAGE_DATABEND_API_TOKEN` | `storage.databend_api_token` | **required** |
//! | `BENDCLAW_STORAGE_DATABEND_WAREHOUSE` | `storage.databend_warehouse` | `default` |
//! | `BENDCLAW_SERVER_BIND_ADDR` | `server.bind_addr`     | `127.0.0.1:8787` |
//! | `BENDCLAW_LOG_LEVEL`        | `log.level`            | `info`         |
//! | `BENDCLAW_LOG_FORMAT`       | `log.format`           | `text`         |
//! | `BENDCLAW_WORKSPACE_ROOT_DIR` | `workspace.root_dir`   | `~/.evotai/workspace`  |
//! | `BENDCLAW_WORKSPACE_SANDBOX` | `workspace.sandbox`    | `false`        |
//! | `BENDCLAW_AUTH_KEY`          | `auth.api_key`         | *(empty)*      |
//! | `BENDCLAW_AUTH_CORS_ORIGINS` | `auth.cors_origins`    | *(default whitelist)* |
//! | `BENDCLAW_INSTANCE_ID`       | `instance_id`          | **required**   |
//! | `BENDCLAW_DIRECTIVE_API_BASE` | `directive.api_base`  | *(optional)*   |
//! | `BENDCLAW_DIRECTIVE_TOKEN`   | `directive.token`      | *(optional)*   |
//! | `BENDCLAW_LLM_NAME`          | `llm.providers[0].name` | *(optional)* |
//! | `BENDCLAW_LLM_PROVIDER`      | `llm.providers[0].provider` | *(optional)* |
//! | `BENDCLAW_LLM_BASE_URL`      | `llm.providers[0].base_url` | *(optional)* |
//! | `BENDCLAW_LLM_API_KEY`       | `llm.providers[0].api_key` | *(optional)* |
//! | `BENDCLAW_LLM_MODEL`         | `llm.providers[0].model` | *(optional)* |
//! | `BENDCLAW_LLM_WEIGHT`        | `llm.providers[0].weight` | `100`      |
//! | `BENDCLAW_LLM_TEMPERATURE`   | `llm.providers[0].temperature` | `0.7` |
//! | `BENDCLAW_ADMIN_BIND_ADDR`   | `admin.bind_addr`      | *(optional)*   |

use std::fs;

use anyhow::Context;
use serde::Deserialize;
use serde::Serialize;

/// Dot-directory name under `$HOME` for all bendclaw state, config, and logs.
pub const EVOTAI_DIR_NAME: &str = ".evotai";

pub use crate::llm::config::LLMConfig;
/// Workspace configuration for agent file operations and command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceConfig {
    /// Root directory for all workspace data. Default: "~/.evotai/workspace"
    /// Internal layout: {root_dir}/skills/, {root_dir}/{user_id}/{agent_id}/{session_id}/
    pub root_dir: String,
    /// Command idle timeout in seconds (no output = timeout). Default: 30
    pub command_timeout_secs: u64,
    /// Max output bytes from command execution. Default: 1MB
    pub max_output_bytes: usize,
    /// Allowlisted system env vars inherited by subprocess. Default: PATH, HOME, etc.
    pub safe_env_vars: Vec<String>,
    /// Enable sandbox mode — file tools can only access paths inside the workspace directory.
    /// When false (default), file tools can access any path on the host.
    pub sandbox: bool,
}

fn default_safe_env_vars() -> Vec<String> {
    [
        "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TERM", "LANG", "LC_ALL", "LC_CTYPE", "PWD",
        "TMPDIR",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            root_dir: dirs_default_workspace_dir(),
            command_timeout_secs: 30,
            max_output_bytes: 1_048_576,
            safe_env_vars: default_safe_env_vars(),
            sandbox: false,
        }
    }
}

impl WorkspaceConfig {
    /// Per-session workspace directory: {root_dir}/{user_id}/{agent_id}/{session_id}
    pub fn session_dir(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
    ) -> std::path::PathBuf {
        std::path::PathBuf::from(&self.root_dir)
            .join(user_id)
            .join(agent_id)
            .join(session_id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BendClawConfig {
    /// Unique identifier for this bendclaw instance. Must match the AgentOS ID
    /// assigned by the console. Used to filter tasks that belong to this instance.
    /// Required when running alongside other bendclaw instances sharing the same DB.
    pub instance_id: String,
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub log: LogConfig,
    pub llm: LLMConfig,
    pub hub: Option<HubConfig>,
    pub workspace: WorkspaceConfig,
    pub auth: AuthConfig,
    /// Optional cluster configuration for distributed agent execution.
    /// When present, enables cluster registration and dispatch tools.
    pub cluster: Option<ClusterConfig>,
    /// Optional directive configuration for platform-driven agent behavior.
    /// When present, queries the evot-ai platform for directives injected into the system prompt.
    pub directive: Option<DirectiveConfig>,
    /// Optional admin API configuration.
    /// When present, starts a separate HTTP server for internal control-plane endpoints (no auth).
    pub admin: Option<AdminConfig>,
}

/// Admin API configuration for internal control-plane endpoints.
/// Listens on a separate port with no authentication — intended for internal network only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    /// Bind address for the admin API. Example: "127.0.0.1:8788"
    pub bind_addr: String,
}

/// Directive configuration for platform-driven agent behavior.
/// When configured, bendclaw queries the evot-ai platform for directives (e.g. resource warnings)
/// and injects them into the agent's system prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectiveConfig {
    /// evot-ai platform API base URL.
    pub api_base: String,
    /// evot-ai platform API token.
    pub token: String,
}

/// Cluster configuration for distributed agent execution.
/// Enables registration with the evot-ai platform and dispatch of subtasks to peer nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// Base URL of the cluster registry service (evot-ai platform).
    pub registry_url: String,
    /// API token for the cluster registry service.
    pub registry_token: String,
    /// Public base URL that other nodes use to reach this instance.
    /// Required — must be routable from peer nodes (not 127.0.0.1).
    /// Example: "https://node1.example.com:8787"
    #[serde(default)]
    pub advertise_url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    /// API key for Bearer token authentication. Empty = auth disabled.
    pub api_key: String,
    /// Allowed CORS origins when auth is enabled. Empty = use default whitelist.
    pub cors_origins: Vec<String>,
}

impl AuthConfig {
    pub fn is_enabled(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// Returns the CORS origins to use when auth is enabled.
    pub fn allowed_origins(&self) -> Vec<String> {
        if !self.cors_origins.is_empty() {
            return self.cors_origins.clone();
        }
        vec![
            "https://app.evot.ai".to_string(),
            "https://evot.ai".to_string(),
            "http://localhost:3000".to_string(),
            "http://localhost:3001".to_string(),
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub bind_addr: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:8787".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    /// Databend Cloud API base URL. Required.
    pub databend_api_base_url: String,
    /// Databend Cloud API token. Required.
    pub databend_api_token: String,
    /// Databend Cloud warehouse name. Default: "default"
    pub databend_warehouse: String,
    /// Prefix for agent databases. Default: "bendclaw_"
    pub db_prefix: String,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            databend_api_base_url: "https://api.databend.com/v1".to_string(),
            databend_api_token: String::new(),
            databend_warehouse: "default".to_string(),
            db_prefix: "bendclaw_".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    pub level: String,
    /// `"text"` (default) or `"json"`.
    pub format: String,
    /// Directory for log files. Empty = no file logging.
    pub dir: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        let dir = dirs_default_log_dir();
        Self {
            level: "info".to_string(),
            format: "text".to_string(),
            dir,
        }
    }
}

fn dirs_default_log_dir() -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let path = std::path::PathBuf::from(home)
            .join(EVOTAI_DIR_NAME)
            .join("logs");
        return path.to_string_lossy().into_owned();
    }
    String::new()
}

fn dirs_default_workspace_dir() -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let path = std::path::PathBuf::from(home)
            .join(EVOTAI_DIR_NAME)
            .join("workspace");
        return path.to_string_lossy().into_owned();
    }
    "./workspace".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HubConfig {
    /// Git repo URL for hub skills. Default: https://github.com/EvotAI/skills
    pub repo_url: String,
    /// Sync interval in seconds. Default: 86400 (24 hours).
    pub sync_interval_secs: u64,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            repo_url: "https://github.com/EvotAI/skills".to_string(),
            sync_interval_secs: 86400,
        }
    }
}

impl BendClawConfig {
    /// Load from a TOML file, then apply env-var overrides.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file: {path}"))?;
        let mut cfg: Self = toml::from_str(&content)
            .with_context(|| format!("failed to parse config file: {path}"))?;
        cfg.apply_env();
        Ok(cfg)
    }

    /// Apply `BENDCLAW_*` environment variable overrides.
    /// Env always takes precedence over file values.
    pub fn apply_env(&mut self) {
        override_str(
            &mut self.storage.databend_api_base_url,
            "BENDCLAW_STORAGE_DATABEND_API_BASE_URL",
        );
        override_str(
            &mut self.storage.databend_api_token,
            "BENDCLAW_STORAGE_DATABEND_API_TOKEN",
        );
        override_str(
            &mut self.storage.databend_warehouse,
            "BENDCLAW_STORAGE_DATABEND_WAREHOUSE",
        );
        override_str(&mut self.storage.db_prefix, "BENDCLAW_STORAGE_DB_PREFIX");
        override_str(&mut self.server.bind_addr, "BENDCLAW_SERVER_BIND_ADDR");
        override_str(&mut self.log.level, "BENDCLAW_LOG_LEVEL");
        override_str(&mut self.log.format, "BENDCLAW_LOG_FORMAT");
        override_str(&mut self.log.dir, "BENDCLAW_LOG_DIR");
        override_str(&mut self.workspace.root_dir, "BENDCLAW_WORKSPACE_ROOT_DIR");
        override_bool(&mut self.workspace.sandbox, "BENDCLAW_WORKSPACE_SANDBOX");
        override_str(&mut self.instance_id, "BENDCLAW_INSTANCE_ID");
        override_str(&mut self.auth.api_key, "BENDCLAW_AUTH_KEY");
        if let Ok(v) = std::env::var("BENDCLAW_AUTH_CORS_ORIGINS") {
            if !v.is_empty() {
                self.auth.cors_origins = v.split(',').map(|s| s.trim().to_string()).collect();
            }
        }
        // Hub config env overrides
        if let Some(hub) = self.hub.as_mut() {
            override_str(&mut hub.repo_url, "BENDCLAW_HUB_REPO_URL");
        }

        // Directive config env overrides
        if let Some(directive) = self.directive.as_mut() {
            override_str(&mut directive.api_base, "BENDCLAW_DIRECTIVE_API_BASE");
            override_str(&mut directive.token, "BENDCLAW_DIRECTIVE_TOKEN");
        } else {
            let api_base = std::env::var("BENDCLAW_DIRECTIVE_API_BASE").unwrap_or_default();
            let token = std::env::var("BENDCLAW_DIRECTIVE_TOKEN").unwrap_or_default();
            if !api_base.is_empty() && !token.is_empty() {
                self.directive = Some(DirectiveConfig { api_base, token });
            }
        }

        // Admin config env overrides
        if let Some(admin) = self.admin.as_mut() {
            override_str(&mut admin.bind_addr, "BENDCLAW_ADMIN_BIND_ADDR");
        } else {
            let bind_addr = std::env::var("BENDCLAW_ADMIN_BIND_ADDR").unwrap_or_default();
            if !bind_addr.is_empty() {
                self.admin = Some(AdminConfig { bind_addr });
            }
        }

        // Cluster config env overrides — create from env if both vars are set
        if let Some(cluster) = self.cluster.as_mut() {
            override_str(&mut cluster.registry_url, "BENDCLAW_CLUSTER_REGISTRY_URL");
            override_str(
                &mut cluster.registry_token,
                "BENDCLAW_CLUSTER_REGISTRY_TOKEN",
            );
            override_str(&mut cluster.advertise_url, "BENDCLAW_CLUSTER_ADVERTISE_URL");
        } else {
            let url = std::env::var("BENDCLAW_CLUSTER_REGISTRY_URL").unwrap_or_default();
            let token = std::env::var("BENDCLAW_CLUSTER_REGISTRY_TOKEN").unwrap_or_default();
            if !url.is_empty() && !token.is_empty() {
                self.cluster = Some(ClusterConfig {
                    registry_url: url,
                    registry_token: token,
                    advertise_url: std::env::var("BENDCLAW_CLUSTER_ADVERTISE_URL")
                        .unwrap_or_default(),
                });
            }
        }

        // LLM provider env overrides — single-provider shorthand
        let llm_name = std::env::var("BENDCLAW_LLM_NAME").unwrap_or_default();
        if !llm_name.is_empty() {
            use crate::llm::config::ProviderEndpoint;
            self.llm.providers = vec![ProviderEndpoint {
                name: llm_name,
                provider: std::env::var("BENDCLAW_LLM_PROVIDER").unwrap_or_default(),
                base_url: std::env::var("BENDCLAW_LLM_BASE_URL").unwrap_or_default(),
                api_key: std::env::var("BENDCLAW_LLM_API_KEY").unwrap_or_default(),
                model: std::env::var("BENDCLAW_LLM_MODEL").unwrap_or_default(),
                weight: std::env::var("BENDCLAW_LLM_WEIGHT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(100),
                temperature: std::env::var("BENDCLAW_LLM_TEMPERATURE")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.7),
                input_price: 0.0,
                output_price: 0.0,
            }];
        }
    }

    /// Apply CLI argument overrides — highest priority, beats file and env.
    pub fn apply_cli(&mut self, cli: &crate::cli::CliOverrides) {
        if let Some(v) = &cli.storage_api_base_url {
            self.storage.databend_api_base_url = v.clone();
        }
        if let Some(v) = &cli.storage_api_token {
            self.storage.databend_api_token = v.clone();
        }
        if let Some(v) = &cli.storage_warehouse {
            self.storage.databend_warehouse = v.clone();
        }
        if let Some(v) = &cli.bind_addr {
            self.server.bind_addr = v.clone();
        }
        if let Some(v) = &cli.log_level {
            self.log.level = v.clone();
        }
        if let Some(v) = &cli.log_format {
            self.log.format = v.clone();
        }
        if let Some(v) = &cli.auth_key {
            self.auth.api_key = v.clone();
        }
    }

    /// Log the full config with sensitive fields redacted.
    pub fn log_non_defaults(&self) {
        match serde_json::to_value(self) {
            Ok(v) => {
                let redacted = crate::observability::redaction::redact(v);
                tracing::info!("config: {redacted}");
            }
            Err(e) => {
                tracing::warn!("failed to serialize config: {e}");
            }
        }
    }

    /// Validate that all required fields are present. Call after all layers applied.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.storage.databend_api_base_url.is_empty() {
            anyhow::bail!(
                "missing required configuration:\n  \
                 storage.databend_api_base_url  →  set BENDCLAW_STORAGE_DATABEND_API_BASE_URL \
                 or [storage] databend_api_base_url in config file"
            );
        }
        if self.storage.databend_api_token.is_empty() {
            anyhow::bail!(
                "missing required configuration:\n  \
                 storage.databend_api_token  →  set BENDCLAW_STORAGE_DATABEND_API_TOKEN \
                 or [storage] databend_api_token in config file"
            );
        }
        if self.instance_id.is_empty() {
            anyhow::bail!(
                "missing required configuration:\n  \
                 instance_id  →  set BENDCLAW_INSTANCE_ID \
                 or instance_id in config file"
            );
        }
        if let Some(ref directive) = self.directive {
            if directive.api_base.is_empty() || directive.token.is_empty() {
                anyhow::bail!(
                    "invalid directive configuration:\n  \
                     directive.api_base and directive.token must both be set when [directive] is present"
                );
            }
        }
        if let Some(ref admin) = self.admin {
            if admin.bind_addr.is_empty() {
                anyhow::bail!(
                    "invalid admin configuration:\n  \
                     admin.bind_addr must be set when [admin] is present"
                );
            }
        }
        if let Some(ref cluster) = self.cluster {
            if cluster.advertise_url.is_empty() {
                anyhow::bail!(
                    "missing required configuration:\n  \
                     cluster.advertise_url  →  set BENDCLAW_CLUSTER_ADVERTISE_URL \
                     or [cluster] advertise_url in config file.\n  \
                     This must be a URL reachable by peer nodes (not 127.0.0.1)."
                );
            }
        }
        Ok(())
    }
}

/// Override `field` with the env var value. Env always wins over file.
fn override_str(field: &mut String, env_var: &str) {
    if let Ok(v) = std::env::var(env_var) {
        if !v.is_empty() {
            *field = v;
        }
    }
}

/// Override a bool `field` with the env var value ("true"/"1" = true, "false"/"0" = false).
fn override_bool(field: &mut bool, env_var: &str) {
    if let Ok(v) = std::env::var(env_var) {
        match v.as_str() {
            "true" | "1" => *field = true,
            "false" | "0" => *field = false,
            _ => {}
        }
    }
}
