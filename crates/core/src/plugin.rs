use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use crate::i18n::{Locale, current_language, tf};
use crate::io_err;

/// Install the MDWisp skills into `<project>/.agents/skills` for Codex /
/// Cursor, without replacing anything the user wrote there.
pub fn install_skills(plugin_root: &Path, project: &Path) -> Result<PathBuf, String> {
    let target_root = project.join(".agents").join("skills");
    let mut files: Vec<(PathBuf, Vec<u8>)> = vec![];
    for skill in ["doc", "share"] {
        let source = plugin_root.join("mdwisp").join("skills").join(skill);
        let target = target_root.join(format!("mdwisp-{skill}"));
        let file = if current_language() == Locale::En { "SKILL.en.md" } else { "SKILL.md" };
        let body = fs::read_to_string(source.join(file)).map_err(io_err)?;
        files.push((target.join("SKILL.md"), rename_skill(&body, skill).into_bytes()));
        if skill == "doc" {
            let check = fs::read(source.join("scripts").join("check.mjs")).map_err(io_err)?;
            files.push((target.join("scripts").join("check.mjs"), check));
        }
    }
    // Check every destination before writing anything: custom edits must survive.
    for (path, content) in &files {
        match fs::read(path) {
            Ok(existing) if existing != *content => return Err(tf("已有不同内容，请先备份并移走后重试：{0}", &[&path.display()])),
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(e.to_string()),
            _ => {}
        }
    }
    for (path, content) in &files {
        fs::create_dir_all(path.parent().unwrap()).map_err(io_err)?;
        match fs::OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut f) => f.write_all(content).map_err(io_err)?,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(target_root)
}

/// The first `name: doc` / `name: share` line becomes `name: mdwisp-<skill>`.
/// A git checkout may have CRLF endings; the `\r` stays.
fn rename_skill(body: &str, skill: &str) -> String {
    let mut at = 0;
    for raw in body.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line == "name: doc" || line == "name: share" {
            return format!("{}name: mdwisp-{skill}{}", &body[..at], &body[at + line.len()..]);
        }
        at += raw.len() + 1;
    }
    body.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::set_language;

    fn plugin() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin")
    }

    #[test]
    fn installs_portable_namespaced_skills_and_the_working_validator_for_both_clients() {
        let _lang = crate::LANG.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("AGENTS.md"), "project rules").unwrap();
        let legacy = root.join(".agents/skills/mdview-doc");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("SKILL.md"), "custom legacy skill").unwrap();
        let target = install_skills(&plugin(), root).unwrap();
        assert_eq!(target, root.join(".agents").join("skills"));
        for name in ["doc", "share"] {
            let source = fs::read_to_string(plugin().join("mdwisp").join("skills").join(name).join("SKILL.md")).unwrap();
            let installed = fs::read_to_string(target.join(format!("mdwisp-{name}")).join("SKILL.md")).unwrap();
            assert_eq!(installed, source.replacen(&format!("name: {name}"), &format!("name: mdwisp-{name}"), 1));
        }
        for doc in ["欢迎使用", "Welcome"] {
            let stock = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stock").join(doc);
            let checked = std::process::Command::new("node").arg(target.join("mdwisp-doc/scripts/check.mjs")).arg(stock).status().unwrap();
            assert!(checked.success(), "{doc}");
        }
        install_skills(&plugin(), root).unwrap();
        assert_eq!(fs::read_to_string(root.join("AGENTS.md")).unwrap(), "project rules");
        assert_eq!(fs::read_to_string(legacy.join("SKILL.md")).unwrap(), "custom legacy skill");
    }

    #[test]
    fn refuses_custom_content_before_installing_any_other_skill() {
        let _lang = crate::LANG.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let dest = root.join(".agents/skills/mdwisp-share");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("SKILL.md"), "custom").unwrap();
        assert!(install_skills(&plugin(), root).unwrap_err().contains("已有不同内容"));
        assert_eq!(fs::read_to_string(dest.join("SKILL.md")).unwrap(), "custom");
        assert!(!root.join(".agents/skills/mdwisp-doc").exists());
    }

    #[test]
    fn installs_english_rules_from_the_plugin_source_when_english_is_selected() {
        let _lang = crate::LANG.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        set_language(Locale::En);
        let result = install_skills(&plugin(), dir.path());
        set_language(Locale::ZhCn);
        let body = fs::read_to_string(result.unwrap().join("mdwisp-doc/SKILL.md")).unwrap();
        assert!(body.contains("# Write a portable document folder"));
        assert!(body.contains("name: mdwisp-doc"));
    }
}
