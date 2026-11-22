# datasets/gold/

Hand-written exemplar dialogue rows (`gold: true`, `provenance.source:
"hand_written"`) that seed each dataset family and pin the
`dialogue.schema.json` contract. Every file here must pass `tutor-validate`;
a test in `crates/tutor-schema` validates them on each build.

- `gold-derivatives-chain-rule.json` — correction row, `derivatives.chain_rule`, ladder reaches L1.
- `gold-algebra-sign-error.json` — correction row, `algebra.sign_error`, ladder reaches L2.
- `gold-confirmation.json` — confirmation row (correct first attempt), no hints.
