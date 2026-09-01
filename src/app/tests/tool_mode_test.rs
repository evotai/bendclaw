//! Tests for `ToolMode` capability policy.

use evot::agent::ToolMode;

/// Interactive and planning runs are human-in-the-loop: a person is watching
/// and can steer or interrupt, so they impose no execution limits (pi parity).
/// A long build, a slow training run, or a slow human reply must never
/// terminate the agent.
#[test]
fn interactive_modes_are_human_in_the_loop() {
    assert!(ToolMode::Interactive.is_interactive());
    assert!(ToolMode::Planning.is_interactive());
}

/// Autonomous modes keep their execution limits as a runaway-cost safety net,
/// since no human is watching to interrupt a spinning loop.
#[test]
fn autonomous_modes_keep_limits() {
    assert!(!ToolMode::Headless.is_interactive());
    assert!(!ToolMode::Readonly.is_interactive());
}

#[test]
fn background_processes_are_tui_only() {
    assert!(ToolMode::Interactive.allows_background_processes());
    assert!(ToolMode::Planning.allows_background_processes());
    assert!(!ToolMode::Headless.allows_background_processes());
    assert!(!ToolMode::Readonly.allows_background_processes());
}

/// Host-owned tools attach in every mode except Readonly forks, which run
/// without a host.
#[test]
fn only_readonly_forbids_host_tools() {
    assert!(ToolMode::Interactive.allows_host_tools());
    assert!(ToolMode::Headless.allows_host_tools());
    assert!(ToolMode::Planning.allows_host_tools());
    assert!(!ToolMode::Readonly.allows_host_tools());
}
