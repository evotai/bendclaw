//! Streaming ordered-list numbering state.
//!
//! The streamdown parser may emit `Ordered(1)` for every item when the source
//! markdown uses the common `1. / 1. / 1.` pattern.  This module tracks
//! nesting levels and assigns correct sequential numbers so the terminal
//! output always shows `1. 2. 3.` regardless of what the parser reports.

use streamdown_parser::ParseEvent;

/// Per-level entry: (indent, is_ordered).
#[derive(Debug)]
struct Level {
    indent: usize,
    _ordered: bool,
}

/// Tracks ordered-list numbering across streaming parse events.
#[derive(Debug, Default)]
pub struct ListState {
    /// Stack of nesting levels.
    stack: Vec<Level>,
    /// Current number at each level (parallel to `stack`).
    numbers: Vec<usize>,
    /// `true` after a `ListEnd` — deferred so a continuation item can resume.
    pending_reset: bool,
}

impl ListState {
    /// Adjust the stack for the current item's indent and type, then return
    /// the next sequential number for ordered items.
    pub fn next_number(&mut self, indent: usize, ordered: bool) -> usize {
        self.resume_if_pending();
        self.adjust_for_indent(indent, ordered);

        if let Some(n) = self.numbers.last_mut() {
            *n += 1;
            *n
        } else {
            1
        }
    }

    /// Mark the list as "maybe finished" — a subsequent `ListItem` will
    /// resume, anything else will reset.
    pub fn mark_pending_reset(&mut self) {
        self.pending_reset = true;
    }

    /// Reset the entire state (called when a non-list event breaks context).
    pub fn reset(&mut self) {
        self.stack.clear();
        self.numbers.clear();
        self.pending_reset = false;
    }

    /// Returns `true` if `event` should trigger a full reset of pending state.
    pub fn should_reset(event: &ParseEvent) -> bool {
        !matches!(
            event,
            ParseEvent::ListItem { .. }
                | ParseEvent::ListEnd
                | ParseEvent::EmptyLine
                | ParseEvent::Newline
        )
    }

    // -- private helpers ----------------------------------------------------

    fn resume_if_pending(&mut self) {
        self.pending_reset = false;
    }

    fn adjust_for_indent(&mut self, indent: usize, ordered: bool) {
        // Pop levels deeper than current indent.
        while let Some(top) = self.stack.last() {
            if top.indent > indent {
                self.stack.pop();
                self.numbers.pop();
            } else {
                break;
            }
        }

        // Push a new level if needed.
        let need_push = self
            .stack
            .last()
            .map(|top| indent > top.indent)
            .unwrap_or(true);

        if need_push {
            self.stack.push(Level {
                indent,
                _ordered: ordered,
            });
            self.numbers.push(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_numbers_for_flat_list() {
        let mut state = ListState::default();
        assert_eq!(state.next_number(0, true), 1);
        assert_eq!(state.next_number(0, true), 2);
        assert_eq!(state.next_number(0, true), 3);
    }

    #[test]
    fn nested_lists_have_independent_counters() {
        let mut state = ListState::default();
        assert_eq!(state.next_number(0, true), 1);
        // Nested child
        assert_eq!(state.next_number(1, true), 1);
        assert_eq!(state.next_number(1, true), 2);
        // Back to parent
        assert_eq!(state.next_number(0, true), 2);
    }

    #[test]
    fn reset_restarts_numbering() {
        let mut state = ListState::default();
        assert_eq!(state.next_number(0, true), 1);
        assert_eq!(state.next_number(0, true), 2);
        state.reset();
        assert_eq!(state.next_number(0, true), 1);
    }

    #[test]
    fn pending_reset_resumes_on_list_item() {
        let mut state = ListState::default();
        assert_eq!(state.next_number(0, true), 1);
        state.mark_pending_reset();
        // Next item resumes instead of resetting
        assert_eq!(state.next_number(0, true), 2);
    }

    #[test]
    fn pending_reset_clears_on_non_list_event() {
        let mut state = ListState::default();
        assert_eq!(state.next_number(0, true), 1);
        state.mark_pending_reset();
        // A heading breaks the list context
        assert!(ListState::should_reset(&ParseEvent::Heading {
            level: 1,
            content: "title".into(),
        }));
    }

    #[test]
    fn unordered_does_not_affect_ordered_counter() {
        let mut state = ListState::default();
        assert_eq!(state.next_number(0, true), 1);
        // Unordered child at deeper indent
        let _ = state.next_number(1, false);
        // Parent ordered continues
        assert_eq!(state.next_number(0, true), 2);
    }
}
