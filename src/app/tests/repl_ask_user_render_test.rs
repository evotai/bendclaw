//! Tests for the ask_user state machine and rendering.

use bend_engine::tools::AskUserOption;
use bend_engine::tools::AskUserQuestion;
use bend_engine::tools::AskUserRequest;
use bend_engine::tools::AskUserResponse;
use bendclaw::cli::repl::ask_user::build_confirmation;
use bendclaw::cli::repl::ask_user::build_question_block;
use bendclaw::cli::repl::ask_user::build_skipped;
use bendclaw::cli::repl::ask_user::handle_key;
use bendclaw::cli::repl::ask_user::none_index_for;
use bendclaw::cli::repl::ask_user::physical_row_count;
use bendclaw::cli::repl::ask_user::total_items_for;
use bendclaw::cli::repl::ask_user::AskUserAction;
use bendclaw::cli::repl::ask_user::AskUserState;
use bendclaw::cli::repl::ask_user::InputMode;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;

const WIDE: usize = 200;
const NO_MOD: KeyModifiers = KeyModifiers::NONE;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn single_request() -> AskUserRequest {
    AskUserRequest {
        questions: vec![AskUserQuestion {
            header: "Cache".into(),
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
        }],
    }
}

fn two_question_request() -> AskUserRequest {
    AskUserRequest {
        questions: vec![
            AskUserQuestion {
                header: "Cache".into(),
                question: "Which cache strategy?".into(),
                options: vec![
                    AskUserOption {
                        label: "In-memory".into(),
                        description: "Zero deps".into(),
                    },
                    AskUserOption {
                        label: "Redis".into(),
                        description: "Shared".into(),
                    },
                ],
            },
            AskUserQuestion {
                header: "Auth".into(),
                question: "Which auth method?".into(),
                options: vec![
                    AskUserOption {
                        label: "OAuth".into(),
                        description: "Delegated".into(),
                    },
                    AskUserOption {
                        label: "JWT".into(),
                        description: "Stateless".into(),
                    },
                ],
            },
        ],
    }
}

fn state(n: usize) -> AskUserState {
    AskUserState::new(n)
}

fn state_with_selected(n: usize, qi: usize, sel: usize) -> AskUserState {
    let mut s = AskUserState::new(n);
    s.active_question = qi;
    s.states[qi].selected = sel;
    s
}

fn state_typing(n: usize, qi: usize, draft: &str) -> AskUserState {
    let mut s = AskUserState::new(n);
    s.active_question = qi;
    s.input_mode = InputMode::Typing;
    s.states[qi].draft = draft.into();
    s
}

// ===========================================================================
// Helper function tests
// ===========================================================================

#[test]
fn total_items_includes_none() {
    let req = single_request();
    assert_eq!(total_items_for(&req.questions[0]), 3);
}

#[test]
fn none_index_is_option_count() {
    let req = single_request();
    assert_eq!(none_index_for(&req.questions[0]), 2);
}

// ===========================================================================
// State machine: vertical navigation
// ===========================================================================

#[test]
fn select_down_from_first() {
    let req = single_request();
    let (s, action) = handle_key(&req, state(1), KeyCode::Down, NO_MOD);
    assert_eq!(s.states[0].selected, 1);
    assert_eq!(action, AskUserAction::Redraw);
    assert_eq!(s.input_mode, InputMode::Selecting);
}

#[test]
fn select_down_to_none_enters_typing() {
    let req = single_request();
    // selected=1, Down → none_idx=2 → auto typing
    let (s, action) = handle_key(&req, state_with_selected(1, 0, 1), KeyCode::Down, NO_MOD);
    assert_eq!(s.states[0].selected, 2);
    assert_eq!(s.input_mode, InputMode::Typing);
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn select_down_wraps_from_none_to_first() {
    let req = single_request();
    // At none (2), Down → wraps to 0
    let (s, _) = handle_key(&req, state_with_selected(1, 0, 2), KeyCode::Down, NO_MOD);
    assert_eq!(s.states[0].selected, 0);
    assert_eq!(s.input_mode, InputMode::Selecting);
}

#[test]
fn select_up_from_first_wraps_to_none_enters_typing() {
    let req = single_request();
    let (s, _) = handle_key(&req, state(1), KeyCode::Up, NO_MOD);
    assert_eq!(s.states[0].selected, 2); // none
    assert_eq!(s.input_mode, InputMode::Typing);
}

#[test]
fn select_up_from_second() {
    let req = single_request();
    let (s, action) = handle_key(&req, state_with_selected(1, 0, 1), KeyCode::Up, NO_MOD);
    assert_eq!(s.states[0].selected, 0);
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn select_j_moves_down() {
    let req = single_request();
    let (s, _) = handle_key(&req, state(1), KeyCode::Char('j'), NO_MOD);
    assert_eq!(s.states[0].selected, 1);
}

#[test]
fn select_k_moves_up() {
    let req = single_request();
    let (s, _) = handle_key(
        &req,
        state_with_selected(1, 0, 1),
        KeyCode::Char('k'),
        NO_MOD,
    );
    assert_eq!(s.states[0].selected, 0);
}

// ===========================================================================
// State machine: horizontal navigation (multi-question)
// ===========================================================================

#[test]
fn left_switches_to_previous_question() {
    let req = two_question_request();
    let mut s = state(2);
    s.active_question = 1;
    let (s, action) = handle_key(&req, s, KeyCode::Left, NO_MOD);
    assert_eq!(s.active_question, 0);
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn right_switches_to_next_question() {
    let req = two_question_request();
    let (s, action) = handle_key(&req, state(2), KeyCode::Right, NO_MOD);
    assert_eq!(s.active_question, 1);
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn left_wraps_from_first_to_last() {
    let req = two_question_request();
    let (s, _) = handle_key(&req, state(2), KeyCode::Left, NO_MOD);
    assert_eq!(s.active_question, 1);
}

#[test]
fn right_wraps_from_last_to_first() {
    let req = two_question_request();
    let mut s = state(2);
    s.active_question = 1;
    let (s, _) = handle_key(&req, s, KeyCode::Right, NO_MOD);
    assert_eq!(s.active_question, 0);
}

#[test]
fn left_right_noop_for_single_question() {
    let req = single_request();
    let (_, action) = handle_key(&req, state(1), KeyCode::Left, NO_MOD);
    assert_eq!(action, AskUserAction::Noop);
    let (_, action) = handle_key(&req, state(1), KeyCode::Right, NO_MOD);
    assert_eq!(action, AskUserAction::Noop);
}

// ===========================================================================
// State machine: Enter / digit confirm
// ===========================================================================

#[test]
fn enter_on_option_submits_single_question() {
    let req = single_request();
    let (_, action) = handle_key(&req, state(1), KeyCode::Enter, NO_MOD);
    match action {
        AskUserAction::Submit(AskUserResponse::Answered(answers)) => {
            assert_eq!(answers.len(), 1);
            assert_eq!(answers[0].answer, "In-memory (Recommended)");
            assert_eq!(answers[0].header, "Cache");
        }
        other => panic!("expected Submit(Answered), got {other:?}"),
    }
}

#[test]
fn digit_selects_option_and_submits() {
    let req = single_request();
    let (_, action) = handle_key(&req, state(1), KeyCode::Char('2'), NO_MOD);
    match action {
        AskUserAction::Submit(AskUserResponse::Answered(answers)) => {
            assert_eq!(answers[0].answer, "Redis");
        }
        other => panic!("expected Submit(Answered), got {other:?}"),
    }
}

#[test]
fn enter_on_none_enters_typing() {
    let req = single_request();
    let s = state_with_selected(1, 0, 2); // none
    let (s, action) = handle_key(&req, s, KeyCode::Enter, NO_MOD);
    assert_eq!(s.input_mode, InputMode::Typing);
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn digit_for_none_enters_typing() {
    let req = single_request();
    let (s, action) = handle_key(&req, state(1), KeyCode::Char('3'), NO_MOD);
    assert_eq!(s.input_mode, InputMode::Typing);
    assert_eq!(s.states[0].selected, 2);
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn digit_out_of_range_is_noop() {
    let req = single_request();
    let (_, action) = handle_key(&req, state(1), KeyCode::Char('9'), NO_MOD);
    assert_eq!(action, AskUserAction::Noop);
}

// ===========================================================================
// State machine: multi-question advance
// ===========================================================================

#[test]
fn enter_advances_to_next_unanswered() {
    let req = two_question_request();
    // Answer first question
    let (s, action) = handle_key(&req, state(2), KeyCode::Enter, NO_MOD);
    // Should advance to question 1
    assert_eq!(s.active_question, 1);
    assert_eq!(action, AskUserAction::Redraw);
    assert!(s.states[0].answer.is_some());
    assert!(s.states[1].answer.is_none());
}

#[test]
fn answering_all_questions_submits() {
    let req = two_question_request();
    // Answer first question
    let (s, _) = handle_key(&req, state(2), KeyCode::Enter, NO_MOD);
    assert_eq!(s.active_question, 1);
    // Answer second question
    let (_, action) = handle_key(&req, s, KeyCode::Enter, NO_MOD);
    match action {
        AskUserAction::Submit(AskUserResponse::Answered(answers)) => {
            assert_eq!(answers.len(), 2);
            assert_eq!(answers[0].answer, "In-memory");
            assert_eq!(answers[1].answer, "OAuth");
        }
        other => panic!("expected Submit(Answered), got {other:?}"),
    }
}

#[test]
fn can_go_back_and_change_answer() {
    let req = two_question_request();
    // Answer first question (In-memory)
    let (s, _) = handle_key(&req, state(2), KeyCode::Enter, NO_MOD);
    assert_eq!(s.active_question, 1);
    // Go back to first question
    let (s, _) = handle_key(&req, s, KeyCode::Left, NO_MOD);
    assert_eq!(s.active_question, 0);
    // Change to Redis (index 1)
    let (s, _) = handle_key(&req, s, KeyCode::Char('2'), NO_MOD);
    // Should advance to question 1 (still unanswered)
    assert_eq!(s.active_question, 1);
    assert_eq!(s.states[0].answer.as_deref(), Some("Redis"));
}

// ===========================================================================
// State machine: Esc / Ctrl-C
// ===========================================================================

#[test]
fn esc_in_selection_cancels_turn() {
    let req = single_request();
    let (_, action) = handle_key(&req, state(1), KeyCode::Esc, NO_MOD);
    assert_eq!(action, AskUserAction::CancelTurn);
}

#[test]
fn ctrl_c_in_selection_exits_run() {
    let req = single_request();
    let (_, action) = handle_key(&req, state(1), KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(action, AskUserAction::ExitRun);
}

// ===========================================================================
// State machine: typing mode
// ===========================================================================

#[test]
fn typing_char_appends_to_draft() {
    let req = single_request();
    let mut s = state_typing(1, 0, "");
    s.states[0].selected = 2;
    let (s, action) = handle_key(&req, s, KeyCode::Char('a'), NO_MOD);
    assert_eq!(s.states[0].draft, "a");
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn typing_backspace_removes_last() {
    let req = single_request();
    let mut s = state_typing(1, 0, "abc");
    s.states[0].selected = 2;
    let (s, _) = handle_key(&req, s, KeyCode::Backspace, NO_MOD);
    assert_eq!(s.states[0].draft, "ab");
}

#[test]
fn typing_enter_with_content_submits() {
    let req = single_request();
    let mut s = state_typing(1, 0, "SQLite");
    s.states[0].selected = 2;
    let (_, action) = handle_key(&req, s, KeyCode::Enter, NO_MOD);
    match action {
        AskUserAction::Submit(AskUserResponse::Answered(answers)) => {
            assert_eq!(answers[0].answer, "SQLite");
        }
        other => panic!("expected Submit(Answered), got {other:?}"),
    }
}

#[test]
fn typing_enter_empty_returns_to_selecting() {
    let req = single_request();
    let mut s = state_typing(1, 0, "");
    s.states[0].selected = 2;
    let (s, action) = handle_key(&req, s, KeyCode::Enter, NO_MOD);
    assert_eq!(s.input_mode, InputMode::Selecting);
    assert_eq!(s.states[0].selected, 2); // stays on none
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn typing_esc_returns_to_selecting_stays_on_none() {
    let req = single_request();
    let mut s = state_typing(1, 0, "partial");
    s.states[0].selected = 2;
    let (s, action) = handle_key(&req, s, KeyCode::Esc, NO_MOD);
    assert_eq!(s.input_mode, InputMode::Selecting);
    assert_eq!(s.states[0].selected, 2); // stays on none
    assert_eq!(s.states[0].draft, "partial"); // draft preserved
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn typing_left_switches_question_preserves_draft() {
    let req = two_question_request();
    let mut s = state_typing(2, 0, "hello");
    s.states[0].selected = 2;
    let (s, action) = handle_key(&req, s, KeyCode::Left, NO_MOD);
    assert_eq!(s.active_question, 1);
    assert_eq!(s.input_mode, InputMode::Selecting);
    assert_eq!(s.states[0].draft, "hello"); // preserved
    assert_eq!(action, AskUserAction::Redraw);
}

#[test]
fn typing_ctrl_c_exits_run() {
    let req = single_request();
    let mut s = state_typing(1, 0, "abc");
    s.states[0].selected = 2;
    let (_, action) = handle_key(&req, s, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(action, AskUserAction::ExitRun);
}

// ===========================================================================
// Rendering tests
// ===========================================================================

#[test]
fn single_question_no_tab_bar() {
    let req = single_request();
    let s = state(1);
    let (output, _) = build_question_block(&req, &s, WIDE);
    assert!(output.contains("Which cache strategy?"));
    assert!(!output.contains("☐ Cache")); // no tab bar
    assert!(!output.contains("☑ Cache"));
}

#[test]
fn multi_question_shows_tab_bar() {
    let req = two_question_request();
    let s = state(2);
    let (output, _) = build_question_block(&req, &s, WIDE);
    assert!(output.contains("☐ Cache")); // active tab with checkbox
    assert!(output.contains("☐ Auth")); // inactive tab with checkbox
}

#[test]
fn answered_tab_shows_checked_box() {
    let req = two_question_request();
    let mut s = state(2);
    s.states[0].answer = Some("In-memory".into());
    s.active_question = 1;
    let (output, _) = build_question_block(&req, &s, WIDE);
    assert!(output.contains("☑ Cache")); // answered checkbox
    assert!(output.contains("☐ Auth")); // active unanswered
}

#[test]
fn options_appear_in_output() {
    let req = single_request();
    let s = state(1);
    let (output, _) = build_question_block(&req, &s, WIDE);
    assert!(output.contains("In-memory (Recommended)"));
    assert!(output.contains("Redis"));
    assert!(output.contains("None of the above"));
}

#[test]
fn first_option_highlighted() {
    let req = single_request();
    let s = state(1);
    let (output, _) = build_question_block(&req, &s, WIDE);
    assert!(output.contains("› 1."));
    assert!(!output.contains("› 2."));
}

#[test]
fn typing_mode_shows_inline_input() {
    let req = single_request();
    let mut s = state_typing(1, 0, "hello");
    s.states[0].selected = 2;
    let (output, _) = build_question_block(&req, &s, WIDE);
    assert!(output.contains("hello█"));
    assert!(output.contains("Esc back to list"));
    assert!(!output.contains("None of the above"));
    assert!(!output.contains("Type something"));
}

#[test]
fn typing_mode_empty_shows_cursor_only() {
    let req = single_request();
    let mut s = state_typing(1, 0, "");
    s.states[0].selected = 2;
    let (output, _) = build_question_block(&req, &s, WIDE);
    // Should show just cursor, no placeholder
    assert!(output.contains("█"));
    assert!(!output.contains("Type something"));
}

#[test]
fn selecting_on_none_shows_placeholder_with_cursor() {
    let req = single_request();
    let s = state_with_selected(1, 0, 2); // cursor on none
    let (output, _) = build_question_block(&req, &s, WIDE);
    assert!(output.contains("Type something..."));
    assert!(output.contains("█")); // cursor visible
    assert!(!output.contains("None of the above"));
}

#[test]
fn answered_option_shows_checkmark() {
    let req = single_request();
    let mut s = state(1);
    s.states[0].answer = Some("Redis".into());
    let (output, _) = build_question_block(&req, &s, WIDE);
    assert!(output.contains("Redis ✓"));
}

#[test]
fn custom_answer_shown_on_none_line() {
    let req = single_request();
    let mut s = state_with_selected(1, 0, 2); // cursor on none
    s.states[0].answer = Some("SQLite".into());
    let (output, _) = build_question_block(&req, &s, WIDE);
    assert!(output.contains("SQLite ✓"));
}

#[test]
fn custom_answer_shown_when_cursor_not_on_none() {
    let req = single_request();
    let mut s = state(1); // cursor on first option
    s.states[0].answer = Some("SQLite".into());
    let (output, _) = build_question_block(&req, &s, WIDE);
    // None line should show the custom answer instead of "None of the above"
    assert!(output.contains("SQLite"));
    assert!(output.contains("✓"));
}

#[test]
fn multi_question_footer_shows_arrow_hint() {
    let req = two_question_request();
    let s = state(2);
    let (output, _) = build_question_block(&req, &s, WIDE);
    assert!(output.contains("←→ question"));
}

#[test]
fn single_question_footer_no_arrow_hint() {
    let req = single_request();
    let s = state(1);
    let (output, _) = build_question_block(&req, &s, WIDE);
    assert!(!output.contains("←→ question"));
}

// ===========================================================================
// Rendering helpers
// ===========================================================================

#[test]
fn confirmation_contains_checkmark() {
    let text = build_confirmation("Redis");
    assert!(text.contains("✓"));
    assert!(text.contains("Redis"));
}

#[test]
fn skipped_contains_dash() {
    let text = build_skipped();
    assert!(text.contains("skipped"));
}

#[test]
fn physical_row_count_empty_is_one() {
    assert_eq!(physical_row_count("", 80), 1);
}

#[test]
fn physical_row_count_wraps() {
    assert_eq!(physical_row_count(&"a".repeat(81), 80), 2);
}
