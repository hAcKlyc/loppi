//! Shipped reference material and installation metadata; no content access.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::{fs, io::Write};

/// Independent from the product version: bump when the installed Skill changes.
pub const SKILL_VERSION: &str = "1.0.0";
const README: &str = include_str!("../../../resources/agent/readme.md");
pub const SKILL_FILES: [(&str, &str); 6] = [
    (
        "SKILL.md",
        include_str!("../../../resources/agent/loppi-context/SKILL.md"),
    ),
    (
        "references/cli.md",
        include_str!("../../../resources/agent/loppi-context/references/cli.md"),
    ),
    (
        "references/mcp.md",
        include_str!("../../../resources/agent/loppi-context/references/mcp.md"),
    ),
    (
        "references/evidence.md",
        include_str!("../../../resources/agent/loppi-context/references/evidence.md"),
    ),
    (
        "references/troubleshooting.md",
        include_str!("../../../resources/agent/loppi-context/references/troubleshooting.md"),
    ),
    (
        "references/workflows.md",
        include_str!("../../../resources/agent/loppi-context/references/workflows.md"),
    ),
];
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
pub fn skill_directory() -> Result<PathBuf, String> {
    Ok(std::path::absolute(crate::data_dir()?)
        .map_err(|e| e.to_string())?
        .join("skills")
        .join("loppi"))
}

fn render(text: &str, cli: &Path, directory: &Path) -> String {
    text.replace("{{LOPPI_CLI}}", &cli.to_string_lossy())
        .replace("{{SKILL_DIRECTORY}}", &directory.to_string_lossy())
        .replace("{{SKILL_VERSION}}", SKILL_VERSION)
}

fn directory(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| e.to_string())?;
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!(
            "Skill directory is not a regular directory: {}",
            path.display()
        ));
    }
    crate::platform::set_mode(path, 0o700).map_err(|e| e.to_string())
}

/// Publish only a complete, replaceable derived file; never touch Agent workspaces.
fn publish_at(destination: &Path, text: &str) -> Result<(), String> {
    directory(
        destination
            .parent()
            .ok_or("Skill directory needs a parent")?,
    )?;
    directory(destination)?;
    // Serialize the check and replacement across processes. On Windows, a
    // concurrent replacement can leave the pathname pending deletion even when
    // every individual rename is atomic. Keep the shared lock file in place.
    let publication = crate::platform::open_directory(destination).map_err(|e| e.to_string())?;
    fs2::FileExt::lock_exclusive(&publication).map_err(|e| e.to_string())?;
    let target = destination.join("SKILL.md");
    match fs::symlink_metadata(&target) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
            return Err("Official SKILL.md is not a regular file".into());
        }
        Ok(_) => {
            if fs::read_to_string(&target).map_err(|e| e.to_string())? == text {
                return Ok(());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let temporary = destination.join(format!(".skill-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        crate::platform::set_mode(&temporary, 0o600).map_err(|e| e.to_string())?;
        file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        crate::platform::rename(&temporary, &target).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn publish_skill(cli: &Path) -> Result<PathBuf, String> {
    let path = skill_directory()?;
    publish_at(&path, &render(SKILL_FILES[0].1, cli, &path))?;
    Ok(path)
}

pub fn readme(cli: &Path) -> Result<String, String> {
    let path = publish_skill(cli)?;
    Ok(render(README, cli, &path))
}

pub fn info(cli: &Path) -> Result<Value, String> {
    let directory = publish_skill(cli)?;
    let p = cli.to_string_lossy();
    let codex = format!(
        "[mcp_servers.loppi]\ncommand = {}\nargs = [\"mcp\", \"serve\"]\n",
        serde_json::to_string(&*p).unwrap()
    );
    let skill = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".agents/skills/loppi");
    let prompt = format!(
        "请将下列本机文件夹中的 loppi Skill 复制并安装到当前工作区适用的技能目录，然后读取 SKILL.md，按其中指引运行 CLI 的 readme 和 --help 验证接入。保留已有的用户自定义内容。\n\nSkill 文件夹：\n{}",
        directory.display()
    );
    Ok(
        json!({"cli_path":p,"cli_available":cli.is_file(),"api_version":crate::context::API_VERSION,"codex_config":codex,"mcp_config":{"mcpServers":{"loppi":{"command":p,"args":["mcp","serve"]}}},"codex_command":format!("codex mcp add loppi -- {} mcp serve",shell_quote(&p)),"skill_command":format!("{} integrations install-skill --destination {}",shell_quote(&p),shell_quote(&skill.to_string_lossy())),"example_command":format!("{} context list --date today",shell_quote(&p)),"skill_prompt":prompt,"skill_directory":directory,"skill_version":SKILL_VERSION}),
    )
}
pub fn guide(topic: &str) -> Option<&'static str> {
    SKILL_FILES
        .iter()
        .find(|(name, _)| *name == format!("references/{topic}.md"))
        .map(|(_, s)| *s)
}
pub fn install_skill(destination: &Path, cli: &Path) -> Result<(), String> {
    // Never overwrite a locally customized skill. Repeat installs are no-ops.
    let directory = publish_skill(cli)?;
    let files: Vec<_> = SKILL_FILES
        .iter()
        .map(|(name, text)| (*name, render(text, cli, &directory)))
        .collect();
    if destination.exists() {
        for (name, text) in &files {
            if std::fs::read_to_string(destination.join(name))
                .ok()
                .as_ref()
                != Some(text)
            {
                return Err("A different skill already exists at this path. Choose an empty destination or remove the old copy explicitly.".into());
            }
        }
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or("destination needs a parent directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(".loppi-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&temporary).map_err(|e| e.to_string())?;
    let result = (|| {
        for (name, text) in files {
            let path = temporary.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(path, text).map_err(|e| e.to_string())?;
        }
        // rename cannot replace a non-empty existing skill directory.
        crate::platform::rename(&temporary, destination).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temporary(PathBuf);
    impl Temporary {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("loppi-skill-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temporary {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn published_skill_is_complete_idempotent_and_replaces_old_version() {
        let temp = Temporary::new();
        let path = temp.0.join("数据目录 with spaces/skills/loppi");
        publish_at(&path, "old skill").unwrap();
        let text = render(
            SKILL_FILES[0].1,
            Path::new("/Applications/loppi app/loppi"),
            &path,
        );
        publish_at(&path, &text).unwrap();
        let modified = fs::metadata(path.join("SKILL.md"))
            .unwrap()
            .modified()
            .unwrap();
        publish_at(&path, &text).unwrap();
        assert_eq!(
            fs::metadata(path.join("SKILL.md"))
                .unwrap()
                .modified()
                .unwrap(),
            modified
        );
        assert_eq!(fs::read_to_string(path.join("SKILL.md")).unwrap(), text);
        assert_eq!(
            fs::read_dir(&path).unwrap().count(),
            1 + usize::from(cfg!(windows))
        );
        assert!(text.contains(&format!("version: \"{SKILL_VERSION}\"")));
        assert!(text.contains("主动从"));
        assert!(text.contains("重新读取"));
        assert!(!text.contains("{{"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path.join("SKILL.md"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn concurrent_publication_exposes_only_complete_files() {
        let temp = Temporary::new();
        let path = temp.0.join("skills/loppi");
        publish_at(&path, "initial skill").unwrap();
        for round in 0..4 {
            let versions: Vec<_> = (0..8)
                .map(|writer| format!("complete skill {round}/{writer}\n").repeat(1000))
                .collect();
            let start = std::sync::Barrier::new(versions.len());
            std::thread::scope(|scope| {
                for text in &versions {
                    let (path, start) = (&path, &start);
                    scope.spawn(move || {
                        start.wait();
                        publish_at(path, text).unwrap();
                    });
                }
            });
            let published = fs::read_to_string(path.join("SKILL.md")).unwrap();
            assert!(versions.contains(&published));
            assert_eq!(
                fs::read_dir(&path).unwrap().count(),
                1 + usize::from(cfg!(windows))
            );
        }
    }
    #[test]
    fn invalid_target_is_reported_without_partial_file() {
        let temp = Temporary::new();
        let path = temp.0.join("skills/loppi");
        fs::create_dir_all(path.join("SKILL.md")).unwrap();
        assert!(publish_at(&path, "new").is_err());
        assert_eq!(
            fs::read_dir(&path).unwrap().count(),
            1 + usize::from(cfg!(windows))
        );
    }
    #[cfg(unix)]
    #[test]
    fn symlink_target_does_not_overwrite_unrelated_content() {
        let temp = Temporary::new();
        let path = temp.0.join("skills/loppi");
        fs::create_dir_all(&path).unwrap();
        let other = temp.0.join("user-content");
        fs::write(&other, "keep").unwrap();
        std::os::unix::fs::symlink(&other, path.join("SKILL.md")).unwrap();
        assert!(publish_at(&path, "new").is_err());
        assert_eq!(fs::read_to_string(other).unwrap(), "keep");
    }
    #[test]
    fn windows_paths_and_independent_version_render_verbatim() {
        let cli = Path::new(r"D:\应用目录\loppi app\loppi.exe");
        let directory = Path::new(r"D:\用户数据\skills\loppi");
        for template in [SKILL_FILES[0].1, README] {
            let text = render(template, cli, directory);
            assert!(text.contains(&*cli.to_string_lossy()));
            assert!(text.contains(&*directory.to_string_lossy()));
            assert!(text.contains(SKILL_VERSION));
            assert!(!text.contains("{{"));
        }
    }
}
