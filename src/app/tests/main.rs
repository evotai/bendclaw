#[path = "agent_prompt_test.rs"]
mod agent_prompt_test;
#[path = "agent_thinking_test.rs"]
mod agent_thinking_test;
#[path = "agent_variable_test.rs"]
mod agent_variable_test;
#[path = "auth_test.rs"]
mod auth_test;
#[path = "auto_compact_test.rs"]
mod auto_compact_test;
#[path = "background_reclaim_test.rs"]
mod background_reclaim_test;
#[path = "command_test.rs"]
mod command_test;
#[path = "compact_test.rs"]
mod compact_test;
#[path = "conf_load_test.rs"]
mod conf_load_test;
#[path = "console_routes_test.rs"]
mod console_routes_test;
#[path = "dashboard_search_test.rs"]
mod dashboard_search_test;
#[path = "delivery_test.rs"]
mod delivery_test;
#[path = "feishu_message_test.rs"]
mod feishu_message_test;
#[path = "feishu_sink_test.rs"]
mod feishu_sink_test;
#[path = "id_validation_test.rs"]
mod id_validation_test;
#[path = "manual_compact_llm_test.rs"]
mod manual_compact_llm_test;
#[path = "model_metadata_test.rs"]
mod model_metadata_test;
#[path = "orchestrator_compact_test.rs"]
mod orchestrator_compact_test;
#[path = "resume_search_test.rs"]
mod resume_search_test;
#[path = "run_ask_channel_test.rs"]
mod run_ask_channel_test;
#[path = "run_queue_test.rs"]
mod run_queue_test;
#[path = "schema_compat_test.rs"]
mod schema_compat_test;
#[path = "search_test.rs"]
mod search_test;
#[path = "server_protocol_test.rs"]
mod server_protocol_test;
#[path = "session_locator_test.rs"]
mod session_locator_test;
#[path = "session_observability_test.rs"]
mod session_observability_test;
#[path = "session_test.rs"]
mod session_test;
#[path = "settings_test.rs"]
mod settings_test;
#[path = "skill_loader_test.rs"]
mod skill_loader_test;
#[path = "skill_prompt_test.rs"]
mod skill_prompt_test;
#[path = "storage_memory_test.rs"]
mod storage_memory_test;
#[path = "storage_test.rs"]
mod storage_test;
#[path = "tool_mode_test.rs"]
mod tool_mode_test;
#[path = "types_transcript_stats_test.rs"]
mod types_transcript_stats_test;

/// Ensure every `*_test.rs` file in this directory is listed as a module above.
/// Fails at test-time (not compile-time) but catches forgotten additions in CI.
#[test]
fn all_test_files_included() {
    let test_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut missing = Vec::new();
    let main_src = include_str!("main.rs");
    for entry in std::fs::read_dir(&test_dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with("_test.rs") {
            let mod_name = name.trim_end_matches(".rs");
            if !main_src.contains(mod_name) {
                missing.push(name);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "Test files not included in tests/main.rs: {:?}\nAdd them as modules.",
        missing
    );
}
