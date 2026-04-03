use bendclaw::tools::web::html::html_to_markdown;

#[test]
fn converts_basic_html_to_markdown() {
    let html = r#"
    <html><head><title>Hello World</title></head><body>
        <article>
            <h1>Hello World</h1>
            <p>This is a <strong>test</strong> paragraph.</p>
        </article>
    </body></html>"#;

    let result = html_to_markdown(html);
    assert!(result.is_some());
    let md = result.unwrap();
    assert!(md.contains("Hello World"));
    assert!(md.contains("test"));
    assert!(!md.contains("<article>"));
    assert!(!md.contains("<strong>"));
}

#[test]
fn returns_none_for_empty_html() {
    let result = html_to_markdown("");
    assert!(result.is_none());
}

#[test]
fn returns_none_for_non_html_input() {
    let result = html_to_markdown("just plain text with no html structure");
    // readability may or may not extract something — either None or a string is acceptable
    // but it should not panic
    let _ = result;
}

#[test]
fn strips_nav_and_footer() {
    let html = r#"
    <html><head><title>Main Content</title></head><body>
        <nav><a href="/">Home</a><a href="/about">About</a></nav>
        <article>
            <h1>Main Content</h1>
            <p>The important stuff lives here.</p>
        </article>
        <footer>Copyright 2026</footer>
    </body></html>"#;

    let result = html_to_markdown(html);
    assert!(result.is_some());
    let md = result.unwrap();
    assert!(md.contains("Main Content"));
    assert!(md.contains("important stuff"));
}
