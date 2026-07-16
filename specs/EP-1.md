# EP-1 · Project State Query over the Socket Interface — PRD

## Overview

A client connected to the daemon's socket can ask for the project state and receive
a structured answer describing what is currently active and what is pending. This
mirrors the existing `status` query but is scoped to project data rather than
playback/clock state, and it works the same way regardless of what previously
happened on the connection (loading, committing, or leaving a project pending).

**Confidence Level:** 95% — the wire shape, absent-entry representation, project
data completeness, mode-independence, and latency enforcement mechanism are all
pinned down. What remains is a minor implementation-level detail (the exact nested
field names within the "current"/"pending" project object beyond "complete data"),
which is not a requirement gap.

---

## User Journeys

### UJ-1 · Operator inspects the active project via a script

A script or tool connected to the daemon's socket sends a project-state query after
a project has been loaded and committed. It receives the full project data back
under a "current" entry so it can display or act on it without separately tracking
what was loaded.

### UJ-2 · Client checks project state before any project has been loaded

A newly started daemon has no project loaded yet. A client queries project state
early in a session — for example, before deciding whether to create a project — and
receives a response that clearly shows no current project exists, without that
being treated as an error.

### UJ-3 · Client inspects a staged-but-uncommitted project

An operator has sent a new project to the daemon but not yet committed it, for
example while reviewing it before switching over. Querying project state shows both
what is still active and what is waiting to replace it, letting the operator
confirm the staged content before committing.

---

## Functional Requirements

| ID  | Requirement |
|-----|-------------|
| F-1 | When a project has been made active, a project-state query returns that project's complete data (the same shape a client would send to load a project — header, full tracks, notes, and pitch bends) under a "current" entry in the response. |
| F-2 | When no project has ever been made active, a project-state query response omits the "current" key entirely, indicating there is no current project rather than returning an error. |
| F-3 | When a project has been staged but not yet committed, a project-state query includes that project's complete data under a "pending" entry in the response. |
| F-4 | When no project is staged, a project-state query response omits the "pending" entry entirely. |
| F-5 | When both a current and a pending project exist at the same time, a single project-state query returns both entries together in the same response. |
| F-6 | The socket command field value that invokes this query is `"project"`. |
| F-7 | The project-state query response is identical in shape and content regardless of daemon mode (standalone or sync). |

---

## Non-Functional Requirements

| ID   | Requirement |
|------|-------------|
| NF-1 | A project-state query is read-only: it must not create, modify, commit, or discard any current or pending project as a side effect of being answered. |
| NF-2 | Handling a project-state query (`project get`) must not introduce any latency into the player's loop. |
| NF-3 | The project-state query handler must never block or share its execution context (task/thread) with the component driving playback timing; this is enforced structurally by running the handler on the daemon's async `tokio::select!` event loop with no synchronous I/O or locks shared with the playback path, rather than via a numeric latency bound. |

---

## Acceptance Criteria

| ID   | Given | When | Then |
|------|-------|------|------|
| AC-1 | A project has been made active | A client sends a `"project"` query | The response includes a "current" entry with that project's complete data (header, tracks, notes, pitch bends) |
| AC-2 | No project has ever been made active | A client sends a `"project"` query | The response omits the "current" key entirely; this is not treated as an error |
| AC-3 | A project has been staged but not committed | A client sends a `"project"` query | The response includes a "pending" entry with that project's complete data |
| AC-4 | No project is staged | A client sends a `"project"` query | The response omits the "pending" entry entirely |
| AC-5 | Both a current and a pending project exist | A client sends a single `"project"` query | The response includes both the "current" and "pending" entries together |
| AC-6 | The daemon is running in sync mode instead of standalone mode, with the same current/pending state | A client sends a `"project"` query | The response is identical in shape and content to the standalone-mode response |
| AC-7 | The project-state query handler is implemented as part of the daemon's async event loop | A client sends `"project"` queries while a project is actively playing | The handler completes without blocking or acquiring any lock shared with the playback timing path, verified via code/architecture review against the `tokio::select!` / `spawn_blocking` design rather than a numeric latency measurement |

---

## Open Questions

None.

---

## Refinement Log

### Cycle 1 — Confidence: 60%
- Created PRD from roadmap EP-1 section (no prior PRD existed).
- Added: Q1 (socket command name), Q2 (absent-"current" representation), Q3 (project data shape/completeness), Q4 (mode-dependence of the response).

### Cycle 2 — Confidence: 80%
- Reconciled: Q1 → F-6 (command name is `"project"`), Q2 → F-2/AC-2 (absent "current" omits the key entirely), Q3 → F-1/F-3/AC-1/AC-3 (complete project structure, not a summary), Q4 → F-7/AC-6 (response is mode-independent).
- Added: Q5 (enforcement/testability of the NF-2 latency requirement).

### Cycle 3 — Confidence: 95%
- Reconciled: Q5 → NF-3 (non-blocking structural enforcement, no shared execution context with playback), AC-7 (verified via architecture review, not a numeric benchmark).
- No new questions added; PRD is complete.
