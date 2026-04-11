//! Shared HTTP client factory with default user-agent.

use crate::provider::error::ProviderError;

const USER_AGENT: &str = "bendclaw/0.1.0";

/// Create a `reqwest::Client` with the default bendclaw user-agent.
pub fn new_client() -> Result<reqwest::Client, ProviderError> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| ProviderError::Other(format!("Failed to build HTTP client: {e}")))
}
