//! Local workspace, persistence, and conflict-safety primitives.

mod event_store;
mod guarded_fs;

pub use event_store::{CommandClaim, EventRecord, EventStore, SessionRecord, WorkspaceRecord};
pub use guarded_fs::{
    DirectoryEntry, FileSnapshot, GuardedFileSystem, GuardedFsError, LineEnding, WorkspaceRoot,
};
