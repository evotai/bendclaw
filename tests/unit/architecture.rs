use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context as _;
use anyhow::Result;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    visit_rust_files(root, &mut files);
    files.sort();
    files
}

fn visit_rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_rust_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

fn rel(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .display()
        .to_string()
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(idx, _)| {
        let start_ok = idx == 0
            || !haystack[..idx]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        let end = idx + needle.len();
        let end_ok = end == haystack.len()
            || !haystack[end..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        start_ok && end_ok
    })
}

#[test]
fn storage_does_not_depend_on_kernel() -> Result<()> {
    let root = repo_root().join("src/storage");
    let mut offenders = Vec::new();
    for path in rust_files_under(&root) {
        let text = read(&path)?;
        if text.contains("kernel::") {
            offenders.push(rel(&path));
        }
    }
    assert!(
        offenders.is_empty(),
        "storage must not reference kernel: {offenders:#?}"
    );
    Ok(())
}

#[test]
fn llm_does_not_depend_on_kernel() -> Result<()> {
    let root = repo_root().join("src/llm");
    let mut offenders = Vec::new();
    for path in rust_files_under(&root) {
        let text = read(&path)?;
        if text.contains("kernel::") {
            offenders.push(rel(&path));
        }
    }
    assert!(
        offenders.is_empty(),
        "llm must not reference kernel: {offenders:#?}"
    );
    Ok(())
}

#[test]
fn service_http_and_service_do_not_touch_storage_infra() -> Result<()> {
    let root = repo_root().join("src/service");
    let mut offenders = Vec::new();
    for path in rust_files_under(&root) {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name != "http.rs" && name != "service.rs" {
            continue;
        }
        let text = read(&path)?;
        // Service files may reference dal records/repos and sql::escape,
        // but should not directly use low-level storage infra (Pool, sql
        // builders, migrator).
        let bad = text.contains("storage::pool")
            || text.contains("storage::migrator")
            || contains_word(&text, "Pool");
        if bad {
            offenders.push(rel(&path));
        }
    }
    assert!(
        offenders.is_empty(),
        "service http/service must not reference Pool/sql/migrator: {offenders:#?}"
    );
    Ok(())
}

#[test]
fn service_query_does_not_call_repo_write_methods() -> Result<()> {
    let root = repo_root().join("src/service");
    let write_markers = [
        ".insert(",
        ".upsert(",
        ".update(",
        ".delete(",
        ".save_batch(",
        ".exec(",
    ];
    let mut offenders = Vec::new();
    for path in rust_files_under(&root) {
        if path.file_name().and_then(|name| name.to_str()) != Some("query.rs") {
            continue;
        }
        let text = read(&path)?;
        if write_markers.iter().any(|marker| text.contains(marker)) {
            offenders.push(rel(&path));
        }
    }
    assert!(
        offenders.is_empty(),
        "service query.rs must stay read-only: {offenders:#?}"
    );
    Ok(())
}
