# Quadratic secant slopes: local proof

This is the next proof step for the
[derivative-definition slice (#15)](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/15),
following its taxonomy in #47. It makes no tutor dialogues or dataset records.

Run from the repository root with Rust and Julia:

```sh
cargo test --locked -p tutor-generate --test quadratic_secant_proof -- --include-ignored
```

The verifier only needs Julia's standard runtime. Set `JULIA_EXE` to its
executable if it is not already on your PATH. Nothing runs a model, writes
training data, or adds secant to the production generator registry.

The Rust fixture draws seeded nondegenerate quadratics and builds four
shrinking nonzero steps on each side. Each slope and its absolute error are
recorded as exact fractions. The limit witness is the constant plus linear
term of the difference quotient: the linear term tends to zero as the step
tends to zero. Julia independently evaluates the original quadratic at
each step, recomputes the limit and linear term by central differences, and
rejects altered slopes, errors or invalid step grids. The proof is wired
only to the test and passes numeric frames to Julia over local standard
input; it does not add a production wire operation.

This proof does not authorize a derivative-definition generation batch.
An approved scope/spec, production schema and verification wiring, the
student and dialogue layers, held-out checks, and QA remain separate.
