---
description: Iteratively refine a technical specification with open Q&A until confidence reaches 90%. Reconciles answered questions into architecture, data model, and tasks.
argument-hint: <spec-name>
---

You are running /refine-spec for spec **$ARGUMENTS**.

Work through the steps below **in order**. Do not skip any step.

---

## Step 1 — Load context

Read `spec/$ARGUMENTS-spec.md`.

If the file does not exist, output the following message and stop immediately. Do not proceed to any further step.

> No spec found for $ARGUMENTS. Run `/create-spec $ARGUMENTS` first to generate the technical specification.

---

## Step 2 — Reconcile answered questions

Read the **Open Questions** section of `spec/$ARGUMENTS-spec.md`.

For every question whose **Answer** field is filled in:

1. Interpret what the answer implies for the specification.
2. Apply the implication to the appropriate section:
   - An implementation detail or component behaviour → update **Architecture Overview** or the relevant **Components** subsection
   - A type, field, or schema detail → add or update a row in **Data Model**
   - A new or revised behaviour that needs testing → add task rows to **Implementation Tasks**, keeping strict TDD order (test before impl)
   - A high-impact architectural choice with remaining trade-offs → add a new **D-N** entry to **Open Decisions**
3. Remove the answered question from the Open Questions section entirely.
4. Record what was reconciled in the Revision Log entry you will write in Step 5.

If there are no answered questions, skip this step and proceed to Step 3.

---

## Step 3 — Assess confidence

Analyse the current state of `spec/$ARGUMENTS-spec.md` and assign a confidence level (0–100%) reflecting how completely and unambiguously the specification covers the PRD.

Use this rubric:

| Dimension | Questions to ask |
|-----------|-----------------|
| PRD coverage | Does every F-x and AC-x in the corresponding PRD have at least one task? |
| TDD ordering | Does every impl task have a test task that precedes it? |
| Architecture clarity | Are all components and their interactions described without ambiguity? |
| Data model completeness | Are all types and structures needed to implement the requirements defined? |
| Task specificity | Is each task small enough to be implemented and reviewed independently? |
| Open ambiguities | How many unanswered questions and unchecked high-impact decisions remain? |

Write a short plain-English explanation of what is missing or ambiguous that justifies the score.

Update the **Confidence Level** line in the document with the new score and explanation.

---

## Step 4 — Add new questions if confidence < 90%

If confidence is below 90%, identify the most critical unresolved ambiguities and add them to the **Open Questions** section.

Rules for questions:
- Focus on the gaps that would most increase confidence when answered.
- Each question must be specific and answerable.
- Provide 2–4 concrete options per question.
- Mark exactly one option as *(recommended — {one-line reason})*.
- Leave the **Answer** field empty for the user to fill in.
- Assign sequential Q-numbers continuing from any existing ones.

If confidence is 90% or above, do not add questions. State that the specification is complete.

---

## Step 5 — Update the Revision Log

Append one new log entry at the bottom of the Revision Log section. Keep entries compact — one per cycle.

Format:

```
### Cycle N — Confidence: X%
- Reconciled: Q-1 → architecture updated (logger uses tracing crate), Q-2 → data model updated (LogLevel enum)
- Added: Q-3 (async runtime choice), Q-4 (test isolation strategy)
```

If nothing was reconciled and nothing was added, still append the entry with the updated score and a brief note.

---

## Step 6 — Save the file

Write the fully updated document to `spec/$ARGUMENTS-spec.md`.

The document must maintain sections in this exact order:

```
# $ARGUMENTS · {Epic Title} — Technical Specification

## Overview
**Confidence Level:** X% — {one-sentence explanation}

---

## Architecture Overview

---

## Components
### {Component Name}

---

## Data Model
| Type | Fields | Notes |
|------|--------|-------|

---

## Implementation Tasks
| ID | Task | Type | PRD ref | Depends on |
|----|------|------|---------|------------|

---

## Open Questions
{Only unanswered questions live here. Answered questions are removed after reconciliation.}

---

## Open Decisions
{High-impact choices awaiting user selection via checkbox. Reconciled by /create-spec.}

---

## Revision Log
{One compact entry per cycle, oldest first.}
```

---

## Open question format

```
### Q-N · {Short descriptive title}

{The question in one or two sentences.}

**Options**
- A. {Option} — {brief rationale}
- B. {Option} — {brief rationale} *(recommended — {one-line reason})*
- C. {Option} — {brief rationale}

**Answer:**
```
