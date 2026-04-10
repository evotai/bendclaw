/// Tracks consecutive Ctrl+C presses on an empty input line.
///
/// - Ctrl+C while the line has content → clear the line, reset counter.
/// - Ctrl+C on an empty line (first time) → `Action::ShowHint` (display a
///   timed "press Ctrl+C again to exit" hint).
/// - Ctrl+C on an empty line (second consecutive) → `Action::Exit`.
/// - Hint timeout → resets the counter (the 1-second window expired).
/// - Ctrl+C during the hint window → `Action::Exit`.
/// - Any normal input resets the counter.
#[derive(Debug, Default)]
pub struct InterruptHandler {
    pending: bool,
}

/// What the REPL loop should do after a Ctrl+C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Clear the line and keep the loop running (line had content).
    Clear,
    /// Clear the line and show a timed exit hint.
    ShowHint,
    /// Exit the REPL.
    Exit,
}

impl InterruptHandler {
    pub fn new() -> Self {
        Self { pending: false }
    }

    /// Call when the user presses Ctrl+C.
    ///
    /// `line_empty` indicates whether the input line was empty at the time
    /// of the interrupt. When the line has content, the interrupt just clears
    /// it and resets the exit counter.
    pub fn on_interrupt(&mut self, line_empty: bool) -> Action {
        if !line_empty {
            self.pending = false;
            return Action::Clear;
        }
        if self.pending {
            self.pending = false;
            Action::Exit
        } else {
            self.pending = true;
            Action::ShowHint
        }
    }

    /// Call when the user provides normal input (resets the counter).
    pub fn on_input(&mut self) {
        self.pending = false;
    }

    /// Call when the hint's 1-second window expires without a second Ctrl+C.
    /// Resets the counter so the next Ctrl+C starts over with a new hint.
    pub fn on_hint_timeout(&mut self) {
        self.pending = false;
    }

    /// Call when the user presses Ctrl+C during the hint window.
    /// Returns `Exit` and resets internal state.
    pub fn on_hint_ctrl_c(&mut self) -> Action {
        self.pending = false;
        Action::Exit
    }
}
