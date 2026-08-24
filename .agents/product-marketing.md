# Product Marketing Context

**Document version:** v1

**Last updated:** 2026-08-23

## Product overview

**One-liner:** Leave is the Devin-native mobile workspace for your own machine.

**What it does:** Leave connects a phone or tablet to the supported local Devin
CLI/ACP agent, approved repository roots, and owner-granted workspace tools. It
keeps local rules, skills, files, history, and approvals in the same working
environment.

**Category:** Mobile coding-agent client and secure local workspace

**Product type:** Open-source software with an optional hosted relay

**Business model:** Free public beta with fair-use quotas; billing is outside v1

## Target audience

**Target companies:** Individual developers and software teams already using
Devin on local machines.

**Decision-makers:** Developers, engineering managers, and security-conscious
platform leads.

**Primary use case:** Continue and supervise local Devin work away from the
computer without moving the repository into a new cloud VM.

**Jobs to be done:**

- Resume local sessions and answer approvals from a phone.
- Inspect or change code, Git state, terminals, and local app previews.
- Share bounded workspace access with an attributed team role.

## Personas

| Persona | Cares about | Challenge | Value we promise |
|---|---|---|---|
| Solo developer | Continuity and speed | Local agent stops being reachable when they leave the desk | A mobile workspace attached to the same machine and repository |
| Engineering manager | Safe oversight | Remote approvals and shared access lack clear responsibility | Roles, device grants, and encrypted audit events |
| Security lead | Containment and evidence | Generic tunnels expose broad machine access | Approved roots, outbound-only host, explicit capabilities, and a blind relay design |

## Problems and pain points

**Core problem:** The official mobile PWA controls Devin Cloud sessions, not the
developer's existing local environment.

**Why alternatives fall short:** Devin-specific clients expose a narrower agent
pane, while broad ACP clients do not publish a tested Devin compatibility
contract. Generic remote desktop makes phone interaction clumsy and grants too
much machine access.

**What it costs:** Developers interrupt active work, leave approvals waiting, or
reconstruct local context in another environment.

**Emotional tension:** Users want mobility without sending a repository,
terminal, or local customization through an opaque service.

## Competitive landscape

**Direct:** MobileVibe and DevinX provide named Devin access but do not document
the same complete local workspace surface.

**Secondary:** Shellular and Happier provide broad mobile agent workspaces but do
not publish a version-tested Devin adapter.

**Indirect:** Devin Cloud's PWA runs cloud sessions; remote desktop exposes the
whole screen and has weak phone ergonomics.

## Differentiation

**Key differentiators:**

- A published compatibility matrix for supported Devin CLI, ACP, and extensions.
- Local session continuity plus files, Git, scoped PTY, and managed app preview.
- Project and optional global customization through documented interfaces.
- Blind relay design, per-device revocation, team roles, and action attribution.

**Why customers choose us:** They want their real local workspace on mobile and
need a narrower trust boundary than remote desktop.

## Objections

| Objection | Response |
|---|---|
| "Devin already has a mobile PWA." | The official PWA controls Cloud sessions. Leave targets supported local sessions and machine state. |
| "Can the relay read my code?" | The design carries workspace content inside an MLS channel. The product will make no zero-knowledge claim before external review. |
| "Does it reproduce every Devin Desktop feature?" | No. The compatibility matrix lists supported agent and workspace surfaces; Desktop-only editor UI stays out of the claim. |

**Anti-persona:** A user seeking an unattended public shell, unsupported token
sharing, Desktop reverse engineering, or a hosted replacement for the official
Devin service.

## Switching dynamics

**Push:** Local sessions become unreachable away from the workstation.

**Pull:** A phone-native workspace tied to the same repository and agent.

**Habit:** Users keep a laptop open, hand work to Cloud, or use remote desktop.

**Anxiety:** Relay privacy, device theft, accidental shell access, and Devin
compatibility.

## Customer language

**How they describe the problem:**

- "I want the same local Devin session on my phone."
- "Let me approve the agent without opening my laptop."

**How they describe us:**

- "The Devin-native mobile workspace for my own machine."

**Words to use:** local workspace, supported interface, approved root, device
grant, encrypted relay, compatibility matrix

**Words to avoid:** all Desktop features, official Devin client, zero knowledge
before review, unrestricted remote shell

## Brand voice

**Tone:** Calm and candid

**Style:** Direct, technical when needed, and precise about security boundaries

**Personality:** Useful, careful, restrained, independent

## Proof points

**Metrics:** None yet. Do not invent adoption or performance claims.

**Customers:** Private research alpha.

**Value themes:**

| Theme | Proof |
|---|---|
| Supported Devin integration | Versioned compatibility matrix and contract tests |
| Local containment | Registered roots and host-side authorization |
| Relay privacy | Public protocol and threat model, pending external review |

## Goals

**Business goal:** Reach a safe public beta after legal and cryptography gates.

**Conversion action:** Install the local host and pair an approved device.

**Current metrics:** No public usage.

## Changelog

- v1 (2026-08-23): Initial context based on the approved product plan and current competitor research.
