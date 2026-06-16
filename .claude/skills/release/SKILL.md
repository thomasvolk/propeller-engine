---
name: release
description: "Bumps the project version in Cargo.toml and updates docs/CHANGELOG.md with commits from the current branch relative to main. Requires a semver version string as the argument."
---

# release

You bump the project version and record the changes introduced by the current branch.

`$ARGUMENTS` is the new version string (e.g. `0.2.0`). It is mandatory. If it is empty or
missing, stop immediately and tell the user: "Usage: /release <version>  — e.g. /release 0.2.0"

## Steps

### 1. Validate the argument

- `$ARGUMENTS` must be a non-empty string matching the pattern `MAJOR.MINOR.PATCH` (each
  segment one or more digits). Allow an optional leading `v` (strip it before writing).
- If the pattern does not match, stop and report: "Version must follow semver: MAJOR.MINOR.PATCH"

### 2. Read current state

Run these in parallel:

- `cat Cargo.toml` — to read the current version
- `git log main..HEAD --oneline` — to list commits on this branch
- `git diff main..HEAD --stat` — to summarise files changed
- Check whether `docs/CHANGELOG.md` exists and, if so, read it

### 3. Update Cargo.toml

In `Cargo.toml`, replace the `version = "..."` line inside `[package]` with
`version = "<new-version>"`. Do not touch any other line.

After editing, run `cargo check --quiet` to confirm the file is still valid. If it fails,
restore the original version and report the error.

### 4. Update docs/CHANGELOG.md

The file uses this structure (newest release at the top):

```
# Changelog

## [<version>] - <YYYY-MM-DD>

### Changes

<bullet list of commit subjects from step 2, one per line, prefixed with `- `>

### Files changed

<bullet list from `git diff main..HEAD --stat`, trimmed to file paths only>

---

<existing content below this line>
```

Rules:
- If the file does not exist, create it with just the new release block under a `# Changelog`
  heading.
- If the file exists, insert the new release block immediately after the `# Changelog` heading,
  before any previous release block. Never remove or modify earlier blocks.
- Use today's date (`currentDate` from the system context, or `date -I` if unavailable) in
  `YYYY-MM-DD` format.
- Keep commit subjects exactly as written in the git log — do not paraphrase or summarise.
- Omit merge commits (lines starting with "Merge").
- The `### Files changed` section lists only the file paths (left column of `--stat`), one per
  line, prefixed with `- `.
- Separate the new block from any previous block with a `---` horizontal rule.

### 5. Run cargo fmt

Run `cargo fmt` to ensure Cargo.toml formatting is correct (rustfmt touches TOML only if the
project has a `rustfmt.toml` that covers it; the step is harmless either way).

### 6. Report

Print a concise summary:

```
Version bumped: <old> → <new>
Changelog: docs/CHANGELOG.md updated with <N> commits
```

Do not create a git commit. Leave staging and committing to the user.
