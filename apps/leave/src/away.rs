//! Private away access through the owner's Tailscale network.

use anyhow::{Context, bail};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use tokio::process::Command;

/// Verified tailnet owner and HTTPS endpoint.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwayAccess {
    /// Tailscale login accepted by the Leave host.
    pub owner_login: String,
    /// Tailnet-only HTTPS URL configured by Tailscale Serve.
    pub url: String,
}

/// Configure a persistent tailnet-only reverse proxy to Leave's loopback port.
pub async fn enable(port: u16) -> anyhow::Result<AwayAccess> {
    let status = tailscale_status().await?;
    let owner_login = owner_login(&status)?;
    let dns_name = status
        .pointer("/Self/DNSName")
        .and_then(Value::as_str)
        .map(|value| value.trim_end_matches('.').to_owned())
        .filter(|value| !value.is_empty())
        .context("Tailscale did not report a MagicDNS name")?;
    let target = format!("127.0.0.1:{port}");
    let output = tailscale_command()
        .context("Tailscale is not installed; install and sign in to Tailscale first")?
        .args(["serve", "--bg", "--yes", &target])
        .output()
        .await
        .context("could not start Tailscale")?;
    if !output.status.success() {
        let message = command_detail(&output.stdout, &output.stderr);
        bail!(if message.is_empty() {
            format!("tailscale serve exited with {}", output.status)
        } else {
            message
        });
    }
    Ok(AwayAccess {
        owner_login,
        url: format!("https://{dns_name}"),
    })
}

/// Remove the current Tailscale Serve mapping.
pub async fn disable() -> anyhow::Result<String> {
    let output = tailscale_command()
        .context("Tailscale is not installed")?
        .args(["serve", "off"])
        .output()
        .await
        .context("could not start Tailscale")?;
    if !output.status.success() {
        bail!(command_detail(&output.stdout, &output.stderr));
    }
    Ok(command_detail(&output.stdout, &output.stderr))
}

/// Return the machine's current Serve configuration.
pub async fn status() -> anyhow::Result<Value> {
    let output = tailscale_command()
        .context("Tailscale is not installed")?
        .args(["serve", "status", "--json"])
        .output()
        .await
        .context("could not start Tailscale")?;
    if !output.status.success() {
        bail!(command_detail(&output.stdout, &output.stderr));
    }
    serde_json::from_slice(&output.stdout).context("Tailscale returned invalid status JSON")
}

async fn tailscale_status() -> anyhow::Result<Value> {
    let output = tailscale_command()
        .context("Tailscale is not installed; install and sign in to Tailscale first")?
        .args(["status", "--json"])
        .output()
        .await
        .context("could not start Tailscale")?;
    if !output.status.success() {
        bail!(command_detail(&output.stdout, &output.stderr));
    }
    let status: Value =
        serde_json::from_slice(&output.stdout).context("Tailscale returned invalid status JSON")?;
    if status.get("BackendState").and_then(Value::as_str) != Some("Running") {
        bail!("Tailscale is installed but not connected; run tailscale up first");
    }
    Ok(status)
}

/// Build a command for PATH installs and standard desktop app locations.
pub(crate) fn tailscale_command() -> Option<Command> {
    let path = discover_tailscale_binary()?;
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new(path);
        command.env("TAILSCALE_BE_CLI", "1");
        return Some(command);
    }
    #[cfg(not(target_os = "macos"))]
    Some(Command::new(path))
}

fn discover_tailscale_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("LEAVE_TAILSCALE_BIN").map(PathBuf::from)
        && path.is_file()
    {
        return Some(path);
    }
    let executable = if cfg!(windows) {
        "tailscale.exe"
    } else {
        "tailscale"
    };
    if let Some(paths) = std::env::var_os("PATH")
        && let Some(path) = std::env::split_paths(&paths)
            .map(|directory| directory.join(executable))
            .find(|candidate| candidate.is_file())
    {
        return Some(path);
    }

    #[cfg(target_os = "macos")]
    {
        let system = PathBuf::from("/Applications/Tailscale.app/Contents/MacOS/Tailscale");
        if system.is_file() {
            return Some(system);
        }
        if let Some(home) = dirs::home_dir() {
            let user = home.join("Applications/Tailscale.app/Contents/MacOS/Tailscale");
            if user.is_file() {
                return Some(user);
            }
        }
    }

    #[cfg(windows)]
    for root in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
    {
        let candidate = root.join("Tailscale").join("tailscale.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn owner_login(status: &Value) -> anyhow::Result<String> {
    if let Some(login) = status
        .pointer("/Self/UserProfile/LoginName")
        .and_then(Value::as_str)
        .or_else(|| status.pointer("/Self/LoginName").and_then(Value::as_str))
    {
        return Ok(login.to_ascii_lowercase());
    }
    let user_id = status
        .pointer("/Self/UserID")
        .and_then(Value::as_u64)
        .context("Tailscale did not report the current user identity")?;
    status
        .get("User")
        .and_then(|users| users.get(user_id.to_string()))
        .and_then(|user| user.get("LoginName"))
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .context("Tailscale did not report the current user's login name")
}

fn command_detail(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout).trim().to_owned();
    if stdout.is_empty() {
        String::from_utf8_lossy(stderr).trim().to_owned()
    } else {
        stdout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_owner_from_tailscale_user_map() -> anyhow::Result<()> {
        let status = serde_json::json!({
            "Self": {"UserID": 42},
            "User": {"42": {"LoginName": "Owner@Example.com"}}
        });
        assert_eq!(owner_login(&status)?, "owner@example.com");
        Ok(())
    }
}
