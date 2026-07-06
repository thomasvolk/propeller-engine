---
name: readme
description: "Creates, updates, or reorganizes a README.md file following the Write the Docs beginner's guide, keeping docs/json-socket-interface.md, docs/known-issues.md, and docs/internals.md in sync. Use when the user wants to write a new README, improve an existing one, add missing sections, restructure documentation, or make a project easier to understand and adopt."
---

# README

You write, update, or reorganize a project's `README.md` following the Write the Docs beginner's guide to documentation, and keep the companion files in `docs/` consistent with it.

## Startup

1. Check whether a `README.md` already exists at the project root.
2. Read the project's build system file (`Cargo.toml`, `package.json`, `pyproject.toml`, etc.) to learn the project name, description, version, and license.
3. Briefly explore the project structure to understand what it does, how it is built, and how it is run. Focus on entry points, key directories, and any existing docs.
4. If a `README.md` exists, read it in full before making any changes.
5. Read every file in `docs/` that exists. These are companion pages the README links out to for extended detail — see "Companion docs/ files" below.

## Mode detection

Determine the operating mode from context:

- **Create** — no `README.md` exists. Build one from scratch.
- **Update** — a `README.md` exists and the user wants to add or improve specific sections.
- **Reorganize** — a `README.md` exists but its structure does not follow the guide. Reorder and rewrite for clarity without removing information.

If the user did not specify a mode, infer it from what is present: no file → create; file exists with good content but poor structure → reorganize; file exists but is missing sections → update.

## Principles (from the Write the Docs guide)

Apply these throughout every mode:

- **State the problem first.** Open with what the project solves and why it exists — not with implementation details or history.
- **Show, don't tell.** Include a concrete code or usage example early. Readers decide whether to adopt a project based on what they see it doing.
- **Lower the barrier to entry.** Installation and first-run steps must be explicit, accurate, and short. Link out to extended guides rather than embedding them.
- **Invite contribution.** Explain how to contribute, where to report issues, and where the source lives.
- **Be findable.** State the license clearly so potential users know the terms immediately.
- **Use plain markdown.** Write only in native markdown syntax — headings, lists, fenced code blocks, bold, italic, tables, blockquotes. Never use HTML tags.
- **Stay honest.** Only document what is true of the project right now. Do not describe aspirational features as if they exist.

## Required sections (in this order)

Every README must contain all of the following, in this sequence:

### 1. Title and one-line description

The project name as an H1 heading, followed immediately by a single sentence that states what the project does and who it is for. No preamble.

### 2. Problem statement

Two to four sentences explaining the problem the project solves and why it matters. Answer: *Why does this project exist?* This section may be folded into the title block if it fits naturally.

### 3. Quick example

A fenced code block showing the most common use case. Annotate with comments only if the code is not self-explanatory. The example should be runnable or closely approximate something runnable.

### 4. Installation

Numbered steps to get from zero to a working installation. Keep it to the minimum required. If setup is complex, provide the minimal steps here and link to a dedicated `INSTALL.md` or docs page.

### 5. Usage

Show how to run or use the project after installation. Cover the primary workflow. For CLIs, list the main commands. For libraries, show the typical import and call pattern.

### 6. Features (optional but recommended)

A short bulleted list of the project's main capabilities. Omit if the Quick Example and Usage sections already make this clear.

### 7. Contributing

Explain how to contribute: where to open issues, how to submit pull requests, coding conventions, and any contributor licence agreement. Link to a `CONTRIBUTING.md` if one exists.

Include direct links to:
- The issue tracker
- The source code repository (if not already the README's host)

### 8. Support

One or two sentences on where users can ask for help: mailing list, chat channel, GitHub Discussions, Stack Overflow tag, etc.

### 9. License

State the license name and include a brief licence notice or a link to the full `LICENSE` file. Example:

```
MIT — see [LICENSE](LICENSE) for details.
```

## Companion docs/ files

The README is not the only place that documents the runtime interface, operational quirks, and
internal design. `docs/` holds three companion pages that must stay consistent with whatever the
README says. Whenever a README edit touches the area a given file covers, update that file in the
same pass — do not leave it to drift out of sync.

| File                            | Covers                                                                                            | Update it when README changes...                                                                                                        |
| ------------------------------- | ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `docs/json-socket-interface.md` | Full command reference, field reference, error codes, and worked examples for the JSON socket API | The "Runtime interface" or "Managing projects" sections: new/renamed commands, fields, ranges, or error codes                           |
| `docs/known-issues.md`          | Limitations, surprising behaviors, and workarounds encountered in live use                        | Port-selection env vars, timing behavior, or any workaround the README documents that also has a known-issue entry                      |
| `docs/internals.md`             | Internal architecture: process model, domain data structures, IPC dispatch, loop engine internals | The "Features" section, the domain model (`Project`/`Track`/`Note` and friends), or anything describing how the engine works internally |

Rules for keeping them in sync:

- Treat field/command/error-code tables in `docs/json-socket-interface.md` as the detailed
  counterpart of README's "Runtime interface" section. If you add a field to one, add it to the
  other with matching name, type, and range.
- `docs/internals.md` embeds real Rust struct/enum snippets (e.g. `Project`, `Track`, `Command`).
  If the README's description of the domain model or feature set implies a struct changed, check
  the actual source (not just the README) before editing `internals.md`'s snippets — copy what the
  code says, don't infer it from README prose alone.
- Each companion file ends with a "See also" section linking back to specific README anchors
  (e.g. `../README.md#runtime-interface`). If you rename or remove a README heading, update every
  anchor link in `docs/` that points to it.
- Do not duplicate full explanations in the README. The README should summarize and link to the
  `docs/` page for depth, matching the pattern already used (e.g. "See the Runtime interface
  section" cross-links). Never inline the full command/error-code reference from
  `docs/json-socket-interface.md` into the README.
- If a README change has no counterpart concept in `docs/`, leave the `docs/` files untouched —
  do not manufacture unrelated edits.

## Optional sections

Include these only when they add real value:

- **Badges** — build status, coverage, version. Place immediately below the title. Do not add badges for things that are not actively maintained.
- **Screenshots / demo** — useful for visual tools or UIs.
- **Roadmap** — only if the project is actively maintained and the roadmap is kept up to date.
- **Acknowledgements** — credits to significant dependencies or inspirations.
- **Changelog** — link to `CHANGELOG.md` if one exists; do not inline it.

Do not add a FAQ section. The Write the Docs guide warns that FAQs become outdated, scatter related information, and are a symptom of documentation that needs to be reorganized rather than extended.

## How you work

### Create mode

1. Gather all facts from the codebase (name, purpose, dependencies, build commands, run commands, license).
2. Draft each required section in order.
3. Write the Quick Example using real invocations from the project — do not invent commands.
4. Write the file.
5. Check each `docs/` companion file against the table above; update any that now need a matching entry (e.g. a newly documented command needs a `docs/json-socket-interface.md` entry).
6. Report which sections you included, what assumptions you made, and which `docs/` files you touched.

### Update mode

1. List which required sections are present and which are missing or thin.
2. Confirm with the user which sections to add or improve, unless the task is unambiguous.
3. Edit only the sections that need work. Preserve the author's voice in sections that are already good.
4. For each edited section, check the Companion docs/ files table: if the section maps to a `docs/` file, update that file's corresponding table/example/snippet in the same pass.
5. Report what changed, in the README and in `docs/`.

### Reorganize mode

1. Audit the existing sections against the required order above.
2. Identify sections that are misplaced, redundant, or should be merged.
3. Rewrite the file with sections in the correct order. Preserve all existing content — move and reshape, do not delete.
4. If any heading anchors changed (renamed or moved in a way that changes the `#anchor`), grep `docs/` for links to the old anchor and update them.
5. Report the before/after structure, and any `docs/` links you fixed.

## Project-specific rules

- Do not reference the `specs/` directory in the README. It is a temporary working folder used during development and is not part of the public-facing project.
- Do not reference the `.claude/` directory in the README. It is a temporary working folder used during development and is not part of the public-facing project.

## Quality checklist

Before writing the final file, verify:

- [ ] Title and one-line description are present and accurate.
- [ ] Problem statement answers *why this project exists*.
- [ ] Quick Example uses real, runnable commands from the project.
- [ ] Installation steps are numbered and accurate.
- [ ] License is stated.
- [ ] No HTML tags in the file.
- [ ] No section describes features that do not yet exist.
- [ ] No FAQ section.
- [ ] No references to the `specs/` or `.claude/` directories.
- [ ] Every `docs/` companion file affected by the change (per the Companion docs/ files table) has been checked and, if needed, updated to match.
- [ ] `docs/` anchor links to the README (`../README.md#...`) still resolve to headings that exist.
