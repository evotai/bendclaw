pub mod app;
pub mod channels;
pub mod cli;
pub mod client;
pub mod config;
pub mod kernel;
pub mod llm;
pub mod observability;
pub mod service;
pub mod sessions;
pub mod skills;
pub mod storage;
pub mod tasks;
pub mod tools;
pub mod tracing_fmt;
pub mod types;
pub mod version;

// Pipeline contracts (Phase 0 — canonical boundaries)
pub mod binding;
pub mod execution;
pub mod planning;
pub mod request;
pub mod result;
