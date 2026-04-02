use std::backtrace::Backtrace;
use std::fmt;

pub struct ErrorCode {
    pub code: u16,
    pub name: &'static str,
    pub message: String,
    pub stacks: Vec<String>,
    pub backtrace: Backtrace,
}

impl ErrorCode {
    pub fn new(code: u16, name: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            name,
            message: message.into(),
            stacks: Vec::new(),
            backtrace: Backtrace::capture(),
        }
    }

    #[must_use]
    #[track_caller]
    pub fn with_context(mut self, ctx: impl FnOnce() -> String) -> Self {
        self.stacks.push(ctx());
        self
    }

    #[must_use]
    pub fn add_message(mut self, msg: impl Into<String>) -> Self {
        let prefix = msg.into();
        self.message = format!("{prefix}: {}", self.message);
        self
    }

    #[must_use]
    pub fn add_message_back(mut self, msg: impl Into<String>) -> Self {
        let suffix = msg.into();
        self.message = format!("{}: {suffix}", self.message);
        self
    }

    pub fn http_status(&self) -> u16 {
        match self.code {
            Self::NOT_FOUND => 404,
            Self::AUTH_REQUEST..=Self::AUTH_CREDENTIALS => 401,
            Self::AUTH_TOKEN_EXPIRED | Self::AUTH_PARSE => 401,
            Self::DENIED => 403,
            Self::QUOTA_EXCEEDED => 429,
            Self::TIMEOUT | Self::SKILL_TIMEOUT => 408,
            _ => 500,
        }
    }
}

impl fmt::Debug for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ErrorCode")
            .field("code", &self.code)
            .field("name", &self.name)
            .field("message", &self.message)
            .field("stacks", &self.stacks)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.name, self.message)?;
        for ctx in &self.stacks {
            write!(f, "\n  caused by: {ctx}")?;
        }
        let bt = self.backtrace.to_string();
        if !bt.is_empty() && !bt.contains("disabled") {
            write!(f, "\n\nBacktrace:\n{bt}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ErrorCode {}

macro_rules! build_agent_errors {
    ($( ($code:expr, $name:ident, $const_name:ident, $label:expr) ),* $(,)?) => {
        impl ErrorCode {
            $(
                pub const $const_name: u16 = $code;
                #[track_caller]
                pub fn $name(message: impl Into<String>) -> Self {
                    Self::new($code, $label, message)
                }
            )*
        }
    };
}

build_agent_errors!(
    (1001, internal, INTERNAL, "Internal"),
    (1002, config, CONFIG, "Config"),
    (1003, invalid_input, INVALID_INPUT, "InvalidInput"),
    (1004, not_found, NOT_FOUND, "NotFound"),
    (1005, timeout, TIMEOUT, "Timeout"),
    (
        1100,
        storage_connection,
        STORAGE_CONNECTION,
        "StorageConnection"
    ),
    (1101, storage_exec, STORAGE_EXEC, "StorageExec"),
    (1102, storage_query, STORAGE_QUERY, "StorageQuery"),
    (
        1103,
        storage_migration,
        STORAGE_MIGRATION,
        "StorageMigration"
    ),
    (1104, storage_serde, STORAGE_SERDE, "StorageSerialization"),
    (1105, storage_gateway, STORAGE_GATEWAY, "StorageGateway"),
    (1200, llm_request, LLM_REQUEST, "LlmRequest"),
    (1201, llm_response, LLM_RESPONSE, "LlmResponse"),
    (1202, llm_rate_limit, LLM_RATE_LIMIT, "LlmRateLimit"),
    (1203, llm_server, LLM_SERVER, "LlmServerError"),
    (1204, llm_parse, LLM_PARSE, "LlmParse"),
    (
        1205,
        llm_context_overflow,
        LLM_CONTEXT_OVERFLOW,
        "LlmContextOverflow"
    ),
    (1300, auth_request, AUTH_REQUEST, "AuthRequest"),
    (1301, auth_credentials, AUTH_CREDENTIALS, "AuthCredentials"),
    (
        1302,
        auth_token_expired,
        AUTH_TOKEN_EXPIRED,
        "AuthTokenExpired"
    ),
    (1303, auth_parse, AUTH_PARSE, "AuthParse"),
    (1400, denied, DENIED, "Denied"),
    (1401, quota_exceeded, QUOTA_EXCEEDED, "QuotaExceeded"),
    (1500, skill_not_found, SKILL_NOT_FOUND, "SkillNotFound"),
    (1501, skill_exec, SKILL_EXEC, "SkillExec"),
    (1502, skill_timeout, SKILL_TIMEOUT, "SkillTimeout"),
    (1503, skill_serde, SKILL_SERDE, "SkillSerialization"),
    (1504, skill_validation, SKILL_VALIDATION, "SkillValidation"),
    (
        1505,
        skill_requirements,
        SKILL_REQUIREMENTS,
        "SkillRequirements"
    ),
    (1600, sandbox, SANDBOX, "Sandbox"),
    (
        1700,
        cluster_registration,
        CLUSTER_REGISTRATION,
        "ClusterRegistration"
    ),
    (
        1701,
        cluster_discovery,
        CLUSTER_DISCOVERY,
        "ClusterDiscovery"
    ),
    (1702, cluster_dispatch, CLUSTER_DISPATCH, "ClusterDispatch"),
    (1703, cluster_collect, CLUSTER_COLLECT, "ClusterCollect"),
    (1800, channel_send, CHANNEL_SEND, "ChannelSend"),
    (
        1801,
        channel_rate_limited,
        CHANNEL_RATE_LIMITED,
        "ChannelRateLimited"
    ),
    (1802, channel_timeout, CHANNEL_TIMEOUT, "ChannelTimeout"),
);

macro_rules! impl_from_for_error_code {
    ($($t:ty),*) => {
        $(impl From<$t> for ErrorCode {
            fn from(e: $t) -> Self { ErrorCode::internal(e.to_string()) }
        })*
    };
}

impl_from_for_error_code!(
    serde_json::Error,
    std::io::Error,
    anyhow::Error,
    std::num::ParseIntError,
    std::num::ParseFloatError
);

pub type Result<T> = std::result::Result<T, ErrorCode>;

pub trait ResultExt<T> {
    fn with_context(self, ctx: impl FnOnce() -> String) -> Result<T>;
}

impl<T> ResultExt<T> for Result<T> {
    #[track_caller]
    fn with_context(self, ctx: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|e| e.with_context(ctx))
    }
}

pub trait OptionExt<T> {
    fn ok_or_not_found(self, msg: impl FnOnce() -> String) -> Result<T>;
    fn ok_or_error(self, f: impl FnOnce() -> ErrorCode) -> Result<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_or_not_found(self, msg: impl FnOnce() -> String) -> Result<T> {
        self.ok_or_else(|| ErrorCode::not_found(msg()))
    }

    fn ok_or_error(self, f: impl FnOnce() -> ErrorCode) -> Result<T> {
        self.ok_or_else(f)
    }
}
