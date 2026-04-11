use bend_engine::tools::AskUserOption;
use bend_engine::tools::AskUserRequest;
use bend_engine::tools::AskUserResponse;
use bendclaw::cli::repl::ask_user::build_confirmation;
use bendclaw::cli::repl::ask_user::build_question_block;
use bendclaw::cli::repl::ask_user::build_question_block_typing;
use bendclaw::cli::repl::ask_user::build_skipped;
use bendclaw::cli::repl::ask_user::handle_key;
use bendclaw::cli::repl::ask_user::none_number;
use bendclaw::cli::repl::ask_user::physical_row_count;
use bendclaw::cli::repl::ask_user::total_items;
use bendclaw::cli::repl::ask_user::AskUserAction;
use bendclaw::cli::repl::ask_user::AskUserMode;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;

const WIDE: usize = 200;
const NO_MOD: KeyModifiers = KeyModifiers::NONE;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn two_option_request() -> AskUserRequest {
    AskUserRequest {
        question: "Which cache strategy?".into(),
        options: vec![
            AskUserOption {
                label: "In-memory (Recommended)".into(),
                description: "Zero deps, HashMap + TTL".into(),
            },
            AskUserOption {
                label: "Redis".into(),
                description: "Shared across instances".into(),
            },
        ],
    }
}

fn three_option_request() -> AskUserRequest {
    AskUserRequest {
        question: "Which approach?".into(),
        options: vec![
            AskUserOption {
                label: "Option A (Recommended)".into(),
                description: "First choice".into(),
            },
            AskUserOption {
                label: "Option B".into(),
                description: "Second choice".into(),
            },
            AskUserOption {
                label: "Option C".into(),
                description: "Third choice".into(),
            },
        ],
    }
}

fn four_option_request() -> AskUserRequest {
    AskUserRequest {
        question: "Pick a DB?".into(),
        options: vec![
            AskUserOption {
                label: "Postgres".into(),
                description: "Relational".into(),
            },
            AskUserOption {
                label: "MongoDB".into(),
                description: "Document store".into(),
            },
            AskUserOption {
                label: "Redis".into(),
                description: "In-memory KV".into(),
            },
            AskUserOption {
                label: "SQLite".into(),
                description: "Embedded".into(),
            },
        ],
    }
}

fn selecting(selected: usize) -> AskUserMode {
    AskUserMode::Selecting { selected }
}

fn typing(selected: usize, input: &str) -> AskUserMode {
    AskUserMode::Typing {
        selected,
        input: input.into(),
    }
}

// ===========================================================================
// Rendering tests
// ===========================================================================

#[test]
fn first_option_highlighted_when_selected_zero() {
    let req = two_option_request();
    let (output, _) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("› 1."));
    assert!(!output.contains("› 2."));
}

#[test]
fn second_option_highlighted_when_selected_one() {
    let req = two_option_request();
    let (output, _) = build_question_block(&req, 1, WIDE);
    assert!(output.contains("› 2."));
}

#[test]
fn none_of_above_highlighted_when_selected_last() {
    let req = two_option_request();
    let (output, _) = build_question_block(&req, req.options.len(), WIDE);
    assert!(output.contains("› 3. None of the above"));
}

#[test]
fn question_text_appears_in_output() {
    let req = two_option_request();
    let (output, _) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("Which cache strategy?"));
}

#[test]
fn option_labels_appear_in_output() {
    let req = two_option_request();
    let (output, _) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("In-memory (Recommended)"));
    assert!(output.contains("Redis"));
}

#[test]
fn option_descriptions_appear_in_output() {
    let req = two_option_request();
    let (output, _) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("Zero deps, HashMap + TTL"));
    assert!(output.contains("Shared across instances"));
}

#[test]
fn none_of_above_always_present() {
    let req = two_option_request();
    let (output, _) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("None of the above"));
}

#[test]
fn footer_hint_shows_correct_range() {
    let req = two_option_request();
    let (output, _) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("1-2 pick"));
    assert!(output.contains("3 custom"));

    let req3 = three_option_request();
    let (output3, _) = build_question_block(&req3, 0, WIDE);
    assert!(output3.contains("1-3 pick"));
    assert!(output3.contains("4 custom"));
}

#[test]
fn footer_custom_key_matches_none_number_four_options() {
    let req = four_option_request();
    let (output, _) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("1-4 pick"));
    assert!(output.contains("5 custom"));
}

// ---------------------------------------------------------------------------
// Line counts
// ---------------------------------------------------------------------------

#[test]
fn line_count_correct_for_two_options() {
    let req = two_option_request();
    let (_, lines) = build_question_block(&req, 0, WIDE);
    assert_eq!(lines, 9);
}

#[test]
fn line_count_correct_for_three_options() {
    let req = three_option_request();
    let (_, lines) = build_question_block(&req, 0, WIDE);
    assert_eq!(lines, 11);
}

#[test]
fn line_count_correct_for_four_options() {
    let req = four_option_request();
    let (_, lines) = build_question_block(&req, 0, WIDE);
    // question(1) + blank(1) + 4*(label+desc)(8) + none(1) + blank(1) + footer(1) = 13
    assert_eq!(lines, 13);
}

#[test]
fn typing_mode_has_extra_line_vs_selection() {
    let req = two_option_request();
    let (_, select_rows) = build_question_block(&req, 0, WIDE);
    let (_, typing_rows) = build_question_block_typing(&req, req.options.len(), WIDE, "");
    assert_eq!(typing_rows, select_rows + 1);
}

// ---------------------------------------------------------------------------
// Sequential numbering
// ---------------------------------------------------------------------------

#[test]
fn none_number_is_sequential_two_options() {
    let req = two_option_request();
    let (output, _) = build_question_block(&req, 0, WIDE);
    assert!(output.contains(" 3. None of the above"));
}

#[test]
fn none_number_is_sequential_three_options() {
    let req = three_option_request();
    let (output, _) = build_question_block(&req, 0, WIDE);
    assert!(output.contains(" 4. None of the above"));
}

#[test]
fn none_number_is_sequential_four_options() {
    let req = four_option_request();
    let (output, _) = build_question_block(&req, 0, WIDE);
    assert!(output.contains(" 5. None of the above"));
}

#[test]
fn no_zero_numbering_anywhere() {
    for req in [
        two_option_request(),
        three_option_request(),
        four_option_request(),
    ] {
        let (output, _) = build_question_block(&req, 0, WIDE);
        assert!(!output.contains(" 0."), "found ' 0.' in output");
        assert!(!output.contains("0 custom"), "found '0 custom' in output");
    }
}

// ---------------------------------------------------------------------------
// Highlight exclusivity
// ---------------------------------------------------------------------------

#[test]
fn selecting_none_does_not_highlight_regular_options() {
    let req = two_option_request();
    let (output, _) = build_question_block(&req, req.options.len(), WIDE);
    assert!(output.contains("› 3."));
    assert!(!output.contains("› 1."));
    assert!(!output.contains("› 2."));
}

#[test]
fn selecting_first_does_not_highlight_none() {
    let req = two_option_request();
    let (output, _) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("› 1."));
    assert!(!output.contains("› 3."));
}

// ---------------------------------------------------------------------------
// Typing mode rendering
// ---------------------------------------------------------------------------

#[test]
fn typing_mode_shows_input_field() {
    let req = two_option_request();
    let (output, _) = build_question_block_typing(&req, req.options.len(), WIDE, "hello");
    assert!(output.contains("hello█"));
}

#[test]
fn typing_mode_shows_back_hint() {
    let req = two_option_request();
    let (output, _) = build_question_block_typing(&req, req.options.len(), WIDE, "");
    assert!(output.contains("Esc back to list"));
}

#[test]
fn typing_mode_still_shows_options() {
    let req = two_option_request();
    let (output, _) = build_question_block_typing(&req, req.options.len(), WIDE, "test");
    assert!(output.contains("In-memory (Recommended)"));
    assert!(output.contains("Redis"));
    assert!(output.contains("None of the above"));
}

#[test]
fn typing_mode_empty_input_shows_placeholder() {
    let req = two_option_request();
    let (output, _) = build_question_block_typing(&req, req.options.len(), WIDE, "");
    assert!(output.contains("Type something..."));
    assert!(!output.contains("█"));
}

#[test]
fn typing_mode_nonempty_shows_cursor_not_placeholder() {
    let req = two_option_request();
    let (output, _) = build_question_block_typing(&req, req.options.len(), WIDE, "x");
    assert!(output.contains("x█"));
    assert!(!output.contains("Type something..."));
}

#[test]
fn typing_mode_placeholder_not_in_selection_mode() {
    let req = two_option_request();
    let (output, _) = build_question_block(&req, 0, WIDE);
    assert!(!output.contains("Type something..."));
}

#[test]
fn selection_mode_does_not_show_input_field() {
    let req = two_option_request();
    let (output, _) = build_question_block(&req, 0, WIDE);
    assert!(!output.contains("Esc back to list"));
    assert!(output.contains("Esc skip"));
}

// ---------------------------------------------------------------------------
// Confirmation / skipped
// ---------------------------------------------------------------------------

#[test]
fn confirmation_contains_checkmark_and_label() {
    let text = build_confirmation("Redis");
    assert!(text.contains("✓"));
    assert!(text.contains("Redis"));
}

#[test]
fn skipped_contains_dash() {
    let text = build_skipped();
    assert!(text.contains("skipped"));
}

// ---------------------------------------------------------------------------
// physical_row_count
// ---------------------------------------------------------------------------

#[test]
fn physical_row_count_empty_line_is_one() {
    assert_eq!(physical_row_count("", 80), 1);
}

#[test]
fn physical_row_count_short_ascii_is_one() {
    assert_eq!(physical_row_count("hello", 80), 1);
}

#[test]
fn physical_row_count_exact_width_is_one() {
    assert_eq!(physical_row_count(&"a".repeat(80), 80), 1);
}

#[test]
fn physical_row_count_wraps_at_boundary() {
    assert_eq!(physical_row_count(&"a".repeat(81), 80), 2);
}

#[test]
fn physical_row_count_cjk_double_width() {
    assert_eq!(physical_row_count(&"修".repeat(20), 80), 1);
    assert_eq!(physical_row_count(&"修".repeat(41), 80), 2);
}

#[test]
fn physical_row_count_cjk_narrow_terminal() {
    assert_eq!(physical_row_count("修复 plan mode", 10), 2);
}

#[test]
fn physical_row_count_ignores_ansi_codes() {
    assert_eq!(physical_row_count("\x1b[36m\x1b[1mhello\x1b[0m", 80), 1);
}

#[test]
fn cjk_question_wraps_on_narrow_terminal() {
    let req = AskUserRequest {
        question: "修复 plan mode session title 的方案，你倾向哪种？".into(),
        options: vec![AskUserOption {
            label: "Option A".into(),
            description: "Short".into(),
        }],
    };
    let (_, rows_wide) = build_question_block(&req, 0, WIDE);
    let (_, rows_narrow) = build_question_block(&req, 0, 40);
    assert!(
        rows_narrow > rows_wide,
        "narrow ({rows_narrow}) should exceed wide ({rows_wide})"
    );
}

// ---------------------------------------------------------------------------
// Helper function tests
// ---------------------------------------------------------------------------

#[test]
fn total_items_includes_none() {
    assert_eq!(total_items(&two_option_request()), 3);
    assert_eq!(total_items(&three_option_request()), 4);
    assert_eq!(total_items(&four_option_request()), 5);
}

#[test]
fn none_number_is_option_count_plus_one() {
    assert_eq!(none_number(&two_option_request()), 3);
    assert_eq!(none_number(&three_option_request()), 4);
    assert_eq!(none_number(&four_option_request()), 5);
}

// ===========================================================================
// State machine tests — handle_key
// ===========================================================================

// ---------------------------------------------------------------------------
// Selection mode: navigation
// ---------------------------------------------------------------------------

#[test]
fn select_down_from_first() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, selecting(0), KeyCode::Down, NO_MOD);
    assert_eq!(mode, selecting(1));
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn select_down_wraps_to_first() {
    let req = two_option_request();
    // total = 3 (2 options + none), last index = 2
    let (mode, action) = handle_key(&req, selecting(2), KeyCode::Down, NO_MOD);
    assert_eq!(mode, selecting(0));
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn select_up_from_first_wraps_to_last() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, selecting(0), KeyCode::Up, NO_MOD);
    assert_eq!(mode, selecting(2));
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn select_up_from_second() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, selecting(1), KeyCode::Up, NO_MOD);
    assert_eq!(mode, selecting(0));
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn select_j_moves_down() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, selecting(0), KeyCode::Char('j'), NO_MOD);
    assert_eq!(mode, selecting(1));
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn select_k_moves_up() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, selecting(1), KeyCode::Char('k'), NO_MOD);
    assert_eq!(mode, selecting(0));
    assert_eq!(action, AskUserAction::Redraw);
}

// ---------------------------------------------------------------------------
// Selection mode: Enter
// ---------------------------------------------------------------------------

#[test]
fn enter_on_first_option_submits_selected() {
    let req = two_option_request();
    let (_, action) = handle_key(&req, selecting(0), KeyCode::Enter, NO_MOD);
    assert_eq!(
        action,
        AskUserAction::Submit(AskUserResponse::Selected("In-memory (Recommended)".into()))
    );
}

#[test]
fn enter_on_second_option_submits_selected() {
    let req = two_option_request();
    let (_, action) = handle_key(&req, selecting(1), KeyCode::Enter, NO_MOD);
    assert_eq!(
        action,
        AskUserAction::Submit(AskUserResponse::Selected("Redis".into()))
    );
}

#[test]
fn enter_on_none_enters_typing_mode() {
    let req = two_option_request();
    let none_idx = req.options.len();
    let (mode, action) = handle_key(&req, selecting(none_idx), KeyCode::Enter, NO_MOD);
    assert_eq!(mode, typing(none_idx, ""));
    assert_eq!(action, AskUserAction::Redraw);
}

// ---------------------------------------------------------------------------
// Selection mode: digit keys
// ---------------------------------------------------------------------------

#[test]
fn digit_1_selects_first_option() {
    let req = two_option_request();
    let (_, action) = handle_key(&req, selecting(0), KeyCode::Char('1'), NO_MOD);
    assert_eq!(
        action,
        AskUserAction::Submit(AskUserResponse::Selected("In-memory (Recommended)".into()))
    );
}

#[test]
fn digit_2_selects_second_option() {
    let req = two_option_request();
    let (_, action) = handle_key(&req, selecting(0), KeyCode::Char('2'), NO_MOD);
    assert_eq!(
        action,
        AskUserAction::Submit(AskUserResponse::Selected("Redis".into()))
    );
}

#[test]
fn digit_for_none_enters_typing() {
    let req = two_option_request();
    // 2 options → digit '3' = none
    let (mode, action) = handle_key(&req, selecting(0), KeyCode::Char('3'), NO_MOD);
    assert_eq!(mode, typing(2, ""));
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn digit_for_none_four_options() {
    let req = four_option_request();
    // 4 options → digit '5' = none
    let (mode, action) = handle_key(&req, selecting(0), KeyCode::Char('5'), NO_MOD);
    assert_eq!(mode, typing(4, ""));
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn digit_out_of_range_is_noop() {
    let req = two_option_request();
    // 2 options + none = 3 items, digit '4' is out of range
    let (mode, action) = handle_key(&req, selecting(0), KeyCode::Char('4'), NO_MOD);
    assert_eq!(mode, selecting(0));
    assert_eq!(action, AskUserAction::Noop);
}

#[test]
fn digit_9_out_of_range_is_noop() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, selecting(0), KeyCode::Char('9'), NO_MOD);
    assert_eq!(mode, selecting(0));
    assert_eq!(action, AskUserAction::Noop);
}

// ---------------------------------------------------------------------------
// Selection mode: Esc / Ctrl-C
// ---------------------------------------------------------------------------

#[test]
fn esc_in_selection_submits_skipped() {
    let req = two_option_request();
    let (_, action) = handle_key(&req, selecting(0), KeyCode::Esc, NO_MOD);
    assert_eq!(action, AskUserAction::Submit(AskUserResponse::Skipped));
}

#[test]
fn ctrl_c_in_selection_exits_run() {
    let req = two_option_request();
    let (_, action) = handle_key(
        &req,
        selecting(0),
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    );
    assert_eq!(action, AskUserAction::ExitRun);
}

// ---------------------------------------------------------------------------
// Selection mode: unknown keys
// ---------------------------------------------------------------------------

#[test]
fn unknown_key_in_selection_is_noop() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, selecting(0), KeyCode::Char('x'), NO_MOD);
    assert_eq!(mode, selecting(0));
    assert_eq!(action, AskUserAction::Noop);
}

// ---------------------------------------------------------------------------
// Typing mode: character input
// ---------------------------------------------------------------------------

#[test]
fn typing_char_appends_to_input() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, typing(2, ""), KeyCode::Char('a'), NO_MOD);
    assert_eq!(mode, typing(2, "a"));
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn typing_multiple_chars() {
    let req = two_option_request();
    let (mode, _) = handle_key(&req, typing(2, "he"), KeyCode::Char('l'), NO_MOD);
    assert_eq!(mode, typing(2, "hel"));
}

// ---------------------------------------------------------------------------
// Typing mode: backspace
// ---------------------------------------------------------------------------

#[test]
fn typing_backspace_removes_last_char() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, typing(2, "abc"), KeyCode::Backspace, NO_MOD);
    assert_eq!(mode, typing(2, "ab"));
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn typing_backspace_on_empty_stays_empty() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, typing(2, ""), KeyCode::Backspace, NO_MOD);
    assert_eq!(mode, typing(2, ""));
    assert_eq!(action, AskUserAction::Redraw);
}

// ---------------------------------------------------------------------------
// Typing mode: Enter
// ---------------------------------------------------------------------------

#[test]
fn typing_enter_with_content_submits_custom() {
    let req = two_option_request();
    let (_, action) = handle_key(&req, typing(2, "Use SQLite"), KeyCode::Enter, NO_MOD);
    assert_eq!(
        action,
        AskUserAction::Submit(AskUserResponse::Custom("Use SQLite".into()))
    );
}

#[test]
fn typing_enter_trims_whitespace() {
    let req = two_option_request();
    let (_, action) = handle_key(&req, typing(2, "  hello  "), KeyCode::Enter, NO_MOD);
    assert_eq!(
        action,
        AskUserAction::Submit(AskUserResponse::Custom("hello".into()))
    );
}

#[test]
fn typing_enter_empty_returns_to_selection() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, typing(2, ""), KeyCode::Enter, NO_MOD);
    assert_eq!(mode, selecting(2));
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn typing_enter_whitespace_only_returns_to_selection() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, typing(2, "   "), KeyCode::Enter, NO_MOD);
    assert_eq!(mode, selecting(2));
    assert_eq!(action, AskUserAction::Redraw);
}

// ---------------------------------------------------------------------------
// Typing mode: Esc
// ---------------------------------------------------------------------------

#[test]
fn typing_esc_returns_to_selection() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, typing(2, "partial"), KeyCode::Esc, NO_MOD);
    assert_eq!(mode, selecting(2));
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn typing_esc_preserves_selected_index() {
    let req = two_option_request();
    let (mode, _) = handle_key(&req, typing(2, ""), KeyCode::Esc, NO_MOD);
    assert_eq!(mode, selecting(2));
}

// ---------------------------------------------------------------------------
// Typing mode: Ctrl-C
// ---------------------------------------------------------------------------

#[test]
fn typing_ctrl_c_exits_run() {
    let req = two_option_request();
    let (_, action) = handle_key(
        &req,
        typing(2, "abc"),
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    );
    assert_eq!(action, AskUserAction::ExitRun);
}

// ---------------------------------------------------------------------------
// Typing mode: unknown keys
// ---------------------------------------------------------------------------

#[test]
fn typing_unknown_key_is_noop() {
    let req = two_option_request();
    let (mode, action) = handle_key(&req, typing(2, "abc"), KeyCode::Tab, NO_MOD);
    assert_eq!(mode, typing(2, "abc"));
    assert_eq!(action, AskUserAction::Noop);
}

// ---------------------------------------------------------------------------
// Multi-step interaction sequences
// ---------------------------------------------------------------------------

#[test]
fn full_flow_navigate_to_none_type_and_submit() {
    let req = two_option_request();

    // Start at first option, press Down twice to reach "None of the above"
    let (mode, _) = handle_key(&req, selecting(0), KeyCode::Down, NO_MOD);
    assert_eq!(mode, selecting(1));
    let (mode, _) = handle_key(&req, mode, KeyCode::Down, NO_MOD);
    assert_eq!(mode, selecting(2));

    // Press Enter to enter typing mode
    let (mode, action) = handle_key(&req, mode, KeyCode::Enter, NO_MOD);
    assert_eq!(mode, typing(2, ""));
    assert_eq!(action, AskUserAction::Redraw);

    // Type "SQLite"
    let (mode, _) = handle_key(&req, mode, KeyCode::Char('S'), NO_MOD);
    let (mode, _) = handle_key(&req, mode, KeyCode::Char('Q'), NO_MOD);
    let (mode, _) = handle_key(&req, mode, KeyCode::Char('L'), NO_MOD);
    assert_eq!(mode, typing(2, "SQL"));

    // Submit
    let (_, action) = handle_key(&req, mode, KeyCode::Enter, NO_MOD);
    assert_eq!(
        action,
        AskUserAction::Submit(AskUserResponse::Custom("SQL".into()))
    );
}

#[test]
fn full_flow_type_then_esc_then_select() {
    let req = two_option_request();

    // Digit '3' to enter typing mode
    let (mode, _) = handle_key(&req, selecting(0), KeyCode::Char('3'), NO_MOD);
    assert_eq!(mode, typing(2, ""));

    // Type something then Esc back
    let (mode, _) = handle_key(&req, mode, KeyCode::Char('x'), NO_MOD);
    let (mode, action) = handle_key(&req, mode, KeyCode::Esc, NO_MOD);
    assert_eq!(mode, selecting(2));
    assert_eq!(action, AskUserAction::Redraw);

    // Now press '1' to select first option
    let (_, action) = handle_key(&req, mode, KeyCode::Char('1'), NO_MOD);
    assert_eq!(
        action,
        AskUserAction::Submit(AskUserResponse::Selected("In-memory (Recommended)".into()))
    );
}

#[test]
fn full_flow_type_backspace_retype_submit() {
    let req = two_option_request();

    let (mode, _) = handle_key(&req, selecting(0), KeyCode::Char('3'), NO_MOD);
    let (mode, _) = handle_key(&req, mode, KeyCode::Char('a'), NO_MOD);
    let (mode, _) = handle_key(&req, mode, KeyCode::Char('b'), NO_MOD);
    assert_eq!(mode, typing(2, "ab"));

    // Backspace twice
    let (mode, _) = handle_key(&req, mode, KeyCode::Backspace, NO_MOD);
    let (mode, _) = handle_key(&req, mode, KeyCode::Backspace, NO_MOD);
    assert_eq!(mode, typing(2, ""));

    // Retype
    let (mode, _) = handle_key(&req, mode, KeyCode::Char('z'), NO_MOD);
    let (_, action) = handle_key(&req, mode, KeyCode::Enter, NO_MOD);
    assert_eq!(
        action,
        AskUserAction::Submit(AskUserResponse::Custom("z".into()))
    );
}
