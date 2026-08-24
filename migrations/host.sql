CREATE TABLE IF NOT EXISTS workspaces (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  canonical_path TEXT NOT NULL UNIQUE,
  expose_history INTEGER NOT NULL DEFAULT 0,
  expose_project_customization INTEGER NOT NULL DEFAULT 1,
  expose_global_customization INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS command_deduplication (
  command_id TEXT PRIMARY KEY NOT NULL,
  claimed_at_unix_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS events (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id TEXT NOT NULL UNIQUE,
  workspace_id TEXT NOT NULL,
  session_id TEXT,
  kind TEXT NOT NULL,
  occurred_at_unix_ms INTEGER NOT NULL,
  payload BLOB NOT NULL,
  FOREIGN KEY(workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS events_workspace_sequence_idx
  ON events(workspace_id, sequence);
CREATE TABLE IF NOT EXISTS sessions (
  session_id TEXT PRIMARY KEY NOT NULL,
  workspace_id TEXT NOT NULL,
  title TEXT NOT NULL,
  state TEXT NOT NULL,
  created_at_unix_ms INTEGER NOT NULL,
  updated_at_unix_ms INTEGER NOT NULL,
  FOREIGN KEY(workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS sessions_workspace_updated_idx
  ON sessions(workspace_id, updated_at_unix_ms DESC);
CREATE INDEX IF NOT EXISTS events_session_sequence_idx
  ON events(session_id, sequence);
