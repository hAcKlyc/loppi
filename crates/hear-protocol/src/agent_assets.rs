//! Shipped reference material and installation metadata; no content access.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
pub const SKILL_FILES: [(&str, &str); 6] = [
    (
        "SKILL.md",
        include_str!("../../../resources/agent/hear-context/SKILL.md"),
    ),
    (
        "references/cli.md",
        include_str!("../../../resources/agent/hear-context/references/cli.md"),
    ),
    (
        "references/mcp.md",
        include_str!("../../../resources/agent/hear-context/references/mcp.md"),
    ),
    (
        "references/evidence.md",
        include_str!("../../../resources/agent/hear-context/references/evidence.md"),
    ),
    (
        "references/troubleshooting.md",
        include_str!("../../../resources/agent/hear-context/references/troubleshooting.md"),
    ),
    (
        "references/workflows.md",
        include_str!("../../../resources/agent/hear-context/references/workflows.md"),
    ),
];
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
pub fn info(cli: &Path) -> Value {
    let p = cli.to_string_lossy();
    let codex = format!(
        "[mcp_servers.hear]\ncommand = {}\nargs = [\"mcp\", \"serve\"]\n",
        serde_json::to_string(&*p).unwrap()
    );
    let skill = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".agents/skills/hear-context");
    json!({"cli_path":p,"cli_available":cli.is_file(),"api_version":crate::context::API_VERSION,"codex_config":codex,"mcp_config":{"mcpServers":{"hear":{"command":p,"args":["mcp","serve"]}}},"codex_command":format!("codex mcp add hear -- {} mcp serve",shell_quote(&p)),"skill_command":format!("{} integrations install-skill --destination {}",shell_quote(&p),shell_quote(&skill.to_string_lossy())),"example_command":format!("{} context list --date today",shell_quote(&p))})
}
pub fn guide(topic: &str) -> Option<&'static str> {
    SKILL_FILES
        .iter()
        .find(|(name, _)| *name == format!("references/{topic}.md"))
        .map(|(_, s)| *s)
}
pub fn install_skill(destination: &Path, cli: &Path) -> Result<(), String> {
    // Never overwrite a locally customized skill. Repeat installs are no-ops.
    let files: Vec<_> = SKILL_FILES
        .iter()
        .map(|(name, text)| (*name, text.replace("{{HEAR_CLI}}", &cli.to_string_lossy())))
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
    let temporary = parent.join(format!(".hear-context-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&temporary).map_err(|e| e.to_string())?;
    let result = (|| {
        for (name, text) in files {
            let path = temporary.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(path, text).map_err(|e| e.to_string())?;
        }
        // rename cannot replace a non-empty existing skill directory.
        std::fs::rename(&temporary, destination).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&temporary);
    }
    result
}
