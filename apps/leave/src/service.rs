//! Per-user host service installation for supported desktop platforms.

use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use uuid::Uuid;

/// Options persisted into the per-user Leave host service.
#[derive(Clone)]
pub struct ServiceInstall {
    /// Registered workspace identifier.
    pub workspace: Uuid,
    /// Packaged PWA directory.
    pub web_dir: PathBuf,
    /// Absolute state directory used by the setup command.
    pub data_dir: PathBuf,
    /// Exact official Devin CLI selected during setup.
    pub devin_binary: PathBuf,
    /// Exact managed browser selected during setup.
    pub chrome_binary: Option<PathBuf>,
    /// Loopback port for the host and Tailscale Serve backend.
    pub port: u16,
    /// Use private Tailscale away access.
    pub away: bool,
    /// Grant raw PTY access.
    pub terminal: bool,
    /// Grant managed preview access.
    pub preview: bool,
}

/// Install and start the current Leave binary as a per-user service.
#[allow(unreachable_code)]
pub async fn install(options: &ServiceInstall) -> anyhow::Result<String> {
    let binary = std::env::current_exe().context("could not locate the Leave executable")?;
    let mut resolved = options.clone();
    resolved.web_dir = tokio::fs::canonicalize(&options.web_dir)
        .await
        .context("the production PWA directory does not exist; run `pnpm build` first")?;
    resolved.data_dir = tokio::fs::canonicalize(&options.data_dir)
        .await
        .context("could not resolve the Leave data directory")?;
    #[cfg(target_os = "linux")]
    return install_systemd(&binary, &resolved).await;
    #[cfg(target_os = "macos")]
    return install_launch_agent(&binary, &resolved).await;
    #[cfg(windows)]
    return install_windows_task(&binary, &resolved).await;
    bail!("service installation is not supported on this platform")
}

/// Stop and remove Leave's per-user service registration.
#[allow(unreachable_code)]
pub async fn uninstall() -> anyhow::Result<String> {
    #[cfg(target_os = "linux")]
    return uninstall_systemd().await;
    #[cfg(target_os = "macos")]
    return uninstall_launch_agent().await;
    #[cfg(windows)]
    return uninstall_windows_task().await;
    bail!("service installation is not supported on this platform")
}

/// Ask the operating system for current service state.
#[allow(unreachable_code)]
pub async fn status() -> anyhow::Result<String> {
    #[cfg(target_os = "linux")]
    return command_output(Command::new("systemctl").args([
        "--user",
        "status",
        "leave.service",
        "--no-pager",
    ]))
    .await;
    #[cfg(target_os = "macos")]
    return command_output(
        Command::new("launchctl").args(["print", &format!("gui/{}/com.leave.host", user_id()?)]),
    )
    .await;
    #[cfg(windows)]
    return command_output(Command::new("schtasks").args(["/Query", "/TN", "LeaveHost", "/V"]))
        .await;
    bail!("service installation is not supported on this platform")
}

fn service_arguments(options: &ServiceInstall) -> Vec<String> {
    let mut arguments = vec![
        "--data-dir".into(),
        options.data_dir.to_string_lossy().into_owned(),
        "serve".into(),
        "--workspace".into(),
        options.workspace.to_string(),
        "--port".into(),
        options.port.to_string(),
        "--web-dir".into(),
        options.web_dir.to_string_lossy().into_owned(),
        "--devin-binary".into(),
        options.devin_binary.to_string_lossy().into_owned(),
    ];
    if options.away {
        arguments.push("--away".into());
    }
    if options.terminal {
        arguments.push("--grant-terminal".into());
    }
    if options.preview {
        arguments.push("--grant-preview".into());
        if let Some(chrome_binary) = &options.chrome_binary {
            arguments.push("--chrome-binary".into());
            arguments.push(chrome_binary.to_string_lossy().into_owned());
        }
    }
    arguments
}

#[cfg(target_os = "linux")]
async fn install_systemd(binary: &Path, options: &ServiceInstall) -> anyhow::Result<String> {
    let directory = dirs::config_dir()
        .context("operating system did not provide a config directory")?
        .join("systemd/user");
    tokio::fs::create_dir_all(&directory).await?;
    let unit = directory.join("leave.service");
    let mut command = escape_systemd(binary.to_string_lossy().as_ref());
    for argument in service_arguments(options) {
        command.push(' ');
        command.push_str(&escape_systemd(&argument));
    }
    let contents = format!(
        "[Unit]\nDescription=Leave local Devin host\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nExecStart={command}\nRestart=on-failure\nRestartSec=5\nNoNewPrivileges=true\nPrivateTmp=true\n\n[Install]\nWantedBy=default.target\n"
    );
    tokio::fs::write(&unit, contents).await?;
    command_output(Command::new("systemctl").args(["--user", "daemon-reload"])).await?;
    command_output(Command::new("systemctl").args(["--user", "enable", "--now", "leave.service"]))
        .await?;
    Ok(format!("Leave service installed at {}", unit.display()))
}

#[cfg(target_os = "linux")]
async fn uninstall_systemd() -> anyhow::Result<String> {
    let unit = dirs::config_dir()
        .context("operating system did not provide a config directory")?
        .join("systemd/user/leave.service");
    let _ = command_output(Command::new("systemctl").args([
        "--user",
        "disable",
        "--now",
        "leave.service",
    ]))
    .await;
    if unit.is_file() {
        tokio::fs::remove_file(&unit).await?;
    }
    command_output(Command::new("systemctl").args(["--user", "daemon-reload"])).await?;
    Ok("Leave user service removed".into())
}

#[cfg(target_os = "macos")]
async fn install_launch_agent(binary: &Path, options: &ServiceInstall) -> anyhow::Result<String> {
    let directory = dirs::home_dir()
        .context("operating system did not provide a home directory")?
        .join("Library/LaunchAgents");
    tokio::fs::create_dir_all(&directory).await?;
    let plist = directory.join("com.leave.host.plist");
    let mut values = vec![binary.to_string_lossy().into_owned()];
    values.extend(service_arguments(options));
    let arguments = values
        .iter()
        .map(|value| format!("    <string>{}</string>", xml_escape(value)))
        .collect::<Vec<_>>()
        .join("\n");
    let contents = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>com.leave.host</string>\n<key>ProgramArguments</key><array>\n{arguments}\n</array>\n<key>RunAtLoad</key><true/>\n<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n<key>ProcessType</key><string>Background</string>\n</dict></plist>\n"
    );
    tokio::fs::write(&plist, contents).await?;
    let domain = format!("gui/{}", user_id()?);
    let _ = command_output(Command::new("launchctl").args([
        "bootout",
        &domain,
        plist.to_string_lossy().as_ref(),
    ]))
    .await;
    command_output(Command::new("launchctl").args([
        "bootstrap",
        &domain,
        plist.to_string_lossy().as_ref(),
    ]))
    .await?;
    Ok(format!(
        "Leave LaunchAgent installed at {}",
        plist.display()
    ))
}

#[cfg(target_os = "macos")]
async fn uninstall_launch_agent() -> anyhow::Result<String> {
    let plist = dirs::home_dir()
        .context("operating system did not provide a home directory")?
        .join("Library/LaunchAgents/com.leave.host.plist");
    let domain = format!("gui/{}", user_id()?);
    let _ = command_output(Command::new("launchctl").args([
        "bootout",
        &domain,
        plist.to_string_lossy().as_ref(),
    ]))
    .await;
    if plist.is_file() {
        tokio::fs::remove_file(plist).await?;
    }
    Ok("Leave LaunchAgent removed".into())
}

#[cfg(target_os = "macos")]
fn user_id() -> anyhow::Result<String> {
    let output = std::process::Command::new("id").arg("-u").output()?;
    if !output.status.success() {
        bail!("could not determine the current user id");
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

#[cfg(windows)]
async fn install_windows_task(binary: &Path, options: &ServiceInstall) -> anyhow::Result<String> {
    let mut command = format!("\"{}\"", binary.display());
    for argument in service_arguments(options) {
        command.push(' ');
        command.push_str(&format!("\"{}\"", argument.replace('"', "\\\"")));
    }
    command_output(Command::new("schtasks").args([
        "/Create",
        "/TN",
        "LeaveHost",
        "/SC",
        "ONLOGON",
        "/RL",
        "LIMITED",
        "/TR",
        &command,
        "/F",
    ]))
    .await?;
    command_output(Command::new("schtasks").args(["/Run", "/TN", "LeaveHost"])).await?;
    Ok("Leave per-user scheduled task installed".into())
}

#[cfg(windows)]
async fn uninstall_windows_task() -> anyhow::Result<String> {
    command_output(Command::new("schtasks").args(["/Delete", "/TN", "LeaveHost", "/F"])).await?;
    Ok("Leave scheduled task removed".into())
}

#[cfg(target_os = "linux")]
fn escape_systemd(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

async fn command_output(command: &mut Command) -> anyhow::Result<String> {
    let output = command
        .output()
        .await
        .context("could not run service manager")?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let detail = if stdout.is_empty() { stderr } else { stdout };
    if !output.status.success() {
        bail!(if detail.is_empty() {
            format!("service manager exited with {}", output.status)
        } else {
            detail
        });
    }
    Ok(detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_keeps_setup_paths_and_port() {
        let options = ServiceInstall {
            workspace: Uuid::nil(),
            web_dir: PathBuf::from("/opt/leave/share/leave/web"),
            data_dir: PathBuf::from("/var/lib/leave user"),
            devin_binary: PathBuf::from("/opt/Devin App/devin"),
            chrome_binary: Some(PathBuf::from("/opt/chrome/chromium")),
            port: 9876,
            away: true,
            terminal: true,
            preview: true,
        };
        let arguments = service_arguments(&options);
        for expected in [
            "/var/lib/leave user",
            "/opt/Devin App/devin",
            "/opt/chrome/chromium",
            "9876",
            "--away",
            "--grant-terminal",
            "--grant-preview",
        ] {
            assert!(arguments.iter().any(|argument| argument == expected));
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn systemd_arguments_are_quoted() {
        assert_eq!(escape_systemd("/tmp/Leave App"), "\"/tmp/Leave App\"");
    }
}
