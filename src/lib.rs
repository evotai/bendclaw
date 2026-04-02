pub mod app;
pub mod base;
pub mod cli;
pub mod client;
pub mod config;
pub mod kernel;
pub mod llm;
pub mod observability;
pub mod service;
pub mod storage;
pub mod tracing_fmt;
pub mod version;

// Pipeline contracts (Phase 0 — canonical boundaries)
pub mod binding;
pub mod execution;
pub mod planning;
pub mod request;
pub mod result;
