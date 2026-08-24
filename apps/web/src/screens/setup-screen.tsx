import { useEffect, useRef, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeft,
  ArrowRight,
  Browser,
  Check,
  Copy,
  Desktop,
  DeviceMobile,
  FolderOpen,
  GlobeSimple,
  Key,
  LockKey,
  PlugsConnected,
  TerminalWindow,
  WarningCircle
} from "@phosphor-icons/react";
import {
  fetchSetupStatus,
  launchSetupWorkspace,
  loginSetupDevin,
  selectSetupWorkspace
} from "../lib/api";
import type { SetupLaunchRequest, SetupStatus, SetupTool } from "../types";

const steps = ["Computer", "Workspace", "Access", "Review"] as const;

export function SetupScreen() {
  const queryClient = useQueryClient();
  const [token] = useState(() => {
    const queryToken = new URLSearchParams(location.search).get("token");
    const fragmentToken = new URLSearchParams(location.hash.slice(1)).get("token");
    const value = queryToken ?? fragmentToken ?? sessionStorage.getItem("leave-setup-token") ?? "";
    if (value) sessionStorage.setItem("leave-setup-token", value);
    if (queryToken || fragmentToken) history.replaceState(null, "", "/setup");
    return value;
  });
  const [step, setStep] = useState(0);
  const [workspacePath, setWorkspacePath] = useState("");
  const [away, setAway] = useState(false);
  const [background, setBackground] = useState(true);
  const [terminal, setTerminal] = useState(false);
  const [preview, setPreview] = useState(false);
  const [globalCustomization, setGlobalCustomization] = useState(false);
  const errorRef = useRef<HTMLDivElement>(null);
  const setup = useQuery({
    queryKey: ["setup-status", token],
    queryFn: ({ signal }) => fetchSetupStatus(token, signal),
    enabled: Boolean(token),
    retry: false,
    refetchInterval: 5_000
  });
  const login = useMutation({
    mutationFn: () => loginSetupDevin(token),
    onSuccess: (status) => queryClient.setQueryData(["setup-status", token], status)
  });
  const picker = useMutation({
    mutationFn: () => selectSetupWorkspace(token),
    onSuccess: (selection) => {
      if (selection.path) setWorkspacePath(selection.path);
    }
  });
  const launch = useMutation({
    mutationFn: (body: SetupLaunchRequest) => launchSetupWorkspace(token, body)
  });
  const error = setup.error ?? login.error ?? picker.error ?? launch.error;

  useEffect(() => {
    if (error) errorRef.current?.focus();
  }, [error]);

  useEffect(() => {
    document.getElementById("setup-heading")?.focus({ preventScroll: true });
  }, [step]);

  if (!token) {
    return <SetupUnavailable title="This setup link is incomplete" detail="Open Leave Setup from the desktop shortcut to create a fresh private link." />;
  }
  if (setup.isPending) {
    return <div className="setup-loading" role="status"><span className="loading-dot" />Checking this computer…</div>;
  }
  if (!setup.data) {
    return <SetupUnavailable title="Leave Setup could not connect" detail={setup.error?.message ?? "Open Leave Setup again."} />;
  }

  const status = setup.data;
  const canContinue = step === 0 ? status.devin.ready : step === 1 ? Boolean(workspacePath.trim()) : true;

  function next() {
    if (step < steps.length - 1 && canContinue) setStep((current) => current + 1);
  }

  function start() {
    launch.mutate({
      workspacePath: workspacePath.trim(),
      port: status.hostPort,
      away,
      background,
      terminal,
      preview,
      globalCustomization
    });
  }

  return (
    <div className="setup-page">
      <header className="setup-intro">
        <p className="eyebrow">First-run setup · {status.platform.label}</p>
        <h1 id="setup-heading" tabIndex={-1}>{launch.data ? "Leave is ready" : "Connect this computer"}</h1>
        <p>{launch.data ? "Your workspace host is running. You can return here later to change access." : "Leave will connect Devin, approve one workspace, and set up private access without copying credentials."}</p>
      </header>

      {!launch.data && (
        <ol className="setup-progress" aria-label="Setup progress">
          {steps.map((label, index) => (
            <li className={index === step ? "current" : index < step ? "complete" : ""} aria-current={index === step ? "step" : undefined} key={label}>
              <span>{index < step ? <Check aria-hidden="true" size={14} /> : index + 1}</span>
              <strong>{label}</strong>
            </li>
          ))}
        </ol>
      )}

      {error && <div className="setup-error" role="alert" tabIndex={-1} ref={errorRef}><WarningCircle aria-hidden="true" size={19} /><div><strong>Leave needs your attention</strong><span>{error.message}</span></div></div>}

      <section className="setup-card">
        {launch.data ? (
          <SuccessStep result={launch.data} />
        ) : step === 0 ? (
          <ComputerStep status={status} loginPending={login.isPending} onLogin={() => login.mutate()} onRefresh={() => void setup.refetch()} />
        ) : step === 1 ? (
          <WorkspaceStep value={workspacePath} example={status.workspaceExample} pickerAvailable={status.folderPickerAvailable} pickerPending={picker.isPending} pickerDetail={picker.data?.detail} onChange={setWorkspacePath} onPick={() => picker.mutate()} />
        ) : step === 2 ? (
          <AccessStep
            status={status}
            away={away}
            background={background}
            terminal={terminal}
            preview={preview}
            globalCustomization={globalCustomization}
            onAway={setAway}
            onBackground={setBackground}
            onTerminal={setTerminal}
            onPreview={setPreview}
            onGlobalCustomization={setGlobalCustomization}
          />
        ) : (
          <ReviewStep status={status} workspacePath={workspacePath} away={away} background={background} terminal={terminal} preview={preview} globalCustomization={globalCustomization} />
        )}

        {!launch.data && (
          <footer className="setup-actions">
            <button className="button secondary" type="button" disabled={step === 0 || launch.isPending} onClick={() => setStep((current) => Math.max(0, current - 1))}><ArrowLeft aria-hidden="true" size={16} />Back</button>
            {step < steps.length - 1 ? (
              <button className="button" type="button" disabled={!canContinue} onClick={next}>Continue<ArrowRight aria-hidden="true" size={16} /></button>
            ) : (
              <button className="button" type="button" disabled={launch.isPending} onClick={start}>{launch.isPending ? "Starting Leave…" : "Start Leave"}<ArrowRight aria-hidden="true" size={16} /></button>
            )}
          </footer>
        )}
      </section>
      <p className="setup-footnote"><LockKey aria-hidden="true" size={15} />Setup accepts commands only through this private localhost session.</p>
    </div>
  );
}

function ComputerStep({ status, loginPending, onLogin, onRefresh }: {
  status: SetupStatus;
  loginPending: boolean;
  onLogin: () => void;
  onRefresh: () => void;
}) {
  return (
    <div className="setup-step">
      <div className="setup-step-heading"><span className="setup-step-icon"><Desktop aria-hidden="true" size={21} /></span><div><h2>Check this computer</h2><p>Devin is required. Phone access and browser preview are optional.</p></div></div>
      <div className="setup-checks">
        <ToolRow icon={PlugsConnected} tool={status.devin} action={status.devin.installed && !status.devin.ready ? <button className="button compact" type="button" disabled={loginPending} onClick={onLogin}>{loginPending ? "Waiting for Devin…" : "Connect Devin"}</button> : undefined} />
        <ToolRow icon={DeviceMobile} tool={status.tailscale} optional />
        <ToolRow icon={Browser} tool={status.browser} optional />
      </div>
      <button className="text-button" type="button" onClick={onRefresh}>Check again</button>
    </div>
  );
}

function ToolRow({ icon: Icon, tool, optional, action }: { icon: typeof Desktop; tool: SetupTool; optional?: boolean; action?: ReactNode }) {
  return (
    <div className="setup-tool-row">
      <span className="tool-row-icon"><Icon aria-hidden="true" size={19} /></span>
      <div><strong>{tool.label}{optional ? <small>Optional</small> : null}</strong><p>{tool.detail}</p>{tool.path && <code>{tool.path}</code>}</div>
      <span className={`setup-state ${tool.ready ? "ready" : "needs-action"}`}>{tool.ready ? <><Check aria-hidden="true" size={14} />Ready</> : tool.installed ? "Needs sign-in" : "Not installed"}</span>
      {action ?? (!tool.installed && tool.url ? <a className="button secondary compact" href={tool.url} target="_blank" rel="noreferrer">Install</a> : null)}
    </div>
  );
}

function WorkspaceStep({ value, example, pickerAvailable, pickerPending, pickerDetail, onChange, onPick }: { value: string; example: string; pickerAvailable: boolean; pickerPending: boolean; pickerDetail?: string; onChange: (value: string) => void; onPick: () => void }) {
  return (
    <div className="setup-step">
      <div className="setup-step-heading"><span className="setup-step-icon"><FolderOpen aria-hidden="true" size={21} /></span><div><h2>Choose your workspace</h2><p>Leave can access this folder and its Git repository. You can add more workspaces later.</p></div></div>
      <div className="setup-field"><label htmlFor="workspace-path">Workspace folder</label><div><input id="workspace-path" value={value} onChange={(event) => onChange(event.target.value)} placeholder={example} autoComplete="off" required />{pickerAvailable && <button className="button secondary" type="button" disabled={pickerPending} onClick={onPick}><FolderOpen aria-hidden="true" size={17} />{pickerPending ? "Opening…" : "Choose folder"}</button>}</div><small>{pickerDetail ?? "Leave resolves the real path and rejects folders reached through traversal."}</small></div>
    </div>
  );
}

function AccessStep(props: {
  status: { platform: { serviceLabel: string }; tailscale: SetupTool; browser: SetupTool };
  away: boolean; background: boolean; terminal: boolean; preview: boolean; globalCustomization: boolean;
  onAway: (value: boolean) => void; onBackground: (value: boolean) => void; onTerminal: (value: boolean) => void; onPreview: (value: boolean) => void; onGlobalCustomization: (value: boolean) => void;
}) {
  return (
    <div className="setup-step">
      <div className="setup-step-heading"><span className="setup-step-icon"><Key aria-hidden="true" size={21} /></span><div><h2>Choose access</h2><p>Leave keeps sensitive capabilities off until you turn them on.</p></div></div>
      <fieldset className="setup-options"><legend>Connection</legend>
        <Option checked={props.background} onChange={props.onBackground} icon={Desktop} title="Keep Leave running" detail={`Start at sign-in using a ${props.status.platform.serviceLabel}.`} />
        <Option checked={props.away} onChange={props.onAway} icon={DeviceMobile} title="Open from my phone" detail={props.status.tailscale.ready ? "Use your private Tailscale network. Other tailnet users are rejected." : "Sign in to Tailscale on this computer before enabling phone access."} disabled={!props.status.tailscale.ready} />
      </fieldset>
      <fieldset className="setup-options"><legend>Workspace capabilities</legend>
        <Option checked={props.terminal} onChange={props.onTerminal} icon={TerminalWindow} title="Terminal" detail="Run shell commands as your signed-in computer account." />
        <Option checked={props.preview} onChange={props.onPreview} icon={Browser} title="Browser preview" detail="Control an isolated Chromium profile limited to local app URLs." disabled={!props.status.browser.ready} />
      </fieldset>
      <details className="setup-advanced"><summary>Advanced customization access</summary><Option checked={props.globalCustomization} onChange={props.onGlobalCustomization} icon={GlobeSimple} title="User-global customization" detail="Let this workspace read or change global skills, plugins, and MCP settings." /></details>
    </div>
  );
}

function Option({ checked, onChange, icon: Icon, title, detail, disabled }: { checked: boolean; onChange: (value: boolean) => void; icon: typeof Desktop; title: string; detail: string; disabled?: boolean }) {
  return <label className={`setup-option ${disabled ? "disabled" : ""}`}><input type="checkbox" checked={checked} disabled={disabled} onChange={(event) => onChange(event.target.checked)} /><span className="option-icon"><Icon aria-hidden="true" size={19} /></span><span><strong>{title}</strong><small>{detail}</small></span></label>;
}

function ReviewStep({ status, workspacePath, away, background, terminal, preview, globalCustomization }: { status: { platform: { label: string; serviceLabel: string } }; workspacePath: string; away: boolean; background: boolean; terminal: boolean; preview: boolean; globalCustomization: boolean }) {
  const capabilities = [terminal && "Terminal", preview && "Browser preview", globalCustomization && "Global customization"].filter(Boolean);
  return <div className="setup-step"><div className="setup-step-heading"><span className="setup-step-icon"><Check aria-hidden="true" size={21} /></span><div><h2>Review and start</h2><p>Leave will apply these choices on {status.platform.label}.</p></div></div><dl className="setup-review"><div><dt>Workspace</dt><dd className="mono">{workspacePath}</dd></div><div><dt>Runs</dt><dd>{background ? `At sign-in through ${status.platform.serviceLabel}` : "Until this computer restarts or the host exits"}</dd></div><div><dt>Phone</dt><dd>{away ? "Private Tailscale access" : "This computer only"}</dd></div><div><dt>Extra access</dt><dd>{capabilities.length ? capabilities.join(", ") : "None"}</dd></div></dl></div>;
}

function SuccessStep({ result }: { result: { localUrl: string; awayUrl: string | null; workspacePath: string; background: boolean } }) {
  const [copied, setCopied] = useState(false);
  async function copyAwayUrl() {
    if (!result.awayUrl) return;
    try {
      await navigator.clipboard.writeText(result.awayUrl);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }
  return <div className="setup-success"><span className="setup-success-mark"><Check aria-hidden="true" size={26} /></span><h2>Workspace connected</h2><p className="mono">{result.workspacePath}</p><a className="button setup-open-button" href={result.localUrl}>Open Leave<ArrowRight aria-hidden="true" size={16} /></a>{result.awayUrl && <div className="away-result"><strong>On your phone</strong><p>Open this private address after signing in to Tailscale.</p><div><code>{result.awayUrl}</code><button className="icon-button" type="button" aria-label="Copy phone address" onClick={() => void copyAwayUrl()}>{copied ? <Check aria-hidden="true" size={18} /> : <Copy aria-hidden="true" size={18} />}</button></div></div>}<small>{result.background ? "Leave will restart when you sign in to this computer." : "Keep Leave Setup open while you use this workspace."}</small></div>;
}

function SetupUnavailable({ title, detail }: { title: string; detail: string }) {
  return <div className="setup-unavailable"><WarningCircle aria-hidden="true" size={27} /><h1>{title}</h1><p>{detail}</p></div>;
}
