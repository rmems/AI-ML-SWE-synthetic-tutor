# legacy/

The pre-pivot `NeuroMathPlayground.jl` package, kept as-is so its history and examples stay usable. Nothing here feeds the dataset pipeline; new work goes in `crates/` (Rust) and `math-julia/` (Julia math worker). Moved with `git mv` in [RM-2488](https://linear.app/rpd-34/issue/RM-2488) / [#5](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/5), so `git log --follow` still shows each file's history.

## What moved

| Before | After |
|---|---|
| `Project.toml`, `Manifest.toml` | `legacy/NeuroMathPlayground/Project.toml`, `Manifest.toml` |
| `src/` (LIF/STDP neurons, stochastic processes, signals, Kelly, neuro helpers, math, GPU fallback) | `legacy/NeuroMathPlayground/src/` |
| `test/` | `legacy/NeuroMathPlayground/test/` |
| `examples/`, `sandbox/`, `notebooks/` | `legacy/NeuroMathPlayground/examples/`, `sandbox/`, `notebooks/` |

## What was reused

The finite-difference calculus and vector helpers in `src/math.jl` (`gradient_approx`, `hessian_approx`, `softmax`, `normalize_vec`) are the only part useful for problem generation. They were copied, not moved, into `math-julia/src/numerics.jl` (package `TutorMath`), where they cross-check generated answers. Their tests were copied too. The originals stay here so the legacy package still runs unchanged.

## What stayed out, and why

The neuron, stochastic, signal, Kelly, and GPU code has no role in tutoring-dataset generation, so it was not carried forward. It stays runnable for reference:

```sh
julia --project=legacy/NeuroMathPlayground -e 'using Pkg; Pkg.instantiate(); Pkg.test()'
```
