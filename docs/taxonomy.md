# Misconception taxonomy

The versioned label set for synthetic student errors, owned by
[#6](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/6) (Linear
[RM-2489](https://linear.app/rpd-34/issue/RM-2489)). A tutoring dataset without a
misconception label teaches "respond to a wrong answer"; with one it teaches
diagnosis. Every planted error in a dataset row carries exactly one primary ID
from this taxonomy — "generic wrong answer" is not a label.

## Where it lives

- `schemas/taxonomy.yaml` — the data: one entry per misconception, in the entry
  format below. This is the source of truth for the *content* of each label.
- `tutor_schema::MisconceptionId` (`crates/tutor-schema`) — the same IDs as a
  serde enum, so the simulator, problem families, and eval validate labels
  against one list. Serialized as the dotted ID string.
- `crates/tutor-schema/tests/taxonomy_sync.rs` — keeps the two in sync (no YAML
  dependency: it checks the `id:` lines and required fields textually).

## ID format and versioning

IDs are `<area>.<snake_case_name>` with area one of `algebra`, `functions`,
`trig`, `limits`, `derivatives`, `integrals`. The pre-pivot seed list used
`area:kebab-name`; v1 normalizes to the dotted form used by curriculum topic IDs.

Rules, also in the `taxonomy.yaml` header:

- An id, once used in a published dataset row, never changes meaning.
- Split or rename an entry only by adding a new id and marking the old one
  `status: deprecated`. Deprecated ids stay parseable forever.
- New ids are added only with a worked wrong example. No example, no id.
- Bump `version` on every change to the file; `TAXONOMY_VERSION` in
  `tutor-schema` must match.

## Entry format

| Field | Content |
|---|---|
| `id` | Stable namespaced ID. |
| `status` | `active` or `deprecated`. |
| `error_pattern` | The faulty reasoning, one sentence — the pattern, not the topic name. |
| `bites_at` | The step in the student's work where the error bites. |
| `wrong_attempt` | A minimal worked example of the error (student work + correct result). |
| `correct_reasoning` | The right reasoning in one or two lines. |
| `diagnostic_question` | What a tutor should ask to surface the misconception. |
| `first_hint` | Hint-ladder level 1: the first hint a tutor should give. |
| `topics` | At least two curriculum topic IDs where the error can appear. |

## v1 scope

Twenty entries: 4 algebra, 3 functions, 2 trig, 3 limits, 5 derivatives, 3
integrals. The seed list's `stats:variance-vs-std` and two `linear-algebra`
entries are deferred — they are outside the math-first curriculum scope of the
pilot and can return as new ids when those topics land (stats and linear algebra
are not seeded areas yet).
