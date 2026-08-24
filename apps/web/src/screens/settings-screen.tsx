import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Database, Fingerprint, Globe, Key, LockKey, Trash, WifiHigh } from "@phosphor-icons/react";
import { fetchLocalStatus } from "../lib/api";
import { clearEncryptedCache } from "../lib/offline-store";
import { CustomizationPanel } from "../components/customization-panel";

function Toggle({ checked, onChange, label, description, disabled = false }: {
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
  description: string;
  disabled?: boolean;
}) {
  return (
    <label className={`setting-row ${disabled ? "disabled" : ""}`}>
      <span><strong>{label}</strong><span>{description}</span></span>
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} disabled={disabled} />
      <span className="switch" aria-hidden="true" />
    </label>
  );
}

export function SettingsScreen() {
  const [cacheMessage, setCacheMessage] = useState("");
  const host = useQuery({
    queryKey: ["local-status"],
    queryFn: ({ signal }) => fetchLocalStatus(signal)
  });

  async function clearCache() {
    await clearEncryptedCache();
    setCacheMessage("Encrypted device cache cleared.");
  }

  return (
    <div className="page settings-page">
      <header className="page-header"><div><p className="eyebrow">Device policy</p><h1>Settings</h1><p className="page-description">Local preferences and owner-approved exposure controls.</p></div></header>

      <div className="settings-grid">
        <section className="settings-section">
          <header><span className="settings-icon"><LockKey aria-hidden="true" size={18} weight="regular" /></span><div><h2>Hosted relay gate</h2><p>Public internet routing stays off until every MLS release check passes.</p></div></header>
          <div className="gate-list">
            <div><span>OpenMLS provider graph</span><strong className="gate-blocked">Blocked</strong></div>
            <div><span>Native ↔ WASM vectors</span><strong className="gate-pending">Pending</strong></div>
            <div><span>External cryptography review</span><strong className="gate-pending">Pending</strong></div>
          </div>
          <div className="inline-note compact-note"><Fingerprint aria-hidden="true" size={18} weight="regular" /><span>This gate is separate from private Tailscale access on your own devices.</span></div>
        </section>

        <section className="settings-section">
          <header><span className="settings-icon"><WifiHigh aria-hidden="true" size={18} weight="regular" /></span><div><h2>Away access</h2><p>Tailnet-only HTTPS, restricted to the host owner's Tailscale identity.</p></div></header>
          <div className="gate-list">
            <div><span>Current mode</span><strong>{host.data?.mode ?? "unknown"}</strong></div>
            <div><span>Terminal grant</span><strong>{host.data?.capabilities.terminal ? "On" : "Off"}</strong></div>
            <div><span>Preview grant</span><strong>{host.data?.capabilities.preview ? "On" : "Off"}</strong></div>
          </div>
          {host.data?.awayUrl ? <a className="button secondary compact away-link" href={host.data.awayUrl}>Open tailnet URL</a> : <p className="fine-print">Start with <code>leave connect . --away</code> after installing Tailscale on the computer and phone.</p>}
        </section>

        <section className="settings-section">
          <header><span className="settings-icon"><Database aria-hidden="true" size={18} weight="regular" /></span><div><h2>Workspace exposure</h2><p>These controls do not change what Devin reads locally.</p></div></header>
          <Toggle checked={host.data?.workspace.expose_history ?? false} onChange={() => undefined} label="Local history" description="Read-only here. Set when registering the workspace with the local CLI." disabled />
          <Toggle checked={host.data?.workspace.expose_project_customization ?? false} onChange={() => undefined} label="Project customization" description="Rules, skills, local plugins, and project MCP management." disabled />
          <Toggle checked={host.data?.workspace.expose_global_customization ?? false} onChange={() => undefined} label="Global customization" description="User-level changes require this separate registration grant." disabled />
        </section>

        <section className="settings-section">
          <header><span className="settings-icon"><Globe aria-hidden="true" size={18} weight="regular" /></span><div><h2>Diagnostics</h2><p>Leave accepts a fixed metadata whitelist.</p></div></header>
          <Toggle checked={false} onChange={() => undefined} label="Product diagnostics" description="No diagnostics are transmitted by the local alpha." disabled />
          <p className="fine-print">The protocol schema already rejects prompts, paths, commands, diffs, terminal output, and model output.</p>
        </section>

        <section className="settings-section data-section">
          <header><span className="settings-icon"><Key aria-hidden="true" size={18} weight="regular" /></span><div><h2>Local device data</h2><p>Only encrypted envelopes belong in the offline cache.</p></div></header>
          <button className="button secondary destructive-outline" type="button" onClick={() => void clearCache()}>
            <Trash aria-hidden="true" size={16} weight="regular" /> Clear encrypted cache
          </button>
          {cacheMessage && <p className="success-message" role="status">{cacheMessage}</p>}
        </section>

        <CustomizationPanel enabled={host.data?.capabilities.projectCustomization ?? false} globalEnabled={host.data?.capabilities.globalCustomization ?? false} />
      </div>
    </div>
  );
}
