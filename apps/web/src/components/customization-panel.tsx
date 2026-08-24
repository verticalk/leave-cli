import { useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { ArrowsClockwise, BracketsCurly, Check, PlugsConnected, PuzzlePiece, WarningCircle } from "@phosphor-icons/react";
import { getCustomization, mutateCustomization, type CustomizationMutation } from "../lib/api";

type Category = "rules" | "skills" | "plugins" | "mcp";

const categories: Array<{ id: Category; label: string; icon: typeof BracketsCurly }> = [
  { id: "rules", label: "Rules", icon: BracketsCurly },
  { id: "skills", label: "Skills", icon: Check },
  { id: "plugins", label: "Plugins", icon: PuzzlePiece },
  { id: "mcp", label: "MCP", icon: PlugsConnected }
];

export function CustomizationPanel({ enabled, globalEnabled }: { enabled: boolean; globalEnabled: boolean }) {
  const [category, setCategory] = useState<Category>("rules");
  const [detailInput, setDetailInput] = useState("");
  const [detailName, setDetailName] = useState("");
  const globalCategoryBlocked = category === "plugins" && !globalEnabled;
  const listing = useQuery({
    queryKey: ["customization", category],
    queryFn: ({ signal }) => getCustomization(category, undefined, signal),
    enabled: enabled && !globalCategoryBlocked
  });
  const detail = useQuery({
    queryKey: ["customization-detail", category, detailName],
    queryFn: ({ signal }) => getCustomization(category, detailName, signal),
    enabled: enabled && !globalCategoryBlocked && Boolean(detailName)
  });

  if (!enabled) {
    return <div className="settings-section customization-section"><header><div><h2>Devin customization</h2><p>Project customization was not granted when this workspace was registered.</p></div></header></div>;
  }

  return (
    <section className="settings-section customization-section">
      <header><span className="settings-icon"><BracketsCurly aria-hidden="true" size={18} /></span><div><h2>Devin customization</h2><p>Uses the documented <code>devin rules</code>, <code>skills</code>, <code>plugins</code>, and <code>mcp</code> commands.</p></div></header>
      <div className="customization-tabs" role="tablist" aria-label="Customization categories">
        {categories.map(({ id, label, icon: Icon }) => <button type="button" role="tab" aria-selected={category === id} className={category === id ? "active" : ""} onClick={() => { setCategory(id); setDetailName(""); }} key={id}><Icon aria-hidden="true" size={16} />{label}</button>)}
      </div>
      <div className="customization-grid">
        <div className="customization-output">
          <header><strong>Installed and available</strong><button className="icon-button" type="button" aria-label={`Refresh ${category}`} onClick={() => void listing.refetch()}><ArrowsClockwise aria-hidden="true" size={16} /></button></header>
          {globalCategoryBlocked && <div className="inline-note"><WarningCircle aria-hidden="true" size={18} /><span>Plugin inventory and changes require the separate global customization grant.</span></div>}
          {listing.isPending && <div className="tool-state" role="status">Loading {category}…</div>}
          {listing.error && <ToolError>{listing.error.message}</ToolError>}
          {listing.data && <pre><code>{listing.data.output || `No ${category} reported by Devin.`}</code></pre>}
          <form className="detail-form" onSubmit={(event) => { event.preventDefault(); setDetailName(detailInput.trim()); }}>
            <label htmlFor="customization-name">Show details by name</label>
            <div><input id="customization-name" value={detailInput} onChange={(event) => setDetailInput(event.target.value)} /><button className="button secondary compact" type="submit" disabled={!detailInput.trim()}>Show</button></div>
          </form>
          {detail.isPending && <div className="tool-state" role="status">Loading details…</div>}
          {detail.error && <ToolError>{detail.error.message}</ToolError>}
          {detail.data && <pre className="detail-output"><code>{detail.data.output}</code></pre>}
        </div>
        <div className="customization-actions">
          {(category === "rules" || category === "skills") ? (
            <div className="inline-note"><BracketsCurly aria-hidden="true" size={18} /><span>Edit project-backed rule and skill files from the Files tab. Leave will show the exact BLAKE3 conflict state before any overwrite.</span></div>
          ) : globalCategoryBlocked ? <div className="inline-note"><WarningCircle aria-hidden="true" size={18} /><span>Run <code>leave connect /path/to/repo --expose-global-customization</code> on the host to opt in.</span></div> : <MutationForm key={category} category={category} globalEnabled={globalEnabled} onDone={() => void listing.refetch()} />}
        </div>
      </div>
    </section>
  );
}

function MutationForm({ category, globalEnabled, onDone }: { category: "plugins" | "mcp"; globalEnabled: boolean; onDone: () => void }) {
  const [action, setAction] = useState(category === "plugins" ? "install" : "enable");
  const [name, setName] = useState("");
  const [scope, setScope] = useState("local");
  const [transport, setTransport] = useState("http");
  const [url, setUrl] = useState("");
  const [command, setCommand] = useState("");
  const [argumentsText, setArgumentsText] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [reviewed, setReviewed] = useState(false);
  const expected = useMemo(() => {
    if (!name.trim()) return "";
    if (category === "plugins") return `${action.toUpperCase()} PLUGIN ${name.trim()}`;
    return `${action.toUpperCase()} MCP ${name.trim()}`;
  }, [action, category, name]);
  const mutation = useMutation({
    mutationFn: (body: CustomizationMutation) => mutateCustomization(body),
    onSuccess: () => {
      setConfirmation("");
      setReviewed(false);
      onDone();
    }
  });
  const actions = category === "plugins" ? ["install", "update", "remove"] : ["add", "enable", "disable", "remove"];

  function submit(event: React.FormEvent) {
    event.preventDefault();
    mutation.mutate({
      kind: category === "plugins" ? "plugin" : "mcp",
      action,
      name: name.trim(),
      scope,
      transport,
      url,
      command,
      arguments: argumentsText.split("\n").map((value) => value.trim()).filter(Boolean),
      confirmation
    });
  }

  return (
    <form className="mutation-form" onSubmit={submit}>
      <h3>Manage {category === "plugins" ? "plugin" : "MCP server"}</h3>
      <p>Executable configuration requires review plus an exact typed confirmation.</p>
      <div className="field-pair">
        <label>Action<select value={action} onChange={(event) => { setAction(event.target.value); setConfirmation(""); setReviewed(false); }}>{actions.map((value) => <option value={value} key={value}>{value}</option>)}</select></label>
        {category === "mcp" && <label>Scope<select value={scope} onChange={(event) => setScope(event.target.value)}><option value="local">Local</option><option value="project">Project</option>{globalEnabled && <option value="user">User</option>}</select></label>}
      </div>
      <label>{category === "plugins" && action === "install" ? "GitHub source or HTTPS Git URL" : "Name"}<input value={name} onChange={(event) => { setName(event.target.value); setConfirmation(""); setReviewed(false); }} required /></label>
      {category === "mcp" && action === "add" && (
        <>
          <label>Transport<select value={transport} onChange={(event) => setTransport(event.target.value)}><option value="http">HTTP</option><option value="sse">SSE</option><option value="stdio">stdio</option></select></label>
          {transport === "stdio" ? <><label>Command<input value={command} onChange={(event) => setCommand(event.target.value)} required /></label><label>Arguments, one per line<textarea rows={3} value={argumentsText} onChange={(event) => setArgumentsText(event.target.value)} /></label></> : <label>Server URL<input type="url" value={url} onChange={(event) => setUrl(event.target.value)} required /></label>}
        </>
      )}
      <label className="review-check"><input type="checkbox" checked={reviewed} onChange={(event) => setReviewed(event.target.checked)} /><span>I reviewed the source, scope, and executable impact.</span></label>
      <label>Type <code>{expected || "choose a name first"}</code><input value={confirmation} onChange={(event) => setConfirmation(event.target.value)} autoComplete="off" required /></label>
      <button className="button" type="submit" disabled={!reviewed || !expected || confirmation !== expected || mutation.isPending}>{mutation.isPending ? "Applying…" : "Apply through Devin"}</button>
      {mutation.error && <ToolError>{mutation.error.message}</ToolError>}
      {mutation.data && <p className="success-message" role="status">{mutation.data.output || "Devin applied the change."}</p>}
    </form>
  );
}

function ToolError({ children }: { children: React.ReactNode }) {
  return <div className="tool-error" role="alert"><WarningCircle aria-hidden="true" size={16} />{children}</div>;
}
