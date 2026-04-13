use std::fs;
use std::path::PathBuf;

use bendclaw::cli::repl::skill_cmd::copy_dir_excluding_git;
use bendclaw::cli::repl::skill_cmd::extract_frontmatter;
use bendclaw::cli::repl::skill_cmd::is_valid_skill_name;
use bendclaw::cli::repl::skill_cmd::parse_github_source;
use bendclaw::cli::repl::skill_cmd::parse_variables_from_frontmatter;

// ---------------------------------------------------------------------------
// parse_github_source
// ---------------------------------------------------------------------------

#[test]
fn parse_owner_repo_shorthand() {
    let src = parse_github_source("databendlabs/bendskills").unwrap();
    assert_eq!(src.repo, "databendlabs/bendskills");
    assert!(src.git_ref.is_none());
    assert!(src.subpath.is_none());
}

#[test]
fn parse_github_url_plain() {
    let src = parse_github_source("https://github.com/databendlabs/bendskills").unwrap();
    assert_eq!(src.repo, "databendlabs/bendskills");
    assert!(src.git_ref.is_none());
    assert!(src.subpath.is_none());
}

#[test]
fn parse_github_url_trailing_slash() {
    let src = parse_github_source("https://github.com/databendlabs/bendskills/").unwrap();
    assert_eq!(src.repo, "databendlabs/bendskills");
    assert!(src.git_ref.is_none());
    assert!(src.subpath.is_none());
}

#[test]
fn parse_github_url_dot_git() {
    let src = parse_github_source("https://github.com/databendlabs/bendskills.git").unwrap();
    assert_eq!(src.repo, "databendlabs/bendskills");
    assert!(src.git_ref.is_none());
    assert!(src.subpath.is_none());
}

#[test]
fn parse_github_tree_url_with_subpath() {
    let src =
        parse_github_source("https://github.com/databendlabs/bendskills/tree/main/skills/feishu")
            .unwrap();
    assert_eq!(src.repo, "databendlabs/bendskills");
    assert_eq!(src.git_ref.as_deref(), Some("main"));
    assert_eq!(src.subpath, Some(PathBuf::from("skills/feishu")));
}

#[test]
fn parse_github_tree_url_branch_only() {
    let src =
        parse_github_source("https://github.com/databendlabs/bendskills/tree/develop").unwrap();
    assert_eq!(src.repo, "databendlabs/bendskills");
    assert_eq!(src.git_ref.as_deref(), Some("develop"));
    assert!(src.subpath.is_none());
}

#[test]
fn parse_github_tree_url_deep_subpath() {
    let src = parse_github_source("https://github.com/owner/repo/tree/v2/a/b/c").unwrap();
    assert_eq!(src.repo, "owner/repo");
    assert_eq!(src.git_ref.as_deref(), Some("v2"));
    assert_eq!(src.subpath, Some(PathBuf::from("a/b/c")));
}

#[test]
fn parse_invalid_source_single_word() {
    assert!(parse_github_source("foobar").is_err());
}

#[test]
fn parse_invalid_source_too_many_slashes() {
    assert!(parse_github_source("a/b/c").is_err());
}

#[test]
fn parse_invalid_source_domain_like() {
    // "github.com/foo" — owner contains a dot, rejected as shorthand
    assert!(parse_github_source("github.com/foo").is_err());
}

#[test]
fn parse_http_url() {
    let src = parse_github_source("http://github.com/owner/repo").unwrap();
    assert_eq!(src.repo, "owner/repo");
}

#[test]
fn parse_github_blob_url_rejected() {
    assert!(
        parse_github_source("https://github.com/owner/repo/blob/main/skills/feishu/SKILL.md")
            .is_err()
    );
}

#[test]
fn parse_github_commit_url_rejected() {
    assert!(parse_github_source("https://github.com/owner/repo/commit/abc123").is_err());
}

// ---------------------------------------------------------------------------
// is_valid_skill_name
// ---------------------------------------------------------------------------

#[test]
fn valid_skill_names() {
    assert!(is_valid_skill_name("feishu"));
    assert!(is_valid_skill_name("databend-cloud"));
    assert!(is_valid_skill_name("my_skill.v2"));
    assert!(is_valid_skill_name("A-Z_test.123"));
}

#[test]
fn invalid_skill_names() {
    assert!(!is_valid_skill_name(""));
    assert!(!is_valid_skill_name("."));
    assert!(!is_valid_skill_name(".."));
    assert!(!is_valid_skill_name("../etc"));
    assert!(!is_valid_skill_name("foo/bar"));
    assert!(!is_valid_skill_name("foo\\bar"));
    assert!(!is_valid_skill_name("hello world"));
}

// ---------------------------------------------------------------------------
// extract_frontmatter
// ---------------------------------------------------------------------------

#[test]
fn extract_frontmatter_basic() {
    let content = "---\ndescription: hello\n---\n\nBody text.";
    let fm = extract_frontmatter(content).unwrap();
    assert_eq!(fm.trim(), "description: hello");
}

#[test]
fn extract_frontmatter_with_leading_whitespace() {
    let content = "  ---\nfoo: bar\n---\n";
    let fm = extract_frontmatter(content).unwrap();
    assert_eq!(fm.trim(), "foo: bar");
}

#[test]
fn extract_frontmatter_missing_opening() {
    assert!(extract_frontmatter("no frontmatter here").is_none());
}

#[test]
fn extract_frontmatter_missing_closing() {
    assert!(extract_frontmatter("---\nfoo: bar\n").is_none());
}

// ---------------------------------------------------------------------------
// parse_variables_from_frontmatter
// ---------------------------------------------------------------------------

#[test]
fn parse_variables_single() {
    let fm = "\ndescription: test\nvariables:\n  - name: FOO\n    description: a foo token\n    required: true\n";
    let vars = parse_variables_from_frontmatter(fm);
    assert_eq!(vars.len(), 1);
    assert_eq!(vars[0].name, "FOO");
    assert_eq!(vars[0].description, "a foo token");
}

#[test]
fn parse_variables_multiple() {
    let fm =
        "\nvariables:\n  - name: A\n    description: first\n  - name: B\n    description: second\n";
    let vars = parse_variables_from_frontmatter(fm);
    assert_eq!(vars.len(), 2);
    assert_eq!(vars[0].name, "A");
    assert_eq!(vars[0].description, "first");
    assert_eq!(vars[1].name, "B");
    assert_eq!(vars[1].description, "second");
}

#[test]
fn parse_variables_no_description() {
    let fm = "\nvariables:\n  - name: TOKEN\n";
    let vars = parse_variables_from_frontmatter(fm);
    assert_eq!(vars.len(), 1);
    assert_eq!(vars[0].name, "TOKEN");
    assert_eq!(vars[0].description, "");
}

#[test]
fn parse_variables_empty_when_no_variables_section() {
    let fm = "\ndescription: just a skill\nname: test\n";
    let vars = parse_variables_from_frontmatter(fm);
    assert!(vars.is_empty());
}

#[test]
fn parse_variables_quoted_values() {
    let fm = "\nvariables:\n  - name: \"MY_VAR\"\n    description: 'a quoted desc'\n";
    let vars = parse_variables_from_frontmatter(fm);
    assert_eq!(vars.len(), 1);
    assert_eq!(vars[0].name, "MY_VAR");
    assert_eq!(vars[0].description, "a quoted desc");
}

// ---------------------------------------------------------------------------
// copy_dir_excluding_git
// ---------------------------------------------------------------------------

#[test]
fn copy_excludes_git_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");

    // Create source structure
    fs::create_dir_all(src.join(".git/objects")).unwrap();
    fs::write(src.join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    fs::write(src.join("SKILL.md"), "---\ndescription: test\n---\n").unwrap();
    fs::create_dir_all(src.join("scripts")).unwrap();
    fs::write(src.join("scripts/run.sh"), "#!/bin/bash").unwrap();

    copy_dir_excluding_git(&src, &dst).unwrap();

    // .git should not be copied
    assert!(!dst.join(".git").exists());
    // Other files should be copied
    assert!(dst.join("SKILL.md").exists());
    assert!(dst.join("scripts/run.sh").exists());
    assert_eq!(
        fs::read_to_string(dst.join("SKILL.md")).unwrap(),
        "---\ndescription: test\n---\n"
    );
}

#[test]
fn copy_preserves_nested_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");

    fs::create_dir_all(src.join("a/b/c")).unwrap();
    fs::write(src.join("a/b/c/deep.txt"), "deep").unwrap();

    copy_dir_excluding_git(&src, &dst).unwrap();

    assert_eq!(
        fs::read_to_string(dst.join("a/b/c/deep.txt")).unwrap(),
        "deep"
    );
}
