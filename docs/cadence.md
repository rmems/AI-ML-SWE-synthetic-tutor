# Batch cadence and the generation run log

The dataset-factory equivalent of the pre-pivot study log and weekly plan
([issue #7](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/7)).
Batches are produced on a stated cadence, and every run leaves a
machine-written log entry complete enough to reproduce the batch or reject
it.

## The run log

Every pipeline command that commits a batch appends one entry to
`run-log.jsonl` next to the batch output (the default; `--run-log <path>`
overrides it). Today that means `tutor-generate run` (`generate` stage) and
`tutor-generate simulate` (`simulate` stage). An entry is written whenever a
bundle is committed — including a run that verified nothing — and never
otherwise.

- **Format:** one JSON object per line, `log_version` 1 — the contract is
  [`schemas/run-log.schema.json`](../schemas/run-log.schema.json), generated
  from `tutor_runlog::RunLogEntry`. Each `run` entry carries the run id and
  date, the pipeline's git commit and Cargo/rustc versions, the Julia version
  when the math worker answered `ping`, the family and generation spec it ran
  under, generator names + versions + seeds + parameter ranges, the teacher
  and its rights-registry entry when a teacher lane was used, counts measured
  from the written output (generated / verified / rejected per reason code /
  deduplicated / decontaminated / exported), the split manifest hash, and the
  QA sample summary. Fields a run could not have known are `null` with a
  documented meaning — never invented; counts are never estimated.
- **Append-only with a hash chain:** appending never rewrites earlier lines.
  Each entry records `prev_entry_sha256`, the SHA-256 of the previous
  serialized line, so an edited, reordered, or mid-truncated log fails
  `tutor-runlog verify <log>` (also available as
  `tutor_runlog::RunLog::entries`, which refuses a partial read). Appending
  itself refuses to extend a log whose chain does not verify, holds a
  `<log>.lock` lock file so two writers sharing a log cannot interleave
  (a stale lock from a killed append is removed manually), and is proven
  out before a batch replaces the previous one: a `--run-log` path that
  aliases a batch artifact, or a `replaces` naming no earlier run, fails
  before anything is overwritten.
- **Reruns:** a rerun is a new entry. When it supersedes an earlier batch it
  names that run in `replaces` — passed explicitly via `--replaces <run-id>`
  (or `RunConfig::replaces`), never inferred by the log, so a new batch is
  never mislabeled as a replacement. `replaces` must resolve to an earlier
  run in the same log, the replaced entry is never edited, and run ids are
  unique — a duplicate is rejected.
- **A batch with no log entry does not exist** for release purposes, however
  good its rows look.

## The cadence

One family at a time runs to a release; no parallel half-finished families.
Per release, in this order:

1. **Generation** — `tutor-generate run` produces candidates; each is
   verified independently before `verified/` is written (the generation and
   verification gates travel together in one logged run).
2. **Verification** — already embedded in the generation run; for later
   stages it means every produced row replays its own verification record.
3. **QA sample** — sampled human review with recorded verdicts (#21).
4. **Eval spot-check** — the batch against the held-out eval set.
5. **Export** — dedup, decontamination, frozen splits, and the family
   release (#8, #21).

Each step is logged before the next starts: a step with no entry in the
batch's `run-log.jsonl` has not happened for release purposes.

## Gate failures pause the family

A failed quality gate pauses the cadence for that family. The pause is
recorded in the same log as a `gate_pause` entry carrying the family, the
gate (`generation`, `verification`, `qa_sample`, `eval_spot_check`,
`export`), the triggering `run_id` when one exists, and the evidence:

```sh
cargo run -p tutor-runlog -- pause datasets/<family>/run-log.jsonl \
  --family <family> --gate verification \
  --detail "rejection rate above the family gate" \
  --run-id <failing-run-id> \
  --evidence verify_answer_mismatch=12
```

The family stays paused until the fix lands as a **new run** whose entry
names the failed run via `--replaces`. A logged run is never edited —
history stays as written, fixes append.
