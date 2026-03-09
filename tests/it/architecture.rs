use std::fs;
use std::path::Path;
use std::path::PathBuf;

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

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
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
fn storage_does_not_depend_on_kernel() {
    let root = repo_root().join("src/storage");
    let offenders: Vec<_> = rust_files_under(&root)
        .into_iter()
        .filter_map(|path| {
            let text = read(&path);
            text.contains("kernel::").then(|| rel(&path))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "storage must not reference kernel: {offenders:#?}"
    );
}

#[test]
fn llm_does_not_depend_on_kernel() {
    let root = repo_root().join("src/llm");
    let offenders: Vec<_> = rust_files_under(&root)
        .into_iter()
        .filter_map(|path| {
            let text = read(&path);
            text.contains("kernel::").then(|| rel(&path))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "llm must not reference kernel: {offenders:#?}"
    );
}

#[test]
fn service_http_and_service_do_not_touch_storage_infra() {
    let root = repo_root().join("src/service");
    let mut offenders = Vec::new();
    for path in rust_files_under(&root) {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name != "http.rs" && name != "service.rs" {
            continue;
        }
        let text = read(&path);
        let bad = text.contains("storage::sql")
            || text.contains("storage::dal")
            || text.contains("dal::")
            || text.contains("sql::")
            || contains_word(&text, "Pool");
        if bad {
            offenders.push(rel(&path));
        }
    }
    assert!(
        offenders.is_empty(),
        "service http/service must not reference Pool/sql/dal: {offenders:#?}"
    );
}

#[test]
fn service_query_does_not_call_repo_write_methods() {
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
        let text = read(&path);
        if write_markers.iter().any(|marker| text.contains(marker)) {
            offenders.push(rel(&path));
        }
    }
    assert!(
        offenders.is_empty(),
        "service query.rs must stay read-only: {offenders:#?}"
    );
}
