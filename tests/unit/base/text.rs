use bendclaw::base::truncate_bytes_on_char_boundary;
use bendclaw::base::truncate_chars_with_ellipsis;

#[test]
fn truncate_chars_with_ellipsis_preserves_unicode_boundaries() {
    let text = "请分析这个项目，给出详细的改进方案。".repeat(20);
    let truncated = truncate_chars_with_ellipsis(&text, 120);
    assert!(truncated.ends_with("..."));
    assert_eq!(truncated.chars().count(), 120);
}

#[test]
fn truncate_bytes_on_char_boundary_preserves_unicode_boundaries() {
    let text = "abc的def";
    let truncated = truncate_bytes_on_char_boundary(text, 5);
    assert_eq!(truncated, "abc");
}
