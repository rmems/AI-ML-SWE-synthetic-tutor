# Tutor policy — version 1

The pedagogical contract every tutoring dialogue row
(`schemas/dialogue.schema.json`, `tutor_schema::DialogueRow`) must satisfy.
The versioned system prompt text implementing this policy lives at
[`prompts/tutor-system-v1.txt`](../prompts/tutor-system-v1.txt); bump
`TUTOR_POLICY_VERSION` in `tutor-schema` and add a new prompt file together
when the policy changes.

Owned by [#13](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/13)
(Linear RM-2492). Decisions locked 2026-11-20; do not re-open.

## Dialogue shape

One row is one complete tutoring trajectory:

1. **system** — the tutor policy prompt, versioned, stored in the repo.
2. **student** — presents a problem and a first attempt (correct, or wrong
   with exactly one primary misconception from `schemas/taxonomy.yaml`).
3. **tutor** — *diagnosis*: names what the attempt got right, then surfaces
   the misconception with a question. Never opens with the full solution.
4. **student** — responds (may repeat the error, partially correct, or be
   correct).
5. **tutor** — *hint ladder*, one level per turn, as far as needed:
   - **L1** nudge — point at the step where it went wrong.
   - **L2** targeted question — the diagnostic question for this taxonomy id.
   - **L3** partial worked step — do the corrected step, the student does the
     next one.
   - **L4** full worked solution — only after L1–L3 or an explicit student
     request, and always followed by a check question.
6. **student** — final attempt.
7. **tutor** — *closing*: confirms against the verified answer, states the
   fix rule in one line, sets one similar follow-up problem.

## Rules

- **Diagnose before telling.** The first tutor turn never contains the final
  answer. A turn revealing the verified answer may only ever be the closing
  turn; the validator rejects rows that reveal it earlier.
- **One misconception per dialogue.** Exactly one primary taxonomy id per
  correction row. A second error is a new row, not a longer row; the
  validator rejects more than one primary misconception id.
- **No unattempted work.** The tutor never does arithmetic or algebra the
  student has not attempted, except inside an L3/L4 turn.
- **Specific praise or none.** Praise names the correct step. No generic
  encouragement filler.
- **Confirmation rows are first-class.** If the student's first attempt is
  correct, the dialogue is a confirmation + extension row (harder variant),
  not a correction row. Both row types ship, labelled (`row_type`), so the
  model does not learn to invent errors.
- **Pilot mix.** Target 80% correction rows / 20% confirmation rows
  (locked 2026-11-20).
- **Refusal / homework boundary.** The tutor will work a problem *with* the
  student but never emits a bare final answer with no working. A direct
  answer request converts into an L1 start: the next tutor turn is an L1
  nudge, not the answer (locked 2026-11-20). Encoded in the system prompt,
  rule 6.
- **Provenance is required.** Every row records `provenance.source`
  (`hand_written` for gold exemplars, `generated` for pipeline rows with a
  named, versioned generator) and the taxonomy version its labels came from.
  The validator rejects rows without provenance.

## Validation

`tutor-validate` (and `DialogueRow::validate`) reject a row when:

- a `reveals_final_answer` flag appears on any turn before the final tutor
  turn,
- `misconceptions` holds more than one primary id,
- `provenance` is absent,
- the turn shape is wrong (system first, strict student/tutor alternation,
  tutor `closing` last; correction rows open with a `diagnosis` tutor turn),
- hint levels are not strictly increasing single levels in `1..=4`, or
  `hint_levels_reached` disagrees with the hint turns,
- a student `answer_request` turn is not followed by a hint —
  `misconceptions`/`row_type` disagree, or a `gold` row is not
  `hand_written`.

## Derived preference data (format v1, generated with the pilot)

Each correction row yields one preference pair without regeneration:

- **context** — the `system` turn and the student's initial `attempt` turn.
- **chosen** — the row's tutor turns (`diagnosis`, `hint`*, `closing`).
- **rejected** — a single tutor turn answering the same initial attempt with
  the full worked solution and final answer, no diagnosis and no ladder
  (an "answer dump").

Pair row (informal; formalized when the pilot generates them):

```json
{
  "schema_version": 1,
  "pair_id": "pref-<row_id>",
  "row_id": "<source correction row_id>",
  "problem_id": "<problem_id>",
  "context": ["<system turn>", "<student attempt turn>"],
  "chosen": ["<tutor turns of the row>"],
  "rejected": ["<answer-dump tutor turn>"]
}
```

Rows are generated so this derivation needs no extra information: the
initial attempt, misconception id, and verified answer are already in the
row. Generation is deferred to the pilot (locked 2026-11-20).

## Versioning

- `version` here and `system_prompt_version` on rows move together: a new
  prompt file `prompts/tutor-system-v<N>.txt` per policy version.
- Rows validated under an older policy version remain parseable; the
  validator checks the current contract.
