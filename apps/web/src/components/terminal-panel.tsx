import { useEffect, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Copy, TerminalWindow, WarningCircle } from "@phosphor-icons/react";
import { createTerminal } from "../lib/api";

export function TerminalPanel({ enabled }: { enabled: boolean }) {
  const container = useRef<HTMLDivElement>(null);
  const socketRef = useRef<WebSocket | undefined>(undefined);
  const [connection, setConnection] = useState<"idle" | "connecting" | "open" | "closed">("idle");
  const create = useMutation({ mutationFn: () => createTerminal(30, 100) });

  useEffect(() => {
    if (!create.data || !container.current) return;
    const terminal = new Terminal({
      cursorBlink: true,
      cursorStyle: "bar",
      fontFamily: '"JetBrains Mono", monospace',
      fontSize: 13,
      lineHeight: 1.25,
      scrollback: 0,
      theme: {
        background: "#111820",
        foreground: "#d6dde5",
        cursor: "#87a5bc",
        selectionBackground: "#31465a"
      }
    });
    terminal.open(container.current);
    terminal.focus();
    const protocol = location.protocol === "https:" ? "wss:" : "ws:";
    const socket = new WebSocket(`${protocol}//${location.host}/api/v1/local/terminals/${create.data.terminalId}/ws`);
    socket.binaryType = "arraybuffer";
    socketRef.current = socket;
    setConnection("connecting");
    socket.addEventListener("open", () => setConnection("open"));
    socket.addEventListener("close", () => setConnection("closed"));
    socket.addEventListener("message", (event) => {
      if (event.data instanceof ArrayBuffer) terminal.write(new Uint8Array(event.data));
      if (event.data instanceof Blob) void event.data.arrayBuffer().then((buffer) => terminal.write(new Uint8Array(buffer)));
    });
    const input = terminal.onData((data) => {
      if (socket.readyState === WebSocket.OPEN) socket.send(new TextEncoder().encode(data));
    });
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const cols = Math.max(20, Math.floor(entry.contentRect.width / 8.2));
      const rows = Math.max(5, Math.floor(entry.contentRect.height / 17));
      terminal.resize(cols, rows);
      if (socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type: "resize", cols, rows }));
    });
    observer.observe(container.current);
    return () => {
      observer.disconnect();
      input.dispose();
      socket.close();
      terminal.dispose();
      socketRef.current = undefined;
    };
  }, [create.data]);

  function send(data: string) {
    if (socketRef.current?.readyState === WebSocket.OPEN) socketRef.current.send(new TextEncoder().encode(data));
  }

  if (!enabled) {
    return <CapabilityGate icon={TerminalWindow} title="Terminal grant required">Restart the host with <code>--grant-terminal</code>. The grant applies only to this Leave run and is never implied by owner access.</CapabilityGate>;
  }
  if (!create.data) {
    return (
      <div className="tool-empty full-height">
        <TerminalWindow aria-hidden="true" size={28} />
        <strong>Open a workspace terminal</strong>
        <span>This starts a real PTY as your logged-in user. Leave does not cache its scrollback.</span>
        <button className="button" type="button" disabled={create.isPending} onClick={() => create.mutate()}>{create.isPending ? "Opening…" : "Open terminal"}</button>
        {create.error && <div className="tool-error" role="alert"><WarningCircle aria-hidden="true" size={16} />{create.error.message}</div>}
      </div>
    );
  }
  return (
    <div className="terminal-workbench">
      <header><div><TerminalWindow aria-hidden="true" size={17} /><strong>{create.data.shell}</strong></div><span className={`connection-text ${connection}`}>{connection}</span></header>
      <div className="terminal-viewport" ref={container} />
      <div className="terminal-toolbar" aria-label="Terminal keys">
        <button type="button" onClick={() => send("\u0003")}>Ctrl C</button>
        <button type="button" onClick={() => send("\t")}>Tab</button>
        <button type="button" aria-label="Arrow left" onClick={() => send("\u001b[D")}><ArrowLeft aria-hidden="true" size={17} /></button>
        <button type="button" aria-label="Arrow up" onClick={() => send("\u001b[A")}><ArrowUp aria-hidden="true" size={17} /></button>
        <button type="button" aria-label="Arrow down" onClick={() => send("\u001b[B")}><ArrowDown aria-hidden="true" size={17} /></button>
        <button type="button" aria-label="Arrow right" onClick={() => send("\u001b[C")}><ArrowRight aria-hidden="true" size={17} /></button>
        <button type="button" onClick={() => void navigator.clipboard.readText().then(send).catch(() => undefined)}><Copy aria-hidden="true" size={16} />Paste</button>
      </div>
    </div>
  );
}

function CapabilityGate({ icon: Icon, title, children }: { icon: typeof TerminalWindow; title: string; children: React.ReactNode }) {
  return <div className="feature-gate"><Icon aria-hidden="true" size={27} /><strong>{title}</strong><p>{children}</p></div>;
}
