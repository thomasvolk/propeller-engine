---
description: Implement an epic from its technical specification using TDD. Reads the spec, writes tests, implements requirements, runs tests, fixes issues until all pass, then delegates final QA to /verify-epic.
argument-hint: <epic-id>
---

You are running /implement-epic for epic **$ARGUMENTS**.

Work through the steps below **in order**. Do not skip any step.

---

## Step 1 — Load context

Read the following files:

- `spec/$ARGUMENTS.md` — the PRD for this epic (must exist)
- `spec/$ARGUMENTS-spec.md` — the technical specification (must exist)

If `spec/$ARGUMENTS.md` does not exist, stop and output:

> No PRD found for $ARGUMENTS. Run `/refine-epic $ARGUMENTS` first.

If `spec/$ARGUMENTS-spec.md` does not exist, stop and output:

> No spec found for $ARGUMENTS. Run `/create-spec $ARGUMENTS` first.

---

## Step 2 — Analyze the specification

Before writing any code, build a complete mental model of what you are about to implement:

1. Read the **Architecture Overview** and **Components** sections. Understand the component boundaries and interactions.
2. Read the **Data Model**. Note every type, its fields, and its role.
3. Read the **Implementation Tasks** table. Group tasks into TDD pairs: each `test` task with the `impl` task that follows it. Note the dependency chain.
4. Read all **AC-x** rows in the PRD so you understand the expected behaviour before writing code.

Do not proceed until you can answer: what does this epic build, how does it work, and in what order will you implement it?

---

## Step 3 — Prepare the project

Check whether the project already has a build system file (e.g. `Cargo.toml` for Rust, `package.json` for Node, `pyproject.toml` for Python).

If no build system file exists at the project root:

- Initialise a new project appropriate for the language chosen in the spec (e.g. `cargo init --name propeller` for Rust).
- Create the standard directory structure for that language.

If the project already exists, verify it compiles cleanly before adding any new code:

```
cargo build   # or equivalent for the project language
```

Fix any pre-existing compilation errors before proceeding. Do not add new code on top of a broken build.

---

## Step 4 — Implement tasks in TDD order

Work through every TDD pair from the Implementation Tasks table, strictly in dependency order (honour the **Depends on** column). For each pair:

### 4a — Write the test

Implement the test described in the `test` task row. Place it in the appropriate location:

- Unit tests: inside the source module they test, in a `#[cfg(test)]` block (Rust) or equivalent.
- Integration tests: in a `tests/` directory, one file per logical feature area.

The test must:
- Target exactly the behaviour described in the task and its PRD ref.
- Be runnable in isolation (no hidden global state).
- Fail at this point (red phase — the implementation does not exist yet).

Run the test to confirm it compiles and fails for the right reason:

```
cargo test <test_name>   # or equivalent
```

If the test does not even compile, fix the compilation error before continuing.

### 4b — Implement the code

Write the minimum code needed to make the test pass. Do not implement features not yet covered by a test. Reference the **Architecture Overview** and **Data Model** from the spec to guide structure.

### 4c — Run and verify

Run the test again:

```
cargo test <test_name>   # or equivalent
```

If it fails, fix the implementation and re-run. Keep iterating until this specific test passes.

Also run the full test suite after each impl task to catch regressions:

```
cargo test   # or equivalent
```

Fix any regressions before moving on to the next TDD pair. Do not accumulate broken tests.

---

## Step 5 — QA: invoke /verify-epic

With all TDD pairs complete, hand off to the verify-epic skill to run the full test suite and produce the AC coverage report:

Invoke `/verify-epic $ARGUMENTS`.

verify-epic will:
1. Run the complete test suite.
2. Map every passing and failing test to the epic's acceptance criteria.
3. Output the Verification Report.

**If verify-epic reports failing tests**, fix the code — do not modify tests to make them pass artificially — then invoke `/verify-epic $ARGUMENTS` again. Repeat until no tests are failing.

The implementation is complete when verify-epic produces its Verification Report with no failing tests.
