//! Capability level for a run's built-in tool set.
//!
//! `ToolMode` is pure policy: it decides which *engine-owned* tools are active
//! and how the system prompt frames the turn. Host-owned tools (ask_user,
//! plan, and any future domain tool) are injected orthogonally via
//! [`super::HostTools`], so this enum carries no callbacks and stays trivially
//! cloneable and inspectable.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolMode {
    /// REPL interactive: full built-in tools.
    Interactive,
    /// Oneshot / API / headless: full built-in tools.
    Headless,
    /// Plan mode: write tools degraded.
    Planning,
    /// Forked conversation: read-only.
    Readonly,
}

impl ToolMode {
    /// Whether host-owned tools may be attached in this mode. Readonly forks
    /// run without a host, so they never carry host tools.
    pub fn allows_host_tools(self) -> bool {
        !matches!(self, ToolMode::Readonly)
    }

    /// Whether the run is human-in-the-loop (a person is watching and can steer
    /// or interrupt). Interactive sessions impose no execution limits — like
    /// pi, the loop stops only on error, abort, or when there is no more work.
    /// A long build, a slow training run, or a slow human reply must never
    /// terminate the agent. Autonomous modes (Headless, Readonly) keep their
    /// limits as a runaway-cost safety net.
    pub fn is_interactive(self) -> bool {
        matches!(self, ToolMode::Interactive | ToolMode::Planning)
    }

    /// Background shell tasks are a TUI-only capability. Gateway/headless
    /// runs keep Bash synchronous even if a caller supplies background fields.
    pub fn allows_background_processes(self) -> bool {
        matches!(self, ToolMode::Interactive | ToolMode::Planning)
    }
}
