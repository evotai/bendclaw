//! Unit tests for [`Skill::compute_sha256`].

use bendclaw::kernel::skills::model::skill::Skill;
use bendclaw::kernel::skills::model::skill::SkillFile;

fn base_skill() -> Skill {
    Skill {
        name: "test-skill".to_string(),
        version: "1.0.0".to_string(),
        description: "a test skill".to_string(),
        scope: Default::default(),
        source: Default::default(),
        user_id: String::new(),
        created_by: None,
        last_used_by: None,
        timeout: 30,
        executable: false,
        parameters: vec![],
        content: "# docs".to_string(),
        files: vec![],
        requires: None,
        manifest: None,
    }
}

#[test]
fn identical_skills_produce_same_hash() {
    let a = base_skill();
    let b = base_skill();
    assert_eq!(a.compute_sha256(), b.compute_sha256());
}

#[test]
fn name_change_alters_hash() {
    let a = base_skill();
    let mut b = base_skill();
    b.name = "other-name".to_string();
    assert_ne!(a.compute_sha256(), b.compute_sha256());
}

#[test]
fn version_change_alters_hash() {
    let a = base_skill();
    let mut b = base_skill();
    b.version = "2.0.0".to_string();
    assert_ne!(a.compute_sha256(), b.compute_sha256());
}

#[test]
fn description_change_alters_hash() {
    let a = base_skill();
    let mut b = base_skill();
    b.description = "different description".to_string();
    assert_ne!(a.compute_sha256(), b.compute_sha256());
}

#[test]
fn timeout_change_alters_hash() {
    let a = base_skill();
    let mut b = base_skill();
    b.timeout = 90;
    assert_ne!(a.compute_sha256(), b.compute_sha256());
}

#[test]
fn content_change_alters_hash() {
    let a = base_skill();
    let mut b = base_skill();
    b.content = "# different docs".to_string();
    assert_ne!(a.compute_sha256(), b.compute_sha256());
}

#[test]
fn file_order_does_not_affect_hash() {
    let mut a = base_skill();
    a.files = vec![
        SkillFile {
            path: "b.py".to_string(),
            body: "B".to_string(),
        },
        SkillFile {
            path: "a.py".to_string(),
            body: "A".to_string(),
        },
    ];
    let mut b = base_skill();
    b.files = vec![
        SkillFile {
            path: "a.py".to_string(),
            body: "A".to_string(),
        },
        SkillFile {
            path: "b.py".to_string(),
            body: "B".to_string(),
        },
    ];
    assert_eq!(
        a.compute_sha256(),
        b.compute_sha256(),
        "file order must not affect hash (sorted internally)"
    );
}

#[test]
fn adding_file_alters_hash() {
    let a = base_skill();
    let mut b = base_skill();
    b.files = vec![SkillFile {
        path: "run.py".to_string(),
        body: "print('hi')".to_string(),
    }];
    assert_ne!(a.compute_sha256(), b.compute_sha256());
}

#[test]
fn removing_file_alters_hash() {
    let mut a = base_skill();
    a.files = vec![SkillFile {
        path: "run.py".to_string(),
        body: "print('hi')".to_string(),
    }];
    let b = base_skill();
    assert_ne!(a.compute_sha256(), b.compute_sha256());
}

#[test]
fn changing_file_body_alters_hash() {
    let mut a = base_skill();
    a.files = vec![SkillFile {
        path: "run.py".to_string(),
        body: "v1".to_string(),
    }];
    let mut b = base_skill();
    b.files = vec![SkillFile {
        path: "run.py".to_string(),
        body: "v2".to_string(),
    }];
    assert_ne!(a.compute_sha256(), b.compute_sha256());
}

#[test]
fn changing_file_path_alters_hash() {
    let mut a = base_skill();
    a.files = vec![SkillFile {
        path: "old.py".to_string(),
        body: "body".to_string(),
    }];
    let mut b = base_skill();
    b.files = vec![SkillFile {
        path: "new.py".to_string(),
        body: "body".to_string(),
    }];
    assert_ne!(a.compute_sha256(), b.compute_sha256());
}
