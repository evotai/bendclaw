//! Multi-skill sync tests: multiple skills coexist, one local node.
//!
//! Covers: incremental diff (unchanged skill not rewritten), mixed
//! add+update+delete reconciled in one sync, file-level mixed changes.

use anyhow::Result;
use bendclaw_test_harness::setup::uid;

use super::fixtures::skill_plain;
use super::fixtures::skill_with_files;
use super::fixtures::Cluster;

#[tokio::test]
async fn unchanged_skill_not_rewritten() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name_a = uid("sk");
    let name_b = uid("sk");

    c.remote
        .publish(&skill_plain(&name_a, "1.0.0", "a-body"))
        .await?;
    c.remote
        .publish(&skill_plain(&name_b, "1.0.0", "b-body"))
        .await?;
    c.local_a.sync().await?;

    let mtime_before = c.local_a.mtime(&name_b, "SKILL.md");

    c.remote
        .publish(&skill_plain(&name_a, "1.0.0", "a-updated"))
        .await?;
    c.local_a.sync().await?;

    c.local_a.assert_content(&name_a, "a-updated");
    c.local_a.assert_content(&name_b, "b-body");
    assert_eq!(
        c.local_a.mtime(&name_b, "SKILL.md"),
        mtime_before,
        "skill-B SKILL.md must not be rewritten"
    );
    Ok(())
}

#[tokio::test]
async fn add_update_delete_reconciled_in_one_sync() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let keep = uid("sk");
    let update = uid("sk");
    let delete = uid("sk");

    for n in [&keep, &update, &delete] {
        c.remote
            .publish(&skill_plain(n, "1.0.0", n.as_str()))
            .await?;
    }
    c.local_a.sync().await?;
    c.local_b.sync().await?;
    assert_eq!(c.local_a.skill_count(), 3);

    let new = uid("sk");
    c.remote
        .publish(&skill_plain(&update, "2.0.0", "changed"))
        .await?;
    c.remote.remove(&delete).await?;
    c.remote
        .publish(&skill_plain(&new, "1.0.0", "fresh"))
        .await?;

    c.local_a.sync().await?;

    c.local_a.assert_content(&keep, keep.as_str());
    c.local_a.assert_version(&update, "2.0.0");
    c.local_a.assert_content(&update, "changed");
    c.local_a.assert_not_cached(&delete);
    c.local_a.assert_skill_dir_absent(&delete);
    c.local_a.assert_content(&new, "fresh");
    c.local_a.assert_skill_dir_exists(&new);
    assert_eq!(c.local_a.skill_count(), 3, "keep + update + new = 3");
    Ok(())
}

#[tokio::test]
async fn file_level_mixed_changes_in_one_sync() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let grow = uid("sk");
    let shrink = uid("sk");
    let gone = uid("sk");

    c.remote
        .publish(&skill_with_files(&grow, "1.0.0", vec![(
            "scripts/run.py",
            "print('run')",
        )]))
        .await?;
    c.remote
        .publish(&skill_with_files(&shrink, "1.0.0", vec![
            ("scripts/run.py", "print('run')"),
            ("config/extra.json", "{}"),
        ]))
        .await?;
    c.remote
        .publish(&skill_plain(&gone, "1.0.0", "body"))
        .await?;
    c.local_a.sync().await?;

    c.remote
        .publish(&skill_with_files(&grow, "1.1.0", vec![
            ("scripts/run.py", "print('run')"),
            ("tests/test.py", "def test(): pass"),
        ]))
        .await?;
    c.remote
        .publish(&skill_with_files(&shrink, "1.1.0", vec![(
            "scripts/run.py",
            "print('run')",
        )]))
        .await?;
    c.remote.remove(&gone).await?;

    c.local_a.sync().await?;

    c.local_a
        .assert_file(&grow, "tests/test.py", "def test(): pass");
    c.local_a
        .assert_file(&shrink, "scripts/run.py", "print('run')");
    c.local_a.assert_no_path(&shrink, "config/extra.json");
    c.local_a.assert_no_path(&shrink, "config");
    c.local_a.assert_not_cached(&gone);
    c.local_a.assert_skill_dir_absent(&gone);
    Ok(())
}
