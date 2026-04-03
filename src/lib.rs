// Foundation
pub mod config;
pub mod llm;
pub mod storage;
pub mod types;

// Pipeline: request → binding → planning → execution → result
pub mod binding;
pub mod execution;
pub mod planning;
pub mod request;
pub mod result;

// Domain modules
pub mod agent_store;
pub mod channels;
pub mod cluster;
pub mod directive;
pub mod lease;
pub mod memory;
pub mod sessions;
pub mod skills;
pub mod subscriptions;
pub mod tasks;
pub mod tools;
pub mod traces;
pub mod variables;
pub mod workbench;
pub mod writer;

// Runtime + API
pub mod cli;
pub mod client;
pub mod observability;
pub mod runtime;
pub mod server;
pub mod tracing_fmt;
pub mod version;
