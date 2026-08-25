import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { Link, useParams } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeft,
  BracketsCurly,
  Check,
  ChatCircleDots,
  Eye,
  Folders,
  Key,
  PaperPlaneTilt,
  Play,
  StopCircle,
  TerminalWindow,
  UserCircle,
  WarningCircle,
  X
} from "@phosphor-icons/react";
import {
  appendLiveEvent,
  cancelSession,
  connectLocalEvents,
  decidePermission,
  fetchLocalStatus,
  getSession,
  getSessionEvents,
  resumeSession,
  sendPrompt,
  type SocketState
} from "../lib/api";
import { StatusPill } from "../components/status-pill";
import type { EventPage, LocalEvent, SessionTab } from "../types";

const FilesPanel = lazy(() => import("../components/files-panel").then((module) => ({ default: module.FilesPanel })));
const TerminalPanel = lazy(() => import("../components/terminal-panel").then((module) => ({ default: module.TerminalPanel })));
const PreviewPanel = lazy(() => import("../components/preview-panel").then((module) => ({ default: module.PreviewPanel })));

const tabs: Array<{ id: SessionTab; label: string; icon: typeof ChatCircleDots }> = [
  { id: "chat", label: "Chat", icon: ChatCircleDots },
  { id: "files", label: "Files", icon: Folders },
  { id: "terminal", label: "Terminal", icon: TerminalWindow },
  { id: "preview", label: "Preview", icon: Eye }
];

interface TimelineMessage {
  id: string;
  kind: "user" | "agent" | "thought" | "tool" | "approval" | "system";
  author: string;
  body: string;
  meta?: string;
  requestId?: string;
  options?: PermissionOption[];
  resolution?: string;
}

interface PermissionOption {
  optionId: string;
  name: string;
  kind: string;
}

interface PlanEntry {
  content: string;
  status: string;
}

export function SessionScreen() {
  const { sessionId = "" } = useParams({ strict: false });
  const queryClient = useQueryClient();
  const [activeTab, setActiveTab] = useState<SessionTab>("chat");
  const [socketState, setSocketState] = useState<SocketState>("connecting");
  const [draft, setDraft] = useState(() => localStorage.getItem(`leave-draft:${sessionId}`) ?? "");
  const timelineEnd = useRef<HTMLDivElement>(null);

  const host = useQuery({
    queryKey: ["local-status"],
    queryFn: ({ signal }) => fetchLocalStatus(signal),
    refetchInterval: 5_000
  });
  const session = useQuery({
    queryKey: ["session", sessionId],
    queryFn: ({ signal }) => getSession(sessionId, signal),
    enabled: Boolean(sessionId)
  });
  const events = useQuery({
    queryKey: ["session-events", sessionId],
    queryFn: ({ signal }) => getSessionEvents(sessionId, 0, signal),
    enabled: Boolean(sessionId)
  });

  useEffect(() => {
    localStorage.setItem(`leave-draft:${sessionId}`, draft);
  }, [draft, sessionId]);

  useEffect(() => connectLocalEvents({
    onEvent: (event) => {
      void queryClient.invalidateQueries({ queryKey: ["workspace-events"] });
      void queryClient.invalidateQueries({ queryKey: ["sessions"] });
      if (event.sessionId !== sessionId) return;
      queryClient.setQueryData<EventPage>(
        ["session-events", sessionId],
        (page) => appendLiveEvent(page, event)
      );
      if (["prompt_completed", "prompt_failed", "session_cancelled", "session_resumed"].includes(event.kind)) {
        void queryClient.invalidateQueries({ queryKey: ["session", sessionId] });
      }
    },
    onReplayRequired: () => {
      void queryClient.invalidateQueries({ queryKey: ["session-events", sessionId] });
    },
    onStateChange: setSocketState
  }), [queryClient, sessionId]);

  const messages = useMemo(() => buildTimeline(events.data?.events ?? []), [events.data?.events]);
  const plan = useMemo(() => extractLatestPlan(events.data?.events ?? []), [events.data?.events]);

  useEffect(() => {
    timelineEnd.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [messages.length]);

  const send = useMutation({
    mutationFn: ({ text, commandId }: { text: string; commandId: string }) => sendPrompt(sessionId, text, commandId),
    retry: false,
    onSuccess: async () => {
      setDraft("");
      localStorage.removeItem(`leave-draft:${sessionId}`);
      await queryClient.invalidateQueries({ queryKey: ["session-events", sessionId] });
      await queryClient.invalidateQueries({ queryKey: ["session", sessionId] });
    }
  });
  const cancel = useMutation({
    mutationFn: () => cancelSession(sessionId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["session", sessionId] });
      await queryClient.invalidateQueries({ queryKey: ["session-events", sessionId] });
    }
  });
  const resume = useMutation({
    mutationFn: () => resumeSession(sessionId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["session", sessionId] });
      await queryClient.invalidateQueries({ queryKey: ["session-events", sessionId] });
    }
  });

  const agentReady = host.data?.agent.state === "ready";
  const sessionWorking = session.data?.state === "working";
  const sessionNeedsResume = session.data?.state === "offline";
  const canSend = agentReady && !sessionWorking && !sessionNeedsResume && draft.trim().length > 0 && !send.isPending;

  function submitPrompt(event: React.FormEvent) {
    event.preventDefault();
    const text = draft.trim();
    if (!text || !canSend) return;
    send.mutate({ text, commandId: crypto.randomUUID() });
  }

  if (session.isPending) return <div className="panel-loading" role="status">Loading Devin session…</div>;
  if (session.error) return <SessionError message={session.error.message} />;
  if (!session.data) return <SessionError message="Session not found." />;

  return (
    <div className="session-page">
      <header className="session-header">
        <div className="session-title-group">
          <Link to="/sessions" className="icon-button back-button" aria-label="Back to sessions"><ArrowLeft aria-hidden="true" size={18} weight="regular" /></Link>
          <div><h1>{session.data.title}</h1><p><span className="mono">{host.data?.workspace.name ?? "workspace"}</span><span className="dot-separator">·</span>Devin ACP</p></div>
        </div>
        <div className="session-header-actions">
          <StatusPill state={sessionWorking ? "working" : sessionNeedsResume ? "offline" : "idle"} />
          {sessionNeedsResume ? (
            <button className="button compact" type="button" disabled={!agentReady || resume.isPending} onClick={() => resume.mutate()}>
              <Play aria-hidden="true" size={16} /> {resume.isPending ? "Resuming…" : "Resume"}
            </button>
          ) : (
            <button className="icon-button" type="button" aria-label="Stop current Devin turn" disabled={!sessionWorking || cancel.isPending} onClick={() => cancel.mutate()}><StopCircle aria-hidden="true" size={18} weight="regular" /></button>
          )}
        </div>
      </header>

      <div className="session-tabs" role="tablist" aria-label="Session tools">
        {tabs.map(({ id, label, icon: Icon }) => (
          <button key={id} type="button" role="tab" aria-selected={activeTab === id} className={activeTab === id ? "active" : ""} onClick={() => setActiveTab(id)}>
            <Icon aria-hidden="true" size={17} weight="regular" /> {label}
          </button>
        ))}
      </div>

      <div className="session-body">
        <section className="session-primary">
          {activeTab === "chat" ? (
            <>
              <div className="timeline" aria-live="polite">
                <div className="sync-marker"><span>{socketState === "open" ? "Live ACP stream" : "Reconnecting"}</span></div>
                {events.isPending && <div className="query-state compact" role="status">Loading session history…</div>}
                {events.error && <div className="query-state error compact" role="alert"><WarningCircle size={18} /><span>{events.error.message}</span></div>}
                {!events.isPending && messages.length === 0 && (
                  <div className="session-empty"><ChatCircleDots aria-hidden="true" size={24} /><strong>Ready for your first prompt</strong><span>Messages and tool updates will stream here from Devin.</span></div>
                )}
                {messages.map((message) => <TimelineEntry message={message} key={message.id} />)}
                <div ref={timelineEnd} />
              </div>
              {(send.error || cancel.error || resume.error) && (
                <div className="session-action-error" role="alert">
                  <WarningCircle aria-hidden="true" size={17} />
                  <span>{send.error?.message ?? cancel.error?.message ?? resume.error?.message}</span>
                  {send.error && <small>Leave did not retry. Check the activity log before sending the prompt again.</small>}
                </div>
              )}
              <form className="composer" onSubmit={submitPrompt}>
                <label className="sr-only" htmlFor="session-prompt">Message Devin</label>
                <textarea
                  id="session-prompt"
                  value={draft}
                  onChange={(event) => setDraft(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                      event.preventDefault();
                      event.currentTarget.form?.requestSubmit();
                    }
                  }}
                  placeholder={sessionNeedsResume ? "Resume this session before sending a prompt" : "Message Devin"}
                  rows={2}
                  disabled={!agentReady || sessionNeedsResume}
                />
                <div className="composer-footer">
                  <span>{sessionWorking ? "Devin is working. Stop the turn to interrupt it." : "Ctrl or ⌘ + Enter to send · draft stays on this device"}</span>
                  <button className="send-button" type="submit" disabled={!canSend} aria-label="Send message to Devin"><PaperPlaneTilt aria-hidden="true" size={17} weight="regular" /></button>
                </div>
              </form>
            </>
          ) : activeTab === "files" ? (
            <Suspense fallback={<div className="panel-loading" role="status">Loading files…</div>}><FilesPanel /></Suspense>
          ) : activeTab === "terminal" ? (
            <Suspense fallback={<div className="panel-loading" role="status">Loading terminal…</div>}><TerminalPanel enabled={host.data?.capabilities.terminal ?? false} /></Suspense>
          ) : (
            <Suspense fallback={<div className="panel-loading" role="status">Loading preview…</div>}><PreviewPanel enabled={host.data?.capabilities.preview ?? false} /></Suspense>
          )}
        </section>

        <aside className="context-rail" aria-label="Session context" tabIndex={0}>
          <section className="context-section">
            <header><h2>Plan</h2><span>{plan.length ? `${plan.filter((item) => item.status === "completed").length} of ${plan.length}` : "No plan"}</span></header>
            {plan.length > 0 ? (
              <ol className="plan-list">
                {plan.map((entry, index) => (
                  <li className={entry.status === "completed" ? "complete" : entry.status === "in_progress" ? "current" : ""} key={`${entry.content}-${index}`}>
                    <span>{entry.status === "completed" ? <Check aria-hidden="true" size={13} weight="bold" /> : entry.status === "in_progress" ? <Play aria-hidden="true" size={12} weight="fill" /> : index + 1}</span>
                    <p>{entry.content}</p>
                  </li>
                ))}
              </ol>
            ) : <p className="context-empty">Devin has not published a plan.</p>}
          </section>
          <section className="context-section session-details">
            <header><h2>Connection</h2></header>
            <dl>
              <div><dt>Agent</dt><dd>{host.data?.agent.state ?? "unknown"}</dd></div>
              <div><dt>Stream</dt><dd>{socketState}</dd></div>
              <div><dt>Mode</dt><dd>{host.data?.mode ?? "unknown"}</dd></div>
              <div><dt>Cursor</dt><dd className="mono">seq {events.data?.nextCursor ?? 0}</dd></div>
            </dl>
          </section>
          <section className="context-section session-details">
            <header><h2>Session</h2></header>
            <dl>
              <div><dt>State</dt><dd>{session.data.state}</dd></div>
              <div><dt>ID</dt><dd className="mono">{session.data.session_id.slice(0, 12)}</dd></div>
              <div><dt>Writer</dt><dd>This device</dd></div>
            </dl>
          </section>
        </aside>
      </div>
    </div>
  );
}

function TimelineEntry({ message }: { message: TimelineMessage }) {
  const Icon = message.kind === "user"
    ? UserCircle
    : message.kind === "agent" || message.kind === "thought"
      ? BracketsCurly
      : message.kind === "approval"
        ? Key
        : message.kind === "system"
          ? WarningCircle
          : TerminalWindow;
  return (
    <article className={`timeline-item item-${message.kind}`}>
      <span className="timeline-avatar" aria-hidden="true"><Icon size={17} weight="regular" /></span>
      <div className="timeline-content">
        <header><strong>{message.author}</strong>{message.meta && <span>{message.meta}</span>}</header>
        <p className="message-body">{message.body}</p>
        {message.kind === "tool" && <div className="tool-result"><Check aria-hidden="true" size={15} weight="bold" />{message.meta ?? "Tool update"}</div>}
        {message.kind === "approval" && message.requestId && (
          <ApprovalActions requestId={message.requestId} options={message.options ?? []} resolution={message.resolution} />
        )}
      </div>
    </article>
  );
}

function ApprovalActions({ requestId, options, resolution }: { requestId: string; options: PermissionOption[]; resolution?: string }) {
  const queryClient = useQueryClient();
  const decision = useMutation({
    mutationFn: (optionId: string) => decidePermission(requestId, optionId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["session-events"] })
  });

  if (resolution) {
    return <div className="approval-result"><Check aria-hidden="true" size={16} weight="bold" />{resolution}</div>;
  }
  return (
    <div className="approval-box">
      <div className="risk-line"><Key aria-hidden="true" size={15} weight="regular" /><strong>Devin needs permission</strong><span>Expires in 5m</span></div>
      <div className="approval-actions">
        {options.map((option) => (
          <button
            className={`button ${option.kind.startsWith("reject") ? "secondary" : ""}`}
            type="button"
            disabled={decision.isPending}
            onClick={() => decision.mutate(option.optionId)}
            key={option.optionId}
          >
            {option.kind.startsWith("reject") ? <X aria-hidden="true" size={15} weight="bold" /> : <Check aria-hidden="true" size={15} weight="bold" />}
            {option.name}
          </button>
        ))}
      </div>
      {decision.error && <p className="form-error" role="alert">{decision.error.message}</p>}
    </div>
  );
}

function SessionError({ message }: { message: string }) {
  return <div className="session-error-page" role="alert"><WarningCircle aria-hidden="true" size={28} /><h1>Session unavailable</h1><p>{message}</p><Link className="button secondary" to="/sessions">Back to sessions</Link></div>;
}

export function buildTimeline(events: LocalEvent[]): TimelineMessage[] {
  const messages: TimelineMessage[] = [];
  const resolutions = new Map<string, string>();
  for (const event of events) {
    if (event.kind === "permission_resolved") {
      resolutions.set(String(event.payload.requestId ?? ""), String(event.payload.status ?? "resolved"));
    }
  }

  for (const event of events) {
    if (event.kind === "user_prompt") {
      messages.push({ id: event.eventId, kind: "user", author: "You", body: String(event.payload.text ?? ""), meta: formatTime(event.occurredAtUnixMs) });
      continue;
    }
    if (event.kind === "permission_requested") {
      const request = record(event.payload.request);
      const toolCall = record(request.toolCall);
      const requestId = String(event.payload.requestId ?? event.eventId);
      messages.push({
        id: event.eventId,
        kind: "approval",
        author: "Permission",
        body: String(toolCall.title ?? "Devin requested a protected operation."),
        meta: formatTime(event.occurredAtUnixMs),
        requestId,
        options: array(request.options).map((value) => {
          const option = record(value);
          return { optionId: String(option.optionId ?? ""), name: String(option.name ?? "Choose"), kind: String(option.kind ?? "allow_once") };
        }).filter((option) => option.optionId),
        resolution: resolutions.get(requestId)
      });
      continue;
    }
    if (event.kind === "session_update") {
      appendSessionUpdate(messages, event);
      continue;
    }
    if (event.kind === "prompt_failed") {
      messages.push({ id: event.eventId, kind: "system", author: "Leave", body: String(event.payload.message ?? "The Devin turn failed."), meta: formatTime(event.occurredAtUnixMs) });
      continue;
    }
    if (event.kind === "session_cancelled") {
      messages.push({ id: event.eventId, kind: "system", author: "Leave", body: "The current Devin turn was cancelled.", meta: formatTime(event.occurredAtUnixMs) });
    }
  }
  return messages;
}

function authorFor(kind: TimelineMessage["kind"]) {
  return kind === "user" ? "You" : kind === "thought" ? "Devin thought" : "Devin";
}

function appendSessionUpdate(messages: TimelineMessage[], event: LocalEvent) {
  const update = record(event.payload.update);
  const kind = String(update.sessionUpdate ?? "");
  if (["agent_message_chunk", "agent_thought_chunk", "user_message_chunk"].includes(kind)) {
    const content = record(update.content);
    const text = String(content.text ?? "");
    if (!text) return;
    const messageKind: TimelineMessage["kind"] = kind === "agent_message_chunk" ? "agent" : kind === "user_message_chunk" ? "user" : "thought";
    const messageId = String(update.messageId ?? "");
    if (messageId) {
      const id = `chunk:${messageId}:${messageKind}`;
      const existing = messages.find((message) => message.id === id);
      if (existing) {
        existing.body += text;
      } else {
        messages.push({ id, kind: messageKind, author: authorFor(messageKind), body: text, meta: formatTime(event.occurredAtUnixMs) });
      }
      return;
    }
    // Devin's ACP stream may omit message IDs. Then one streamed reply must
    // still stay one bubble: keep appending to the bubble currently growing,
    // until a different kind of event interrupts it.
    const previous = messages[messages.length - 1];
    if (previous && previous.kind === messageKind && previous.id.startsWith("chunk:")) {
      previous.body += text;
      return;
    }
    messages.push({ id: `chunk:auto:${event.eventId}`, kind: messageKind, author: authorFor(messageKind), body: text, meta: formatTime(event.occurredAtUnixMs) });
    return;
  }
  if (kind === "tool_call" || kind === "tool_call_update") {
    messages.push({
      id: event.eventId,
      kind: "tool",
      author: "Devin tool",
      body: String(update.title ?? `Tool ${String(update.toolCallId ?? "update")}`),
      meta: String(update.status ?? (kind === "tool_call" ? "started" : "updated"))
    });
  }
}

function extractLatestPlan(events: LocalEvent[]): PlanEntry[] {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    if (event.kind !== "session_update") continue;
    const update = record(event.payload.update);
    if (update.sessionUpdate !== "plan") continue;
    return array(update.entries).map((value) => {
      const entry = record(value);
      return { content: String(entry.content ?? "Plan item"), status: String(entry.status ?? "pending") };
    });
  }
  return [];
}

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function array(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function formatTime(timestamp: number) {
  return new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(timestamp);
}
