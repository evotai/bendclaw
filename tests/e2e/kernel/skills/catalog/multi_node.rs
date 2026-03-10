//! Multi-node sync tests: one remote, two independent local nodes.
//!
//! Covers: both locals mirror the same skill, independent sync schedules,
//! stale node convergence, dual-node delete cleanup.

use anyhow::Result;
use bendclaw_test_harness::setup::uid;

use super::fixtures::skill_plain;
use super::fixtures::skill_with_files;
use super::fixtures::Cluster;

// ─────────────────────────────────────────────────────────────────────────────
// ADD
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn skill_published_propagates_to_both_locals() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_plain(&name, "1.0.0", "initial content"))
        .await?;

    c.local_a.sync().await?;
    c.local_b.sync().await?;

    c.local_a.assert_cached(&name);
    c.local_a.assert_content(&name, "initial content");
    c.local_a.assert_skill_dir_exists(&name);

    c.local_b.assert_cached(&name);
    c.local_b.assert_content(&name, "initial content");
    c.local_b.assert_skill_dir_exists(&name);
    Ok(())
}

#[tokio::test]
async fn both_locals_independently_mirror_same_skill() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![
            ("scripts/run.py", "print('hello')"),
            ("config/settings.toml", "[defaults]\nretries = 3"),
        ]))
        .await?;

    c.local_a.sync().await?;
    c.local_b.sync().await?;

    for local in [&c.local_a, &c.local_b] {
        local.assert_file(&name, "scripts/run.py", "print('hello')");
        local.assert_file(&name, "config/settings.toml", "[defaults]\nretries = 3");
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// UPDATE
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn each_local_syncs_independently() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_plain(&name, "1.0.0", "original"))
        .await?;
    c.local_a.sync().await?;
    c.local_b.sync().await?;

    c.remote
        .publish(&skill_plain(&name, "1.0.0", "updated"))
        .await?;

    c.local_a.sync().await?;
    c.local_a.assert_content(&name, "updated");

    // local_b still stale.
    c.local_b.assert_content(&name, "original");

    c.local_b.sync().await?;
    c.local_b.assert_content(&name, "updated");
    Ok(())
}

#[tokio::test]
async fn stale_local_converges_to_latest_on_single_sync() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote.publish(&skill_plain(&name, "1.0.0", "v1")).await?;
    c.local_a.sync().await?;

    // local_b is offline while v2 and v3 are published.
    c.remote.publish(&skill_plain(&name, "2.0.0", "v2")).await?;
    c.remote.publish(&skill_plain(&name, "3.0.0", "v3")).await?;
    c.local_a.sync().await?;

    c.local_b.sync().await?;
    c.local_b.assert_version(&name, "3.0.0");
    c.local_b.assert_content(&name, "v3");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// DELETE
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn both_locals_clean_up_on_delete() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_with_files(&name, "1.0.0", vec![(
            "scripts/run.py",
            "print('x')",
        )]))
        .await?;
    c.local_a.sync().await?;
    c.local_b.sync().await?;

    c.remote.remove(&name).await?;
    c.local_a.sync().await?;
    c.local_b.sync().await?;

    c.local_a.assert_not_cached(&name);
    c.local_b.assert_not_cached(&name);
    c.local_a.assert_skill_dir_absent(&name);
    c.local_b.assert_skill_dir_absent(&name);
    Ok(())
}

#[tokio::test]
async fn stale_local_cleans_up_on_next_sync() -> Result<()> {
    let user_id = Some(uid("u"));
    let c = Cluster::new(user_id.as_deref()).await?;
    let name = uid("sk");

    c.remote
        .publish(&skill_plain(&name, "1.0.0", "body"))
        .await?;
    c.local_a.sync().await?;
    c.local_b.sync().await?;

    c.remote.remove(&name).await?;

    c.local_a.sync().await?;
    c.local_a.assert_not_cached(&name);
    c.local_b.assert_cached(&name); // still stale

    c.local_b.sync().await?;
    c.local_b.assert_not_cached(&name);
    c.local_b.assert_skill_dir_absent(&name);
    Ok(())
}
