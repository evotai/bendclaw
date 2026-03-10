//! Single-skill sync tests: one remote skill, one local node.
//!
//! Covers: publish, field round-trip, all FS semantics (create/update/delete/
//! rename/move), metadata-triggered refetch, delete cleanup, idempotent sync.

use anyhow::Context;
use anyhow::Result;
use bendclaw_test_harness::setup::uid;

use super::fixtures::skill_plain;
use super::fixtures::skill_with_files;
use super::fixtures::skill_with_meta;
use super::fixtures::Cluster;

// ─────────────────────────────────────────────────────────────────────────────
// ADD / PUBLISH
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn locals_are_empty_before_first_sync() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_plain(&name, "1.0.0", "body"))
        .await?;

    c.local_a.assert_not_cached(&name);
    assert_eq!(c.local_a.skill_count(), 0);
    Ok(())
}

#[tokio::test]
async fn all_fields_survive_round_trip() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_plain(&name, "3.1.4", "## Docs\nDo the thing."))
        .await?;
    c.local_a.sync().await?;

    let sk = c.local_a.get(&name).context("cache miss")?;
    assert_eq!(sk.name, name);
    assert_eq!(sk.version, "3.1.4");
    assert_eq!(sk.description, format!("description of {name}"));
    assert_eq!(sk.timeout, 45);
    assert!(!sk.executable);
    assert_eq!(sk.content, "## Docs\nDo the thing.");

    let md = std::fs::read_to_string(c.local_a.skill_dir(&name).join("SKILL.md"))?;
    assert!(md.contains("3.1.4"));
    assert!(md.contains(&format!("description of {name}")));
    assert!(md.contains("45"));
    assert!(md.contains("## Docs\nDo the thing."));
    Ok(())
}

#[tokio::test]
async fn skill_without_files_creates_skill_md_only() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_plain(&name, "1.0.0", "body"))
        .await?;
    c.local_a.sync().await?;

    c.local_a.assert_skill_dir_exists(&name);
    assert!(!c.local_a.is_executable(&name));

    let entries: Vec<_> = std::fs::read_dir(c.local_a.skill_dir(&name))?
        .map(|e| e.map(|de| de.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    assert_eq!(entries.len(), 1, "only SKILL.md should exist");
    assert_eq!(entries[0], "SKILL.md");
    Ok(())
}

#[tokio::test]
async fn skill_with_files_creates_full_directory_tree() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![
            ("scripts/run.py", "print('run')"),
            ("config/defaults.json", r#"{"k":"v"}"#),
            ("references/usage.md", "# Usage"),
        ]))
        .await?;
    c.local_a.sync().await?;

    assert!(c.local_a.is_executable(&name));
    c.local_a
        .assert_file(&name, "scripts/run.py", "print('run')");
    c.local_a
        .assert_file(&name, "config/defaults.json", r#"{"k":"v"}"#);
    c.local_a
        .assert_file(&name, "references/usage.md", "# Usage");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// FS CREATE
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn skill_dir_and_skill_md_created_on_publish() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_plain(&name, "1.0.0", "body"))
        .await?;
    c.local_a.sync().await?;

    c.local_a.assert_skill_dir_exists(&name);
    let md = std::fs::read_to_string(c.local_a.skill_dir(&name).join("SKILL.md"))?;
    assert!(md.contains("name:"), "SKILL.md must contain front-matter");
    assert!(md.contains(&name));
    Ok(())
}

#[tokio::test]
async fn file_in_skill_root_is_created() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![
            ("README.md", "# top-level readme"),
            ("LICENSE", "MIT"),
        ]))
        .await?;
    c.local_a.sync().await?;

    c.local_a
        .assert_file(&name, "README.md", "# top-level readme");
    c.local_a.assert_file(&name, "LICENSE", "MIT");
    Ok(())
}

#[tokio::test]
async fn single_level_subdir_is_created() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![(
            "scripts/run.py",
            "print('run')",
        )]))
        .await?;
    c.local_a.sync().await?;

    assert!(c.local_a.skill_dir(&name).join("scripts").is_dir());
    c.local_a
        .assert_file(&name, "scripts/run.py", "print('run')");
    Ok(())
}

#[tokio::test]
async fn multi_level_subdir_is_created() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![(
            "data/processed/results.json",
            r#"{"ok":true}"#,
        )]))
        .await?;
    c.local_a.sync().await?;

    assert!(c.local_a.skill_dir(&name).join("data/processed").is_dir());
    c.local_a
        .assert_file(&name, "data/processed/results.json", r#"{"ok":true}"#);
    Ok(())
}

#[tokio::test]
async fn multiple_files_in_same_subdir_are_all_created() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![
            ("scripts/run.py", "print('main')"),
            ("scripts/helper.py", "def greet(n): return n"),
            ("scripts/constants.py", "TIMEOUT = 30"),
        ]))
        .await?;
    c.local_a.sync().await?;

    c.local_a
        .assert_file(&name, "scripts/run.py", "print('main')");
    c.local_a
        .assert_file(&name, "scripts/helper.py", "def greet(n): return n");
    c.local_a
        .assert_file(&name, "scripts/constants.py", "TIMEOUT = 30");
    Ok(())
}

#[tokio::test]
async fn multiple_independent_subdirs_are_all_created() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![
            ("scripts/run.py", "print('run')"),
            ("config/settings.json", r#"{"v":1}"#),
            ("references/guide.md", "# Guide"),
            ("tests/test_run.py", "def test_main(): pass"),
        ]))
        .await?;
    c.local_a.sync().await?;

    for dir in ["scripts", "config", "references", "tests"] {
        assert!(
            c.local_a.skill_dir(&name).join(dir).is_dir(),
            "{dir}/ must exist"
        );
    }
    c.local_a
        .assert_file(&name, "scripts/run.py", "print('run')");
    c.local_a
        .assert_file(&name, "config/settings.json", r#"{"v":1}"#);
    c.local_a
        .assert_file(&name, "references/guide.md", "# Guide");
    c.local_a
        .assert_file(&name, "tests/test_run.py", "def test_main(): pass");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// FS UPDATE
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn new_file_added_to_existing_subdir() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![(
            "scripts/run.py",
            "print('main')",
        )]))
        .await?;
    c.local_a.sync().await?;

    c.remote
        .publish(&skill_with_files(&name, "1.1.0", vec![
            ("scripts/run.py", "print('main')"),
            ("scripts/utils.py", "def helper(): pass"),
        ]))
        .await?;
    c.local_a.sync().await?;

    c.local_a
        .assert_file(&name, "scripts/run.py", "print('main')");
    c.local_a
        .assert_file(&name, "scripts/utils.py", "def helper(): pass");
    Ok(())
}

#[tokio::test]
async fn new_subdir_added_to_skill() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![(
            "scripts/run.py",
            "print('run')",
        )]))
        .await?;
    c.local_a.sync().await?;

    c.remote
        .publish(&skill_with_files(&name, "1.1.0", vec![
            ("scripts/run.py", "print('run')"),
            ("tests/test_run.py", "def test_main(): assert True"),
        ]))
        .await?;
    c.local_a.sync().await?;

    c.local_a
        .assert_file(&name, "scripts/run.py", "print('run')");
    c.local_a
        .assert_file(&name, "tests/test_run.py", "def test_main(): assert True");
    assert!(c.local_a.skill_dir(&name).join("tests").is_dir());
    Ok(())
}

#[tokio::test]
async fn deeply_nested_subdir_added() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![(
            "scripts/run.py",
            "pass",
        )]))
        .await?;
    c.local_a.sync().await?;

    c.remote
        .publish(&skill_with_files(&name, "1.1.0", vec![
            ("scripts/run.py", "pass"),
            ("data/raw/2024/input.csv", "col1,col2\n1,2"),
        ]))
        .await?;
    c.local_a.sync().await?;

    assert!(c.local_a.skill_dir(&name).join("data/raw/2024").is_dir());
    c.local_a
        .assert_file(&name, "data/raw/2024/input.csv", "col1,col2\n1,2");
    Ok(())
}

#[tokio::test]
async fn file_content_updated_in_subdir() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![
            ("scripts/run.py", "print('v1')"),
            ("config/settings.json", r#"{"v":1}"#),
        ]))
        .await?;
    c.local_a.sync().await?;

    c.remote
        .publish(&skill_with_files(&name, "1.1.0", vec![
            ("scripts/run.py", "print('v2')"),
            ("config/settings.json", r#"{"v":1}"#),
        ]))
        .await?;
    c.local_a.sync().await?;

    c.local_a
        .assert_file(&name, "scripts/run.py", "print('v2')");
    c.local_a
        .assert_file(&name, "config/settings.json", r#"{"v":1}"#);
    Ok(())
}

#[tokio::test]
async fn root_level_file_content_updated() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![
            ("README.md", "# v1"),
            ("scripts/run.py", "pass"),
        ]))
        .await?;
    c.local_a.sync().await?;

    c.remote
        .publish(&skill_with_files(&name, "1.1.0", vec![
            ("README.md", "# v2"),
            ("scripts/run.py", "pass"),
        ]))
        .await?;
    c.local_a.sync().await?;

    c.local_a.assert_file(&name, "README.md", "# v2");
    c.local_a.assert_file(&name, "scripts/run.py", "pass");
    Ok(())
}

#[tokio::test]
async fn file_removed_from_skill() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![
            ("scripts/run.py", "print('run')"),
            ("config/settings.json", r#"{"debug":true}"#),
        ]))
        .await?;
    c.local_a.sync().await?;

    c.remote
        .publish(&skill_with_files(&name, "1.1.0", vec![(
            "scripts/run.py",
            "print('run')",
        )]))
        .await?;
    c.local_a.sync().await?;

    c.local_a
        .assert_file(&name, "scripts/run.py", "print('run')");
    c.local_a.assert_no_path(&name, "config/settings.json");
    Ok(())
}

#[tokio::test]
async fn entire_subdir_removed_from_skill() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![
            ("scripts/run.py", "print('run')"),
            ("config/a.json", "{}"),
            ("config/b.json", "{}"),
        ]))
        .await?;
    c.local_a.sync().await?;

    c.remote
        .publish(&skill_with_files(&name, "2.0.0", vec![(
            "scripts/run.py",
            "print('run')",
        )]))
        .await?;
    c.local_a.sync().await?;

    c.local_a
        .assert_file(&name, "scripts/run.py", "print('run')");
    c.local_a.assert_no_path(&name, "config");
    Ok(())
}

#[tokio::test]
async fn file_renamed_within_same_dir() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![(
            "scripts/run.py",
            "print('run')",
        )]))
        .await?;
    c.local_a.sync().await?;

    c.remote
        .publish(&skill_with_files(&name, "1.1.0", vec![(
            "scripts/main.py",
            "print('run')",
        )]))
        .await?;
    c.local_a.sync().await?;

    c.local_a
        .assert_file(&name, "scripts/main.py", "print('run')");
    c.local_a.assert_no_path(&name, "scripts/run.py");
    Ok(())
}

#[tokio::test]
async fn file_moved_to_different_subdir() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![
            ("lib/utils.py", "def helper(): pass"),
            ("scripts/run.py", "from lib.utils import helper"),
        ]))
        .await?;
    c.local_a.sync().await?;

    c.remote
        .publish(&skill_with_files(&name, "1.1.0", vec![
            ("scripts/utils.py", "def helper(): pass"),
            ("scripts/run.py", "from scripts.utils import helper"),
        ]))
        .await?;
    c.local_a.sync().await?;

    c.local_a
        .assert_file(&name, "scripts/utils.py", "def helper(): pass");
    c.local_a
        .assert_file(&name, "scripts/run.py", "from scripts.utils import helper");
    c.local_a.assert_no_path(&name, "lib");
    Ok(())
}

#[tokio::test]
async fn root_level_file_removed_while_subdir_remains() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![
            ("README.md", "# docs"),
            ("scripts/run.py", "print('run')"),
        ]))
        .await?;
    c.local_a.sync().await?;

    c.remote
        .publish(&skill_with_files(&name, "1.1.0", vec![(
            "scripts/run.py",
            "print('run')",
        )]))
        .await?;
    c.local_a.sync().await?;

    c.local_a.assert_no_path(&name, "README.md");
    c.local_a
        .assert_file(&name, "scripts/run.py", "print('run')");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// METADATA-TRIGGERED REFETCH
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn content_change_triggers_refetch() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_plain(&name, "1.0.0", "original"))
        .await?;
    c.local_a.sync().await?;
    c.local_a.assert_content(&name, "original");

    c.remote
        .publish(&skill_plain(&name, "1.0.0", "updated"))
        .await?;
    c.local_a.sync().await?;

    c.local_a.assert_content(&name, "updated");
    let md = std::fs::read_to_string(c.local_a.skill_dir(&name).join("SKILL.md"))?;
    assert!(md.contains("updated"));
    assert!(!md.contains("original"));
    Ok(())
}

#[tokio::test]
async fn file_content_change_with_same_version_triggers_refetch() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![(
            "scripts/run.py",
            "print('v1')",
        )]))
        .await?;
    c.local_a.sync().await?;
    c.local_a
        .assert_file(&name, "scripts/run.py", "print('v1')");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![(
            "scripts/run.py",
            "print('v2')",
        )]))
        .await?;
    c.local_a.sync().await?;

    c.local_a
        .assert_file(&name, "scripts/run.py", "print('v2')");
    Ok(())
}

#[tokio::test]
async fn version_bump_triggers_refetch() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_plain(&name, "1.0.0", "body"))
        .await?;
    c.local_a.sync().await?;
    c.local_a.assert_version(&name, "1.0.0");

    c.remote
        .publish(&skill_plain(&name, "2.0.0", "body"))
        .await?;
    c.local_a.sync().await?;

    c.local_a.assert_version(&name, "2.0.0");
    let md = std::fs::read_to_string(c.local_a.skill_dir(&name).join("SKILL.md"))?;
    assert!(md.contains("2.0.0"));
    Ok(())
}

#[tokio::test]
async fn description_change_triggers_refetch() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_meta(
            &name,
            "1.0.0",
            "old description",
            30,
            "body",
        ))
        .await?;
    c.local_a.sync().await?;
    c.local_a.assert_description(&name, "old description");

    c.remote
        .publish(&skill_with_meta(
            &name,
            "1.0.0",
            "new description",
            30,
            "body",
        ))
        .await?;
    c.local_a.sync().await?;

    c.local_a.assert_description(&name, "new description");
    let md = std::fs::read_to_string(c.local_a.skill_dir(&name).join("SKILL.md"))?;
    assert!(md.contains("new description"));
    Ok(())
}

#[tokio::test]
async fn timeout_change_triggers_refetch() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_meta(&name, "1.0.0", "desc", 30, "body"))
        .await?;
    c.local_a.sync().await?;
    c.local_a.assert_timeout(&name, 30);

    c.remote
        .publish(&skill_with_meta(&name, "1.0.0", "desc", 90, "body"))
        .await?;
    c.local_a.sync().await?;

    c.local_a.assert_timeout(&name, 90);
    let md = std::fs::read_to_string(c.local_a.skill_dir(&name).join("SKILL.md"))?;
    assert!(md.contains("90"));
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// DELETE
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn delete_evicts_skill_from_cache() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_plain(&name, "1.0.0", "body"))
        .await?;
    c.local_a.sync().await?;
    c.local_a.assert_cached(&name);

    c.remote.remove(&name).await?;
    c.local_a.sync().await?;

    c.local_a.assert_not_cached(&name);
    assert_eq!(c.local_a.skill_count(), 0);
    Ok(())
}

#[tokio::test]
async fn delete_removes_skill_dir_from_fs() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_plain(&name, "1.0.0", "body"))
        .await?;
    c.local_a.sync().await?;
    c.local_a.assert_skill_dir_exists(&name);

    c.remote.remove(&name).await?;
    c.local_a.sync().await?;

    c.local_a.assert_skill_dir_absent(&name);
    Ok(())
}

#[tokio::test]
async fn delete_removes_all_files_recursively() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![
            ("scripts/run.py", "print('run')"),
            ("config/a.json", "{}"),
            ("data/processed/results.json", r#"{"ok":true}"#),
        ]))
        .await?;
    c.local_a.sync().await?;
    assert!(c.local_a.skill_dir(&name).join("data/processed").is_dir());

    c.remote.remove(&name).await?;
    c.local_a.sync().await?;

    c.local_a.assert_skill_dir_absent(&name);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// FULL LIFECYCLE
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn file_set_full_lifecycle() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    // v1: no files.
    c.remote
        .publish(&skill_plain(&name, "1.0.0", "body"))
        .await?;
    c.local_a.sync().await?;
    c.local_a.assert_skill_dir_exists(&name);
    assert!(!c.local_a.is_executable(&name));

    // v2: three files across two dirs.
    c.remote
        .publish(&skill_with_files(&name, "2.0.0", vec![
            ("scripts/run.py", "print('run')"),
            ("config/main.json", r#"{"k":"v"}"#),
            ("config/extra.json", r#"{"x":1}"#),
        ]))
        .await?;
    c.local_a.sync().await?;
    assert!(c.local_a.is_executable(&name));
    c.local_a
        .assert_file(&name, "scripts/run.py", "print('run')");
    c.local_a
        .assert_file(&name, "config/extra.json", r#"{"x":1}"#);

    // v3: drop config/extra.json, add a root-level file.
    c.remote
        .publish(&skill_with_files(&name, "3.0.0", vec![
            ("scripts/run.py", "print('run-v3')"),
            ("config/main.json", r#"{"k":"v"}"#),
            ("CHANGELOG.md", "## v3\n- updated"),
        ]))
        .await?;
    c.local_a.sync().await?;
    c.local_a
        .assert_file(&name, "scripts/run.py", "print('run-v3')");
    c.local_a
        .assert_file(&name, "CHANGELOG.md", "## v3\n- updated");
    c.local_a.assert_no_path(&name, "config/extra.json");

    // v4: back to no files.
    c.remote
        .publish(&skill_plain(&name, "4.0.0", "body"))
        .await?;
    c.local_a.sync().await?;
    c.local_a.assert_no_path(&name, "scripts");
    c.local_a.assert_no_path(&name, "config");
    c.local_a.assert_no_path(&name, "CHANGELOG.md");
    c.local_a.assert_skill_dir_exists(&name);
    c.local_a.assert_cached(&name);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// IDEMPOTENT SYNC
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn noop_sync_is_idempotent() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_plain(&name, "1.0.0", "stable"))
        .await?;
    c.local_a.sync().await?;

    let mtime = c.local_a.mtime(&name, "SKILL.md");

    c.local_a.sync().await?;

    c.local_a.assert_content(&name, "stable");
    assert_eq!(
        c.local_a.mtime(&name, "SKILL.md"),
        mtime,
        "SKILL.md must not be touched on a no-op sync"
    );
    Ok(())
}
