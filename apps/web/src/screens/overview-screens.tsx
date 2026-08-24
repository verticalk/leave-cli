import { Link, useNavigate } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowRight,
  ChatCircleDots,
  Clock,
  ClockCounterClockwise,
  Desktop,
  FileCode,
  FolderOpen,
  Key,
  Plus,
  WarningCircle
} from "@phosphor-icons/react";
import {
  createSession,
  fetchLocalStatus,
  getWorkspaceEvents,
  listSessions,
  listWorkspaces
} from "../lib/api";
import { StatusPill } from "../components/status-pill";
import type { LocalEvent, SessionRecord } from "../types";

export function HostsScreen() {
  const host = useQuery({
    queryKey: ["local-status"],
    queryFn: ({ signal }) => fetchLocalStatus(signal)
  });
  const sessions = useQuery({
    queryKey: ["sessions"],
    queryFn: ({ signal }) => listSessions(signal)
  });
  const agentReady = host.data?.agent.state === "ready";

  return (
    <div className="page page-narrow">
      <header className="page-header">
        <div>
          <p className="eyebrow">Host</p>
          <h1>Your machine</h1>
          <p className="page-description">The repository and Devin process stay on this computer.</p>
        </div>
        <Link className="button" to="/sessions">
          <ChatCircleDots aria-hidden="true" size={17} weight="regular" /> Open sessions
        </Link>
      </header>

      {host.isPending && <LoadingState label="Checking the local host…" />}
      {host.error && <ErrorState title="Host unavailable" message={host.error.message} />}
      {host.data && (
        <section className="host-grid" aria-label="Active host">
          <article className="host-card">
            <div className="host-card-top">
              <span className="device-icon" aria-hidden="true"><Desktop size={21} weight="regular" /></span>
              <StatusPill state={agentReady ? "online" : "offline"} label={agentReady ? "Devin ready" : host.data.agent.state} />
            </div>
            <h2>{host.data.host.name}</h2>
            <p>{host.data.host.platform} · {host.data.host.architecture} · Leave {host.data.version}</p>
            <div className="host-stats">
              <span>1 workspace</span>
              <span>{host.data.mode === "tailnet" ? "Private tailnet access" : "Loopback only"}</span>
            </div>
            <Link className="card-link" to="/sessions">
              Open sessions <ArrowRight aria-hidden="true" size={16} weight="regular" />
            </Link>
          </article>
        </section>
      )}

      <section className="section-block">
        <div className="section-heading">
          <div><p className="eyebrow">Continue</p><h2>Recent sessions</h2></div>
          <Link to="/sessions">View all</Link>
        </div>
        {sessions.isPending && <LoadingState label="Loading sessions…" compact />}
        {sessions.error && <ErrorState title="Sessions unavailable" message={sessions.error.message} compact />}
        {sessions.data && sessions.data.length === 0 && <EmptyState message="No Devin sessions yet. Create one from Sessions." />}
        {sessions.data && sessions.data.length > 0 && <SessionList sessions={sessions.data.slice(0, 4)} />}
      </section>
    </div>
  );
}

export function SessionsScreen() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const host = useQuery({
    queryKey: ["local-status"],
    queryFn: ({ signal }) => fetchLocalStatus(signal)
  });
  const sessions = useQuery({
    queryKey: ["sessions"],
    queryFn: ({ signal }) => listSessions(signal)
  });
  const create = useMutation({
    mutationFn: () => createSession(),
    onSuccess: async (session) => {
      await queryClient.invalidateQueries({ queryKey: ["sessions"] });
      await navigate({ to: "/sessions/$sessionId", params: { sessionId: session.session_id } });
    }
  });
  const agentReady = host.data?.agent.state === "ready";

  return (
    <div className="page page-narrow">
      <header className="page-header">
        <div><p className="eyebrow">Local history</p><h1>Sessions</h1><p className="page-description">Sessions created through the supported Devin ACP interface.</p></div>
        <button className="button" type="button" disabled={!agentReady || create.isPending} onClick={() => create.mutate()} title={agentReady ? undefined : "Connect Devin ACP before creating a session"}>
          <Plus aria-hidden="true" size={17} weight="regular" /> {create.isPending ? "Creating…" : "New session"}
        </button>
      </header>
      {create.error && <p className="form-error" role="alert">{create.error.message}</p>}
      {sessions.isPending && <LoadingState label="Loading sessions…" />}
      {sessions.error && <ErrorState title="Sessions unavailable" message={sessions.error.message} />}
      {sessions.data && sessions.data.length === 0 && <EmptyState message={agentReady ? "Create a session to start working with Devin." : "Start and authenticate Devin CLI, then Leave can create a session."} />}
      {sessions.data && sessions.data.length > 0 && <SessionList sessions={sessions.data} roomy />}
      <div className="inline-note"><ClockCounterClockwise aria-hidden="true" size={18} weight="regular" /><span>Leave stores its own event cursor and uses documented ACP session methods. Private Desktop databases remain untouched.</span></div>
    </div>
  );
}

export function WorkspacesScreen() {
  const workspaces = useQuery({
    queryKey: ["workspaces"],
    queryFn: ({ signal }) => listWorkspaces(signal)
  });
  const host = useQuery({
    queryKey: ["local-status"],
    queryFn: ({ signal }) => fetchLocalStatus(signal)
  });
  const online = host.data?.agent.state === "ready";

  return (
    <div className="page page-narrow">
      <header className="page-header">
        <div><p className="eyebrow">Approved roots</p><h1>Workspaces</h1><p className="page-description">Paths can only be registered from the local Leave CLI.</p></div>
      </header>
      {workspaces.isPending && <LoadingState label="Loading workspace…" />}
      {workspaces.error && <ErrorState title="Workspace unavailable" message={workspaces.error.message} />}
      <div className="workspace-grid">
        {workspaces.data?.map((workspace) => (
          <article className="workspace-card" key={workspace.id}>
            <div className="workspace-title"><span className="repo-mark" aria-hidden="true"><FolderOpen size={19} weight="regular" /></span><div><h2>{workspace.name}</h2><p className="mono workspace-path">{workspace.canonical_path}</p></div></div>
            <div className="workspace-meta"><span>Registered local root</span><StatusPill state={online ? "online" : "offline"} /></div>
            <div className="workspace-footer"><span>{workspace.expose_history ? "History enabled" : "Leave-created history only"}</span><span className="role-label">owner</span></div>
          </article>
        ))}
      </div>
    </div>
  );
}

export function ActivityScreen() {
  const activity = useQuery({
    queryKey: ["workspace-events"],
    queryFn: ({ signal }) => getWorkspaceEvents(0, signal),
    refetchInterval: 5_000
  });

  return (
    <div className="page page-narrow">
      <header className="page-header"><div><p className="eyebrow">Local audit</p><h1>Activity</h1><p className="page-description">Durable host events loaded newest first from this computer.</p></div></header>
      {activity.isPending && <LoadingState label="Loading activity…" />}
      {activity.error && <ErrorState title="Activity unavailable" message={activity.error.message} />}
      {activity.data?.events.length === 0 && <EmptyState message="Host and session events will appear here." />}
      {activity.data && activity.data.events.length > 0 && (
        <div className="activity-list">
          {[...activity.data.events].reverse().map((event) => <ActivityEntry event={event} key={event.eventId} />)}
        </div>
      )}
    </div>
  );
}

function SessionList({ sessions, roomy = false }: { sessions: SessionRecord[]; roomy?: boolean }) {
  return (
    <div className="list-card">
      {sessions.map((session) => (
        <Link className={`session-row ${roomy ? "roomy" : ""}`} to="/sessions/$sessionId" params={{ sessionId: session.session_id }} key={session.session_id}>
          <span className="session-icon"><ChatCircleDots aria-hidden="true" size={18} weight="regular" /></span>
          <span className="session-copy"><strong>{session.title}</strong><span>Devin ACP · updated {formatRelative(session.updated_at_unix_ms)}</span></span>
          <StatusPill state={session.state === "working" ? "working" : session.state === "offline" ? "offline" : "idle"} />
          {roomy ? <ArrowRight aria-hidden="true" size={17} weight="regular" /> : <span className="session-time">{formatRelative(session.updated_at_unix_ms)}</span>}
        </Link>
      ))}
    </div>
  );
}

function ActivityEntry({ event }: { event: LocalEvent }) {
  const presentation = activityPresentation(event);
  const Icon = presentation.icon;
  return (
    <article className="activity-row">
      <span className="activity-icon"><Icon aria-hidden="true" size={18} weight="regular" /></span>
      <div><h2>{presentation.title}</h2><p>{presentation.body}</p></div>
      <span className="activity-time"><Clock aria-hidden="true" size={13} weight="regular" />{formatRelative(event.occurredAtUnixMs)}</span>
    </article>
  );
}

function activityPresentation(event: LocalEvent) {
  switch (event.kind) {
    case "permission_requested": return { icon: Key, title: "Permission requested", body: `Session ${shortId(event.sessionId)}` };
    case "permission_resolved": return { icon: Key, title: "Permission resolved", body: String(event.payload.status ?? "resolved") };
    case "user_prompt": return { icon: ChatCircleDots, title: "Prompt accepted", body: excerpt(String(event.payload.text ?? "")) };
    case "session_created": return { icon: ChatCircleDots, title: "Session created", body: `Session ${shortId(event.sessionId)}` };
    case "prompt_completed": return { icon: ChatCircleDots, title: "Turn completed", body: `Session ${shortId(event.sessionId)}` };
    case "prompt_failed": return { icon: WarningCircle, title: "Turn failed", body: String(event.payload.message ?? "Devin returned an error") };
    case "host_error": return { icon: WarningCircle, title: "ACP unavailable", body: String(event.payload.message ?? "Host error") };
    default: return { icon: FileCode, title: event.kind.replaceAll("_", " "), body: `Sequence ${event.sequence}` };
  }
}

function LoadingState({ label, compact = false }: { label: string; compact?: boolean }) {
  return <div className={`query-state ${compact ? "compact" : ""}`} role="status"><span className="loading-dot" />{label}</div>;
}

function ErrorState({ title, message, compact = false }: { title: string; message: string; compact?: boolean }) {
  return <div className={`query-state error ${compact ? "compact" : ""}`} role="alert"><WarningCircle aria-hidden="true" size={19} /><div><strong>{title}</strong><span>{message}</span></div></div>;
}

function EmptyState({ message }: { message: string }) {
  return <div className="empty-state"><ChatCircleDots aria-hidden="true" size={22} /><p>{message}</p></div>;
}

function formatRelative(timestamp: number) {
  const seconds = Math.max(0, Math.floor((Date.now() - timestamp) / 1_000));
  if (seconds < 60) return "now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" }).format(timestamp);
}

function shortId(value: string | null) {
  return value ? value.slice(0, 8) : "host";
}

function excerpt(value: string) {
  return value.length > 90 ? `${value.slice(0, 87)}…` : value;
}
