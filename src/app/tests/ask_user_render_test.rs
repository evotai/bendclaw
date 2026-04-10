use bend_engine::tools::AskUserOption;
use bend_engine::tools::AskUserRequest;
use bendclaw::cli::repl::ask_user::build_confirmation;
use bendclaw::cli::repl::ask_user::build_question_block;
use bendclaw::cli::repl::ask_user::build_skipped;
use bendclaw::cli::repl::ask_user::physical_row_count;

const WIDE: usize = 200;

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

#[test]
fn first_option_highlighted_when_selected_zero() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("› 1."));
    assert!(output.contains("  2.") || !output.contains("› 2."));
}

#[test]
fn second_option_highlighted_when_selected_one() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 1, WIDE);
    assert!(output.contains("› 2."));
}

#[test]
fn none_of_above_highlighted_when_selected_last() {
    let req = two_option_request();
    let none_idx = req.options.len();
    let (output, _lines) = build_question_block(&req, none_idx, WIDE);
    assert!(output.contains("› 0. None of the above"));
}

#[test]
fn question_text_appears_in_output() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("Which cache strategy?"));
}

#[test]
fn option_labels_appear_in_output() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("In-memory (Recommended)"));
    assert!(output.contains("Redis"));
}

#[test]
fn option_descriptions_appear_in_output() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("Zero deps, HashMap + TTL"));
    assert!(output.contains("Shared across instances"));
}

#[test]
fn none_of_above_always_present() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("None of the above"));
}

#[test]
fn footer_hint_shows_correct_range() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 0, WIDE);
    assert!(output.contains("1-2 pick"));

    let req3 = three_option_request();
    let (output3, _) = build_question_block(&req3, 0, WIDE);
    assert!(output3.contains("1-3 pick"));
}

#[test]
fn line_count_correct_for_two_options_wide_terminal() {
    let req = two_option_request();
    let (_output, lines) = build_question_block(&req, 0, WIDE);
    assert_eq!(lines, 9);
}

#[test]
fn line_count_correct_for_three_options_wide_terminal() {
    let req = three_option_request();
    let (_output, lines) = build_question_block(&req, 0, WIDE);
    assert_eq!(lines, 11);
}

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
    let line = "a".repeat(80);
    assert_eq!(physical_row_count(&line, 80), 1);
}

#[test]
fn physical_row_count_wraps_at_boundary() {
    let line = "a".repeat(81);
    assert_eq!(physical_row_count(&line, 80), 2);
}

#[test]
fn physical_row_count_cjk_double_width() {
    let line = "修".repeat(20);
    assert_eq!(physical_row_count(&line, 80), 1);
    let line = "修".repeat(41);
    assert_eq!(physical_row_count(&line, 80), 2);
}

#[test]
fn physical_row_count_cjk_narrow_terminal() {
    let line = "修复 plan mode";
    assert_eq!(physical_row_count(line, 10), 2);
}

#[test]
fn physical_row_count_ignores_ansi_codes() {
    let line = "\x1b[36m\x1b[1mhello\x1b[0m";
    assert_eq!(physical_row_count(line, 80), 1);
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
    let (_output, rows_wide) = build_question_block(&req, 0, WIDE);
    let (_output, rows_narrow) = build_question_block(&req, 0, 40);
    assert!(
        rows_narrow > rows_wide,
        "narrow ({rows_narrow}) should exceed wide ({rows_wide})"
    );
}

#[test]
fn wide_terminal_matches_logical_line_count() {
    let req = two_option_request();
    let (_output, rows) = build_question_block(&req, 0, WIDE);
    assert_eq!(rows, 9);
}
