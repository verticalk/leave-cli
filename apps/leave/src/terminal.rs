//! Owner-granted persistent PTYs for the active workspace.

use anyhow::{Context, bail};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::{Mutex as AsyncMutex, broadcast};
use uuid::Uuid;

const TERMINAL_CHANNEL_CAPACITY: usize = 512;

/// Metadata for a running host terminal.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalView {
    /// Host-lifetime terminal identifier.
    pub terminal_id: Uuid,
    /// Shell executable selected on the host.
    pub shell: String,
}

#[derive(Clone)]
pub struct TerminalManager {
    root: PathBuf,
    enabled: bool,
    sessions: Arc<AsyncMutex<HashMap<Uuid, Arc<TerminalSession>>>>,
}

struct TerminalSession {
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    _child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    output: broadcast::Sender<Vec<u8>>,
}

impl TerminalManager {
    /// Create a terminal capability boundary for one workspace.
    #[must_use]
    pub fn new(root: PathBuf, enabled: bool) -> Self {
        Self {
            root,
            enabled,
            sessions: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    /// Whether the owner explicitly enabled raw PTYs for this host run.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Spawn a persistent terminal in the registered workspace.
    pub async fn create(&self, rows: u16, cols: u16) -> anyhow::Result<TerminalView> {
        if !self.enabled {
            bail!("terminal access is off; restart Leave with --grant-terminal");
        }
        let root = self.root.clone();
        let shell = default_shell();
        let shell_for_task = shell.clone();
        let session =
            tokio::task::spawn_blocking(move || spawn_terminal(&root, &shell_for_task, rows, cols))
                .await
                .context("terminal worker stopped unexpectedly")??;
        let terminal_id = Uuid::now_v7();
        self.sessions
            .lock()
            .await
            .insert(terminal_id, Arc::new(session));
        Ok(TerminalView { terminal_id, shell })
    }

    /// Return a running terminal.
    async fn session(&self, terminal_id: Uuid) -> anyhow::Result<Arc<TerminalSession>> {
        self.sessions
            .lock()
            .await
            .get(&terminal_id)
            .cloned()
            .context("terminal was not found or has expired")
    }

    /// Subscribe to live output. Scrollback is deliberately not retained.
    pub async fn subscribe(
        &self,
        terminal_id: Uuid,
    ) -> anyhow::Result<broadcast::Receiver<Vec<u8>>> {
        Ok(self.session(terminal_id).await?.output.subscribe())
    }

    /// Write raw bytes to a running terminal.
    pub async fn write(&self, terminal_id: Uuid, bytes: Vec<u8>) -> anyhow::Result<()> {
        if bytes.len() > 64 * 1024 {
            bail!("terminal input frame exceeded 64 KiB");
        }
        let session = self.session(terminal_id).await?;
        tokio::task::spawn_blocking(move || {
            let mut writer = session
                .writer
                .lock()
                .map_err(|_| anyhow::anyhow!("terminal writer lock was poisoned"))?;
            writer.write_all(&bytes)?;
            writer.flush()?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("terminal writer stopped unexpectedly")??;
        Ok(())
    }

    /// Resize a running terminal.
    pub async fn resize(&self, terminal_id: Uuid, rows: u16, cols: u16) -> anyhow::Result<()> {
        if !(2..=500).contains(&rows) || !(2..=500).contains(&cols) {
            bail!("terminal dimensions must be between 2 and 500");
        }
        let session = self.session(terminal_id).await?;
        tokio::task::spawn_blocking(move || {
            session
                .master
                .lock()
                .map_err(|_| anyhow::anyhow!("terminal resize lock was poisoned"))?
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
        })
        .await
        .context("terminal resize worker stopped unexpectedly")??;
        Ok(())
    }
}

fn spawn_terminal(
    root: &Path,
    shell: &str,
    rows: u16,
    cols: u16,
) -> anyhow::Result<TerminalSession> {
    if !(2..=500).contains(&rows) || !(2..=500).contains(&cols) {
        bail!("terminal dimensions must be between 2 and 500");
    }
    let pair = native_pty_system().openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command = CommandBuilder::new(shell);
    command.cwd(root);
    let child = pair.slave.spawn_command(command)?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let master = Mutex::new(pair.master);
    let (output, _) = broadcast::channel(TERMINAL_CHANNEL_CAPACITY);
    let reader_output = output.clone();
    std::thread::Builder::new()
        .name("leave-pty-reader".into())
        .spawn(move || copy_output(&mut *reader, &reader_output))?;
    Ok(TerminalSession {
        writer: Mutex::new(writer),
        master,
        _child: Mutex::new(child),
        output,
    })
}

fn copy_output(reader: &mut dyn Read, output: &broadcast::Sender<Vec<u8>>) {
    let mut buffer = vec![0_u8; 16 * 1024];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let _ = output.send(buffer[..read].to_vec());
            }
        }
    }
}

fn default_shell() -> String {
    #[cfg(windows)]
    {
        std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".into())
    }
    #[cfg(not(windows))]
    {
        std::env::var("SHELL")
            .ok()
            .filter(|shell| Path::new(shell).is_absolute() && Path::new(shell).is_file())
            .unwrap_or_else(|| "/bin/sh".into())
    }
}
