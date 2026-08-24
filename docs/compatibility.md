# Compatibility matrix

**Matrix version:** 0.1

**Updated:** 2026-08-23

**Qualification state:** deterministic ACP contract suite passes; credentialed Devin latest and N-1 runs pending

Leave only claims rows marked supported after CI and manual qualification pass
against the listed Devin version. Unknown versions must show a warning.

| Surface | Interface | Alpha status | Notes |
|---|---|---:|---|
| Start local agent | `devin acp` | Contract-tested | Clean stdio process, no PTY, shell, or TUI parsing |
| ACP initialization | stable ACP v1 | Contract-tested | Negotiated through the official Rust ACP SDK |
| Create a session | ACP `session/new` | Contract-tested | Uses the registered workspace root |
| Stream ACP events | stable ACP v1 | Contract-tested | Appended locally before WebSocket broadcast |
| List Leave sessions | local event store | Supported | Lists sessions created or resumed through this Leave host |
| Import Desktop history | documented CLI JSON / ACP capability | Planned | No private history database access |
| Resume a session | ACP `session/resume` | Contract-tested | Preserves the official agent session ID |
| ATIF import/export | documented CLI | Planned | No proprietary transcript conversion |
| Models and modes | ACP capability negotiation | Planned | Hide unsupported controls |
| Prompts and cancellation | ACP v1 | Contract-tested | One active turn per session; command IDs are deduplicated; uncertain prompts are never retried automatically |
| Permissions | ACP permission requests | Contract-tested | Exact agent option IDs, one pending resolution, five-minute expiry |
| Filesystem | Leave guarded host API | Implemented | UTF-8 files up to 2 MiB; canonical containment, no symlink writes, atomic BLAKE3 conflicts, EOL and mode preservation |
| Git and worktrees | structured host API | Implemented | Status, diff, stage, unstage, commit, push, branches; worktree inventory is read-only; no raw Git arguments |
| Raw terminal | explicit capability | Implemented | `portable-pty`, host-lifetime persistence, live binary WebSocket, no device scrollback cache; off by default |
| Browser preview | local Chromium/CDP | Implemented with system browser | Ephemeral profile, loopback-only approved origin, CDP screenshots/click/text/navigation; pinned automatic CfT download remains gated on trustworthy checksum metadata |
| Project rules/skills | documented Devin CLI and paths | Implemented | List/show through official commands; project files use guarded editor |
| Plugins and MCP | documented Devin CLI | Implemented | List/show plus structured mutations; executable/destructive actions require review and exact phrase |
| Global customization | owner opt-in | Implemented | User-scope MCP remains unavailable without the separate workspace registration grant |
| Private phone access | Tailscale Serve | Implemented | Loopback host, tailnet HTTPS, exact owner identity header; Funnel is unsupported |
| Hosted encrypted relay | RFC 9420 MLS | Release-gated | No plaintext or temporary cipher fallback; OpenMLS audit, vectors, persistence, external review, and legal approval still required |
| Cloud handoff | official Devin session | CLI launcher implemented | User completes `/handoff` in official client |

## Excluded Desktop surfaces

Leave does not claim editor extension hosting, language-server UI, Tab
completion, Codemaps, Quick Review, Code Lenses, or the entire Devin Desktop
command center. A future row may move into scope only when Cognition exposes a
supported interface and contract tests cover it.
