import type {
  ApiErrorBody,
  DirectoryEntry,
  EventPage,
  LocalEvent,
  LocalStatus,
  FileSnapshot,
  GitBranch,
  GitStatus,
  PreviewView,
  PromptAccepted,
  SessionRecord,
  SetupLaunchRequest,
  SetupLaunchResult,
  SetupStatus,
  SetupTailscaleConnection,
  TerminalView,
  WorkspaceRecord
} from "../types";

export class LocalApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    /** Raw tool output, shown only when someone opens the details. */
    readonly detail?: string
  ) {
    super(message);
    this.name = "LocalApiError";
  }
}

async function setupRequest<T>(token: string, path: string, init?: RequestInit): Promise<T> {
  return request<T>(`/api/v1/setup${path}`, {
    ...init,
    headers: { "X-Leave-Setup-Token": token, ...init?.headers }
  });
}

export function fetchSetupStatus(token: string, signal?: AbortSignal) {
  return setupRequest<SetupStatus>(token, "/status", { signal });
}

export function loginSetupDevin(token: string) {
  return setupRequest<SetupStatus>(token, "/auth/login", { method: "POST", body: "{}" });
}

export function installSetupDevin(token: string) {
  return setupRequest<SetupStatus>(token, "/install/devin", { method: "POST", body: "{}" });
}

export function connectSetupTailscale(token: string) {
  return setupRequest<SetupTailscaleConnection>(token, "/tailscale/connect", {
    method: "POST",
    body: "{}"
  });
}

export function selectSetupWorkspace(token: string) {
  return setupRequest<{ path: string | null; detail: string }>(token, "/workspace/select", {
    method: "POST",
    body: "{}"
  });
}

export function launchSetupWorkspace(token: string, body: SetupLaunchRequest) {
  return setupRequest<SetupLaunchResult>(token, "/launch", {
    method: "POST",
    body: JSON.stringify(body)
  });
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...init,
    headers: init?.body
      ? { "Content-Type": "application/json", ...init.headers }
      : init?.headers,
    credentials: "same-origin"
  });
  if (!response.ok) {
    let body: ApiErrorBody = {};
    try {
      body = await response.json() as ApiErrorBody;
    } catch {
      // The HTTP status remains useful when a proxy produced a non-JSON error.
    }
    throw new LocalApiError(
      body.error?.message ?? `Leave host returned ${response.status}`,
      response.status,
      body.error?.detail
    );
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}

export function fetchLocalStatus(signal?: AbortSignal) {
  return request<LocalStatus>("/api/v1/local/status", { signal });
}

export function listWorkspaces(signal?: AbortSignal) {
  return request<WorkspaceRecord[]>("/api/v1/local/workspaces", { signal });
}

export function listSessions(signal?: AbortSignal) {
  return request<SessionRecord[]>("/api/v1/local/sessions", { signal });
}

export function getSession(sessionId: string, signal?: AbortSignal) {
  return request<SessionRecord>(`/api/v1/local/sessions/${encodeURIComponent(sessionId)}`, { signal });
}

export function createSession(title = "") {
  return request<SessionRecord>("/api/v1/local/sessions", {
    method: "POST",
    body: JSON.stringify({ title })
  });
}

export function resumeSession(sessionId: string) {
  return request<void>(`/api/v1/local/sessions/${encodeURIComponent(sessionId)}/resume`, {
    method: "POST",
    body: "{}"
  });
}

export function sendPrompt(sessionId: string, text: string, commandId: string) {
  return request<PromptAccepted>(
    `/api/v1/local/sessions/${encodeURIComponent(sessionId)}/prompts`,
    {
      method: "POST",
      body: JSON.stringify({ commandId, text })
    }
  );
}

export function cancelSession(sessionId: string) {
  return request<void>(`/api/v1/local/sessions/${encodeURIComponent(sessionId)}/cancel`, {
    method: "POST",
    body: "{}"
  });
}

export function decidePermission(requestId: string, optionId: string | null) {
  return request<void>(`/api/v1/local/permissions/${encodeURIComponent(requestId)}`, {
    method: "POST",
    body: JSON.stringify({ optionId })
  });
}

export function getSessionEvents(sessionId: string, after = 0, signal?: AbortSignal) {
  const query = new URLSearchParams({ after: String(after), limit: "1000" });
  return request<EventPage>(
    `/api/v1/local/sessions/${encodeURIComponent(sessionId)}/events?${query}`,
    { signal }
  );
}

export function getWorkspaceEvents(after = 0, signal?: AbortSignal) {
  const query = new URLSearchParams({ after: String(after), limit: "200" });
  return request<EventPage>(`/api/v1/local/events?${query}`, { signal });
}

export function listFiles(path = "", signal?: AbortSignal) {
  const query = new URLSearchParams({ path });
  return request<DirectoryEntry[]>(`/api/v1/local/files?${query}`, { signal });
}

export function readFile(path: string, signal?: AbortSignal) {
  const query = new URLSearchParams({ path });
  return request<FileSnapshot>(`/api/v1/local/file?${query}`, { signal });
}

export function writeFile(path: string, baseHash: string, content: string) {
  return request<FileSnapshot>("/api/v1/local/file", {
    method: "PUT",
    body: JSON.stringify({ path, baseHash, content })
  });
}

export function getGitStatus(signal?: AbortSignal) {
  return request<GitStatus>("/api/v1/local/git/status", { signal });
}

export function getGitDiff(path?: string, staged = false, signal?: AbortSignal) {
  const query = new URLSearchParams({ staged: String(staged) });
  if (path) query.set("path", path);
  return request<{ diff: string }>(`/api/v1/local/git/diff?${query}`, { signal });
}

export function stageGitPaths(paths: string[]) {
  return request<void>("/api/v1/local/git/stage", { method: "POST", body: JSON.stringify({ paths }) });
}

export function unstageGitPaths(paths: string[]) {
  return request<void>("/api/v1/local/git/unstage", { method: "POST", body: JSON.stringify({ paths }) });
}

export function commitGit(message: string) {
  return request<{ output: string }>("/api/v1/local/git/commit", { method: "POST", body: JSON.stringify({ message }) });
}

export function pushGit() {
  return request<{ output: string }>("/api/v1/local/git/push", { method: "POST", body: "{}" });
}

export function listGitBranches(signal?: AbortSignal) {
  return request<GitBranch[]>("/api/v1/local/git/branches", { signal });
}

export function switchGitBranch(name: string, create = false) {
  return request<void>("/api/v1/local/git/branches", { method: "POST", body: JSON.stringify({ name, create }) });
}

export function getCustomization(category: string, name?: string, signal?: AbortSignal) {
  const query = new URLSearchParams({ category });
  if (name) query.set("name", name);
  return request<{ output: string }>(`/api/v1/local/customization?${query}`, { signal });
}

export interface CustomizationMutation {
  kind: "plugin" | "mcp";
  action: string;
  name: string;
  scope?: string;
  url?: string;
  transport?: string;
  command?: string;
  arguments?: string[];
  confirmation: string;
}

export function mutateCustomization(mutation: CustomizationMutation) {
  return request<{ output: string }>("/api/v1/local/customization", {
    method: "POST",
    body: JSON.stringify(mutation)
  });
}

export function createTerminal(rows: number, cols: number) {
  return request<TerminalView>("/api/v1/local/terminals", {
    method: "POST",
    body: JSON.stringify({ rows, cols })
  });
}

export function createPreview(url: string, width: number, height: number) {
  return request<PreviewView>("/api/v1/local/previews", {
    method: "POST",
    body: JSON.stringify({ url, width, height })
  });
}

export type SocketState = "connecting" | "open" | "closed";

export function connectLocalEvents(options: {
  onEvent: (event: LocalEvent) => void;
  onReplayRequired: () => void;
  onStateChange?: (state: SocketState) => void;
}) {
  let stopped = false;
  let socket: WebSocket | undefined;
  let retryTimer: number | undefined;
  let retryCount = 0;

  const connect = () => {
    if (stopped) return;
    options.onStateChange?.("connecting");
    const protocol = location.protocol === "https:" ? "wss:" : "ws:";
    socket = new WebSocket(`${protocol}//${location.host}/api/v1/local/ws`);
    socket.addEventListener("open", () => {
      retryCount = 0;
      options.onStateChange?.("open");
    });
    socket.addEventListener("message", (message) => {
      try {
        const data = JSON.parse(String(message.data)) as
          | { type: "event"; event: LocalEvent }
          | { type: "replay_required" };
        if (data.type === "event") options.onEvent(data.event);
        if (data.type === "replay_required") options.onReplayRequired();
      } catch {
        options.onReplayRequired();
      }
    });
    socket.addEventListener("close", () => {
      options.onStateChange?.("closed");
      if (stopped) return;
      const delay = Math.min(10_000, 500 * 2 ** Math.min(retryCount, 5));
      retryCount += 1;
      retryTimer = window.setTimeout(connect, delay);
    });
  };

  connect();
  return () => {
    stopped = true;
    if (retryTimer !== undefined) window.clearTimeout(retryTimer);
    socket?.close();
  };
}

export function appendLiveEvent(page: EventPage | undefined, event: LocalEvent): EventPage {
  if (!page) return { events: [event], nextCursor: event.sequence };
  if (page.events.some((existing) => existing.eventId === event.eventId)) return page;
  return {
    events: [...page.events, event].sort((left, right) => left.sequence - right.sequence),
    nextCursor: Math.max(page.nextCursor, event.sequence)
  };
}
