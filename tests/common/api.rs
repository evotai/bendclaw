#![cfg_attr(not(feature = "live-tests"), allow(dead_code))]

use axum::body::Body;
use axum::http::Method;
use axum::http::Request;
use axum::http::StatusCode;
use serde_json::Value;
use tower::ServiceExt;

use crate::common::assertions::data_array;
use crate::common::setup::json_body;

/// Small HTTP helper for live API tests.
pub struct TestApi {
    app: axum::Router,
}

impl TestApi {
    pub fn new(app: axum::Router) -> Self {
        Self { app }
    }

    async fn request(
        &self,
        method: Method,
        uri: String,
        user: &str,
        body: Option<Value>,
    ) -> anyhow::Result<axum::response::Response> {
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("x-user-id", user);
        let req = match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body)?))?,
            None => builder.body(Body::empty())?,
        };
        Ok(self.app.clone().oneshot(req).await?)
    }

    async fn request_json(
        &self,
        method: Method,
        uri: String,
        user: &str,
        body: Option<Value>,
    ) -> anyhow::Result<Value> {
        let resp = self.request(method, uri, user, body).await?;
        let status = resp.status();
        let body = json_body(resp).await?;
        if status != StatusCode::OK {
            anyhow::bail!("request failed: status={status}, body={body}");
        }
        Ok(body)
    }

    pub async fn setup_agent(&self, agent_id: &str, user: &str) -> anyhow::Result<()> {
        let resp = self
            .request(
                Method::POST,
                format!("/v1/agents/{agent_id}/setup"),
                user,
                None,
            )
            .await?;
        let status = resp.status();
        if status != StatusCode::OK {
            let body = json_body(resp).await?;
            anyhow::bail!("setup failed: status={status}, body={body}");
        }
        Ok(())
    }

    pub async fn chat(
        &self,
        agent_id: &str,
        session_id: &str,
        user: &str,
        message: &str,
    ) -> anyhow::Result<Value> {
        let run = self
            .request_json(
                Method::POST,
                format!("/v1/agents/{agent_id}/runs"),
                user,
                Some(serde_json::json!({
                    "session_id": session_id,
                    "input": message,
                    "stream": false,
                })),
            )
            .await?;
        Ok(serde_json::json!({
            "ok": true,
            "message": run["output"],
            "run": run,
        }))
    }

    pub async fn get_runs(
        &self,
        agent_id: &str,
        session_id: &str,
        user: &str,
    ) -> anyhow::Result<Vec<Value>> {
        let body = self
            .request_json(
                Method::GET,
                format!("/v1/agents/{agent_id}/sessions/{session_id}/runs"),
                user,
                None,
            )
            .await?;
        Ok(data_array(&body)?.clone())
    }

    pub async fn get_run_detail(
        &self,
        agent_id: &str,
        run_id: &str,
        user: &str,
    ) -> anyhow::Result<Value> {
        self.request_json(
            Method::GET,
            format!("/v1/agents/{agent_id}/runs/{run_id}"),
            user,
            None,
        )
        .await
    }

    pub async fn create_variable(
        &self,
        agent_id: &str,
        user: &str,
        body: Value,
    ) -> anyhow::Result<Value> {
        self.request_json(
            Method::POST,
            format!("/v1/agents/{agent_id}/variables"),
            user,
            Some(body),
        )
        .await
    }

    pub async fn get_variable(
        &self,
        agent_id: &str,
        user: &str,
        var_id: &str,
    ) -> anyhow::Result<Value> {
        self.request_json(
            Method::GET,
            format!("/v1/agents/{agent_id}/variables/{var_id}"),
            user,
            None,
        )
        .await
    }

    pub async fn create_skill(
        &self,
        agent_id: &str,
        user: &str,
        body: Value,
    ) -> anyhow::Result<Value> {
        self.request_json(
            Method::POST,
            format!("/v1/agents/{agent_id}/skills"),
            user,
            Some(body),
        )
        .await
    }
}
