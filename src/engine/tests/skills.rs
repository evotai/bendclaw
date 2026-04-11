use std::fs;
use std::path::Path;

use bendengine::skills::SkillSet;
use tempfile::TempDir;

fn create_skill(dir: &Path, name: &str, description: &str) {
    let skill_dir = dir.join(name);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            "---\nname: {}\ndescription: {}\n---\n\n# {}\n\nInstructions here.\n",
            name, description, name
        ),
    )
    .unwrap();
}

#[test]
fn load_skills_from_directory() {
    let tmp = TempDir::new().unwrap();
    create_skill(tmp.path(), "weather", "Get current weather and forecasts.");
    create_skill(tmp.path(), "git", "Git operations: commit, branch, merge.");

    let skills = SkillSet::load(&[tmp.path()]).unwrap();
    assert_eq!(skills.len(), 2);
    assert_eq!(skills.skills()[0].name, "git");
    assert_eq!(skills.skills()[1].name, "weather");
}

#[test]
fn format_for_prompt_xml() {
    let tmp = TempDir::new().unwrap();
    create_skill(tmp.path(), "weather", "Get weather.");

    let skills = SkillSet::load(&[tmp.path()]).unwrap();
    let prompt = skills.format_for_prompt();

    assert!(prompt.contains("<available_skills>"));
    assert!(prompt.contains("<name>weather</name>"));
    assert!(prompt.contains("<description>Get weather.</description>"));
    assert!(prompt.contains("SKILL.md</location>"));
    assert!(prompt.contains("</available_skills>"));
}

#[test]
fn empty_when_no_skills() {
    let tmp = TempDir::new().unwrap();
    let skills = SkillSet::load(&[tmp.path()]).unwrap();
    assert!(skills.is_empty());
    assert_eq!(skills.format_for_prompt(), "");
}

#[test]
fn later_dirs_override_earlier() {
    let dir1 = TempDir::new().unwrap();
    let dir2 = TempDir::new().unwrap();
    create_skill(dir1.path(), "weather", "Old description.");
    create_skill(dir2.path(), "weather", "New description.");

    let skills = SkillSet::load(&[dir1.path(), dir2.path()]).unwrap();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills.skills()[0].description, "New description.");
}

#[test]
fn skips_nonexistent_dirs() {
    let skills = SkillSet::load(&[Path::new("/nonexistent/path")]).unwrap();
    assert!(skills.is_empty());
}

#[test]
fn skips_dirs_without_skill_md() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("not-a-skill")).unwrap();
    fs::write(tmp.path().join("not-a-skill/README.md"), "hello").unwrap();

    let skills = SkillSet::load(&[tmp.path()]).unwrap();
    assert!(skills.is_empty());
}

#[test]
fn error_on_missing_frontmatter() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp.path().join("bad-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), "# No frontmatter\n").unwrap();

    let result = SkillSet::load(&[tmp.path()]);
    assert!(result.is_err());
}

#[test]
fn error_on_missing_name() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp.path().join("no-name");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\ndescription: Has desc but no name.\n---\n",
    )
    .unwrap();

    let result = SkillSet::load(&[tmp.path()]);
    assert!(result.is_err());
}

#[test]
fn error_on_missing_description() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp.path().join("no-desc");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), "---\nname: no-desc\n---\n").unwrap();

    let result = SkillSet::load(&[tmp.path()]);
    assert!(result.is_err());
}

#[test]
fn quoted_frontmatter_values() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp.path().join("quoted");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: \"quoted\"\ndescription: 'A quoted description.'\n---\n",
    )
    .unwrap();

    let skills = SkillSet::load(&[tmp.path()]).unwrap();
    assert_eq!(skills.skills()[0].name, "quoted");
    assert_eq!(skills.skills()[0].description, "A quoted description.");
}

#[test]
fn xml_escaping() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp.path().join("escape-test");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: escape-test\ndescription: Uses <tags> & \"quotes\"\n---\n",
    )
    .unwrap();

    let skills = SkillSet::load(&[tmp.path()]).unwrap();
    let prompt = skills.format_for_prompt();
    assert!(prompt.contains("&lt;tags&gt;"));
    assert!(prompt.contains("&amp;"));
    assert!(prompt.contains("&quot;quotes&quot;"));
}

#[test]
fn merge_skill_sets() {
    let dir1 = TempDir::new().unwrap();
    let dir2 = TempDir::new().unwrap();
    create_skill(dir1.path(), "weather", "Weather v1.");
    create_skill(dir1.path(), "git", "Git operations.");
    create_skill(dir2.path(), "weather", "Weather v2.");
    create_skill(dir2.path(), "docker", "Docker management.");

    let mut set1 = SkillSet::load(&[dir1.path()]).unwrap();
    let set2 = SkillSet::load(&[dir2.path()]).unwrap();
    set1.merge(set2);

    assert_eq!(set1.len(), 3);
    let names: Vec<&str> = set1.skills().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["docker", "git", "weather"]);
    assert_eq!(
        set1.skills()
            .iter()
            .find(|s| s.name == "weather")
            .unwrap()
            .description,
        "Weather v2."
    );
}

#[test]
fn load_real_agentskills_format() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp.path().join("nano-banana-pro");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: nano-banana-pro
description: Generate or edit images via Gemini 3 Pro Image.
metadata:
  {
    "openclaw":
      {
        "emoji": "🍌",
        "requires": { "bins": ["uv"], "env": ["GEMINI_API_KEY"] },
      },
  }
---

# Nano Banana Pro

Use the bundled script to generate images.
"#,
    )
    .unwrap();

    let skills = SkillSet::load(&[tmp.path()]).unwrap();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills.skills()[0].name, "nano-banana-pro");
    assert_eq!(
        skills.skills()[0].description,
        "Generate or edit images via Gemini 3 Pro Image."
    );
}
