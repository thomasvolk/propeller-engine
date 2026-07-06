---
description: Iteratively refine an epic PRD with open Q&A until confidence reaches 90%. Creates the PRD if it doesn't exist. Reconciles answered questions into requirements and ACs.
argument-hint: <epic-id>
---

You are running /refine-epic for epic **$ARGUMENTS**.

Work through the steps below **in order**. Do not skip any step.

---

## Step 1 — Load context

Read the following files:

- `specs/roadmap.md` — locate the section for $ARGUMENTS and extract its description, requirements, and dependencies
- `specs/$ARGUMENTS.md` — the PRD (may not exist yet)

---

## Step 2 — Create the PRD if it does not exist

If `specs/$ARGUMENTS.md` does not exist, create it using the roadmap entry for $ARGUMENTS as the source of truth.
Populate it with whatever can be derived from the roadmap (overview, initial user journeys, functional requirements, non-functional requirements, ACs).
Leave gaps where the roadmap is silent — those become questions in Step 5.

Use the document structure defined at the bottom of these instructions.

If the file already exists, skip this step and proceed to Step 3.

---

## Step 3 — Reconcile answered questions

Read the **Open Questions** section of `specs/$ARGUMENTS.md`.

For every question whose **Answer** field is filled in:

1. Interpret what the answer implies for the PRD.
2. Add the implication as one or more entries in the appropriate section:
   - A behavioral statement → Functional Requirement (F-x)
   - A quality or constraint → Non-Functional Requirement (NF-x)
   - A user scenario → User Journey (UJ-x)
   - A verifiable outcome → Acceptance Criterion (AC-x)
3. Assign the next available ID in that section.
4. Remove the answered question from the Open Questions section entirely.
5. Record what was reconciled in the Refinement Log entry you will write in Step 6.

If there are no answered questions, skip this step and proceed to Step 4.

---

## Step 4 — Assess confidence

Analyse the current state of `specs/$ARGUMENTS.md` and assign a confidence level (0–100%) that reflects how completely and unambiguously the PRD captures the epic's requirements.

Use this rubric:

| Dimension | Questions to ask |
|-----------|-----------------|
| Roadmap coverage | Does every requirement listed in the roadmap for $ARGUMENTS have matching PRD content? |
| User journey completeness | Are all meaningful user-facing paths represented? |
| Functional requirement specificity | Are requirements unambiguous and individually testable? |
| Non-functional requirements | Are quality constraints (latency, reliability, resource use, etc.) defined where relevant? |
| AC testability | Can each AC be verified with a concrete test? |
| Open ambiguities | How many unresolved questions or unclear statements remain? |

Write a short plain-English explanation of what is missing or ambiguous that justifies the score.

Update the **Confidence Level** line in the document with the new score and explanation.

---

## Step 5 — Add new questions if confidence < 90%

If confidence is below 90%, generate new open questions that address the most critical gaps identified in Step 4.

Rules for questions:
- Focus on the gaps that would most increase confidence when answered.
- Each question must be specific and answerable.
- Provide 2–4 concrete options per question.
- Mark exactly one option as *(recommended)* and give a one-line reason.
- Leave the **Answer** field empty for the user to fill in.
- Assign sequential Q-numbers continuing from any existing ones.

If confidence is 90% or above, do not add questions. State that the PRD is complete.

---

## Step 6 — Update the Refinement Log

Append one new log entry at the bottom of the Refinement Log section. Keep log entries compact — one entry per cycle, no question body text, just IDs and outcomes.

Format:

```
### Cycle N — Confidence: X%
- Reconciled: Q1 → F-3 (startup timeout), Q2 → NF-2 (idle resource constraint)
- Added: Q4 (shutdown signal behaviour), Q5 (observability mechanism)
```

If nothing was reconciled and nothing was added, still append the entry with the updated confidence score and a note explaining why no changes were made.

---

## Step 7 — Save the file

Write the fully updated document back to `specs/$ARGUMENTS.md`.

---

## Document structure

Maintain sections in this exact order:

```
# $ARGUMENTS · {Epic Title} — PRD

## Overview
{One short paragraph from the roadmap description.}

**Confidence Level:** X% — {one-sentence explanation of what is missing}

---

## User Journeys
### UJ-N · {Title}
{Narrative description of the journey.}

---

## Functional Requirements
| ID | Requirement |
|----|-------------|
| F-1 | ... |

---

## Non-Functional Requirements
| ID | Requirement |
|----|-------------|
| NF-1 | ... |

---

## Acceptance Criteria
| ID | Given | When | Then |
|----|-------|------|------|
| AC-1 | ... | ... | ... |

---

## Open Questions
{Only unanswered questions live here. Answered questions are removed after reconciliation.}

---

## Refinement Log
{One compact entry per cycle, oldest first.}
```

---

## Open question format

```
### QN · {Short descriptive title}

{The question in one or two sentences.}

**Options**
- A. {Option} — {brief rationale}
- B. {Option} — {brief rationale} *(recommended — {one-line reason})*
- C. {Option} — {brief rationale}

**Answer:**
```
