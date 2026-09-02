---
name: release
description: "Bumps the project version in Cargo.toml and updates CHANGELOG.md in the project root with a synthesized summary of commits from the current branch relative to main. Takes an explicit semver version as the argument, or auto-increments the patch segment (e.g. 0.2.1 -> 0.2.2) when no argument is given."
---

# release

You bump the project version and record the changes introduced by the current branch.

`$ARGUMENTS` is the new version string (e.g. `0.2.0`). It is optional — if it is empty or
missing, auto-increment instead (see step 1).

## Steps

### 1. Determine the new version

Run `cat Cargo.toml` to read the current `version = "..."` under `[package]`.

- If `$ARGUMENTS` is non-empty: it must match the pattern `MAJOR.MINOR.PATCH` (each segment
  one or more digits). Allow an optional leading `v` (strip it before writing). If the
  pattern does not match, stop and report: "Version must follow semver: MAJOR.MINOR.PATCH"
- If `$ARGUMENTS` is empty or missing: auto-increment the current version's patch segment
  (the third number) by 1 — e.g. `0.2.1` → `0.2.2`. Do not touch the major or minor segments.

### 2. Read remaining current state

Run these in parallel:

- `git log main..HEAD --oneline` — to list commits on this branch
- Check whether `CHANGELOG.md` exists at the project root and, if so, read it

### 3. Update Cargo.toml

In `Cargo.toml`, replace the `version = "..."` line inside `[package]` with
`version = "<new-version>"`. Do not touch any other line.

After editing, run `cargo check --quiet` to confirm the file is still valid. If it fails,
restore the original version and report the error.

### 4. Update CHANGELOG.md

The file lives at the project root (`CHANGELOG.md`) and uses this structure (newest release at
the top):

```
# Changelog

## [<version>] - <YYYY-MM-DD>

### Changes

<synthesized summary — see rules below>

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
- Omit merge commits (lines starting with "Merge") when reading the git log.
- **Do not list raw commit subjects.** Instead, write a concise human-readable summary of what
  changed in this release: group related commits into themes, describe the user-visible effect
  of each change, and write in the past tense (e.g. "Added X", "Fixed Y", "Removed Z"). Aim
  for 3–7 bullet points that give a reader a clear picture of the release without requiring
  them to read individual commits.
- Separate the new block from any previous block with a `---` horizontal rule.

### 5. Link CHANGELOG.md in README.md

Check whether `README.md` already contains a link to `CHANGELOG.md` (a markdown link with
the text `CHANGELOG.md` or `Changelog`, or any `[...](CHANGELOG.md)` pattern).

- If the link is already present, skip this step.
- If it is absent, insert the following section into `README.md` immediately before the
  `## Contributing` heading:

  ```
  ## Changelog

  See [CHANGELOG.md](CHANGELOG.md) for the full release history.

  ```

  Do not modify any other part of `README.md`.

### 6. Run cargo fmt

Run `cargo fmt` to ensure Cargo.toml formatting is correct (rustfmt touches TOML only if the
project has a `rustfmt.toml` that covers it; the step is harmless either way).

### 7. Report

Print a concise summary, noting when the version was auto-incremented:

```
Version bumped: <old> → <new> (auto-incremented patch)
Changelog: CHANGELOG.md updated
README: CHANGELOG.md linked (or already present)
```

Omit the "(auto-incremented patch)" suffix when `$ARGUMENTS` gave the version explicitly.

Do not create a git commit. Leave staging and committing to the user.
