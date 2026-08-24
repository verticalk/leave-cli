import type { ConnectionState } from "../types";

interface StatusPillProps {
  state: ConnectionState | "working" | "waiting" | "idle";
  label?: string;
}

export function StatusPill({ state, label }: StatusPillProps) {
  return (
    <span className={`status-pill status-${state}`}>
      <span className="status-indicator" aria-hidden="true" />
      {label ?? state}
    </span>
  );
}
