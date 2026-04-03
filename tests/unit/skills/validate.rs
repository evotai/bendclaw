//! Tests for skill name, file path, and size validation.

use bendclaw::skills::definition::skill::Skill;
use bendclaw::skills::definition::skill::SkillFile;
use bendclaw::tools::ToolId;

// ═══════════════════════════════════════════════════════════════════════════════
// Skill::validate_name — accepted
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn name_simple_accepted() {
    assert!(Skill::validate_name("ab").is_ok());
    assert!(Skill::validate_name("a1").is_ok());
    assert!(Skill::validate_name("my-skill").is_ok());
}

#[test]
fn name_with_digits_accepted() {
    assert!(Skill::validate_name("json2csv").is_ok());
    assert!(Skill::validate_name("v2-parser").is_ok());
}

#[test]
fn name_max_length_accepted() {
    let name = "a".repeat(64);
    assert!(Skill::validate_name(&name).is_ok());
}

#[test]
fn name_min_length_accepted() {
    assert!(Skill::validate_name("ab").is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Skill::validate_name — rejected: length
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn name_empty_rejected() {
    assert!(Skill::validate_name("").is_err());
}

#[test]
fn name_single_char_rejected() {
    assert!(Skill::validate_name("a").is_err());
}

#[test]
fn name_too_long_rejected() {
    let name = "a".repeat(65);
    assert!(Skill::validate_name(&name).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Skill::validate_name — rejected: character rules
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn name_uppercase_rejected() {
    assert!(Skill::validate_name("My-Skill").is_err());
    assert!(Skill::validate_name("ALLCAPS").is_err());
}

#[test]
fn name_underscore_rejected() {
    assert!(Skill::validate_name("my_skill").is_err());
}

#[test]
fn name_dot_rejected() {
    assert!(Skill::validate_name("my.skill").is_err());
}

#[test]
fn name_space_rejected() {
    assert!(Skill::validate_name("my skill").is_err());
}

#[test]
fn name_special_chars_rejected() {
    assert!(Skill::validate_name("my@skill").is_err());
    assert!(Skill::validate_name("my+skill").is_err());
    assert!(Skill::validate_name("my=skill").is_err());
}

#[test]
fn name_unicode_rejected() {
    assert!(Skill::validate_name("日本語").is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Skill::validate_name — rejected: dash placement
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn name_leading_dash_rejected() {
    assert!(Skill::validate_name("-my-skill").is_err());
}

#[test]
fn name_trailing_dash_rejected() {
    assert!(Skill::validate_name("my-skill-").is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Skill::validate_name — rejected: path traversal
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn name_dotdot_rejected() {
    assert!(Skill::validate_name("..ab").is_err());
}

#[test]
fn name_slash_rejected() {
    assert!(Skill::validate_name("foo/bar").is_err());
}

#[test]
fn name_backslash_rejected() {
    assert!(Skill::validate_name("foo\\bar").is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Skill::validate_name — rejected: reserved names
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn name_all_reserved_tool_ids_rejected() {
    for id in ToolId::ALL {
        let name = id.as_str();
        // Only test names that pass the character rules (no underscores).
        // Names with underscores are already rejected by the char check.
        if name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
            && name.len() >= 2
            && !name.starts_with('-')
            && !name.ends_with('-')
        {
            assert!(
                Skill::validate_name(name).is_err(),
                "reserved name '{name}' should be rejected"
            );
        }
    }
}

#[test]
fn name_bash_reserved() {
    assert!(Skill::validate_name("bash").is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Skill::validate_file_path — accepted
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn path_scripts_py_accepted() {
    assert!(Skill::validate_file_path("scripts/run.py").is_ok());
}

#[test]
fn path_scripts_sh_accepted() {
    assert!(Skill::validate_file_path("scripts/run.sh").is_ok());
}

#[test]
fn path_scripts_nested_accepted() {
    assert!(Skill::validate_file_path("scripts/utils/helper.py").is_ok());
}

#[test]
fn path_references_md_accepted() {
    assert!(Skill::validate_file_path("references/guide.md").is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Skill::validate_file_path — rejected: prefix/extension mismatch
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn path_md_in_scripts_rejected() {
    assert!(Skill::validate_file_path("scripts/readme.md").is_err());
}

#[test]
fn path_py_in_references_rejected() {
    assert!(Skill::validate_file_path("references/evil.py").is_err());
}

#[test]
fn path_sh_in_references_rejected() {
    assert!(Skill::validate_file_path("references/evil.sh").is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Skill::validate_file_path — rejected: bad prefix
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn path_no_prefix_rejected() {
    assert!(Skill::validate_file_path("run.py").is_err());
}

#[test]
fn path_wrong_prefix_rejected() {
    assert!(Skill::validate_file_path("bin/evil.sh").is_err());
    assert!(Skill::validate_file_path("config/settings.py").is_err());
    assert!(Skill::validate_file_path("src/main.py").is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Skill::validate_file_path — rejected: bad extension
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn path_rb_extension_rejected() {
    assert!(Skill::validate_file_path("scripts/run.rb").is_err());
}

#[test]
fn path_js_extension_rejected() {
    assert!(Skill::validate_file_path("scripts/run.js").is_err());
}

#[test]
fn path_no_extension_rejected() {
    assert!(Skill::validate_file_path("scripts/Makefile").is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Skill::validate_file_path — rejected: traversal and absolute
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn path_empty_rejected() {
    assert!(Skill::validate_file_path("").is_err());
}

#[test]
fn path_absolute_rejected() {
    assert!(Skill::validate_file_path("/etc/passwd").is_err());
}

#[test]
fn path_backslash_absolute_rejected() {
    assert!(Skill::validate_file_path("\\etc\\passwd").is_err());
}

#[test]
fn path_dotdot_traversal_rejected() {
    assert!(Skill::validate_file_path("scripts/../../../etc/passwd").is_err());
}

#[test]
fn path_dotdot_in_middle_rejected() {
    assert!(Skill::validate_file_path("scripts/foo/../bar.py").is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Skill::validate_size — accepted
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn size_normal_accepted() {
    let files = vec![SkillFile {
        path: "scripts/run.py".into(),
        body: "print('hi')".into(),
    }];
    assert!(Skill::validate_size("body", &files).is_ok());
}

#[test]
fn size_empty_content_and_no_files_accepted() {
    assert!(Skill::validate_size("", &[]).is_ok());
}

#[test]
fn size_at_content_limit_accepted() {
    let content = "x".repeat(10 * 1024);
    assert!(Skill::validate_size(&content, &[]).is_ok());
}

#[test]
fn size_at_file_limit_accepted() {
    let files = vec![SkillFile {
        path: "scripts/run.py".into(),
        body: "x".repeat(50 * 1024),
    }];
    assert!(Skill::validate_size("ok", &files).is_ok());
}

#[test]
fn size_at_total_limit_accepted() {
    let files: Vec<SkillFile> = (0..4)
        .map(|i| SkillFile {
            path: format!("scripts/f{i}.py"),
            body: "x".repeat(50 * 1024),
        })
        .collect();
    assert!(Skill::validate_size("ok", &files).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Skill::validate_size — rejected
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn size_content_over_limit_rejected() {
    let content = "x".repeat(10 * 1024 + 1);
    assert!(Skill::validate_size(&content, &[]).is_err());
}

#[test]
fn size_single_file_over_limit_rejected() {
    let files = vec![SkillFile {
        path: "scripts/run.py".into(),
        body: "x".repeat(50 * 1024 + 1),
    }];
    assert!(Skill::validate_size("ok", &files).is_err());
}

#[test]
fn size_total_files_over_limit_rejected() {
    let files: Vec<SkillFile> = (0..5)
        .map(|i| SkillFile {
            path: format!("scripts/f{i}.py"),
            body: "x".repeat(45 * 1024),
        })
        .collect();
    assert!(Skill::validate_size("ok", &files).is_err());
}

#[test]
fn size_many_small_files_over_total_rejected() {
    let files: Vec<SkillFile> = (0..210)
        .map(|i| SkillFile {
            path: format!("scripts/f{i}.py"),
            body: "x".repeat(1024),
        })
        .collect();
    assert!(Skill::validate_size("ok", &files).is_err());
}
