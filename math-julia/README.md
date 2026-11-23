# math-julia/

`TutorMath`, the Julia math worker. It generates seeded problems with exact answers and serves them to the Rust pipeline over ZMQ REQ/REP using the `tutor-math/1` JSON protocol (see `src/protocol.jl`); it never writes dataset rows, and every answer is re-verified on the other side of the boundary — Julia-made problems by `tutor-verify` in Rust, Rust-made problems by this worker's `verify_answer` op. Run it with `julia --project=math-julia math-julia/bin/serve.jl` (binds `tcp://127.0.0.1:5557`, override with `TUTOR_MATH_ENDPOINT`) and test it with `julia --project=math-julia -e 'using Pkg; Pkg.test()'`.

## Generators and the Rust/Julia split

Each family declares where its math runs in `tutor-generate`'s registry (`crates/tutor-generate/src/lib.rs`).

| Generator | Math lives in | Why |
|---|---|---|
| `polynomial_derivative_at_point` | Julia (`generators.jl`) | polynomial rendering + differentiation |
| `product_rule_at_point` | Julia (`generators.jl`) | two draws + symbolic differentiation per factor |
| `chain_rule_at_point` | Julia (`generators.jl`) | inner evaluation + symbolic differentiation |
| `binomial_derivative_at_point` | Rust (`crates/tutor-generate`) | two-term power rule needs no symbolic layer |

## Ops

- `ping` — worker identity, Julia version, served generator names.
- `generate_problem` — `{generator, seed}` → one `problem.schema.json` problem.
- `verify_answer` — `{problem}` → `{verified, reason, detail, expected}`: the Julia-side independent re-check used for Rust-generated problems.
