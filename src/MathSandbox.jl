"""
    MathSandbox

A hands-on Julia learning sandbox inspired by the mathematics of Limen-Neural's
neuromorphic computing ecosystem.

## Modules

| Module area     | Functions                                                    |
|-----------------|--------------------------------------------------------------|
| **Neurons**     | `lif_step`, `simulate_lif`, `firing_rate`                    |
| **STDP**        | `stdp_delta_w`, `apply_stdp`                                 |
| **Stochastic**  | `gbm_step`, `simulate_gbm`, `ou_step`, `simulate_ou`         |
| **Spike trains**| `poisson_spike_train`, `inter_spike_intervals`, `cv_isi`     |
| **Signals**     | `compute_hurst`, `compute_hawkes`, `compute_gbm_surprise`    |
| **Kelly**       | `kelly_fraction`, `half_kelly`, `full_kelly`, `confidence_to_kelly` |

## Quick Start

```julia
using MathSandbox

# Simulate a LIF neuron for 200 ms with 3.5 nA input
p  = LIFParams()
ts, vs, spks = simulate_lif(3.5, 200.0, p)
println("Fired \$(length(spks)) spikes")

# Generate a Poisson spike train at 40 Hz
spikes = poisson_spike_train(40.0, 1000.0)

# Extract SNN features from a price series
signal = cumsum(randn(100)) .+ 100.0
f = snn_features(signal)

# Compute optimal bet size
frac = kelly_fraction(win_rate=0.55, avg_win=8.50, avg_loss=5.20)
```

See the `sandbox/` directory for step-by-step worked examples covering each
topic in depth.
"""
module MathSandbox

using Statistics

include("neurons.jl")
include("stochastic.jl")
include("signals.jl")
include("kelly.jl")

# ── Neurons & STDP ────────────────────────────────────────────────────────────
export LIFParams, lif_step, simulate_lif, firing_rate
export STDPParams, stdp_delta_w, apply_stdp

# ── Stochastic Processes ──────────────────────────────────────────────────────
export gbm_step, simulate_gbm, gbm_expected_value
export OUParams, ou_step, simulate_ou, ou_stationary_std
export poisson_spike_train, inter_spike_intervals, cv_isi

# ── Signal Analysis ───────────────────────────────────────────────────────────
export compute_hurst, compute_hawkes, compute_gbm_surprise, snn_features

# ── Kelly Criterion ───────────────────────────────────────────────────────────
export kelly_fraction, half_kelly, full_kelly, confidence_to_kelly
export expected_log_growth, simulate_kelly_growth

end # module
