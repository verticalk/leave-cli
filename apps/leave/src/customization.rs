//! Documented Devin customization commands exposed through structured APIs.

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};
use tokio::process::Command;

const MAX_OUTPUT: usize = 2 * 1024 * 1024;
const PROJECT_SKILL_PATHS: &[&str] = &[".devin/skills", ".cognition/skills", ".agents/skills"];

/// Text returned by a documented Devin configuration command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevinCommandOutput {
    /// Command output with terminal control sequences removed by Devin itself.
    pub output: String,
}

/// One reviewed customization mutation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomizationMutation {
    /// Resource kind: `plugin` or `mcp`.
    pub kind: String,
    /// Supported operation for that kind.
    pub action: String,
    /// Plugin source/name or MCP server name.
    pub name: String,
    /// MCP scope: local, project, or user.
    #[serde(default)]
    pub scope: String,
    /// HTTP/SSE URL for MCP additions.
    #[serde(default)]
    pub url: String,
    /// MCP transport for additions.
    #[serde(default)]
    pub transport: String,
    /// Executable for stdio MCP additions.
    #[serde(default)]
    pub command: String,
    /// Arguments for a stdio MCP command.
    #[serde(default)]
    pub arguments: Vec<String>,
    /// Exact typed phrase required for executable or destructive changes.
    #[serde(default)]
    pub confirmation: String,
}

/// List a documented Devin customization category.
pub async fn list(
    devin: &Path,
    workspace: &Path,
    category: &str,
    allow_global: bool,
) -> anyhow::Result<DevinCommandOutput> {
    if category == "skills" && !allow_global {
        return list_project_skills(workspace).await;
    }
    if category == "plugins" && !allow_global {
        bail!("plugin inventory is global; reconnect with --expose-global-customization");
    }
    if category == "mcp" && !allow_global {
        return Ok(DevinCommandOutput {
            output: "Project/local MCP mutations are enabled. Devin's aggregate list also includes user-global servers, so Leave hides it until global customization is granted.".into(),
        });
    }
    let arguments = match category {
        "rules" => vec!["rules", "list"],
        "skills" => vec!["skills", "list"],
        "plugins" => vec!["plugins", "list"],
        "mcp" => vec!["mcp", "list"],
        _ => bail!("unknown customization category"),
    };
    run(devin, workspace, &arguments).await
}

/// Show one documented rule, skill, plugin, or MCP server.
pub async fn show(
    devin: &Path,
    workspace: &Path,
    category: &str,
    name: &str,
    allow_global: bool,
) -> anyhow::Result<DevinCommandOutput> {
    validate_name(name)?;
    if category == "skills" && !allow_global {
        return show_project_skill(workspace, name).await;
    }
    if matches!(category, "plugins" | "mcp") && !allow_global {
        bail!(
            "this aggregate detail can include global configuration; reconnect with --expose-global-customization"
        );
    }
    let arguments = match category {
        "rules" => vec!["rules", "show", name],
        "skills" => vec!["skills", "show", name],
        "plugins" => vec!["plugins", "info", name],
        "mcp" => vec!["mcp", "get", name],
        _ => bail!("unknown customization category"),
    };
    run(devin, workspace, &arguments).await
}

/// Apply a mutation using only documented Devin CLI arguments.
pub async fn mutate(
    devin: &Path,
    workspace: &Path,
    mutation: &CustomizationMutation,
    allow_global: bool,
) -> anyhow::Result<DevinCommandOutput> {
    validate_name(&mutation.name)?;
    if mutation.kind == "plugin" && !allow_global {
        bail!("plugin management requires --expose-global-customization");
    }
    match (mutation.kind.as_str(), mutation.action.as_str()) {
        ("plugin", "install") => {
            validate_plugin_source(&mutation.name)?;
            require_confirmation(
                &mutation.confirmation,
                &format!("INSTALL PLUGIN {}", mutation.name),
            )?;
            run(
                devin,
                workspace,
                &["plugins", "install", "--yes", "--local", &mutation.name],
            )
            .await
        }
        ("plugin", "remove") => {
            require_confirmation(
                &mutation.confirmation,
                &format!("REMOVE PLUGIN {}", mutation.name),
            )?;
            run(
                devin,
                workspace,
                &["plugins", "remove", "--yes", "--local", &mutation.name],
            )
            .await
        }
        ("plugin", "update") => {
            require_confirmation(
                &mutation.confirmation,
                &format!("UPDATE PLUGIN {}", mutation.name),
            )?;
            run(devin, workspace, &["plugins", "update", &mutation.name]).await
        }
        ("mcp", action @ ("enable" | "disable" | "remove")) => {
            let scope = checked_scope(&mutation.scope, allow_global)?;
            require_confirmation(
                &mutation.confirmation,
                &format!("{} MCP {}", action.to_ascii_uppercase(), mutation.name),
            )?;
            run(
                devin,
                workspace,
                &["mcp", action, "--scope", scope, &mutation.name],
            )
            .await
        }
        ("mcp", "add") => add_mcp(devin, workspace, mutation, allow_global).await,
        _ => bail!("unsupported customization mutation"),
    }
}

async fn list_project_skills(workspace: &Path) -> anyhow::Result<DevinCommandOutput> {
    let mut skills = Vec::new();
    for relative in PROJECT_SKILL_PATHS {
        let directory = workspace.join(relative);
        let mut entries = match tokio::fs::read_dir(&directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            let metadata = tokio::fs::symlink_metadata(entry.path()).await?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                continue;
            }
            let manifest = entry.path().join("SKILL.md");
            let Ok(manifest_metadata) = tokio::fs::symlink_metadata(&manifest).await else {
                continue;
            };
            if manifest_metadata.file_type().is_symlink() || !manifest_metadata.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            skills.push(format!("{name}  {relative}/{name}/SKILL.md"));
        }
    }
    skills.sort();
    Ok(DevinCommandOutput {
        output: skills.join("\n"),
    })
}

async fn show_project_skill(workspace: &Path, name: &str) -> anyhow::Result<DevinCommandOutput> {
    let path = Path::new(name);
    if path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        bail!("project skill name is invalid");
    }
    for relative in PROJECT_SKILL_PATHS {
        let directory = workspace.join(relative).join(name);
        let Ok(directory_metadata) = tokio::fs::symlink_metadata(&directory).await else {
            continue;
        };
        if directory_metadata.file_type().is_symlink() || !directory_metadata.is_dir() {
            continue;
        }
        let manifest = directory.join("SKILL.md");
        let Ok(metadata) = tokio::fs::symlink_metadata(&manifest).await else {
            continue;
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_OUTPUT as u64
        {
            continue;
        }
        return Ok(DevinCommandOutput {
            output: tokio::fs::read_to_string(manifest)
                .await
                .context("project skill is not valid UTF-8")?,
        });
    }
    bail!("project skill was not found in a documented project skill directory")
}

async fn add_mcp(
    devin: &Path,
    workspace: &Path,
    mutation: &CustomizationMutation,
    allow_global: bool,
) -> anyhow::Result<DevinCommandOutput> {
    let scope = checked_scope(&mutation.scope, allow_global)?;
    require_confirmation(
        &mutation.confirmation,
        &format!("ADD MCP {}", mutation.name),
    )?;
    let mut owned = vec![
        "mcp".to_owned(),
        "add".to_owned(),
        "--scope".to_owned(),
        scope.to_owned(),
        mutation.name.clone(),
    ];
    match mutation.transport.as_str() {
        "http" | "sse" => {
            let url = url::Url::parse(&mutation.url).context("MCP URL is invalid")?;
            if !matches!(url.scheme(), "http" | "https") {
                bail!("HTTP MCP URLs must use http or https");
            }
            owned.extend([
                "--transport".into(),
                mutation.transport.clone(),
                "--url".into(),
                mutation.url.clone(),
            ]);
        }
        "stdio" => {
            if mutation.command.trim().is_empty() || mutation.command.contains('\0') {
                bail!("stdio MCP command is invalid");
            }
            if mutation.arguments.len() > 100
                || mutation
                    .arguments
                    .iter()
                    .any(|argument| argument.contains('\0') || argument.len() > 8_192)
            {
                bail!("stdio MCP arguments exceed Leave limits");
            }
            owned.extend([
                "--transport".into(),
                "stdio".into(),
                "--command".into(),
                mutation.command.clone(),
            ]);
            if !mutation.arguments.is_empty() {
                owned.push("--".into());
                owned.extend(mutation.arguments.iter().cloned());
            }
        }
        _ => bail!("MCP transport must be http, sse, or stdio"),
    }
    let arguments = owned.iter().map(String::as_str).collect::<Vec<_>>();
    run(devin, workspace, &arguments).await
}

fn checked_scope(scope: &str, allow_global: bool) -> anyhow::Result<&str> {
    let scope = if scope.is_empty() { "local" } else { scope };
    if !matches!(scope, "local" | "project" | "user") {
        bail!("MCP scope must be local, project, or user");
    }
    if scope == "user" && !allow_global {
        bail!("user-level MCP configuration is not granted for this workspace");
    }
    Ok(scope)
}

fn validate_name(value: &str) -> anyhow::Result<()> {
    if value.trim().is_empty()
        || value.len() > 500
        || value.contains('\0')
        || value.starts_with('-')
        || value.contains(['\r', '\n'])
    {
        bail!("customization name is invalid");
    }
    Ok(())
}

fn validate_plugin_source(value: &str) -> anyhow::Result<()> {
    let github_label = value.split('#').next().unwrap_or(value);
    let owner_repo = github_label.split('/').collect::<Vec<_>>();
    let valid_github_label = owner_repo.len() == 2
        && owner_repo.iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
        });
    let valid_https = url::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https" && url.host_str().is_some() && url.username().is_empty()
    });
    if !valid_github_label && !valid_https {
        bail!("away installation accepts a GitHub owner/repo label or HTTPS Git URL");
    }
    Ok(())
}

fn require_confirmation(actual: &str, expected: &str) -> anyhow::Result<()> {
    if actual != expected {
        bail!("type the exact confirmation phrase: {expected}");
    }
    Ok(())
}

async fn run(
    devin: &Path,
    workspace: &Path,
    arguments: &[&str],
) -> anyhow::Result<DevinCommandOutput> {
    let output = Command::new(devin)
        .args(arguments)
        .current_dir(workspace)
        .output()
        .await
        .context("could not start the documented Devin configuration command")?;
    if output.stdout.len().saturating_add(output.stderr.len()) > MAX_OUTPUT {
        bail!("Devin configuration output exceeded 2 MiB");
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let detail = if stdout.is_empty() { stderr } else { stdout };
    if !output.status.success() {
        bail!(if detail.is_empty() {
            format!("Devin exited with {}", output.status)
        } else {
            detail
        });
    }
    Ok(DevinCommandOutput { output: detail })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_mcp_requires_explicit_workspace_grant() {
        assert!(checked_scope("user", false).is_err());
        assert_eq!(checked_scope("user", true).ok(), Some("user"));
    }

    #[test]
    fn executable_install_confirmation_is_exact() {
        assert!(
            require_confirmation("INSTALL PLUGIN owner/repo", "INSTALL PLUGIN owner/repo").is_ok()
        );
        assert!(require_confirmation("yes", "INSTALL PLUGIN owner/repo").is_err());
    }

    #[tokio::test]
    async fn project_only_skill_listing_does_not_need_global_access() -> anyhow::Result<()> {
        let workspace = tempfile::tempdir()?;
        let skill = workspace.path().join(".devin/skills/review");
        tokio::fs::create_dir_all(&skill).await?;
        tokio::fs::write(skill.join("SKILL.md"), "# Review\n").await?;
        let listing = list_project_skills(workspace.path()).await?;
        assert_eq!(listing.output, "review  .devin/skills/review/SKILL.md");
        let detail = show_project_skill(workspace.path(), "review").await?;
        assert_eq!(detail.output, "# Review\n");
        assert!(
            show_project_skill(workspace.path(), "../review")
                .await
                .is_err()
        );
        Ok(())
    }
}
