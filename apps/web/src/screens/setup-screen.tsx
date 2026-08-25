import { useEffect, useRef, useState, type Ref } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeft,
  ArrowRight,
  Browser,
  Check,
  Copy,
  Desktop,
  DeviceMobile,
  DownloadSimple,
  FolderOpen,
  GlobeSimple,
  Key,
  LockKey,
  PlugsConnected,
  TerminalWindow,
  WarningCircle
} from "@phosphor-icons/react";
import {
  connectSetupTailscale,
  fetchSetupStatus,
  installSetupDevin,
  launchSetupWorkspace,
  loginSetupDevin,
  selectSetupWorkspace,
  LocalApiError
} from "../lib/api";
import { QrCode } from "../components/qr-code";
import type {
  SetupDevinLogin,
  SetupLaunchRequest,
  SetupStatus,
  SetupTailscaleConnection,
  SetupTool,
  SetupToolAction
} from "../types";

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
  const applyStatus = (status: SetupStatus) => queryClient.setQueryData(["setup-status", token], status);
  const login = useMutation({
    mutationFn: () => loginSetupDevin(token),
    onSuccess: () => void setup.refetch()
  });
  const install = useMutation({ mutationFn: () => installSetupDevin(token), onSuccess: applyStatus });
  const tailscale = useMutation({
    mutationFn: () => connectSetupTailscale(token),
    onSuccess: () => void setup.refetch()
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
  const error = setup.error ?? login.error ?? install.error ?? tailscale.error ?? picker.error ?? launch.error;
  const pendingAction = login.isPending ? "connectDevin" : install.isPending ? "installDevin" : tailscale.isPending ? "connectTailscale" : undefined;

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

  function runAction(action: SetupToolAction) {
    if (action.id === "installDevin") install.mutate();
    else if (action.id === "connectDevin") login.mutate();
    else tailscale.mutate();
  }

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

      {error && <SetupErrorBanner error={error} ref={errorRef} />}

      <section className="setup-card">
        {launch.data ? (
          <SuccessStep result={launch.data} />
        ) : step === 0 ? (
          <ComputerStep status={status} pendingAction={pendingAction} devinLogin={login.data} tailscale={tailscale.data} onAction={runAction} onRefresh={() => void setup.refetch()} />
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
            tailscalePending={tailscale.isPending}
            tailscaleResult={tailscale.data}
            onConnectTailscale={() => tailscale.mutate()}
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

function SetupErrorBanner({ error, ref }: { error: Error; ref: Ref<HTMLDivElement> }) {
  const detail = error instanceof LocalApiError ? error.detail : undefined;
  return (
    <div className="setup-error" role="alert" tabIndex={-1} ref={ref}>
      <WarningCircle aria-hidden="true" size={19} />
      <div>
        <strong>Leave needs your attention</strong>
        <span>{error.message}</span>
        {detail && <details className="setup-error-detail"><summary>What the tool reported</summary><pre>{detail}</pre></details>}
      </div>
    </div>
  );
}

function ComputerStep({ status, pendingAction, devinLogin, tailscale, onAction, onRefresh }: {
  status: SetupStatus;
  pendingAction?: string;
  devinLogin?: SetupDevinLogin;
  tailscale?: SetupTailscaleConnection;
  onAction: (action: SetupToolAction) => void;
  onRefresh: () => void;
}) {
  const devin = status.devin;
  const [phoneGuideOpen, setPhoneGuideOpen] = useState(false);
  const showDevinNotice = !devin.ready && (devin.loginPending || Boolean(devinLogin) || Boolean(devin.loginUrl) || Boolean(devin.loginOutput));
  return (
    <div className="setup-step">
      <div className="setup-step-heading"><span className="setup-step-icon"><Desktop aria-hidden="true" size={21} /></span><div><h2>Check this computer</h2><p>Leave sets up what is missing for you. Devin is required; phone access and browser preview are optional.</p></div></div>
      <div className="setup-checks">
        <ToolRow icon={PlugsConnected} tool={devin} pendingAction={pendingAction} onAction={onAction} />
        <ToolRow icon={DeviceMobile} tool={status.tailscale} optional pendingAction={pendingAction} onAction={onAction} />
        <ToolRow icon={Browser} tool={status.browser} optional pendingAction={pendingAction} onAction={onAction} />
      </div>
      <div className="setup-phone-how">
        <span className="phone-how-summary"><DeviceMobile aria-hidden="true" size={17} /><p>Leave can also live on your phone, privately, over your Tailscale network. Never the public internet.</p></span>
        <button className="text-button compact" type="button" aria-expanded={phoneGuideOpen} onClick={() => setPhoneGuideOpen((open) => !open)}>How?</button>
      </div>
      {phoneGuideOpen && (
        <div className="away-result phone-how-guide">
          <strong>How phone access works</strong>
          <PhoneSteps paired={false} />
          <p>Tick <strong>Open from my phone</strong> in the Access step and the finish screen shows your phone's QR code. Skipped it there? Run <code>leave connect . --away</code> in your workspace later.</p>
        </div>
      )}
      {showDevinNotice && <DevinLoginNotice tool={devin} result={devinLogin} />}
      {tailscale?.loginUrl && <TailscaleLoginNotice connection={tailscale} />}
      <button className="text-button" type="button" onClick={onRefresh}>Check again</button>
    </div>
  );
}

function DevinLoginNotice({ tool, result }: { tool: SetupTool; result?: SetupDevinLogin }) {
  const detail = result?.detail ?? tool.detail;
  const url = result?.loginUrl ?? tool.loginUrl;
  return (
    <div className="setup-notice">
      <strong>Finish signing in to Devin</strong>
      <p>{detail}</p>
      {url && <a className="button secondary compact" href={url} target="_blank" rel="noreferrer">Open Devin sign-in<ArrowRight aria-hidden="true" size={15} /></a>}
      {tool.manualCommand && <p>Prefer a terminal, or nothing opened? Run <code>{tool.manualCommand}</code>, finish it there, then choose Check again.</p>}
      {tool.path && <p>Leave checked the Devin command at <code>{tool.path}</code>. If you signed in somewhere else, sign that exact command in.</p>}
      {tool.loginOutput && <details className="setup-error-detail"><summary>What Devin reported</summary><pre>{tool.loginOutput}</pre></details>}
    </div>
  );
}

function TailscaleLoginNotice({ connection }: { connection: SetupTailscaleConnection }) {
  return (
    <div className="setup-notice">
      <strong>Finish signing in to Tailscale</strong>
      <p>{connection.detail}</p>
      {connection.loginUrl && <a className="button secondary compact" href={connection.loginUrl} target="_blank" rel="noreferrer">Open Tailscale sign-in<ArrowRight aria-hidden="true" size={15} /></a>}
    </div>
  );
}

function ToolRow({ icon: Icon, tool, optional, pendingAction, onAction }: {
  icon: typeof Desktop;
  tool: SetupTool;
  optional?: boolean;
  pendingAction?: string;
  onAction: (action: SetupToolAction) => void;
}) {
  const action = tool.action;
  const pending = Boolean(action && pendingAction === action.id);
  return (
    <div className="setup-tool-row">
      <span className="tool-row-icon"><Icon aria-hidden="true" size={19} /></span>
      <div>
        <strong>{tool.label}{optional ? <small>Optional</small> : null}</strong>
        <p>{tool.detail}</p>
        {action && <code title="Leave runs this exact command">{action.command}</code>}
        {!action && tool.manualCommand && <code>{tool.manualCommand}</code>}
      </div>
      <span className={`setup-state ${tool.ready ? "ready" : "needs-action"}`}>{tool.ready ? <><Check aria-hidden="true" size={14} />Ready</> : tool.installed ? "Needs sign-in" : "Not installed"}</span>
      {action ? (
        <button className="button compact" type="button" disabled={pending} onClick={() => onAction(action)} title={action.detail}>
          {pending ? "Working…" : <>{action.id === "installDevin" ? <DownloadSimple aria-hidden="true" size={15} /> : null}{action.label}</>}
        </button>
      ) : !tool.ready && tool.url ? (
        <a className="button secondary compact" href={tool.url} target="_blank" rel="noreferrer">Install</a>
      ) : null}
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
  tailscalePending: boolean; tailscaleResult?: SetupTailscaleConnection; onConnectTailscale: () => void;
  onAway: (value: boolean) => void; onBackground: (value: boolean) => void; onTerminal: (value: boolean) => void; onPreview: (value: boolean) => void; onGlobalCustomization: (value: boolean) => void;
}) {
  const phoneReady = props.status.tailscale.ready;
  return (
    <div className="setup-step">
      <div className="setup-step-heading"><span className="setup-step-icon"><Key aria-hidden="true" size={21} /></span><div><h2>Choose access</h2><p>Leave keeps sensitive capabilities off until you turn them on.</p></div></div>
      <fieldset className="setup-options"><legend>Connection</legend>
        <Option checked={props.background} onChange={props.onBackground} icon={Desktop} title="Keep Leave running" detail={`Start at sign-in using a ${props.status.platform.serviceLabel}.`} />
        <Option checked={props.away} onChange={props.onAway} icon={DeviceMobile} title="Open from my phone" detail={phoneReady ? "Chat with Devin and approve its work from your phone over your private Tailscale network. Other tailnet users are rejected." : "Tailscale is not connected on this computer yet."} disabled={!phoneReady} />
      </fieldset>
      {!phoneReady && (
        <div className="setup-notice">
          <strong>Want to use Leave from your phone?</strong>
          <p>{props.tailscaleResult?.detail ?? (props.status.tailscale.installed ? "Leave can start Tailscale's sign-in for you. Nothing is exposed to the public internet." : "Install Tailscale on this computer and your phone, then check this computer again.")}</p>
          {props.status.tailscale.installed ? (
            <button className="button secondary compact" type="button" disabled={props.tailscalePending} onClick={props.onConnectTailscale}>{props.tailscalePending ? "Working…" : "Connect Tailscale"}</button>
          ) : (
            <a className="button secondary compact" href={props.status.tailscale.url ?? "https://tailscale.com/download"} target="_blank" rel="noreferrer">Get Tailscale</a>
          )}
          {props.tailscaleResult?.loginUrl && <a className="button secondary compact" href={props.tailscaleResult.loginUrl} target="_blank" rel="noreferrer">Open Tailscale sign-in</a>}
        </div>
      )}
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

function SuccessStep({ result }: { result: { localUrl: string; awayUrl: string | null; awayOwner: string | null; workspacePath: string; background: boolean } }) {
  return (
    <div className="setup-success">
      <span className="setup-success-mark"><Check aria-hidden="true" size={26} /></span>
      <h2>Workspace connected</h2>
      <p className="mono">{result.workspacePath}</p>
      <a className="button setup-open-button" href={result.localUrl}>Open Leave<ArrowRight aria-hidden="true" size={16} /></a>
      {result.awayUrl ? <PhonePairing url={result.awayUrl} owner={result.awayOwner} /> : (
        <div className="setup-phone-later">
          <DeviceMobile aria-hidden="true" size={17} />
          <p>Want Leave on your phone too? Install Tailscale on this computer and your phone, sign in with the same account on both, then open a terminal in your workspace and run <code>leave connect . --away</code>, or run <code>leave setup</code> again and tick <strong>Open from my phone</strong>. Nothing is exposed to the public internet.</p>
        </div>
      )}
      <small>{result.background ? "Leave will restart when you sign in to this computer." : "Keep Leave Setup open while you use this workspace."}</small>
    </div>
  );
}

function PhonePairing({ url, owner }: { url: string; owner: string | null }) {
  const [copied, setCopied] = useState(false);
  async function copyUrl() {
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }
  return (
    <div className="away-result">
      <strong>Use Leave on your phone</strong>
      <PhoneSteps owner={owner} paired />
      <div className="phone-pairing">
        <QrCode value={url} title={`Scan to open Leave at ${url}`} />
        <div>
          <p>Your private phone address</p>
          <div className="away-address"><code>{url}</code><button className="icon-button" type="button" aria-label="Copy phone address" onClick={() => void copyUrl()}>{copied ? <Check aria-hidden="true" size={18} /> : <Copy aria-hidden="true" size={18} />}</button></div>
        </div>
      </div>
    </div>
  );
}

function PhoneSteps({ owner, paired }: { owner?: string | null; paired: boolean }) {
  return (
    <ol className="phone-steps">
      <li>
        <strong>Install Tailscale on your phone</strong>
        <small>Get it from the App Store or Google Play, open it, and sign in{owner ? <> with the same account as this computer (<span className="mono">{owner}</span>)</> : " with the same account as this computer"}. Tailscale gives your phone a private, encrypted route to this machine.</small>
      </li>
      <li>
        <strong>Open Leave on the phone</strong>
        {paired ? (
          <small>Point the phone's camera at the code below and tap the link that appears. No QR reader? Copy the address next to the code and open it in your phone's browser instead.</small>
        ) : (
          <small>Once setup finishes with phone access enabled, the finish screen shows a QR code for your private address. Point the phone's camera at it and tap the link, or copy the address into the phone's browser.</small>
        )}
      </li>
      <li>
        <strong>Install it like an app</strong>
        <small>iPhone or iPad: in Safari, tap the Share button (square with an arrow), then <em>Add to Home Screen</em>. Android: in Chrome, tap the three-dot menu, then <em>Add to Home screen</em> or <em>Install app</em>.</small>
      </li>
      <li>
        <strong>You're set</strong>
        <small>Leave opens from your home screen like a native app. Chat with Devin, watch it work, and approve its operations anywhere your phone can reach Tailscale. The app shell is cached, so it opens instantly even on a weak connection.</small>
      </li>
    </ol>
  );
}

function SetupUnavailable({ title, detail }: { title: string; detail: string }) {
  return <div className="setup-unavailable"><WarningCircle aria-hidden="true" size={27} /><h1>{title}</h1><p>{detail}</p></div>;
}
