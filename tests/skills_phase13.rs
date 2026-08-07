use luminus::skill::{SkillRegistry, SkillSource};
use std::fs;
use std::path::PathBuf;

fn temp_project_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("luminus-skill-test-{}-{}", std::process::id(), tag));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn builtin_skills_discovered_and_loaded() {
    let registry = SkillRegistry::new();
    let skills = registry.discover();
    assert!(!skills.is_empty());
    assert!(skills.iter().any(|s| s.name == "fix-tests"));
    assert!(skills.iter().any(|s| s.name == "code-review"));

    let loaded = registry.load_skill("fix-tests").unwrap();
    assert_eq!(loaded.metadata.source, SkillSource::BuiltIn);
    assert!(loaded.content.contains("# Fix Tests"));
}

#[test]
fn project_skill_overrides_builtin() {
    let dir = temp_project_dir("override");
    let project_skills = dir.join(".luminus").join("skills").join("fix-tests");
    fs::create_dir_all(&project_skills).unwrap();

    let custom_skill = "---\nname: fix-tests\ndescription: Custom project fix-tests\nversion: 2.0.0\n---\n\n# Custom Fix Tests\n";
    fs::write(project_skills.join("SKILL.md"), custom_skill).unwrap();

    let orig_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let registry = SkillRegistry::new();
    let meta = registry.find("fix-tests").unwrap();
    assert_eq!(meta.source, SkillSource::Project);

    let loaded = registry.load_skill("fix-tests").unwrap();
    assert!(loaded.content.contains("# Custom Fix Tests"));

    std::env::set_current_dir(orig_dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn list_and_inspect_skills() {
    let registry = SkillRegistry::new();
    let list = registry.list_skills();
    assert!(list.contains("fix-tests (built-in)"));
    assert!(list.contains("code-review (built-in)"));

    let inspect = registry.inspect_skill("code-review").unwrap();
    assert!(inspect.contains("Skill: code-review"));
    assert!(inspect.contains("--- Skill Content ---"));
}
