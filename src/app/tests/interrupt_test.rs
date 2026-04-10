use bendclaw::cli::repl::interrupt::Action;
use bendclaw::cli::repl::interrupt::InterruptHandler;

#[test]
fn first_ctrl_c_on_empty_line_returns_clear() {
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::Clear);
}

#[test]
fn second_consecutive_ctrl_c_on_empty_line_returns_exit() {
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::Clear);
    assert_eq!(handler.on_interrupt(true), Action::Exit);
}

#[test]
fn input_between_ctrl_c_resets_state() {
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::Clear);
    handler.on_input();
    // After normal input, first Ctrl+C is Clear again
    assert_eq!(handler.on_interrupt(true), Action::Clear);
}

#[test]
fn ctrl_c_with_content_always_clears_and_resets() {
    let mut handler = InterruptHandler::new();
    // First Ctrl+C on empty → arms pending
    assert_eq!(handler.on_interrupt(true), Action::Clear);
    // Ctrl+C with content → clears line AND resets pending
    assert_eq!(handler.on_interrupt(false), Action::Clear);
    // So next empty Ctrl+C is Clear, not Exit
    assert_eq!(handler.on_interrupt(true), Action::Clear);
}

#[test]
fn ctrl_c_with_content_never_exits() {
    let mut handler = InterruptHandler::new();
    // Even many Ctrl+C with content never triggers Exit
    for _ in 0..10 {
        assert_eq!(handler.on_interrupt(false), Action::Clear);
    }
}

#[test]
fn content_ctrl_c_between_empty_ctrl_c_resets() {
    let mut handler = InterruptHandler::new();
    // Empty Ctrl+C → arms pending
    assert_eq!(handler.on_interrupt(true), Action::Clear);
    // User types something then Ctrl+C → resets pending
    assert_eq!(handler.on_interrupt(false), Action::Clear);
    // Empty Ctrl+C again → arms pending (not exit)
    assert_eq!(handler.on_interrupt(true), Action::Clear);
    // Empty Ctrl+C → now exits
    assert_eq!(handler.on_interrupt(true), Action::Exit);
}

#[test]
fn exit_resets_so_next_is_clear() {
    let mut handler = InterruptHandler::new();
    assert_eq!(handler.on_interrupt(true), Action::Clear);
    assert_eq!(handler.on_interrupt(true), Action::Exit);
    // After exit, internal state resets
    assert_eq!(handler.on_interrupt(true), Action::Clear);
}
