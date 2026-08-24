import { useEffect, useMemo, useState } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { rust } from "@codemirror/lang-rust";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeft,
  ArrowsClockwise,
  CaretRight,
  Check,
  File,
  FloppyDisk,
  Folder,
  GitBranch,
  GitCommit,
  GitDiff,
  GitPullRequest,
  WarningCircle
} from "@phosphor-icons/react";
import {
  commitGit,
  getGitDiff,
  getGitStatus,
  listFiles,
  listGitBranches,
  pushGit,
  readFile,
  stageGitPaths,
  switchGitBranch,
  unstageGitPaths,
  writeFile
} from "../lib/api";

export function FilesPanel() {
  const [view, setView] = useState<"files" | "git">("files");
  return (
    <div className="workspace-tool">
      <div className="tool-switcher" role="tablist" aria-label="Workspace tools">
        <button type="button" role="tab" aria-selected={view === "files"} className={view === "files" ? "active" : ""} onClick={() => setView("files")}><File aria-hidden="true" size={16} />Files</button>
        <button type="button" role="tab" aria-selected={view === "git"} className={view === "git" ? "active" : ""} onClick={() => setView("git")}><GitBranch aria-hidden="true" size={16} />Git</button>
      </div>
      {view === "files" ? <FileBrowser /> : <GitPanel />}
    </div>
  );
}

function FileBrowser() {
  const queryClient = useQueryClient();
  const [directory, setDirectory] = useState("");
  const [selectedPath, setSelectedPath] = useState<string>();
  const [draft, setDraft] = useState("");
  const [conflict, setConflict] = useState("");
  const listing = useQuery({
    queryKey: ["files", directory],
    queryFn: ({ signal }) => listFiles(directory, signal)
  });
  const file = useQuery({
    queryKey: ["file", selectedPath],
    queryFn: ({ signal }) => readFile(selectedPath ?? "", signal),
    enabled: Boolean(selectedPath)
  });

  useEffect(() => {
    if (file.data) {
      setDraft(file.data.content);
      setConflict("");
    }
  }, [file.data]);

  const save = useMutation({
    mutationFn: () => writeFile(selectedPath ?? "", file.data?.hash ?? "", draft),
    onSuccess: async (snapshot) => {
      queryClient.setQueryData(["file", selectedPath], snapshot);
      setDraft(snapshot.content);
      setConflict("");
      await queryClient.invalidateQueries({ queryKey: ["git-status"] });
    },
    onError: (error) => setConflict(error.message)
  });
  const dirty = Boolean(file.data && draft !== file.data.content);
  const parent = directory.split("/").slice(0, -1).join("/");

  return (
    <div className="file-workbench">
      <aside className="file-tree" aria-label="Workspace files">
        <header>
          <button className="icon-button" type="button" aria-label="Parent directory" disabled={!directory} onClick={() => setDirectory(parent)}><ArrowLeft aria-hidden="true" size={16} /></button>
          <span className="mono" title={directory || "Workspace root"}>{directory || "workspace"}</span>
          <button className="icon-button" type="button" aria-label="Refresh files" onClick={() => void listing.refetch()}><ArrowsClockwise aria-hidden="true" size={16} /></button>
        </header>
        {listing.isPending && <ToolState>Loading files…</ToolState>}
        {listing.error && <ToolError>{listing.error.message}</ToolError>}
        <div className="file-list">
          {listing.data?.map((entry) => (
            <button
              type="button"
              className={selectedPath === entry.path ? "selected" : ""}
              disabled={entry.isSymlink}
              title={entry.isSymlink ? "Symlink navigation is blocked" : entry.path}
              onClick={() => entry.isDirectory ? setDirectory(entry.path) : setSelectedPath(entry.path)}
              key={entry.path}
            >
              {entry.isDirectory ? <Folder aria-hidden="true" size={17} /> : <File aria-hidden="true" size={17} />}
              <span>{entry.name}</span>
              {entry.isDirectory && <CaretRight aria-hidden="true" size={14} />}
            </button>
          ))}
        </div>
      </aside>
      <section className="file-editor">
        {!selectedPath && <div className="tool-empty"><File aria-hidden="true" size={25} /><strong>Choose a text file</strong><span>Reads and writes stay inside the registered workspace.</span></div>}
        {file.isPending && selectedPath && <ToolState>Opening {selectedPath}…</ToolState>}
        {file.error && <ToolError>{file.error.message}</ToolError>}
        {file.data && selectedPath && (
          <>
            <header className="editor-bar">
              <div><strong>{selectedPath.split("/").at(-1)}</strong><span className="mono">{selectedPath} · {file.data.lineEnding.toUpperCase()}</span></div>
              <button className="button compact" type="button" disabled={!dirty || save.isPending} onClick={() => save.mutate()}><FloppyDisk aria-hidden="true" size={16} />{save.isPending ? "Saving…" : "Save"}</button>
            </header>
            {conflict && <div className="conflict-banner" role="alert"><WarningCircle aria-hidden="true" size={17} /><span><strong>File changed on the host.</strong>{conflict}</span><button type="button" className="button secondary compact" onClick={() => void file.refetch()}>Load host version</button></div>}
            <CodeMirror
              className="code-editor"
              value={draft}
              height="100%"
              extensions={selectedPath.endsWith(".rs") ? [rust()] : []}
              onChange={setDraft}
              basicSetup={{ foldGutter: false, highlightActiveLine: true }}
            />
            <footer className="editor-status"><span>{dirty ? "Unsaved changes" : "Saved"}</span><span>{file.data.size.toLocaleString()} bytes · BLAKE3 {file.data.hash.slice(0, 10)}</span></footer>
          </>
        )}
      </section>
    </div>
  );
}

function GitPanel() {
  const queryClient = useQueryClient();
  const [selected, setSelected] = useState<string[]>([]);
  const [diffPath, setDiffPath] = useState<string>();
  const [commitMessage, setCommitMessage] = useState("");
  const [pushArmed, setPushArmed] = useState(false);
  const status = useQuery({ queryKey: ["git-status"], queryFn: ({ signal }) => getGitStatus(signal) });
  const branches = useQuery({ queryKey: ["git-branches"], queryFn: ({ signal }) => listGitBranches(signal) });
  const diff = useQuery({
    queryKey: ["git-diff", diffPath],
    queryFn: ({ signal }) => getGitDiff(diffPath, false, signal),
    enabled: Boolean(diffPath)
  });
  const refresh = async () => {
    setSelected([]);
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["git-status"] }),
      queryClient.invalidateQueries({ queryKey: ["git-branches"] }),
      queryClient.invalidateQueries({ queryKey: ["git-diff"] })
    ]);
  };
  const stage = useMutation({ mutationFn: () => stageGitPaths(selected), onSuccess: refresh });
  const unstage = useMutation({ mutationFn: () => unstageGitPaths(selected), onSuccess: refresh });
  const commit = useMutation({ mutationFn: () => commitGit(commitMessage), onSuccess: async () => { setCommitMessage(""); await refresh(); } });
  const push = useMutation({ mutationFn: pushGit, onSuccess: async () => { setPushArmed(false); await refresh(); } });
  const switchBranch = useMutation({ mutationFn: (name: string) => switchGitBranch(name), onSuccess: refresh });
  const mutationError = stage.error ?? unstage.error ?? commit.error ?? push.error ?? switchBranch.error;

  function toggle(path: string) {
    setSelected((current) => current.includes(path) ? current.filter((item) => item !== path) : [...current, path]);
  }

  return (
    <div className="git-workbench">
      <section className="git-sidebar">
        <header className="git-summary">
          <div><span className="eyebrow">Branch</span><strong><GitBranch aria-hidden="true" size={16} />{status.data?.branch ?? "Loading…"}</strong></div>
          <button className="icon-button" type="button" aria-label="Refresh Git" onClick={() => void refresh()}><ArrowsClockwise aria-hidden="true" size={16} /></button>
        </header>
        <label className="field-label" htmlFor="git-branch">Switch branch</label>
        <select id="git-branch" value={status.data?.branch ?? ""} disabled={switchBranch.isPending} onChange={(event) => switchBranch.mutate(event.target.value)}>
          {branches.data?.map((branch) => <option value={branch.name} key={branch.name}>{branch.name}{branch.current ? " · current" : ""}</option>)}
        </select>
        <div className="git-actions">
          <button className="button secondary compact" type="button" disabled={!selected.length || stage.isPending} onClick={() => stage.mutate()}><Check aria-hidden="true" size={15} />Stage</button>
          <button className="button secondary compact" type="button" disabled={!selected.length || unstage.isPending} onClick={() => unstage.mutate()}>Unstage</button>
        </div>
        {status.isPending && <ToolState>Reading Git status…</ToolState>}
        {status.error && <ToolError>{status.error.message}</ToolError>}
        <div className="change-list">
          {status.data?.entries.map((entry) => (
            <div className={`change-row ${diffPath === entry.path ? "active" : ""}`} key={entry.path}>
              <input aria-label={`Select ${entry.path}`} type="checkbox" checked={selected.includes(entry.path)} onChange={() => toggle(entry.path)} />
              <span className="git-state" aria-label={`Git state ${entry.index}${entry.worktree}`}>{entry.index}{entry.worktree}</span>
              <button type="button" onClick={() => setDiffPath(entry.path)}>{entry.path}</button>
            </div>
          ))}
          {status.data?.entries.length === 0 && <p className="fine-print">Working tree is clean.</p>}
        </div>
        <div className="commit-box">
          <label htmlFor="commit-message">Commit message</label>
          <textarea id="commit-message" value={commitMessage} onChange={(event) => setCommitMessage(event.target.value)} rows={3} />
          <button className="button compact" type="button" disabled={!commitMessage.trim() || commit.isPending} onClick={() => commit.mutate()}><GitCommit aria-hidden="true" size={16} />Commit staged</button>
        </div>
        <button className={`button compact ${pushArmed ? "" : "secondary"}`} type="button" disabled={push.isPending} onClick={() => pushArmed ? push.mutate() : setPushArmed(true)}><GitPullRequest aria-hidden="true" size={16} />{pushArmed ? "Confirm push" : "Push upstream"}</button>
        {mutationError && <ToolError>{mutationError.message}</ToolError>}
      </section>
      <section className="diff-viewer">
        {!diffPath && <div className="tool-empty"><GitDiff aria-hidden="true" size={25} /><strong>Select a changed file</strong><span>Diffs are generated locally with external diff tools disabled.</span></div>}
        {diff.isPending && diffPath && <ToolState>Loading diff…</ToolState>}
        {diff.error && <ToolError>{diff.error.message}</ToolError>}
        {diff.data && <pre><code>{diff.data.diff || "No unstaged diff for this file."}</code></pre>}
      </section>
    </div>
  );
}

function ToolState({ children }: { children: React.ReactNode }) {
  return <div className="tool-state" role="status">{children}</div>;
}

function ToolError({ children }: { children: React.ReactNode }) {
  return <div className="tool-error" role="alert"><WarningCircle aria-hidden="true" size={16} />{children}</div>;
}
