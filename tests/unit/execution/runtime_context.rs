use bendclaw::execution::runtime_context::build_runtime_context;

#[test]
fn runtime_context_includes_time_and_platform() {
    let ctx = build_runtime_context(None, None, None);
    assert!(ctx.contains("Current Time:"));
    assert!(ctx.contains("Platform:"));
    assert!(!ctx.contains("Channel:"));
}

#[test]
fn runtime_context_includes_channel_when_provided() {
    let ctx = build_runtime_context(Some("feishu"), Some("oc_abc123"), None);
    assert!(ctx.contains("Channel: feishu (chat: oc_abc123)"));
}

#[test]
fn runtime_context_skips_empty_channel() {
    let ctx = build_runtime_context(Some(""), None, None);
    assert!(!ctx.contains("Channel:"));
}

#[test]
fn runtime_context_includes_working_directory() {
    let ctx = build_runtime_context(None, None, Some(std::path::Path::new("/home/user")));
    assert!(ctx.contains("Working directory: /home/user"));
}
