use std::sync::Arc;

use evot::storage::fs::FsStorage;
use evot::storage::FavoritesEdit;
use evot::storage::MemoryStorage;
use evot::storage::Storage;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn concurrent_edits(stores: Vec<Arc<dyn Storage>>) -> TestResult {
    let reader = stores[0].clone();
    let mut tasks = Vec::new();
    for (index, storage) in stores.into_iter().enumerate() {
        tasks.push(tokio::spawn(async move {
            storage
                .edit_favorites(FavoritesEdit::Toggle(format!("session-{index}")))
                .await
        }));
    }
    for task in tasks {
        task.await??;
    }
    let ids = reader.load_favorites().await?;
    assert_eq!(ids.len(), 32);
    for index in 0..32 {
        assert!(ids.contains(&format!("session-{index}")));
    }
    let updated = reader
        .edit_favorites(FavoritesEdit::Remove(vec![
            "session-0".into(),
            "session-1".into(),
        ]))
        .await?;
    assert_eq!(updated.removed, 2);
    assert_eq!(updated.ids.len(), 30);
    Ok(())
}

#[tokio::test]
async fn favorites_edits_are_atomic_across_filesystem_handles() -> TestResult {
    let dir = tempfile::tempdir()?;
    let stores: Vec<Arc<dyn Storage>> = (0..32)
        .map(|_| Arc::new(FsStorage::new(dir.path().to_path_buf())) as Arc<dyn Storage>)
        .collect();
    concurrent_edits(stores).await?;
    let doc: evot::types::FavoritesDocument =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("favorites.json"))?)?;
    assert_eq!(doc.version, 1);
    assert_eq!(doc.ids.len(), 30);
    Ok(())
}

#[tokio::test]
async fn favorites_edits_are_atomic_in_memory() -> TestResult {
    let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::new());
    concurrent_edits((0..32).map(|_| storage.clone()).collect()).await
}

#[test]
fn favorites_edits_are_atomic_across_processes() -> TestResult {
    const CHILD_ROOT: &str = "EVOT_FAVORITES_TEST_ROOT";
    const CHILD_ID: &str = "EVOT_FAVORITES_TEST_ID";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let id = std::env::var(CHILD_ID)?;
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(async {
                let storage = FsStorage::new(root.into());
                for index in 0..16 {
                    storage
                        .edit_favorites(FavoritesEdit::Toggle(format!("{id}-{index}")))
                        .await?;
                }
                Ok(())
            });
    }

    let dir = tempfile::tempdir()?;
    let executable = std::env::current_exe()?;
    let mut children = Vec::new();
    for id in 0..4 {
        children.push(
            std::process::Command::new(&executable)
                .args([
                    "--exact",
                    "favorites_atomic_test::favorites_edits_are_atomic_across_processes",
                ])
                .env(CHILD_ROOT, dir.path())
                .env(CHILD_ID, id.to_string())
                .stdout(std::process::Stdio::null())
                .spawn()?,
        );
    }
    // Reap all workers before reporting a failure so none outlive the fixture.
    let mut successful = true;
    for mut child in children {
        successful &= child.wait()?.success();
    }
    assert!(successful, "favorites worker failed");
    let doc: evot::types::FavoritesDocument =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("favorites.json"))?)?;
    assert_eq!(doc.version, 1);
    assert_eq!(doc.ids.len(), 64);
    for id in 0..4 {
        for index in 0..16 {
            assert!(doc.ids.contains(&format!("{id}-{index}")));
        }
    }
    Ok(())
}

#[tokio::test]
async fn invalid_favorites_document_is_not_overwritten() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("favorites.json");
    std::fs::write(&path, "invalid")?;
    let storage = FsStorage::new(dir.path().to_path_buf());
    assert!(storage
        .edit_favorites(FavoritesEdit::Toggle("session".into()))
        .await
        .is_err());
    assert_eq!(std::fs::read_to_string(path)?, "invalid");
    Ok(())
}
