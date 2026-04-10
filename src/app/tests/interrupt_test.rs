use bendclaw::cli::repl::interrupt::Action;
use bendclaw::cli::repl::interrupt::InterruptHandler;

#[test]
fn first_ctrl_c_on_empty_returns_show_hint() {
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
}

#[test]
fn consecutive_on_interrupt_true_returns_exit() {
    // Pure state-machine consistency: two on_interrupt(true) in a row.
    // In practice the second Ctrl+C during the hint window goes through
    // on_hint_ctrl_c(), but this validates internal pending logic.
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
    assert_eq!(handler.on_interrupt(true), Action::Exit);
}

#[test]
fn input_between_ctrl_c_resets_state() {
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
    handler.on_input();
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
}

#[test]
fn ctrl_c_with_content_always_clears_and_resets() {
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
    assert_eq!(handler.on_interrupt(false), Action::Clear);
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
}

#[test]
fn ctrl_c_with_content_never_exits() {
    let mut handler = InterruptHandler::new();
    for _ in 0..10 {
        assert_eq!(handler.on_interrupt(false), Action::Clear);
    }
}

#[test]
fn content_ctrl_c_between_empty_ctrl_c_resets() {
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
    assert_eq!(handler.on_interrupt(false), Action::Clear);
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
    assert_eq!(handler.on_interrupt(true), Action::Exit);
}

#[test]
fn exit_resets_so_next_is_show_hint() {
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
    assert_eq!(handler.on_interrupt(true), Action::Exit);
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
}

// ---------------------------------------------------------------------------
// Hint lifecycle
// ---------------------------------------------------------------------------

#[test]
fn hint_timeout_resets_so_next_ctrl_c_shows_hint_again() {
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
    handler.on_hint_timeout();
    // 1-second window expired → pending reset, next Ctrl+C starts over
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
}

#[test]
fn hint_timeout_then_input_resets() {
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
    handler.on_hint_timeout();
    handler.on_input();
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
}

#[test]
fn hint_ctrl_c_returns_exit() {
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
    assert_eq!(handler.on_hint_ctrl_c(), Action::Exit);
}

#[test]
fn hint_ctrl_c_resets_state() {
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
    assert_eq!(handler.on_hint_ctrl_c(), Action::Exit);
    assert_eq!(handler.on_interrupt(true), Action::ShowHint);
}
