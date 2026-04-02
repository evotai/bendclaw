//! Binding: resolve a request into a fully-assembled run context.
//!
//! Takes an `AgentRequest` and binds agent config, session, tools, skills,
//! memory, and workspace into a single `RunBinding` that the planner can consume.
//!
//! Pipeline position: **second stage** — consumes `request/`, feeds `planning/`.

use async_trait::async_trait;

use crate::types::Result;

/// Canonical contract: bind a request into a run-ready context.
///
/// Implementations resolve agent config, acquire/create a session,
/// assemble the toolset and skill set, and produce a `RunBinding`
/// that carries everything the planner needs.
#[async_trait]
pub trait RunBinder: Send + Sync {
    type Request;
    type Binding;

    async fn bind(&self, request: Self::Request) -> Result<Self::Binding>;
}
