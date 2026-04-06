use std::path::Path;

const PROJECT_CONTEXT_FILES: &[&str] = &["BENDCLAW.md", "CLAUDE.md", "AGENTS.md"];

pub fn load_project_context(cwd: &str) -> Option<String> {
    let mut context = String::new();
    for name in PROJECT_CONTEXT_FILES {
        let path = Path::new(cwd).join(name);
        if let Ok(content) = std::fs::read_to_string(&path) {
            let content = content.trim();
            if !content.is_empty() {
                if !context.is_empty() {
                    context.push_str("\n\n");
                }
                context.push_str(content);
            }
        }
    }
    if context.is_empty() {
        None
    } else {
        Some(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_none_when_no_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(load_project_context(&tmp.path().to_string_lossy()).is_none());
    }

    #[test]
    fn reads_single_context_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("BENDCLAW.md"), "# My Project\nDo X.").unwrap();
        let ctx = load_project_context(&tmp.path().to_string_lossy()).unwrap();
        assert!(ctx.contains("My Project"));
    }

    #[test]
    fn concatenates_multiple_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("BENDCLAW.md"), "part one").unwrap();
        std::fs::write(tmp.path().join("CLAUDE.md"), "part two").unwrap();
        let ctx = load_project_context(&tmp.path().to_string_lossy()).unwrap();
        assert!(ctx.contains("part one"));
        assert!(ctx.contains("part two"));
    }

    #[test]
    fn skips_empty_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("BENDCLAW.md"), "   ").unwrap();
        assert!(load_project_context(&tmp.path().to_string_lossy()).is_none());
    }
}
