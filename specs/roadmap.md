# Roadmap: Query Current and Pending Project State

Operators and scripts need a way to ask the running daemon what project is currently
active and what project (if any) is staged but not yet committed, both directly over
the socket interface and through the CLI. The target end-state is a `project get`
command, and the underlying daemon support it relies on, that reports this state as
JSON and fails clearly when no daemon is running.

---

## Dependency graph

| Epic  | Depends on | Can start in parallel with |
| ----- | ---------- | --------------------------- |
| EP-1  | —          | —                            |
| EP-2  | EP-1       | —                            |

---

## EP-1 — Project State Query over the Socket Interface

A client connected to the daemon's socket can ask for the project state and receive
a structured answer describing what is currently active and what is pending. This
mirrors the existing `status` query but is scoped to project data rather than
playback/clock state, and it works the same way regardless of what previously
happened on the connection (loading, committing, or leaving a project pending).

**Acceptance criteria**

- Given a project has been made active, querying project state returns that
  project's full data under a "current" entry.
- Given no project has ever been made active, querying project state indicates
  there is no current project, rather than returning an error.
- Given a project has been staged but not yet committed, querying project state
  includes that project's full data under a "pending" entry.
- Given no project is staged, querying project state omits the "pending" entry
  entirely rather than returning it as empty or null.
- Given both a current and a pending project exist at the same time, querying
  project state returns both in the same response.

---

## EP-2 — `project get` CLI Command

An operator at the command line can run `propeller project get` to see the same
current/pending project information exposed by EP-1, without needing to speak the
socket protocol directly. The command behaves like the existing `status` command in
how it reaches the daemon and how it reports the daemon being unreachable.

**Acceptance criteria**

- Given a running daemon with an active project, running `propeller project get`
  prints a JSON object to stdout with a "current" entry containing that project's
  data.
- Given a running daemon with a pending project, the printed JSON also includes a
  "pending" entry with that project's data.
- Given a running daemon with no pending project, the printed JSON has no "pending"
  entry.
- Given no daemon is running, running `propeller project get` prints an error
  message and exits without printing project JSON.
- The command reaches the daemon the same way the existing `status` command does.
