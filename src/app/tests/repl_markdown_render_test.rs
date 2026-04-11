use bendclaw::cli::repl::markdown::render::Renderer;
use streamdown_parser::inline::InlineElement;
use streamdown_parser::ListBullet;
use streamdown_parser::ParseEvent;

fn render_events(width: usize, events: &[ParseEvent]) -> String {
    let mut out = Vec::new();
    {
        let mut renderer = Renderer::new(&mut out, width);
        for event in events {
            renderer.render_event(event).unwrap_or_else(|err| {
                panic!("render_event failed: {err}");
            });
        }
    }
    String::from_utf8(out).unwrap_or_else(|err| panic!("utf8 conversion failed: {err}"))
}

#[test]
fn table_uses_box_drawing_borders() {
    let output = render_events(80, &[
        ParseEvent::TableHeader(vec!["Name".into(), "Value".into()]),
        ParseEvent::TableSeparator,
        ParseEvent::TableRow(vec!["foo".into(), "bar".into()]),
        ParseEvent::TableEnd,
    ]);

    assert!(output.contains("┌"));
    assert!(output.contains("┬"));
    assert!(output.contains("┐"));
    assert!(output.contains("├"));
    assert!(output.contains("┼"));
    assert!(output.contains("┤"));
    assert!(output.contains("└"));
    assert!(output.contains("┴"));
    assert!(output.contains("┘"));
}

#[test]
fn table_handles_wide_unicode_content() {
    let output = render_events(80, &[
        ParseEvent::TableHeader(vec!["列".into(), "值".into()]),
        ParseEvent::TableSeparator,
        ParseEvent::TableRow(vec!["中文".into(), "emoji 😀".into()]),
        ParseEvent::TableEnd,
    ]);

    assert!(output.contains("中文"));
    assert!(output.contains("emoji 😀"));
    assert!(output.contains("┌"));
}

#[test]
fn narrow_table_falls_back_to_vertical_format() {
    let output = render_events(20, &[
        ParseEvent::TableHeader(vec!["Column A".into(), "Column B".into()]),
        ParseEvent::TableSeparator,
        ParseEvent::TableRow(vec![
            "A very long value that should wrap vertically".into(),
            "Another long value".into(),
        ]),
        ParseEvent::TableEnd,
    ]);

    assert!(output.contains("Column A:"));
    assert!(output.contains("Column B:"));
    assert!(!output.contains("┌"));
    assert!(!output.contains("┬"));
}

#[test]
fn issue_references_are_rendered_without_osc_links() {
    let output = render_events(80, &[ParseEvent::Text("see evotai/bendclaw#123".into())]);

    assert!(!output.contains("https://github.com/evotai/bendclaw/issues/123"));
    assert!(output.contains("evotai/bendclaw#123"));
    assert!(!output.contains("\x1b]8;;"));
}

#[test]
fn inline_text_issue_references_are_rendered_without_osc_links() {
    let output = render_events(80, &[ParseEvent::InlineElements(vec![
        InlineElement::Text("refs evotai/bendclaw#456".into()),
    ])]);

    assert!(!output.contains("https://github.com/evotai/bendclaw/issues/456"));
    assert!(output.contains("evotai/bendclaw#456"));
    assert!(!output.contains("\x1b]8;;"));
}

#[test]
fn url_fragments_are_not_treated_as_issue_refs() {
    let output = render_events(80, &[ParseEvent::Text(
        "docs: https://example.com/page#section".into(),
    )]);

    assert!(!output.contains("github.com"));
    assert!(output.contains("page#section"));
}

// ---------------------------------------------------------------------------
// Ordered list numbering
// ---------------------------------------------------------------------------

/// Helper: strip ANSI escape sequences so we can assert on visible text.
fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until 'm' (SGR) or 'ST' (hyperlink OSC)
            while let Some(&nc) = chars.peek() {
                let _ = chars.next();
                if nc == 'm' || (nc.is_ascii_uppercase() && nc != 'O') {
                    break;
                }
                // OSC hyperlink: \x1b]8;;...\x1b\\ — skip to backslash
                if nc == '\\' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn ordered_list_all_ones_renders_sequential() {
    // Simulates LLM streaming where every item has `1.`
    let output = render_events(80, &[
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "first".into(),
        },
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "second".into(),
        },
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "third".into(),
        },
    ]);
    let plain = strip_ansi(&output);
    assert!(
        plain.contains("1. first"),
        "expected '1. first', got:\n{plain}"
    );
    assert!(
        plain.contains("2. second"),
        "expected '2. second', got:\n{plain}"
    );
    assert!(
        plain.contains("3. third"),
        "expected '3. third', got:\n{plain}"
    );
}

#[test]
fn nested_ordered_lists_have_independent_counters() {
    let output = render_events(80, &[
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "parent 1".into(),
        },
        ParseEvent::ListItem {
            indent: 1,
            bullet: ListBullet::Ordered(1),
            content: "child a".into(),
        },
        ParseEvent::ListItem {
            indent: 1,
            bullet: ListBullet::Ordered(1),
            content: "child b".into(),
        },
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "parent 2".into(),
        },
    ]);
    let plain = strip_ansi(&output);
    assert!(plain.contains("1. parent 1"), "got:\n{plain}");
    assert!(
        plain.contains("1. child a"),
        "child should restart at 1, got:\n{plain}"
    );
    assert!(plain.contains("2. child b"), "got:\n{plain}");
    assert!(
        plain.contains("2. parent 2"),
        "parent should continue at 2, got:\n{plain}"
    );
}

#[test]
fn list_end_then_new_list_restarts_numbering() {
    let output = render_events(80, &[
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "a".into(),
        },
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "b".into(),
        },
        ParseEvent::ListEnd,
        // A heading breaks the list context
        ParseEvent::Heading {
            level: 2,
            content: "next section".into(),
        },
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "c".into(),
        },
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "d".into(),
        },
    ]);
    let plain = strip_ansi(&output);
    // First list: 1, 2
    assert!(plain.contains("1. a"), "got:\n{plain}");
    assert!(plain.contains("2. b"), "got:\n{plain}");
    // Second list restarts: 1, 2
    assert!(
        plain.contains("1. c"),
        "new list should restart at 1, got:\n{plain}"
    );
    assert!(plain.contains("2. d"), "got:\n{plain}");
}

#[test]
fn mixed_ordered_and_unordered_do_not_interfere() {
    let output = render_events(80, &[
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "one".into(),
        },
        ParseEvent::ListItem {
            indent: 1,
            bullet: ListBullet::Dash,
            content: "bullet".into(),
        },
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "two".into(),
        },
    ]);
    let plain = strip_ansi(&output);
    assert!(plain.contains("1. one"), "got:\n{plain}");
    assert!(plain.contains("- bullet"), "got:\n{plain}");
    assert!(
        plain.contains("2. two"),
        "ordered should continue, got:\n{plain}"
    );
}

#[test]
fn same_indent_ordered_then_unordered_then_ordered_restarts() {
    // ordered list → unordered at same indent → new ordered should restart at 1
    let output = render_events(80, &[
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "first".into(),
        },
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "second".into(),
        },
        ParseEvent::ListEnd,
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Dash,
            content: "bullet a".into(),
        },
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Dash,
            content: "bullet b".into(),
        },
        ParseEvent::ListEnd,
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "new first".into(),
        },
        ParseEvent::ListItem {
            indent: 0,
            bullet: ListBullet::Ordered(1),
            content: "new second".into(),
        },
    ]);
    let plain = strip_ansi(&output);
    // First ordered list: 1, 2
    assert!(plain.contains("1. first"), "got:\n{plain}");
    assert!(plain.contains("2. second"), "got:\n{plain}");
    // Unordered list
    assert!(plain.contains("- bullet a"), "got:\n{plain}");
    assert!(plain.contains("- bullet b"), "got:\n{plain}");
    // New ordered list should restart at 1
    assert!(
        plain.contains("1. new first"),
        "new ordered list should restart at 1, got:\n{plain}"
    );
    assert!(plain.contains("2. new second"), "got:\n{plain}");
}
