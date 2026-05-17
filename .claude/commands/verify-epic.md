---
description: Verify an epic by running its tests and reporting AC coverage. Use when you want to check which acceptance criteria pass or fail for a given epic.
argument-hint: <epic-id>
---

You are running /verify-epic for epic **$ARGUMENTS**.

`$ARGUMENTS` is the epic ID (e.g. `EP-1`). It must be supplied — if it is missing or blank, stop and output:

> Usage: /verify-epic &lt;epic-id&gt; — e.g. /verify-epic EP-1

Work through the steps below **in order**.

---

## Step 1 — Load context

Read the following files:

- `spec/$ARGUMENTS.md` — the PRD (must exist)
- `spec/$ARGUMENTS-spec.md` — the technical specification (must exist)

If `spec/$ARGUMENTS.md` does not exist, stop and output:

> No PRD found for $ARGUMENTS. Run `/refine-epic $ARGUMENTS` first.

If `spec/$ARGUMENTS-spec.md` does not exist, stop and output:

> No spec found for $ARGUMENTS. Run `/create-spec $ARGUMENTS` first.

Extract the list of acceptance criteria (AC-x rows) from the PRD and the list of test tasks (rows where Type = `test`) from the Implementation Tasks table in the spec. You will use both to map test results to ACs.

---

## Step 2 — Detect build system and run tests

Inspect the project root to determine the build system (e.g. `Cargo.toml` → Rust/cargo, `package.json` → Node, `pyproject.toml` → Python).

Run the full test suite using the appropriate command:

| Build system | Command |
|---|---|
| Rust / cargo | `cargo test 2>&1` |
| Node / npm | `npm test 2>&1` |
| Node / yarn | `yarn test 2>&1` |
| Python / pytest | `pytest -v 2>&1` |
| Other | use the project's documented test command |

Capture the complete output, including individual test names, pass/fail status, and any error messages.

If the test command itself fails to run (e.g. compile error, missing toolchain), report the error and stop:

> Test run failed: `<error summary>`. Fix the build before running /verify-epic.

---

## Step 3 — Map test results to acceptance criteria

Using the test output from Step 2 and the AC list from Step 1:

For each AC-x in the PRD:

1. Find the test task(s) in the spec's Implementation Tasks table that reference this AC (column: PRD ref).
2. From those task descriptions, identify the corresponding test name(s) in the test output.
3. Determine the AC status:
   - **done** — at least one matching test passed and directly exercises this AC's Given/When/Then scenario.
   - **partly** — the AC is partially covered: the happy path passes but an edge case, assertion, or layer is missing.
   - **failing** — a matching test exists but is currently failing.
   - **not done** — no test in the output covers this AC.

If a test name does not obviously map to an AC, use the test description and the AC's Given/When/Then text to infer the link. Note any uncertain mappings in the report.

---

## Step 4 — Output the verification report

Output the report in this exact format:

```
## Verification Report — $ARGUMENTS

### Test suite result
<N> passed, <M> failed, <K> ignored.

### Failing tests
<list each failing test with its error, or "None" if all pass>

### AC Coverage

| AC | Status | Test(s) |
|----|--------|---------|
| AC-1 | done | `<test name>` |
| AC-2 | failing | `<test name>` — <one-line error> |
| AC-3 | partly | `<test name>` — happy path passes; <what is missing> |
| AC-4 | not done | No test found |

### Notes
<Any caveats, flaky tests, excluded tests, or recommended next steps.>
```

If all ACs are `done`, end with:

> All acceptance criteria for $ARGUMENTS are verified. ✓
