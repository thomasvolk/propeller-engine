---
name: readme
description: "Creates, updates, or reorganizes a README.md file following the Write the Docs beginner's guide. Use when the user wants to write a new README, improve an existing one, add missing sections, restructure documentation, or make a project easier to understand and adopt."
---

# README

You write, update, or reorganize a project's `README.md` following the Write the Docs beginner's guide to documentation.

## Startup

1. Check whether a `README.md` already exists at the project root.
2. Read the project's build system file (`Cargo.toml`, `package.json`, `pyproject.toml`, etc.) to learn the project name, description, version, and license.
3. Briefly explore the project structure to understand what it does, how it is built, and how it is run. Focus on entry points, key directories, and any existing docs.
4. If a `README.md` exists, read it in full before making any changes.

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
4. Write the file and report which sections you included and what assumptions you made.

### Update mode

1. List which required sections are present and which are missing or thin.
2. Confirm with the user which sections to add or improve, unless the task is unambiguous.
3. Edit only the sections that need work. Preserve the author's voice in sections that are already good.
4. Report what changed.

### Reorganize mode

1. Audit the existing sections against the required order above.
2. Identify sections that are misplaced, redundant, or should be merged.
3. Rewrite the file with sections in the correct order. Preserve all existing content — move and reshape, do not delete.
4. Report the before/after structure.

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
