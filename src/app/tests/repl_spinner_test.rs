use bendclaw::cli::repl::spinner::SpinnerState;

#[test]
fn new_spinner_is_inactive() {
    let state = SpinnerState::new();
    assert!(!state.is_active());
    assert!(state.phase().is_hidden());
}

#[test]
fn activate_sets_verb_phase() {
    let mut state = SpinnerState::new();
    state.activate();
    assert!(state.is_active());
    assert!(state.phase().is_verb());
    assert_eq!(state.frame_index(), 0);
}

#[test]
fn phase_transitions() {
    let mut state = SpinnerState::new();
    state.activate();

    state.set_tool("bash");
    assert!(state.phase().is_tool());

    state.restore_verb();
    assert!(state.phase().is_verb());

    state.deactivate();
    assert!(!state.is_active());
    assert!(state.phase().is_hidden());
}

#[test]
fn glyph_cycles_correctly() {
    let mut state = SpinnerState::new();
    state.activate();

    let count = SpinnerState::glyph_count();
    let first = state.current_glyph().to_string();

    // Advance through all frames
    for _ in 0..count {
        state.render_frame();
    }

    // Should wrap back to the first glyph
    assert_eq!(state.current_glyph(), first);
    assert_eq!(state.frame_index(), count);
}

#[test]
fn add_tokens_accumulates() {
    let mut state = SpinnerState::new();
    state.activate();
    state.add_tokens(100);
    state.add_tokens(50);
    // No public getter for response_tokens, but this should not panic
}

#[test]
fn render_frame_does_nothing_when_inactive() {
    let mut state = SpinnerState::new();
    state.activate();
    state.deactivate();
    state.render_frame();
    // frame should not advance when inactive
    assert_eq!(state.frame_index(), 0);
}

#[test]
fn spinner_stays_active_through_tool_cycle() {
    let mut state = SpinnerState::new();
    state.activate();

    // Simulate ToolStarted -> ToolFinished -> restore_verb
    state.set_tool("bash");
    assert!(state.phase().is_tool());

    state.clear_if_rendered();
    state.restore_verb();
    assert!(state.phase().is_verb());
    assert!(state.is_active());

    // Spinner should still render after tool cycle
    state.render_frame();
    assert_eq!(state.frame_index(), 1);
}

#[test]
fn spinner_renders_continuously_while_active() {
    let mut state = SpinnerState::new();
    state.activate();

    // Render a few frames
    state.render_frame();
    state.render_frame();
    assert_eq!(state.frame_index(), 2);

    // clear_if_rendered does not stop rendering on next tick
    state.clear_if_rendered();
    state.render_frame();
    assert_eq!(state.frame_index(), 3);
}

#[test]
fn spinner_throttles_during_active_streaming() {
    let mut state = SpinnerState::new();
    state.activate();

    // Simulate tokens arriving (makes it "streaming")
    state.add_tokens(10);

    // With STREAMING_FRAME_DIVISOR=4, only every 4th tick advances the frame.
    // tick 1 → skip, tick 2 → skip, tick 3 → skip, tick 4 → render (frame 1)
    state.render_frame(); // tick 1
    state.render_frame(); // tick 2
    state.render_frame(); // tick 3
    assert_eq!(state.frame_index(), 0); // no frame advanced yet

    state.render_frame(); // tick 4 → advances
    assert_eq!(state.frame_index(), 1);

    // Next batch: ticks 5-7 skip, tick 8 renders
    state.add_tokens(5); // keep it "streaming"
    state.render_frame(); // tick 5
    state.render_frame(); // tick 6
    state.render_frame(); // tick 7
    assert_eq!(state.frame_index(), 1);

    state.render_frame(); // tick 8 → advances
    assert_eq!(state.frame_index(), 2);
}

#[test]
fn spinner_runs_full_speed_without_tokens() {
    let mut state = SpinnerState::new();
    state.activate();

    // No tokens added → not streaming → every render_frame advances
    state.render_frame();
    state.render_frame();
    state.render_frame();
    assert_eq!(state.frame_index(), 3);
}

#[test]
fn set_progress_extracts_tail_lines() {
    let mut state = SpinnerState::new();
    state.activate();

    let text = "line1\nline2\nline3\nline4\nline5\nline6\nline7";
    state.set_progress(text);

    // Should keep last 5 lines
    let lines = state.progress_lines();
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0], "line3");
    assert_eq!(lines[4], "line7");
}

#[test]
fn set_progress_fewer_than_max_keeps_all() {
    let mut state = SpinnerState::new();
    state.activate();

    state.set_progress("a\nb");
    assert_eq!(state.progress_lines().len(), 2);
    assert_eq!(state.progress_lines()[0], "a");
    assert_eq!(state.progress_lines()[1], "b");
}

#[test]
fn set_progress_empty_text_yields_no_lines() {
    let mut state = SpinnerState::new();
    state.activate();

    state.set_progress("");
    assert_eq!(state.progress_lines().len(), 0);
}

#[test]
fn restore_verb_clears_progress_lines() {
    let mut state = SpinnerState::new();
    state.activate();

    state.set_progress("line1\nline2\nline3");
    assert_eq!(state.progress_lines().len(), 3);

    state.restore_verb();
    assert!(state.progress_lines().is_empty());
    assert!(state.phase().is_verb());
}

#[test]
fn set_tool_clears_progress_lines() {
    let mut state = SpinnerState::new();
    state.activate();

    state.set_progress("line1\nline2");
    assert_eq!(state.progress_lines().len(), 2);

    state.set_tool("bash");
    assert!(state.progress_lines().is_empty());
    assert!(state.phase().is_tool());
}

#[test]
fn render_frame_progress_tracks_rendered_lines() {
    let mut state = SpinnerState::new();
    state.activate();

    state.set_progress("line1\nline2\nline3");
    state.render_frame();

    // 3 progress + 1 separator + 1 spinner = 5
    assert_eq!(state.rendered_line_count(), 5);
    assert_eq!(state.frame_index(), 1);
}

#[test]
fn clear_if_rendered_resets_rendered_lines() {
    let mut state = SpinnerState::new();
    state.activate();

    state.set_progress("line1\nline2");
    state.render_frame();
    // 2 progress + 1 separator + 1 spinner = 4
    assert_eq!(state.rendered_line_count(), 4);

    state.clear_if_rendered();
    assert_eq!(state.rendered_line_count(), 0);
}

#[test]
fn normal_render_frame_tracks_single_line() {
    let mut state = SpinnerState::new();
    state.activate();

    state.render_frame();
    assert_eq!(state.rendered_line_count(), 1);
}

#[test]
fn activate_resets_progress_state() {
    let mut state = SpinnerState::new();
    state.activate();

    state.set_progress("line1\nline2");
    state.render_frame();
    // 2 progress + 1 separator + 1 spinner = 4
    assert_eq!(state.rendered_line_count(), 4);

    // Re-activate should reset everything
    state.activate();
    assert!(state.progress_lines().is_empty());
    assert_eq!(state.rendered_line_count(), 0);
}

// ---------------------------------------------------------------------------
// build_progress_frame / build_clear_sequence tests
// ---------------------------------------------------------------------------

use bendclaw::cli::repl::spinner::build_clear_sequence;
use bendclaw::cli::repl::spinner::build_progress_frame;

/// Helper: count occurrences of a substring.
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// Helper: count `\r\n` sequences (raw-mode newlines).
fn count_newlines(s: &str) -> usize {
    count_occurrences(s, "\r\n")
}

/// Helper: extract all cursor-up values from `\x1b[NA` sequences.
fn extract_cursor_ups(s: &str) -> Vec<usize> {
    let mut ups = Vec::new();
    let mut rest = s;
    while let Some(pos) = rest.find("\x1b[") {
        rest = &rest[pos + 2..];
        if let Some(a_pos) = rest.find('A') {
            let num_str = &rest[..a_pos];
            if let Ok(n) = num_str.parse::<usize>() {
                ups.push(n);
            }
        }
    }
    ups
}

#[test]
fn progress_frame_first_render_no_cursor_up() {
    let lines = vec!["line1".to_string(), "line2".to_string()];
    let (output, new_lines) = build_progress_frame(&lines, 0, "⠋", "\x1b[90m", "bash", "1.2s");

    // 2 progress + 1 separator + 1 spinner = 4
    assert_eq!(new_lines, 4);

    // No cursor-up on first render (prev_rendered_lines = 0)
    let ups = extract_cursor_ups(&output);
    assert!(
        ups.is_empty(),
        "first render should have no cursor-up, got: {ups:?}"
    );

    // Should have exactly 3 \r\n (2 progress + 1 separator), spinner has none
    assert_eq!(count_newlines(&output), 3);

    // Should contain both progress lines
    assert!(output.contains("line1"));
    assert!(output.contains("line2"));

    // Should contain spinner text
    assert!(output.contains("Running bash…"));
    assert!(output.contains("1.2s"));
}

#[test]
fn progress_frame_subsequent_render_cursor_up() {
    let lines = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    // Previous render had 5 lines (3 progress + 1 separator + 1 spinner)
    let (output, new_lines) = build_progress_frame(&lines, 5, "⠙", "\x1b[90m", "bash", "2.0s");

    assert_eq!(new_lines, 5);

    // Should cursor-up 4 (prev_rendered_lines - 1 = 4)
    let ups = extract_cursor_ups(&output);
    assert!(
        ups.contains(&4),
        "should cursor-up 4 to reach top of previous block, got: {ups:?}"
    );
}

#[test]
fn progress_frame_block_shrinks_pads_to_keep_spinner_pinned() {
    let lines = vec!["only".to_string()];
    // Previous render had 5 lines (3 progress + 1 sep + 1 spinner)
    // Now 1 progress + 1 sep + 1 spinner = 3 content lines, pinned at 5
    let (output, new_lines) = build_progress_frame(&lines, 5, "⠹", "\x1b[90m", "bash", "3.0s");

    // Block stays at 5 (pinned)
    assert_eq!(new_lines, 5);

    // Should have cursor-up 4 at the start (prev 5 - 1)
    let ups = extract_cursor_ups(&output);
    assert!(
        ups.contains(&4),
        "should cursor-up 4 at start, got: {ups:?}"
    );

    // No extra cursor-up needed — padding replaces the old clear-and-return logic
    assert_eq!(
        ups.len(),
        1,
        "only 1 cursor-up (initial), no clear-return, got: {ups:?}"
    );

    // 4 \r\n total: 1 progress + 1 separator + 2 padding, spinner on row 5
    assert_eq!(count_newlines(&output), 4);
}

#[test]
fn progress_frame_block_grows_no_extra_clear() {
    let lines = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    // Previous render had 3 lines, now 3 progress + 1 sep + 1 spinner = 5
    let (output, new_lines) = build_progress_frame(&lines, 3, "⠸", "\x1b[90m", "bash", "4.0s");

    assert_eq!(new_lines, 5);

    // Should cursor-up 2 at the start (prev 3 - 1)
    let ups = extract_cursor_ups(&output);
    assert!(ups.contains(&2), "should cursor-up 2, got: {ups:?}");

    // No extra clear needed — block grew, so no leftover lines
    // Only 1 cursor-up total (the initial one)
    assert_eq!(
        ups.len(),
        1,
        "should have exactly 1 cursor-up, got: {ups:?}"
    );
}

#[test]
fn progress_frame_same_size_no_extra_clear() {
    let lines = vec!["x".to_string(), "y".to_string()];
    // 2 progress + 1 sep + 1 spinner = 4
    let (output, new_lines) = build_progress_frame(&lines, 4, "⠼", "\x1b[90m", "bash", "5.0s");

    assert_eq!(new_lines, 4);

    let ups = extract_cursor_ups(&output);
    // cursor-up 3 at start (prev 4 - 1)
    assert!(ups.contains(&3), "should cursor-up 3, got: {ups:?}");
    // No extra clear
    assert_eq!(
        ups.len(),
        1,
        "should have exactly 1 cursor-up, got: {ups:?}"
    );
}

#[test]
fn progress_frame_single_line_from_single_line() {
    let lines = vec!["one".to_string()];
    // Previous was single-line spinner (rendered_lines = 1)
    // Now 1 progress + 1 sep + 1 spinner = 3
    let (output, new_lines) = build_progress_frame(&lines, 1, "⠴", "\x1b[90m", "bash", "0.5s");

    assert_eq!(new_lines, 3);

    // prev_rendered_lines = 1, so no cursor-up (only cursor-up when > 1)
    let ups = extract_cursor_ups(&output);
    assert!(
        ups.is_empty(),
        "should have no cursor-up from single-line, got: {ups:?}"
    );

    // Starts with \r to return to line start
    assert!(output.starts_with('\r'));
}

#[test]
fn progress_frame_empty_progress_lines() {
    let lines: Vec<String> = vec![];
    let (output, new_lines) = build_progress_frame(&lines, 0, "⠋", "\x1b[90m", "bash", "0s");

    // 0 progress + 1 spinner = 1
    assert_eq!(new_lines, 1);

    // No \r\n (no progress lines)
    assert_eq!(count_newlines(&output), 0);

    // Still has spinner with tool name
    assert!(output.contains("Running bash…"));
}

#[test]
fn progress_frame_all_newlines_are_raw_mode_safe() {
    let lines = vec!["a".to_string(), "b".to_string()];
    let (output, _) = build_progress_frame(&lines, 0, "⠋", "\x1b[90m", "bash", "1s");

    // Every \n must be preceded by \r (raw mode requirement)
    for (i, _) in output.match_indices('\n') {
        assert!(
            i > 0 && output.as_bytes()[i - 1] == b'\r',
            "bare \\n at byte {i} — must use \\r\\n in raw mode"
        );
    }
}

// ---------------------------------------------------------------------------
// build_clear_sequence tests
// ---------------------------------------------------------------------------

#[test]
fn clear_single_line() {
    let seq = build_clear_sequence(1);
    assert_eq!(seq, "\r\x1b[K");
}

#[test]
fn clear_zero_lines() {
    let seq = build_clear_sequence(0);
    // Treat 0 same as 1 (single-line path)
    assert_eq!(seq, "\r\x1b[K");
}

#[test]
fn clear_multi_line_cursor_up_and_erase() {
    let seq = build_clear_sequence(4);

    // Should cursor-up 3 at start (4 - 1)
    assert!(seq.contains("\x1b[3A"), "should start with cursor-up 3");

    // Should have 4 erase-line + newline sequences
    assert_eq!(count_occurrences(&seq, "\r\x1b[K\r\n"), 4);

    // Should cursor-up 4 at end to return to start
    assert!(seq.contains("\x1b[4A"), "should end with cursor-up 4");
}

#[test]
fn clear_multi_line_all_newlines_raw_mode_safe() {
    let seq = build_clear_sequence(3);

    for (i, _) in seq.match_indices('\n') {
        assert!(
            i > 0 && seq.as_bytes()[i - 1] == b'\r',
            "bare \\n at byte {i} — must use \\r\\n in raw mode"
        );
    }
}

#[test]
fn clear_two_lines() {
    let seq = build_clear_sequence(2);

    // cursor-up 1 at start
    assert!(seq.contains("\x1b[1A"), "should cursor-up 1 at start");

    // 2 clear lines
    assert_eq!(count_occurrences(&seq, "\r\x1b[K\r\n"), 2);

    // cursor-up 2 at end
    assert!(seq.contains("\x1b[2A"), "should cursor-up 2 at end");
}

// ---------------------------------------------------------------------------
// Spinner line must stay pinned at a fixed terminal row.
// When progress lines shrink, the gap is padded so the spinner
// doesn't jump upward.
// ---------------------------------------------------------------------------

#[test]
fn progress_frame_shrink_keeps_spinner_pinned() {
    // First render: 3 progress + 1 sep + 1 spinner = 5 total
    let lines_big = vec!["a".into(), "b".into(), "c".into()];
    let (out1, n1) = build_progress_frame(&lines_big, 0, "⠋", "\x1b[90m", "bash", "1s");
    assert_eq!(n1, 5);

    // The spinner line is always preceded by exactly (n-1) \r\n sequences
    assert_eq!(
        count_newlines(&out1),
        4,
        "first render: 4 \\r\\n before spinner"
    );

    // Second render: shrink to 1 progress line.
    // The block should still occupy 5 terminal rows so the spinner stays put.
    let lines_small = vec!["x".into()];
    let (out2, n2) = build_progress_frame(&lines_small, 5, "⠙", "\x1b[90m", "bash", "2s");

    // returned new_lines should stay at prev size (pinned)
    assert_eq!(
        n2, 5,
        "spinner must stay pinned: new_lines should equal prev_rendered_lines"
    );

    // There should be 4 \r\n before the spinner line (to keep it on row 5)
    assert_eq!(
        count_newlines(&out2),
        4,
        "shrunk render must still have 4 \\r\\n to keep spinner on row 5"
    );
}

#[test]
fn progress_frame_shrink_to_zero_keeps_spinner_pinned() {
    // Previous: 3 progress + 1 sep + 1 spinner = 5 lines
    let lines_empty: Vec<String> = vec![];
    let (out, n) = build_progress_frame(&lines_empty, 5, "⠹", "\x1b[90m", "bash", "3s");

    // Spinner must stay on row 5
    assert_eq!(n, 5, "spinner pinned at row 5 even with 0 progress lines");
    assert_eq!(
        count_newlines(&out),
        4,
        "4 \\r\\n needed to reach row 5 for spinner"
    );
}

#[test]
fn progress_frame_grow_expands_block() {
    // Previous: 1 progress + 1 sep + 1 spinner = 3 lines
    // New: 4 progress + 1 sep + 1 spinner = 6 lines → block grows
    let lines = vec!["a".into(), "b".into(), "c".into(), "d".into()];
    let (out, n) = build_progress_frame(&lines, 3, "⠸", "\x1b[90m", "bash", "4s");

    // Block grows to 6
    assert_eq!(n, 6);
    // 5 \r\n before spinner (4 progress + 1 separator)
    assert_eq!(count_newlines(&out), 5);
}
