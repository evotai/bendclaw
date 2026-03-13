use std::time::Duration;

use serde::Deserialize;

use crate::base::ErrorCode;
use crate::base::Result;

/// Client for the evot-ai directive API.
/// Fetches platform-driven directives (e.g. resource warnings) to inject into agent prompts.
pub struct DirectiveClient {
    client: reqwest::Client,
    api_base: String,
    token: String,
}

#[derive(Deserialize)]
struct DirectiveResponse {
    #[serde(default)]
    prompt: String,
}

impl DirectiveClient {
    pub fn new(api_base: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| ErrorCode::internal(format!("failed to build directive client: {e}")))?;
        Ok(Self {
            client,
            api_base: api_base.into().trim_end_matches('/').to_string(),
            token: token.into(),
        })
    }

    /// Fetch the current directive prompt from the platform.
    /// Returns `None` if the platform returns an empty prompt.
    pub async fn get_directive(&self) -> Result<Option<String>> {
        let url = format!("{}/v1/directive", self.api_base);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| ErrorCode::internal(format!("directive request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ErrorCode::internal(format!(
                "directive failed: HTTP {status}: {text}"
            )));
        }

        let body: DirectiveResponse = resp
            .json()
            .await
            .map_err(|e| ErrorCode::internal(format!("directive parse failed: {e}")))?;

        if body.prompt.is_empty() {
            Ok(None)
        } else {
            Ok(Some(body.prompt))
        }
    }
}
