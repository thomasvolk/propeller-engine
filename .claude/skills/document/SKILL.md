---
name: document
description: "Creates a user-facing documentation page for a given topic in docs/<topic>.md. Accepts a topic name as the argument and writes a markdown guide from the perspective of someone using propeller-engine."
---

# document

You create a user-facing documentation page for the topic given in `$ARGUMENTS` and write
it to `docs/$ARGUMENTS.md`.

## What "user perspective" means

Write for a musician or live-coder who is running `propeller` at the command line or
scripting against the Unix socket. They care about:

- What to type and why
- What the output means
- What can go wrong and how to recover
- Concrete examples they can copy and run

Do **not** discuss Rust internals, source-file layout, or implementation details unless
they are directly visible to the user (e.g. environment variable names that come from
the source).

## Startup

1. Read `README.md` to understand the full CLI and socket interface.
2. Read any relevant spec files in `spec/` (e.g. `spec/briefing.md`, `spec/roadmap.md`,
   relevant `EP-*.md` epics) to fill in details not yet in the README.
3. Check whether `docs/$ARGUMENTS.md` already exists; if it does, read it before
   making changes so you can update rather than overwrite.
4. Look at any existing files under `docs/` to match their style and depth.

## Document structure

Use this order, omitting sections that genuinely do not apply to the topic:

1. **H1 title** — the topic name, phrased as a user would say it (e.g. "Sync Mode",
   "Project Files", "MIDI Port Setup").
2. **One-sentence summary** — what this topic is and why a user cares about it.
3. **Overview** — two to five sentences giving context: when this feature is used,
   how it fits into a typical workflow, and any important constraints.
4. **Prerequisites** — bullet list of anything the user must set up first (ports,
   env vars, a running daemon, etc.). Omit if there are none.
5. **Step-by-step guide** — numbered steps covering the primary workflow. Include
   the exact commands or JSON payloads to run. Use shell fenced code blocks for
   CLI commands and `json` fenced code blocks for socket payloads.
6. **Reference** — a table of every relevant CLI flag, environment variable, JSON
   field, or response field for this topic. Columns: Name | Type/Values | Description.
   Format the table with aligned columns so `|` separators line up vertically.
7. **Error codes** — a table of error codes the user may see for this topic.
   Columns: Code | Meaning | How to fix. Align columns as above.
8. **Examples** — two or three self-contained worked examples showing realistic
   use cases. Each example has a short heading, a brief explanation, and the
   command(s) to run.
9. **See also** — links to related `docs/` pages or sections in the README,
   using relative markdown links.

## Quality checklist

Before writing the file, verify:

- [ ] Every CLI command and JSON payload is taken from the README or spec; nothing is invented.
- [ ] All tables have aligned columns (pipe characters line up vertically).
- [ ] No HTML tags anywhere in the file.
- [ ] No section describes behaviour that does not exist yet (check the roadmap if unsure).
- [ ] The tone is direct and task-oriented — tell the user what to do, not what the code does.
- [ ] The file is valid markdown: headings, lists, fenced code blocks, tables only.

## Writing the file

Once you are confident the content is correct and complete, write it to `docs/$ARGUMENTS.md`.
Report the sections you included and any assumptions you made about scope.
