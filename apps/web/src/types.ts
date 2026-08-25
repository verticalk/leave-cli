export type ConnectionState = "online" | "offline" | "blocked";
export type Role = "owner" | "maintainer" | "operator" | "viewer";
export type SessionTab = "chat" | "files" | "terminal" | "preview";
export type AgentState = "starting" | "ready" | "error" | "stopped";

export interface WorkspaceRecord {
  id: string;
  name: string;
  canonical_path: string;
  expose_history: boolean;
  expose_project_customization: boolean;
  expose_global_customization: boolean;
}

export interface SessionRecord {
  session_id: string;
  workspace_id: string;
  title: string;
  state: "working" | "idle" | "offline" | string;
  created_at_unix_ms: number;
  updated_at_unix_ms: number;
}

export interface AgentStatus {
  state: AgentState;
  detail: string;
  agentInfo: unknown | null;
  capabilities: Record<string, unknown> | null;
}

export interface LocalStatus {
  status: string;
  version: string;
  mode: "local" | "tailnet";
  host: {
    name: string;
    platform: string;
    architecture: string;
  };
  workspace: WorkspaceRecord;
  agent: AgentStatus;
  remoteAvailable: boolean;
  awayUrl: string | null;
  capabilities: {
    files: boolean;
    git: boolean;
    projectCustomization: boolean;
    globalCustomization: boolean;
    terminal: boolean;
    preview: boolean;
  };
}

export interface DirectoryEntry {
  path: string;
  name: string;
  isDirectory: boolean;
  isSymlink: boolean;
  size: number;
}

export interface FileSnapshot {
  content: string;
  hash: string;
  size: number;
  lineEnding: "lf" | "crlf";
  mode: number | null;
}

export interface GitStatusEntry {
  path: string;
  originalPath: string | null;
  index: string;
  worktree: string;
}

export interface GitStatus {
  branch: string;
  ahead: number;
  behind: number;
  entries: GitStatusEntry[];
}

export interface GitBranch {
  name: string;
  current: boolean;
  upstream: string | null;
}

export interface TerminalView {
  terminalId: string;
  shell: string;
}

export interface PreviewView {
  previewId: string;
  url: string;
}

export interface LocalEvent {
  sequence: number;
  eventId: string;
  workspaceId: string;
  sessionId: string | null;
  kind: string;
  occurredAtUnixMs: number;
  payload: Record<string, unknown>;
}

export interface EventPage {
  events: LocalEvent[];
  nextCursor: number;
}

export interface PromptAccepted {
  commandId: string;
  duplicate: boolean;
}

export interface ApiErrorBody {
  error?: {
    status?: number;
    message?: string;
    detail?: string;
  };
}

export interface SetupToolAction {
  id: "installDevin" | "connectDevin" | "connectTailscale";
  label: string;
  command: string;
  detail: string;
}

export interface SetupTool {
  installed: boolean;
  ready: boolean;
  label: string;
  detail: string;
  path: string | null;
  url: string | null;
  account: string | null;
  /** A guided sign-in Leave started is still running for this tool. */
  loginPending: boolean;
  /** The sign-in page this tool asked the person to open, when it printed one. */
  loginUrl: string | null;
  /** What the tool printed during the guided sign-in, when one ran. */
  loginOutput: string | null;
  action: SetupToolAction | null;
  manualCommand: string | null;
}

export interface SetupTailscaleConnection {
  connected: boolean;
  loginUrl: string | null;
  detail: string;
}

export interface SetupDevinLogin {
  signedIn: boolean;
  loginUrl: string | null;
  detail: string;
}

export interface SetupStatus {
  version: string;
  platform: {
    id: "linux" | "macos" | "windows" | "unknown";
    label: string;
    serviceLabel: string;
  };
  devin: SetupTool;
  tailscale: SetupTool;
  browser: SetupTool;
  folderPickerAvailable: boolean;
  workspaceExample: string;
  hostPort: number;
}

export interface SetupLaunchRequest {
  workspacePath: string;
  port: number;
  away: boolean;
  background: boolean;
  terminal: boolean;
  preview: boolean;
  globalCustomization: boolean;
}

export interface SetupLaunchResult {
  localUrl: string;
  awayUrl: string | null;
  awayOwner: string | null;
  workspacePath: string;
  background: boolean;
}
