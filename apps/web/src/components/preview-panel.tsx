import { useEffect, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { ArrowClockwise, ArrowRight, CursorClick, Eye, Keyboard, WarningCircle } from "@phosphor-icons/react";
import { createPreview } from "../lib/api";

const VIEWPORT_WIDTH = 390;
const VIEWPORT_HEIGHT = 844;

export function PreviewPanel({ enabled }: { enabled: boolean }) {
  const [url, setUrl] = useState("http://127.0.0.1:5173");
  const [frame, setFrame] = useState("");
  const [text, setText] = useState("");
  const [connection, setConnection] = useState("idle");
  const socket = useRef<WebSocket | undefined>(undefined);
  const create = useMutation({ mutationFn: () => createPreview(url, VIEWPORT_WIDTH, VIEWPORT_HEIGHT) });

  useEffect(() => {
    if (!create.data) return;
    const protocol = location.protocol === "https:" ? "wss:" : "ws:";
    const next = new WebSocket(`${protocol}//${location.host}/api/v1/local/previews/${create.data.previewId}/ws`);
    socket.current = next;
    setConnection("connecting");
    next.addEventListener("open", () => setConnection("open"));
    next.addEventListener("close", () => setConnection("closed"));
    next.addEventListener("message", (event) => {
      try {
        const message = JSON.parse(String(event.data)) as { type: string; mediaType: string; data: string };
        if (message.type === "frame") setFrame(`data:${message.mediaType};base64,${message.data}`);
      } catch {
        // A malformed frame is dropped; the next capture replaces it.
      }
    });
    return () => {
      next.close();
      socket.current = undefined;
    };
  }, [create.data]);

  function control(value: object) {
    if (socket.current?.readyState === WebSocket.OPEN) socket.current.send(JSON.stringify(value));
  }

  function navigate(event: React.FormEvent) {
    event.preventDefault();
    if (!create.data) create.mutate();
    else control({ type: "navigate", url });
  }

  if (!enabled) {
    return <div className="feature-gate"><Eye aria-hidden="true" size={27} /><strong>Preview grant required</strong><p>Install Chromium, then restart Leave with <code>--grant-preview</code>. Leave uses an ephemeral profile and only permits approved loopback origins.</p></div>;
  }

  return (
    <div className="preview-workbench">
      <form className="preview-address" onSubmit={navigate}>
        <button className="icon-button" type="button" aria-label="Refresh preview" disabled={!create.data} onClick={() => control({ type: "navigate", url })}><ArrowClockwise aria-hidden="true" size={17} /></button>
        <label className="sr-only" htmlFor="preview-url">Loopback preview URL</label>
        <input id="preview-url" type="url" inputMode="url" value={url} onChange={(event) => setUrl(event.target.value)} required pattern="http://(localhost|127\.0\.0\.1|\[::1\]).*" />
        <button className="button compact" type="submit" disabled={create.isPending}><ArrowRight aria-hidden="true" size={16} />{create.data ? "Go" : create.isPending ? "Starting…" : "Open"}</button>
      </form>
      {create.error && <div className="tool-error" role="alert"><WarningCircle aria-hidden="true" size={16} />{create.error.message}</div>}
      <div className="preview-stage">
        {!create.data && <div className="tool-empty"><Eye aria-hidden="true" size={27} /><strong>Open a local development server</strong><span>Only HTTP URLs on localhost or a loopback IP are accepted.</span></div>}
        {create.data && !frame && <div className="tool-state" role="status">Starting Chromium and waiting for the first frame…</div>}
        {frame && (
          <button
            className="preview-frame"
            type="button"
            aria-label="Interact with browser preview"
            onClick={(event) => {
              const rect = event.currentTarget.getBoundingClientRect();
              control({
                type: "click",
                x: (event.clientX - rect.left) / rect.width * VIEWPORT_WIDTH,
                y: (event.clientY - rect.top) / rect.height * VIEWPORT_HEIGHT
              });
            }}
          >
            <img src={frame} alt="Live browser preview" draggable={false} />
          </button>
        )}
      </div>
      {create.data && (
        <div className="preview-controls">
          <span><CursorClick aria-hidden="true" size={16} />Tap the frame to click</span>
          <form onSubmit={(event) => { event.preventDefault(); if (text) { control({ type: "text", text }); setText(""); } }}>
            <Keyboard aria-hidden="true" size={16} />
            <label className="sr-only" htmlFor="preview-text">Type into focused browser element</label>
            <input id="preview-text" value={text} onChange={(event) => setText(event.target.value)} placeholder="Type into focused field" />
            <button className="button secondary compact" type="submit" disabled={!text}>Send</button>
          </form>
          <span className={`connection-text ${connection}`}>{connection}</span>
        </div>
      )}
    </div>
  );
}
