//! Tests for the filesystem skill loader (`loader.rs`).

use anyhow::Context as _;
use anyhow::Result;
use bendclaw::kernel::skills::fs::load_skill_from_dir;
use bendclaw::kernel::skills::fs::load_skills;
use bendclaw::kernel::skills::fs::parse_frontmatter;
use bendclaw::kernel::skills::fs::parse_parameters_section;
use tempfile::TempDir;

// ── parse_frontmatter ─────────────────────────────────────────────────────────

#[test]
fn frontmatter_parsed_correctly() {
    let input = "---\nname: my-skill\nversion: 1.0.0\ndescription: hello\ntimeout: 60\n---\n# Body";
    let (fm, body) = parse_frontmatter(input);
    assert_eq!(fm.get("name").map(String::as_str), Some("my-skill"));
    assert_eq!(fm.get("version").map(String::as_str), Some("1.0.0"));
    assert_eq!(fm.get("description").map(String::as_str), Some("hello"));
    assert_eq!(fm.get("timeout").map(String::as_str), Some("60"));
    assert_eq!(body, "# Body");
}

#[test]
fn no_frontmatter_returns_empty_map_and_full_content() {
    let input = "# Just a body\nNo front-matter here.";
    let (fm, body) = parse_frontmatter(input);
    assert!(fm.is_empty());
    assert_eq!(body, input);
}

#[test]
fn frontmatter_with_empty_body() {
    let input = "---\nname: sk\n---\n";
    let (fm, body) = parse_frontmatter(input);
    assert_eq!(fm.get("name").map(String::as_str), Some("sk"));
    assert!(body.is_empty());
}

// ── parse_parameters_section ──────────────────────────────────────────────────

#[test]
fn parameters_parsed_from_body() {
    let body = "## Parameters\n- `--pattern`: regex pattern (required)\n- `--output`: output file\n## Other";
    let params = parse_parameters_section(body);
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "pattern");
    assert!(params[0].required);
    assert_eq!(params[1].name, "output");
    assert!(!params[1].required);
}

#[test]
fn no_parameters_section_returns_empty() {
    let body = "## Usage\nJust run it.";
    let params = parse_parameters_section(body);
    assert!(params.is_empty());
}

#[test]
fn parameters_section_with_no_params_returns_empty() {
    let body = "## Parameters\nNo parameters needed.\n## Other";
    let params = parse_parameters_section(body);
    assert!(params.is_empty());
}

// ── load_skill_from_dir ───────────────────────────────────────────────────────

fn write_skill_md(dir: &std::path::Path, content: &str) -> std::io::Result<()> {
    std::fs::write(dir.join("SKILL.md"), content)
}

#[test]
fn load_skill_from_dir_parses_all_fields() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("my-skill");
    std::fs::create_dir(&skill_dir)?;
    write_skill_md(
        &skill_dir,
        "---\nname: my-skill\nversion: 2.0.0\ndescription: does stuff\ntimeout: 45\n---\n# Docs\nHello.",
    )?;

    let loaded = load_skill_from_dir(&skill_dir, "fallback")
        .ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    assert_eq!(loaded.skill.name, "my-skill");
    assert_eq!(loaded.skill.version, "2.0.0");
    assert_eq!(loaded.skill.description, "does stuff");
    assert_eq!(loaded.skill.timeout, 45);
    assert_eq!(loaded.skill.content, "# Docs\nHello.");
    assert!(!loaded.skill.executable);
    assert_eq!(loaded.fs_dir, skill_dir);
    Ok(())
}

#[test]
fn load_skill_from_dir_uses_fallback_name_when_missing() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir(&skill_dir)?;
    write_skill_md(&skill_dir, "---\nversion: 1.0.0\ndescription: d\n---\nbody")?;

    let loaded = load_skill_from_dir(&skill_dir, "fallback-name")
        .ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    assert_eq!(loaded.skill.name, "fallback-name");
    Ok(())
}

#[test]
fn load_skill_from_dir_detects_executable() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir_all(skill_dir.join("scripts"))?;
    write_skill_md(&skill_dir, "---\nname: sk\n---\nbody")?;
    std::fs::write(skill_dir.join("scripts/run.py"), "print('hi')")?;

    let loaded =
        load_skill_from_dir(&skill_dir, "sk").ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    assert!(loaded.skill.executable);
    Ok(())
}

#[test]
fn load_skill_from_dir_returns_none_without_skill_md() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir(&skill_dir)?;

    assert!(load_skill_from_dir(&skill_dir, "sk").is_none());
    Ok(())
}

// ── load_skills ───────────────────────────────────────────────────────────────

#[test]
fn load_skills_loads_multiple_skill_dirs() -> Result<()> {
    let tmp = TempDir::new()?;
    for name in ["alpha", "beta"] {
        let d = tmp.path().join(name);
        std::fs::create_dir(&d)?;
        write_skill_md(
            &d,
            &format!("---\nname: {name}\nversion: 1.0.0\ndescription: d\n---\nbody"),
        )?;
    }

    let skills = load_skills(tmp.path());
    assert_eq!(skills.len(), 2);
    let names: Vec<_> = skills.iter().map(|s| s.skill.name.as_str()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
    Ok(())
}

#[test]
fn load_skills_skips_underscore_prefixed_dirs() -> Result<()> {
    let tmp = TempDir::new()?;
    let visible = tmp.path().join("visible");
    std::fs::create_dir(&visible)?;
    write_skill_md(
        &visible,
        "---\nname: visible\nversion: 1.0.0\ndescription: d\n---\nbody",
    )?;

    let hidden = tmp.path().join("_hidden");
    std::fs::create_dir(&hidden)?;
    write_skill_md(
        &hidden,
        "---\nname: hidden\nversion: 1.0.0\ndescription: d\n---\nbody",
    )?;

    let skills = load_skills(tmp.path());
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].skill.name, "visible");
    Ok(())
}

#[test]
fn load_skills_picks_latest_version_subdir() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("my-skill");
    std::fs::create_dir(&skill_dir)?;

    for (ver, body) in [("1.0.0", "old"), ("2.0.0", "new")] {
        let vd = skill_dir.join(ver);
        std::fs::create_dir(&vd)?;
        write_skill_md(
            &vd,
            &format!("---\nname: my-skill\nversion: {ver}\ndescription: d\n---\n{body}"),
        )?;
    }

    let skills = load_skills(tmp.path());
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].skill.version, "2.0.0");
    assert_eq!(skills[0].skill.content, "new");
    Ok(())
}

#[test]
fn load_skills_returns_empty_for_nonexistent_dir() {
    let skills = load_skills(std::path::Path::new("/nonexistent/path/12345"));
    assert!(skills.is_empty());
}

// ── LoadedSkill::read_doc ─────────────────────────────────────────────────────

#[test]
fn read_doc_empty_path_returns_content() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir(&skill_dir)?;
    write_skill_md(&skill_dir, "---\nname: sk\n---\n# My Content")?;

    let loaded =
        load_skill_from_dir(&skill_dir, "sk").ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    let doc = loaded
        .read_doc("")
        .ok_or_else(|| anyhow::anyhow!("read_doc returned None"))?;
    assert_eq!(doc, "# My Content");
    Ok(())
}

#[test]
fn read_doc_reads_subpath_file() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir_all(skill_dir.join("references"))?;
    write_skill_md(&skill_dir, "---\nname: sk\n---\nbody")?;
    std::fs::write(skill_dir.join("references/guide.md"), "# Guide")?;

    let loaded =
        load_skill_from_dir(&skill_dir, "sk").ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    let doc = loaded
        .read_doc("references/guide.md")
        .ok_or_else(|| anyhow::anyhow!("read_doc returned None"))?;
    assert_eq!(doc, "# Guide");
    Ok(())
}

#[test]
fn read_doc_lists_md_files_in_directory() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    let refs = skill_dir.join("references");
    std::fs::create_dir_all(&refs)?;
    write_skill_md(&skill_dir, "---\nname: sk\n---\nbody")?;
    std::fs::write(refs.join("a.md"), "A")?;
    std::fs::write(refs.join("b.md"), "B")?;
    std::fs::write(refs.join("c.txt"), "not md")?;

    let loaded =
        load_skill_from_dir(&skill_dir, "sk").ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    let listing = loaded
        .read_doc("references")
        .ok_or_else(|| anyhow::anyhow!("read_doc returned None"))?;
    assert!(listing.contains("a.md"));
    assert!(listing.contains("b.md"));
    assert!(!listing.contains("c.txt"));
    Ok(())
}

#[test]
fn read_doc_returns_none_for_nonexistent_path() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir(&skill_dir)?;
    write_skill_md(&skill_dir, "---\nname: sk\n---\nbody")?;

    let loaded =
        load_skill_from_dir(&skill_dir, "sk").ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    assert!(loaded.read_doc("nonexistent.md").is_none());
    Ok(())
}

// ── LoadedSkill::script_path ──────────────────────────────────────────────────

#[test]
fn requires_parsed_from_frontmatter() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir(&skill_dir)?;
    write_skill_md(
        &skill_dir,
        "---\nname: sk\nversion: 2.0.0\ndescription: with deps\ntimeout: 60\nrequires:\n  bins:\n    - curl\n    - jq\n  env:\n    - DATABEND_DSN\n---\nbody",
    )?;

    let loaded =
        load_skill_from_dir(&skill_dir, "sk").ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    let req = loaded.skill.requires.context("requires must be parsed")?;
    assert_eq!(req.bins, vec!["curl", "jq"]);
    assert_eq!(req.env, vec!["DATABEND_DSN"]);
    assert_eq!(loaded.skill.name, "sk");
    assert_eq!(loaded.skill.version, "2.0.0");
    assert_eq!(loaded.skill.description, "with deps");
    assert_eq!(loaded.skill.timeout, 60);
    Ok(())
}

#[test]
fn no_requires_returns_none() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir(&skill_dir)?;
    write_skill_md(&skill_dir, "---\nname: sk\n---\nbody")?;

    let loaded =
        load_skill_from_dir(&skill_dir, "sk").ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    assert!(loaded.skill.requires.is_none());
    Ok(())
}

#[test]
fn requires_bins_only() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir(&skill_dir)?;
    write_skill_md(
        &skill_dir,
        "---\nname: sk\nrequires:\n  bins:\n    - python3\n---\nbody",
    )?;

    let loaded =
        load_skill_from_dir(&skill_dir, "sk").ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    let req = loaded.skill.requires.context("requires missing")?;
    assert_eq!(req.bins, vec!["python3"]);
    assert!(req.env.is_empty());
    Ok(())
}

#[test]
fn requires_env_only() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir(&skill_dir)?;
    write_skill_md(
        &skill_dir,
        "---\nname: sk\nrequires:\n  env:\n    - API_KEY\n---\nbody",
    )?;

    let loaded =
        load_skill_from_dir(&skill_dir, "sk").ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    let req = loaded.skill.requires.context("requires missing")?;
    assert!(req.bins.is_empty());
    assert_eq!(req.env, vec!["API_KEY"]);
    Ok(())
}

#[test]
fn requires_empty_arrays() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir(&skill_dir)?;
    write_skill_md(
        &skill_dir,
        "---\nname: sk\nrequires:\n  bins: []\n  env: []\n---\nbody",
    )?;

    let loaded =
        load_skill_from_dir(&skill_dir, "sk").ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    let req = loaded.skill.requires.context("requires missing")?;
    assert!(req.bins.is_empty());
    assert!(req.env.is_empty());
    Ok(())
}

#[test]
fn frontmatter_boolean_and_numeric_values_preserved() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir(&skill_dir)?;
    write_skill_md(&skill_dir, "---\nname: sk\ntimeout: 120\n---\nbody")?;

    let loaded =
        load_skill_from_dir(&skill_dir, "sk").ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    assert_eq!(loaded.skill.timeout, 120);
    Ok(())
}

#[test]
fn script_path_returns_some_when_script_exists() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir_all(skill_dir.join("scripts"))?;
    write_skill_md(&skill_dir, "---\nname: sk\n---\nbody")?;
    std::fs::write(skill_dir.join("scripts/run.py"), "pass")?;

    let loaded =
        load_skill_from_dir(&skill_dir, "sk").ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    let sp = loaded
        .script_path()
        .ok_or_else(|| anyhow::anyhow!("script_path missing"))?;
    assert!(sp.ends_with("scripts/run.py"));
    Ok(())
}

#[test]
fn script_path_returns_none_without_scripts_dir() -> Result<()> {
    let tmp = TempDir::new()?;
    let skill_dir = tmp.path().join("sk");
    std::fs::create_dir(&skill_dir)?;
    write_skill_md(&skill_dir, "---\nname: sk\n---\nbody")?;

    let loaded =
        load_skill_from_dir(&skill_dir, "sk").ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;
    assert!(loaded.script_path().is_none());
    Ok(())
}
