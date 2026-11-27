# curriculum/

The topics the datasets must cover and how far each is covered.
[coverage-matrix.md](coverage-matrix.md) ([#16](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/16))
is the coverage index: one row per curriculum topic ID (the `topic` field of
`schemas/problem.schema.json` — dotted IDs, fixed by the matrix) with domain,
prerequisites, available misconception ids, problem-generator status, target /
generated rows, and held-out eval items. Every row has a status (`generator +
verification`, `planned`, `specced`, `unscoped`), and every family/slice issue
is linked both ways there. Generation specs may only target topics that exist
in the matrix, and generated/held-out counts come from split manifests (#8),
not hand edits — so gaps are visible before generation runs.
