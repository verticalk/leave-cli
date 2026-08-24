use blake3::Hash;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;
use tokio::{fs, sync::Mutex};

/// A canonical workspace root registered by the host owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRoot(PathBuf);

impl WorkspaceRoot {
    /// Register an existing directory as a workspace boundary.
    ///
    /// # Errors
    ///
    /// Returns an error when the path cannot be canonicalized or is not a directory.
    pub async fn register(path: impl AsRef<Path>) -> Result<Self, GuardedFsError> {
        let canonical = fs::canonicalize(path.as_ref())
            .await
            .map_err(GuardedFsError::Io)?;
        let metadata = fs::metadata(&canonical).await.map_err(GuardedFsError::Io)?;
        if !metadata.is_dir() {
            return Err(GuardedFsError::NotDirectory);
        }
        Ok(Self(canonical))
    }

    /// Return the canonical root.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    async fn resolve_existing(&self, relative: &Path) -> Result<PathBuf, GuardedFsError> {
        validate_relative(relative)?;
        let candidate = self.0.join(relative);
        let canonical = fs::canonicalize(candidate)
            .await
            .map_err(GuardedFsError::Io)?;
        if !canonical.starts_with(&self.0) {
            return Err(GuardedFsError::OutsideWorkspace);
        }
        Ok(canonical)
    }

    async fn resolve_for_write(&self, relative: &Path) -> Result<PathBuf, GuardedFsError> {
        validate_relative(relative)?;
        let name = relative.file_name().ok_or(GuardedFsError::InvalidPath)?;
        let parent = relative.parent().unwrap_or_else(|| Path::new(""));
        let canonical_parent = fs::canonicalize(self.0.join(parent))
            .await
            .map_err(GuardedFsError::Io)?;
        if !canonical_parent.starts_with(&self.0) {
            return Err(GuardedFsError::OutsideWorkspace);
        }
        let candidate = canonical_parent.join(name);
        if fs::symlink_metadata(&candidate).await.is_ok() {
            let canonical = fs::canonicalize(&candidate)
                .await
                .map_err(GuardedFsError::Io)?;
            if !canonical.starts_with(&self.0) || canonical != candidate {
                return Err(GuardedFsError::SymlinkWriteDenied);
            }
        }
        Ok(candidate)
    }
}

fn validate_relative(path: &Path) -> Result<(), GuardedFsError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(GuardedFsError::InvalidPath);
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(GuardedFsError::OutsideWorkspace);
    }
    Ok(())
}

/// A text file and its optimistic-concurrency hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSnapshot {
    /// UTF-8 file content.
    pub content: String,
    /// Lowercase BLAKE3 hash of the exact stored bytes.
    pub hash: String,
    /// Exact file size in bytes.
    pub size: u64,
    /// Existing line-ending convention.
    pub line_ending: LineEnding,
    /// Unix permission bits when the platform exposes them.
    pub mode: Option<u32>,
}

/// Line-ending convention observed in a text file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineEnding {
    /// Lines use LF separators or the file has no line separator.
    Lf,
    /// At least one CRLF separator was observed.
    Crlf,
}

/// One immediate child returned by a guarded directory listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    /// Path relative to the registered workspace.
    pub path: String,
    /// Final path component for display.
    pub name: String,
    /// Whether this entry is a directory.
    pub is_directory: bool,
    /// Whether this entry is a symbolic link or junction-like link.
    pub is_symlink: bool,
    /// File size for regular files.
    pub size: u64,
}

/// File operations constrained to one registered workspace root.
#[derive(Debug, Clone)]
pub struct GuardedFileSystem {
    root: WorkspaceRoot,
    writer: Arc<Mutex<()>>,
}

impl GuardedFileSystem {
    /// Restrict subsequent operations to a registered root.
    #[must_use]
    pub fn new(root: WorkspaceRoot) -> Self {
        Self {
            root,
            writer: Arc::new(Mutex::new(())),
        }
    }

    /// List one directory without following child symlinks.
    ///
    /// # Errors
    ///
    /// Returns an error for path escape or an operating-system failure.
    pub async fn list_directory(
        &self,
        relative: impl AsRef<Path>,
    ) -> Result<Vec<DirectoryEntry>, GuardedFsError> {
        let relative = relative.as_ref();
        let directory = if relative.as_os_str().is_empty() {
            self.root.as_path().to_path_buf()
        } else {
            self.root.resolve_existing(relative).await?
        };
        let metadata = fs::metadata(&directory).await.map_err(GuardedFsError::Io)?;
        if !metadata.is_dir() {
            return Err(GuardedFsError::NotDirectory);
        }
        let mut reader = fs::read_dir(directory).await.map_err(GuardedFsError::Io)?;
        let mut entries = Vec::new();
        while let Some(entry) = reader.next_entry().await.map_err(GuardedFsError::Io)? {
            let metadata = fs::symlink_metadata(entry.path())
                .await
                .map_err(GuardedFsError::Io)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let child = if relative.as_os_str().is_empty() {
                PathBuf::from(&name)
            } else {
                relative.join(&name)
            };
            entries.push(DirectoryEntry {
                path: path_for_api(&child)?,
                name,
                is_directory: metadata.is_dir(),
                is_symlink: metadata.file_type().is_symlink(),
                size: metadata.len(),
            });
        }
        entries.sort_by(|left, right| {
            right
                .is_directory
                .cmp(&left.is_directory)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(entries)
    }

    /// Read a UTF-8 file and calculate its BLAKE3 content hash.
    ///
    /// # Errors
    ///
    /// Returns an error for path escape, unsupported encoding, or an I/O failure.
    pub async fn read_text(
        &self,
        relative: impl AsRef<Path>,
    ) -> Result<FileSnapshot, GuardedFsError> {
        let path = self.root.resolve_existing(relative.as_ref()).await?;
        let bytes = fs::read(&path).await.map_err(GuardedFsError::Io)?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(GuardedFsError::TooLarge);
        }
        let content = String::from_utf8(bytes).map_err(|_| GuardedFsError::NotUtf8)?;
        let hash = content_hash(content.as_bytes()).to_hex().to_string();
        let line_ending = detect_line_ending(content.as_bytes());
        let metadata = fs::metadata(&path).await.map_err(GuardedFsError::Io)?;
        #[cfg(unix)]
        let mode = Some(file_mode(&metadata));
        #[cfg(not(unix))]
        let mode = None;
        Ok(FileSnapshot {
            size: content.len() as u64,
            content,
            hash,
            line_ending,
            mode,
        })
    }

    /// Atomically write text when the caller still has the current base version.
    ///
    /// # Errors
    ///
    /// Returns an error for path escape, stale content, or an I/O failure.
    pub async fn write_text(
        &self,
        relative: impl AsRef<Path>,
        base_hash: &str,
        content: &str,
    ) -> Result<FileSnapshot, GuardedFsError> {
        let _writer = self.writer.lock().await;
        let path = self.root.resolve_for_write(relative.as_ref()).await?;
        let existing = fs::read(&path).await.ok();
        let current_hash = existing.as_deref().map_or_else(
            || content_hash(&[]).to_hex().to_string(),
            |bytes| content_hash(bytes).to_hex().to_string(),
        );
        if !constant_time_hash_eq(&current_hash, base_hash) {
            return Err(GuardedFsError::Conflict { current_hash });
        }

        let normalized = preserve_line_endings(existing.as_deref(), content);
        let bytes = normalized.as_bytes().to_vec();
        let expected_hash = current_hash;
        let write_path = path.clone();
        let permissions = fs::metadata(&path)
            .await
            .ok()
            .map(|metadata| metadata.permissions());
        tokio::task::spawn_blocking(move || {
            let latest = match std::fs::read(&write_path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => return Err(GuardedFsError::Io(error)),
            };
            let latest_hash = content_hash(&latest).to_hex().to_string();
            if !constant_time_hash_eq(&latest_hash, &expected_hash) {
                return Err(GuardedFsError::Conflict {
                    current_hash: latest_hash,
                });
            }
            let mut file = atomic_write_file::AtomicWriteFile::open(&write_path)
                .map_err(GuardedFsError::Io)?;
            if let Some(permissions) = permissions {
                file.set_permissions(permissions)
                    .map_err(GuardedFsError::Io)?;
            }
            file.write_all(&bytes).map_err(GuardedFsError::Io)?;
            file.commit().map_err(GuardedFsError::Io)
        })
        .await
        .map_err(|error| GuardedFsError::Io(std::io::Error::other(error)))??;

        let hash = content_hash(normalized.as_bytes()).to_hex().to_string();
        let metadata = fs::metadata(&path).await.map_err(GuardedFsError::Io)?;
        #[cfg(unix)]
        let mode = Some(file_mode(&metadata));
        #[cfg(not(unix))]
        let mode = None;
        Ok(FileSnapshot {
            size: normalized.len() as u64,
            line_ending: detect_line_ending(normalized.as_bytes()),
            mode,
            content: normalized,
            hash,
        })
    }
}

fn content_hash(bytes: &[u8]) -> Hash {
    blake3::hash(bytes)
}

fn constant_time_hash_eq(left: &str, right: &str) -> bool {
    use subtle::ConstantTimeEq;
    left.as_bytes().ct_eq(right.as_bytes()).into()
}

fn preserve_line_endings(existing: Option<&[u8]>, content: &str) -> String {
    let uses_crlf = existing.is_some_and(|bytes| bytes.windows(2).any(|pair| pair == b"\r\n"));
    let lf = content.replace("\r\n", "\n");
    if uses_crlf {
        lf.replace('\n', "\r\n")
    } else {
        lf
    }
}

fn detect_line_ending(bytes: &[u8]) -> LineEnding {
    if bytes.windows(2).any(|pair| pair == b"\r\n") {
        LineEnding::Crlf
    } else {
        LineEnding::Lf
    }
}

#[cfg(unix)]
fn file_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

fn path_for_api(path: &Path) -> Result<String, GuardedFsError> {
    let value = path.to_str().ok_or(GuardedFsError::InvalidPath)?;
    Ok(value.replace('\\', "/"))
}

/// Errors which prevent a guarded file operation.
#[derive(Debug, Error)]
pub enum GuardedFsError {
    /// The caller supplied an empty, absolute, or nameless path.
    #[error("path is invalid")]
    InvalidPath,
    /// The workspace registration target is not a directory.
    #[error("workspace path is not a directory")]
    NotDirectory,
    /// Canonicalization found a path outside the registered root.
    #[error("path escapes the registered workspace")]
    OutsideWorkspace,
    /// The requested write target resolves through a symbolic link.
    #[error("writes through symlinks are denied")]
    SymlinkWriteDenied,
    /// Direct editing only supports UTF-8 text files.
    #[error("file is not valid UTF-8")]
    NotUtf8,
    /// Direct editing is capped to keep mobile memory usage bounded.
    #[error("file exceeds the 2 MiB direct-editing limit")]
    TooLarge,
    /// The file no longer matches the caller's optimistic base hash.
    #[error("file changed since it was read; current hash is {current_hash}")]
    Conflict {
        /// Hash of the bytes currently stored on the host.
        current_hash: String,
    },
    /// The operating system rejected a guarded file operation.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn rejects_parent_traversal() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let root = WorkspaceRoot::register(directory.path()).await?;
        let fs = GuardedFileSystem::new(root);
        let Err(error) = fs.read_text("../secret").await else {
            anyhow::bail!("parent traversal was unexpectedly accepted");
        };
        assert!(matches!(error, GuardedFsError::OutsideWorkspace));
        Ok(())
    }

    #[tokio::test]
    async fn rejects_stale_writes_and_preserves_crlf() -> anyhow::Result<()> {
        let directory = tempdir()?;
        tokio::fs::write(directory.path().join("note.txt"), b"one\r\n").await?;
        let root = WorkspaceRoot::register(directory.path()).await?;
        let fs = GuardedFileSystem::new(root);
        let snapshot = fs.read_text("note.txt").await?;
        let updated = fs.write_text("note.txt", &snapshot.hash, "two\n").await?;
        assert_eq!(updated.content, "two\r\n");
        let Err(error) = fs.write_text("note.txt", &snapshot.hash, "three\n").await else {
            anyhow::bail!("stale write was unexpectedly accepted");
        };
        assert!(matches!(error, GuardedFsError::Conflict { .. }));
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_writes_from_one_base_cannot_both_win() -> anyhow::Result<()> {
        let directory = tempdir()?;
        tokio::fs::write(directory.path().join("note.txt"), b"base\n").await?;
        let root = WorkspaceRoot::register(directory.path()).await?;
        let fs = GuardedFileSystem::new(root);
        let snapshot = fs.read_text("note.txt").await?;
        let left_fs = fs.clone();
        let left_hash = snapshot.hash.clone();
        let right_fs = fs.clone();
        let right_hash = snapshot.hash;
        let (left, right) = tokio::join!(
            left_fs.write_text("note.txt", &left_hash, "left\n"),
            right_fs.write_text("note.txt", &right_hash, "right\n")
        );
        assert_ne!(left.is_ok(), right.is_ok());
        assert!(matches!(
            left.err().or_else(|| right.err()),
            Some(GuardedFsError::Conflict { .. })
        ));
        Ok(())
    }
}
