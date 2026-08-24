//! Structured Git operations constrained to the active workspace.

use anyhow::{Context, bail};
use serde::Serialize;
use std::{path::Path, process::Output};
use tokio::process::Command;

const MAX_GIT_OUTPUT: usize = 2 * 1024 * 1024;

/// One porcelain status entry.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusEntry {
    /// Repository-relative path.
    pub path: String,
    /// Optional former path for a rename or copy.
    pub original_path: Option<String>,
    /// Index status character.
    pub index: String,
    /// Worktree status character.
    pub worktree: String,
}

/// Current branch and changed paths.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    /// Checked-out branch, or `HEAD` when detached.
    pub branch: String,
    /// Ahead count reported by Git.
    pub ahead: u32,
    /// Behind count reported by Git.
    pub behind: u32,
    /// Changed and untracked files.
    pub entries: Vec<GitStatusEntry>,
}

/// A local branch.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranch {
    /// Full short branch name.
    pub name: String,
    /// Whether this branch is currently checked out.
    pub current: bool,
    /// Short upstream name when configured.
    pub upstream: Option<String>,
}

/// Read Git status without invoking a shell.
pub async fn status(root: &Path) -> anyhow::Result<GitStatus> {
    let branch_output = run(root, &["status", "--porcelain=v2", "--branch", "-z"]).await?;
    parse_status(&branch_output.stdout)
}

/// Return a bounded unified diff.
pub async fn diff(root: &Path, path: Option<&str>, staged: bool) -> anyhow::Result<String> {
    let mut arguments = vec!["diff", "--no-ext-diff", "--no-color"];
    if staged {
        arguments.push("--cached");
    }
    if let Some(path) = path {
        validate_relative_path(path)?;
        arguments.extend(["--", path]);
    }
    output_text(run(root, &arguments).await?)
}

/// Stage exact repository-relative paths.
pub async fn stage(root: &Path, paths: &[String]) -> anyhow::Result<()> {
    require_paths(paths)?;
    let mut arguments = vec!["--literal-pathspecs", "add", "--"];
    arguments.extend(paths.iter().map(String::as_str));
    run(root, &arguments).await?;
    Ok(())
}

/// Unstage exact repository-relative paths without touching their content.
pub async fn unstage(root: &Path, paths: &[String]) -> anyhow::Result<()> {
    require_paths(paths)?;
    let mut arguments = vec!["--literal-pathspecs", "restore", "--staged", "--"];
    arguments.extend(paths.iter().map(String::as_str));
    run(root, &arguments).await?;
    Ok(())
}

/// Create a commit from the current index.
pub async fn commit(root: &Path, message: &str) -> anyhow::Result<String> {
    let message = message.trim();
    if message.is_empty() || message.chars().count() > 10_000 {
        bail!("commit message must contain between 1 and 10,000 characters");
    }
    output_text(run(root, &["commit", "-m", message]).await?)
}

/// Push with Git's configured upstream and credentials.
pub async fn push(root: &Path) -> anyhow::Result<String> {
    output_text(run(root, &["push"]).await?)
}

/// List local branches with their upstream.
pub async fn branches(root: &Path) -> anyhow::Result<Vec<GitBranch>> {
    let output = output_text(
        run(
            root,
            &[
                "for-each-ref",
                "--format=%(HEAD)%00%(refname:short)%00%(upstream:short)",
                "refs/heads/",
            ],
        )
        .await?,
    )?;
    output
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut fields = line.split('\0');
            let head = fields
                .next()
                .context("Git returned an invalid branch record")?;
            let name = fields
                .next()
                .context("Git returned an invalid branch record")?
                .to_owned();
            let upstream = fields
                .next()
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            Ok(GitBranch {
                name,
                current: head == "*",
                upstream,
            })
        })
        .collect()
}

/// Switch to an existing branch or create a new one from HEAD.
pub async fn switch_branch(root: &Path, name: &str, create: bool) -> anyhow::Result<()> {
    validate_branch(name)?;
    let arguments = if create {
        vec!["switch", "-c", name]
    } else {
        vec!["switch", name]
    };
    run(root, &arguments).await?;
    Ok(())
}

/// Return Git's read-only worktree inventory.
pub async fn worktrees(root: &Path) -> anyhow::Result<String> {
    output_text(run(root, &["worktree", "list", "--porcelain"]).await?)
}

fn parse_status(bytes: &[u8]) -> anyhow::Result<GitStatus> {
    let mut branch = "HEAD".to_owned();
    let mut ahead = 0;
    let mut behind = 0;
    let records = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut index = 0;
    while index < records.len() {
        let record = String::from_utf8_lossy(records[index]);
        index += 1;
        if record.is_empty() {
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.head ") {
            value.clone_into(&mut branch);
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.ab ") {
            for field in value.split_whitespace() {
                if let Some(value) = field.strip_prefix('+') {
                    ahead = value.parse().unwrap_or(0);
                } else if let Some(value) = field.strip_prefix('-') {
                    behind = value.parse().unwrap_or(0);
                }
            }
            continue;
        }
        if record.starts_with("1 ") || record.starts_with("2 ") {
            let fields = record.splitn(9, ' ').collect::<Vec<_>>();
            if fields.len() < 9 {
                bail!("Git returned an invalid status record");
            }
            let xy = fields[1].as_bytes();
            let original_path = if record.starts_with("2 ") {
                let value = records.get(index).context("Git omitted a rename source")?;
                index += 1;
                Some(String::from_utf8_lossy(value).into_owned())
            } else {
                None
            };
            entries.push(GitStatusEntry {
                path: fields[8].to_owned(),
                original_path,
                index: char::from(*xy.first().unwrap_or(&b' ')).to_string(),
                worktree: char::from(*xy.get(1).unwrap_or(&b' ')).to_string(),
            });
            continue;
        }
        if let Some(path) = record.strip_prefix("? ") {
            entries.push(GitStatusEntry {
                path: path.to_owned(),
                original_path: None,
                index: "?".into(),
                worktree: "?".into(),
            });
        }
    }
    Ok(GitStatus {
        branch,
        ahead,
        behind,
        entries,
    })
}

fn require_paths(paths: &[String]) -> anyhow::Result<()> {
    if paths.is_empty() || paths.len() > 500 {
        bail!("select between 1 and 500 paths");
    }
    for path in paths {
        validate_relative_path(path)?;
    }
    Ok(())
}

fn validate_relative_path(value: &str) -> anyhow::Result<()> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\0')
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        bail!("Git path must stay within the workspace");
    }
    Ok(())
}

fn validate_branch(value: &str) -> anyhow::Result<()> {
    if value.is_empty()
        || value.len() > 255
        || value.starts_with('-')
        || value.chars().any(|character| {
            !(character.is_ascii_alphanumeric() || "-_/ .".contains(character)) || character == ' '
        })
        || value.contains("..")
        || value.contains("//")
        || value.ends_with('/')
    {
        bail!("branch name is not accepted by Leave");
    }
    Ok(())
}

async fn run(root: &Path, arguments: &[&str]) -> anyhow::Result<Output> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .await
        .context("could not start Git")?;
    if output.stdout.len().saturating_add(output.stderr.len()) > MAX_GIT_OUTPUT {
        bail!("Git output exceeded the 2 MiB response limit");
    }
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        bail!(if message.is_empty() {
            format!("Git exited with {}", output.status)
        } else {
            message
        });
    }
    Ok(output)
}

fn output_text(output: Output) -> anyhow::Result<String> {
    let mut text = String::from_utf8(output.stdout).context("Git output was not UTF-8")?;
    if text.trim().is_empty() && !output.stderr.is_empty() {
        text = String::from_utf8(output.stderr).context("Git output was not UTF-8")?;
    }
    Ok(text.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_git_path_escape() {
        assert!(validate_relative_path("../outside").is_err());
        assert!(validate_relative_path("src/main.rs").is_ok());
    }

    #[test]
    fn rejects_branch_option_injection() {
        assert!(validate_branch("--detach").is_err());
        assert!(validate_branch("feature/mobile").is_ok());
    }
}
