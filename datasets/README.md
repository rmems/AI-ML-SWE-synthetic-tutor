# datasets/

Build outputs, one folder per dataset family (`datasets/<family>/`), each produced by one generator config and carrying one dataset-card draft. Rows are only written by `crates/tutor-build` after verification, and every row carries provenance (generator, seed, runtime versions, verifier). Generated rows are gitignored; only `README.md` and dataset-card drafts (`datasets/<family>/CARD.md`) are committed. `datasets/gold/` holds the hand-written gold exemplar rows, which are committed. Never write rows to the repo root or to notes folders.
