//! Request: inbound request normalization.
//!
//! Standardizes CLI / HTTP / channel inputs into a common request model.
//! This module owns `AgentRequest` and `OutputFormat` — the canonical
//! entry point for the pipeline.
//!
//! Pipeline position: **first stage** — feeds into `binding/`.
