use serde::{Deserialize, Serialize};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};
use time::OffsetDateTime;
use uuid::Uuid;

/// One locally registered workspace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceRecord {
    /// Stable public workspace identifier.
    pub id: Uuid,
    /// Owner-selected display name.
    pub name: String,
    /// Canonical host path which bounds all file operations.
    pub canonical_path: PathBuf,
    /// Whether supported Devin session history may be shown remotely.
    pub expose_history: bool,
    /// Whether project rules and skills may be shown remotely.
    pub expose_project_customization: bool,
    /// Whether global customization may be shown remotely.
    pub expose_global_customization: bool,
}

/// One Devin session known to the local Leave host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionRecord {
    /// Opaque session identifier issued by the ACP agent.
    pub session_id: String,
    /// Registered workspace that bounds this session.
    pub workspace_id: Uuid,
    /// Human-readable title shown by Leave.
    pub title: String,
    /// Current host-observed lifecycle state.
    pub state: String,
    /// Host timestamp for initial registration.
    pub created_at_unix_ms: i64,
    /// Host timestamp for the latest activity.
    pub updated_at_unix_ms: i64,
}

/// A durable host event appended before network fanout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord {
    /// Host-local monotonic replay position.
    pub sequence: i64,
    /// Globally unique event identifier used for deduplication.
    pub event_id: Uuid,
    /// Workspace which owns the event.
    pub workspace_id: Uuid,
    /// Optional Devin session identifier.
    pub session_id: Option<String>,
    /// Versioned event discriminator.
    pub kind: String,
    /// Host timestamp recorded before fanout.
    pub occurred_at: OffsetDateTime,
    /// Opaque serialized event body.
    pub payload: Vec<u8>,
}

/// Result of claiming an at-least-once command ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClaim {
    /// The host may perform the command once.
    New,
    /// The host has already seen the command identifier.
    Duplicate,
}

/// SQLite-backed authoritative local event and workspace store.
#[derive(Debug, Clone)]
pub struct EventStore {
    pool: SqlitePool,
}

impl EventStore {
    /// Open a local database with durability settings suitable for approvals.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite cannot open, configure, or migrate the file.
    pub async fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let url = format!("sqlite://{}", path.as_ref().display());
        let options = SqliteConnectOptions::from_str(&url)?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            // A single connection is the serialized authoritative writer. This
            // keeps sequence assignment and append-before-fanout ordering simple.
            .max_connections(1)
            .connect_with(options)
            .await?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&self) -> anyhow::Result<()> {
        for statement in include_str!("../../../migrations/host.sql").split(";\n") {
            let statement = statement.trim();
            if !statement.is_empty() {
                sqlx::query(statement).execute(&self.pool).await?;
            }
        }
        Ok(())
    }

    /// Insert or replace owner-controlled workspace metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite rejects the transaction.
    pub async fn upsert_workspace(&self, workspace: &WorkspaceRecord) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO workspaces
             (id, name, canonical_path, expose_history, expose_project_customization, expose_global_customization)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
             name = excluded.name,
             canonical_path = excluded.canonical_path,
             expose_history = excluded.expose_history,
             expose_project_customization = excluded.expose_project_customization,
             expose_global_customization = excluded.expose_global_customization",
        )
        .bind(workspace.id.to_string())
        .bind(&workspace.name)
        .bind(workspace.canonical_path.to_string_lossy().as_ref())
        .bind(workspace.expose_history)
        .bind(workspace.expose_project_customization)
        .bind(workspace.expose_global_customization)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Return registered workspaces sorted for stable CLI output.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite cannot read or decode a workspace row.
    pub async fn list_workspaces(&self) -> anyhow::Result<Vec<WorkspaceRecord>> {
        let rows = sqlx::query(
            "SELECT id, name, canonical_path, expose_history,
                    expose_project_customization, expose_global_customization
             FROM workspaces ORDER BY name, id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_workspace).collect()
    }

    /// Remove one registered workspace without deleting its files.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite cannot delete the registration.
    pub async fn remove_workspace(&self, id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM workspaces WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Insert or update locally known session metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite rejects the transaction.
    pub async fn upsert_session(&self, session: &SessionRecord) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO sessions
             (session_id, workspace_id, title, state, created_at_unix_ms, updated_at_unix_ms)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(session_id) DO UPDATE SET
             workspace_id = excluded.workspace_id,
             title = excluded.title,
             state = excluded.state,
             updated_at_unix_ms = excluded.updated_at_unix_ms",
        )
        .bind(&session.session_id)
        .bind(session.workspace_id.to_string())
        .bind(&session.title)
        .bind(&session.state)
        .bind(session.created_at_unix_ms)
        .bind(session.updated_at_unix_ms)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Return sessions for a workspace, newest activity first.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite cannot read or decode a session row.
    pub async fn list_sessions(&self, workspace_id: Uuid) -> anyhow::Result<Vec<SessionRecord>> {
        let rows = sqlx::query(
            "SELECT session_id, workspace_id, title, state,
                    created_at_unix_ms, updated_at_unix_ms
             FROM sessions WHERE workspace_id = ?
             ORDER BY updated_at_unix_ms DESC, session_id",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_session).collect()
    }

    /// Return one session when it belongs to the requested workspace.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite cannot read or decode the session row.
    pub async fn get_session(
        &self,
        workspace_id: Uuid,
        session_id: &str,
    ) -> anyhow::Result<Option<SessionRecord>> {
        let row = sqlx::query(
            "SELECT session_id, workspace_id, title, state,
                    created_at_unix_ms, updated_at_unix_ms
             FROM sessions WHERE workspace_id = ? AND session_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_session).transpose()
    }

    /// Update the lifecycle state and activity timestamp for one session.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite rejects the update.
    pub async fn update_session_state(
        &self,
        session_id: &str,
        state: &str,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET state = ?, updated_at_unix_ms = ? WHERE session_id = ?",
        )
        .bind(state)
        .bind(unix_millis(OffsetDateTime::now_utc()))
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Mark all persisted sessions in a workspace as needing ACP resume.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite rejects the update.
    pub async fn mark_workspace_sessions_offline(&self, workspace_id: Uuid) -> anyhow::Result<u64> {
        let result = sqlx::query("UPDATE sessions SET state = 'offline' WHERE workspace_id = ?")
            .bind(workspace_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Atomically remember a command ID before performing its side effect.
    ///
    /// # Errors
    ///
    /// Returns an error when the durable command claim cannot be written.
    pub async fn claim_command(&self, command_id: Uuid) -> anyhow::Result<CommandClaim> {
        let result = sqlx::query(
            "INSERT OR IGNORE INTO command_deduplication (command_id, claimed_at_unix_ms) VALUES (?, ?)",
        )
        .bind(command_id.to_string())
        .bind(unix_millis(OffsetDateTime::now_utc()))
        .execute(&self.pool)
        .await?;
        Ok(if result.rows_affected() == 1 {
            CommandClaim::New
        } else {
            CommandClaim::Duplicate
        })
    }

    /// Append an event and return its local sequence.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite cannot durably append the event.
    pub async fn append_event(
        &self,
        workspace_id: Uuid,
        session_id: Option<&str>,
        kind: &str,
        payload: &[u8],
    ) -> anyhow::Result<EventRecord> {
        let event_id = Uuid::now_v7();
        let occurred_at = OffsetDateTime::now_utc();
        let result = sqlx::query(
            "INSERT INTO events
             (event_id, workspace_id, session_id, kind, occurred_at_unix_ms, payload)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(event_id.to_string())
        .bind(workspace_id.to_string())
        .bind(session_id)
        .bind(kind)
        .bind(unix_millis(occurred_at))
        .bind(payload)
        .execute(&self.pool)
        .await?;
        let sequence = result.last_insert_rowid();
        Ok(EventRecord {
            sequence,
            event_id,
            workspace_id,
            session_id: session_id.map(str::to_owned),
            kind: kind.to_owned(),
            occurred_at,
            payload: payload.to_vec(),
        })
    }

    /// Replay events after a client cursor.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite cannot read or decode an event row.
    pub async fn events_after(
        &self,
        workspace_id: Uuid,
        after_sequence: i64,
        limit: u32,
    ) -> anyhow::Result<Vec<EventRecord>> {
        let rows = sqlx::query(
            "SELECT sequence, event_id, workspace_id, session_id, kind, occurred_at_unix_ms, payload
             FROM events WHERE workspace_id = ? AND sequence > ?
             ORDER BY sequence ASC LIMIT ?",
        )
        .bind(workspace_id.to_string())
        .bind(after_sequence)
        .bind(i64::from(limit.min(1_000)))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_event).collect()
    }

    /// Replay events for one session after a client cursor.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite cannot read or decode an event row.
    pub async fn session_events_after(
        &self,
        workspace_id: Uuid,
        session_id: &str,
        after_sequence: i64,
        limit: u32,
    ) -> anyhow::Result<Vec<EventRecord>> {
        let rows = sqlx::query(
            "SELECT sequence, event_id, workspace_id, session_id, kind, occurred_at_unix_ms, payload
             FROM events WHERE workspace_id = ? AND session_id = ? AND sequence > ?
             ORDER BY sequence ASC LIMIT ?",
        )
        .bind(workspace_id.to_string())
        .bind(session_id)
        .bind(after_sequence)
        .bind(i64::from(limit.min(1_000)))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_event).collect()
    }
}

fn row_to_workspace(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<WorkspaceRecord> {
    Ok(WorkspaceRecord {
        id: Uuid::parse_str(row.try_get("id")?)?,
        name: row.try_get("name")?,
        canonical_path: PathBuf::from(row.try_get::<String, _>("canonical_path")?),
        expose_history: row.try_get("expose_history")?,
        expose_project_customization: row.try_get("expose_project_customization")?,
        expose_global_customization: row.try_get("expose_global_customization")?,
    })
}

fn row_to_event(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<EventRecord> {
    let milliseconds: i64 = row.try_get("occurred_at_unix_ms")?;
    Ok(EventRecord {
        sequence: row.try_get("sequence")?,
        event_id: Uuid::parse_str(row.try_get("event_id")?)?,
        workspace_id: Uuid::parse_str(row.try_get("workspace_id")?)?,
        session_id: row.try_get("session_id")?,
        kind: row.try_get("kind")?,
        occurred_at: OffsetDateTime::from_unix_timestamp_nanos(
            i128::from(milliseconds) * 1_000_000,
        )?,
        payload: row.try_get("payload")?,
    })
}

fn row_to_session(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<SessionRecord> {
    Ok(SessionRecord {
        session_id: row.try_get("session_id")?,
        workspace_id: Uuid::parse_str(row.try_get("workspace_id")?)?,
        title: row.try_get("title")?,
        state: row.try_get("state")?,
        created_at_unix_ms: row.try_get("created_at_unix_ms")?,
        updated_at_unix_ms: row.try_get("updated_at_unix_ms")?,
    })
}

fn unix_millis(value: OffsetDateTime) -> i64 {
    value.unix_timestamp() * 1_000 + i64::from(value.millisecond())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn command_claims_are_idempotent_and_events_replay_in_order() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = EventStore::open(directory.path().join("leave.db")).await?;
        let workspace = WorkspaceRecord {
            id: Uuid::now_v7(),
            name: "sample".into(),
            canonical_path: directory.path().to_path_buf(),
            expose_history: true,
            expose_project_customization: true,
            expose_global_customization: false,
        };
        store.upsert_workspace(&workspace).await?;
        let command_id = Uuid::now_v7();
        assert_eq!(store.claim_command(command_id).await?, CommandClaim::New);
        assert_eq!(
            store.claim_command(command_id).await?,
            CommandClaim::Duplicate
        );
        store
            .append_event(workspace.id, Some("s1"), "agent_chunk", b"one")
            .await?;
        store
            .append_event(workspace.id, Some("s1"), "agent_chunk", b"two")
            .await?;
        let session = SessionRecord {
            session_id: "s1".into(),
            workspace_id: workspace.id,
            title: "First session".into(),
            state: "idle".into(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 2,
        };
        store.upsert_session(&session).await?;
        assert_eq!(store.list_sessions(workspace.id).await?, vec![session]);
        let events = store.events_after(workspace.id, 0, 10).await?;
        assert_eq!(events.len(), 2);
        assert!(events[0].sequence < events[1].sequence);
        assert_eq!(
            store
                .session_events_after(workspace.id, "s1", events[0].sequence, 10)
                .await?
                .len(),
            1
        );
        Ok(())
    }
}
