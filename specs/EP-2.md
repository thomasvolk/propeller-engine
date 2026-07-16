# EP-2 · `project get` CLI Command — PRD

## Overview

An operator at the command line can run `propeller project get` to see the same
current/pending project information exposed by EP-1, without needing to speak the
socket protocol directly. The command behaves like the existing `status` command in
how it reaches the daemon and how it reports the daemon being unreachable.

**Confidence Level:** 96% — all previously open questions are now resolved. The
printed-output shape, error conventions for both the daemon-unreachable and
daemon-reported-error failure modes, formatting, and EP-1's wire-format dependency
are all defined, and every functional and non-functional requirement, user journey,
and acceptance criterion is unambiguous and testable. No further gaps are known
within EP-2's scope.

---

## User Journeys

### UJ-1 · Operator checks the active project from the command line

An operator wants to know what project is currently loaded on a running daemon.
They run `propeller project get` and read the printed JSON directly, or pipe it
into another tool, without needing to open a raw socket connection themselves.

### UJ-2 · Operator verifies a staged project before committing

After creating or modifying a project that leaves it pending rather than active,
the operator runs `propeller project get` to see both the still-active project and
the staged one side by side, confirming the staged content looks right before
committing it.

### UJ-3 · Script checks project state without first checking daemon liveness

A script calls `propeller project get` as a state check without separately probing
whether the daemon is running. If the daemon is not running, the script gets a
clear error and a non-zero exit code it can branch on, rather than a hang or a
confusing parse failure.

---

## Functional Requirements

| ID  | Requirement |
|-----|-------------|
| F-1 | Given a running daemon with an active project, `propeller project get` prints compact, single-line JSON to stdout with a "current" entry containing that project's complete data (header, tracks, notes, pitch bends — per EP-1's `"project"` query response), with no wrapping/status field. |
| F-2 | Given a running daemon with a pending project, the printed JSON also includes a "pending" entry with that project's complete data. |
| F-3 | Given a running daemon with no pending project, the printed JSON has no "pending" entry. |
| F-4 | Given a running daemon with no active project, the command still succeeds and prints JSON reflecting that (no "current" key present), rather than erroring. |
| F-5 | Given no daemon is running, `propeller project get` prints an error message and exits without printing project JSON. |
| F-6 | The command resolves and reaches the daemon the same way the existing `status` command does. |
| F-7 | The printed JSON strips any wrapping/status field from the daemon's response, printing only the "current"/"pending" entries. |
| F-8 | On a daemon-unreachable failure, the error message follows the shared project/loop convention — `propeller: cannot connect to <path>: <err>` printed to stderr via the common error handler — not `status`'s bespoke stdout convention. |
| F-9 | The printed JSON is compact (single-line), not pretty-printed/indented. |
| F-10 | On a daemon-reported error response to the "project" query, `propeller project get` reuses the same shared error handler as the daemon-unreachable case (F-8): the daemon's error message is printed to stderr and the command exits non-zero, with no project JSON printed. |

---

## Non-Functional Requirements

| ID   | Requirement |
|------|-------------|
| NF-1 | The command resolves the daemon socket location using the same mechanism (including environment override) as every other CLI command. |
| NF-2 | On any failure (daemon unreachable or daemon-reported error) the command exits with a non-zero status code. |

---

## Acceptance Criteria

| ID   | Given | When | Then |
|------|-------|------|------|
| AC-1 | A running daemon with an active project | Operator runs `propeller project get` | Compact, single-line JSON is printed to stdout with a "current" entry containing that project's complete data, and no wrapping/status field |
| AC-2 | A running daemon with a pending project | Operator runs `propeller project get` | The printed JSON also includes a "pending" entry with that project's complete data |
| AC-3 | A running daemon with no pending project | Operator runs `propeller project get` | The printed JSON has no "pending" entry |
| AC-4 | A running daemon with no active and no pending project | Operator runs `propeller project get` | The command succeeds (exit 0) and prints compact JSON with no "current" and no "pending" key present |
| AC-5 | No daemon is running | Operator runs `propeller project get` | An error message in the form `propeller: cannot connect to <path>: <err>` is printed to stderr via the shared error handler; no project JSON is printed; the command exits non-zero |
| AC-6 | A running daemon returns a daemon-reported error for the "project" query | Operator runs `propeller project get` | The daemon's error message is printed to stderr via the shared error handler; no project JSON is printed; the command exits non-zero |

---

## Open Questions

None. All previously raised questions have been reconciled; the PRD is complete
for EP-2's scope.

---

## Refinement Log

### Cycle 1 — Confidence: 55%
- Created PRD from roadmap EP-2 section (no prior PRD existed).
- Added: Q1 (verbatim vs. trimmed JSON output), Q2 (which error-reporting convention to follow), Q3 (compact vs. pretty-printed output), Q4 (tracking dependency on EP-1's unresolved wire-format questions).

### Cycle 2 — Confidence: 85%
- Reconciled: Q1 → F-7/AC-1 (strip wrapping/status field), Q2 → F-8/AC-5 (stderr, shared error-handler convention), Q3 → F-9/AC-1/AC-4 (compact single-line JSON).
- Resolved (dependency satisfied): Q4 — EP-1's wire-format questions are now answered, so F-1/AC-1/AC-2/AC-4 have been updated to reference the `"project"` command and complete-data shape.
- Added: Q5 (whether NF-3's latency requirement meaningfully applies to this CLI command, given the CLI never runs the daemon's player loop).

### Cycle 3 — Confidence: 88%
- Reconciled: Q5 → NF-3 removed (latency concern is not applicable to the CLI process; covered instead by EP-1's NF-2).
- Added: Q6 (output/exit behaviour when the daemon returns a daemon-reported error, as distinct from being unreachable).

### Cycle 4 — Confidence: 96%
- Reconciled: Q6 → F-10/AC-6 (daemon-reported error reuses the shared error handler,
  stderr, non-zero exit — confirmed against the existing `ClientError::Daemon` arm
  in `handle_client_error`, src/main.rs).
- No new questions added; confidence exceeds 90% and no open questions remain.
