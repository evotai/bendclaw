use std::time::Duration;

use serde::Deserialize;

use crate::client::http_adapter;
use crate::types::http;
use crate::types::ErrorCode;
use crate::types::Result;

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
        let resp = http::send(
            self.client.get(&url).bearer_auth(&self.token),
            http::HttpRequestContext::new("client", "directive_get")
                .with_endpoint("directive")
                .with_url(url.clone()),
        )
        .await
        .map_err(|err| http_adapter::to_internal("directive_get", err))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = http::read_text(
                resp,
                http::HttpRequestContext::new("client", "directive_read_error_body")
                    .with_endpoint("directive")
                    .with_url(url.clone()),
            )
            .await
            .unwrap_or_default();
            return Err(ErrorCode::internal(format!(
                "directive failed: HTTP {status}: {text}"
            )));
        }

        let body: DirectiveResponse = http::read_json(
            resp,
            http::HttpRequestContext::new("client", "directive_decode")
                .with_endpoint("directive")
                .with_url(url.clone()),
        )
        .await
        .map_err(|err| http_adapter::to_internal("directive_decode", err))?;

        if body.prompt.is_empty() {
            Ok(None)
        } else {
            Ok(Some(body.prompt))
        }
    }
}
