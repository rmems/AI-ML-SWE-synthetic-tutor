"""
    NeuroMathPlayground

A personal Julia playground for mathematics and neuromorphic computing.

Load it in the REPL with `using NeuroMathPlayground`, then call `welcome()` for
a quick-start guide or `repl_help()` to list every available function.

## Areas covered

| Area              | Key functions                                          |
|-------------------|--------------------------------------------------------|
| **Neurons / STDP**| `lif_step`, `simulate_lif`, `stdp_delta_w`             |
| **Neuro utils**   | `spike_threshold`, `spike_kernel`, `encode_rate`       |
| **Stochastic**    | `gbm_step`, `simulate_gbm`, `ou_step`, `simulate_ou`   |
| **Spike trains**  | `poisson_spike_train`, `inter_spike_intervals`         |
| **Signals**       | `compute_hurst`, `compute_hawkes`, `snn_features`      |
| **Kelly**         | `kelly_fraction`, `half_kelly`, `full_kelly`           |
| **Math utils**    | `gradient_approx`, `softmax`, `normalize_vec`          |
| **GPU**           | `gpu_info`, `gpu_available`, `cuda_kernel_demo`        |

## Quick start

```julia
using NeuroMathPlayground

welcome()           # print banner + tips
repl_help()         # list all functions

lif_demo()          # LIF neuron walkthrough
math_demo()         # matrix and calculus demos
gpu_info()          # GPU detection

# Simulate a LIF neuron
p = LIFParams()
ts, vs, spks = simulate_lif(3.5, 200.0, p)
println("Fired \$(length(spks)) spikes")
```
"""
module NeuroMathPlayground

using LinearAlgebra
using Statistics
using Random

include("neurons.jl")
include("stochastic.jl")
include("signals.jl")
include("kelly.jl")
include("neuro.jl")
include("math.jl")
include("gpu.jl")

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

# ── Neuromorphic utilities ────────────────────────────────────────────────────
export spike_threshold, lif_activation, spike_kernel
export encode_rate, rate_to_phase, lif_demo, neuro_demo

# ── Math utilities ────────────────────────────────────────────────────────────
export gradient_approx, hessian_approx, softmax, normalize_vec
export mat_demo, math_demo

# ── GPU utilities ─────────────────────────────────────────────────────────────
export gpu_available, gpu_info, cuda_kernel_demo

# ── REPL helpers ──────────────────────────────────────────────────────────────
export welcome, repl_help

const _BANNER = """
╔══════════════════════════════════════════════════════════════╗
║         NeuroMathPlayground.jl  —  v0.1                     ║
║   Julia playground for math & neuromorphic computing        ║
╠══════════════════════════════════════════════════════════════╣
║  📐 Math:   mat_demo()   gradient_approx(f,x)  softmax(v)  ║
║  🧠 Neuro:  lif_demo()   spike_threshold(x)    neuro_demo() ║
║  🖥️  GPU:   gpu_info()   cuda_kernel_demo()                 ║
║  ❓ Help:   repl_help()   (lists everything)                 ║
╚══════════════════════════════════════════════════════════════╝
"""

"""
    welcome()

Print the NeuroMathPlayground welcome banner with quick-start tips.
"""
function welcome()
    println(_BANNER)
end

"""
    repl_help()

Print a reference sheet of all available functions grouped by topic.
"""
function repl_help()
    println("""
NeuroMathPlayground — function reference
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  GPU
    gpu_available()                     → Bool
    gpu_info()                          → print GPU / CUDA info
    cuda_kernel_demo()                  → vector-add kernel demo

  Math utilities
    mat_demo()                          → matrix multiply & decomposition demo
    math_demo()                         → gradient, softmax, normalise demo
    gradient_approx(f, x; h=1e-5)      → central-difference gradient
    hessian_approx(f, x; h=1e-4)       → numerical Hessian matrix
    softmax(v)                          → softmax of a vector
    normalize_vec(v)                    → L2-normalise a vector

  Neuromorphic utilities
    lif_demo()                          → full LIF simulation walkthrough
    neuro_demo()                        → spike functions + rate coding demo
    spike_threshold(x, θ=0.0)          → Heaviside threshold activation
    lif_activation(x; gain=1.0)        → soft LIF activation (rectified log)
    spike_kernel(dt, τ=20.0)           → exponential post-synaptic kernel
    encode_rate(x, lo, hi, T=1000.0)   → rate-code scalar x as spike train
    rate_to_phase(rate, T=1000.0)      → convert Hz rate to first-spike phase

  LIF neuron
    LIFParams(; tau_m, v_rest, ...)    → neuron parameter struct
    lif_step(v, I, p, dt)             → single Euler step
    simulate_lif(I, T, p; dt)         → full simulation
    firing_rate(spikes, T)             → mean firing rate in Hz

  STDP
    STDPParams(; A_plus, tau_plus, …)  → STDP parameter struct
    stdp_delta_w(Δt, p)               → weight change for spike-time gap Δt
    apply_stdp(w, Δt, p)              → update + clamp synaptic weight

  Stochastic processes
    gbm_step(S, μ, σ, dt)             → one GBM step (exact log-normal)
    simulate_gbm(S0, μ, σ, T, dt)    → full GBM path
    gbm_expected_value(S0, μ, T)      → analytical E[S(T)]
    OUParams(; mu, kappa, sigma)       → OU parameter struct
    ou_step(x, p, dt)                 → one OU step
    simulate_ou(x0, p, T, dt)         → full OU path
    ou_stationary_std(p)              → σ / √(2κ)
    poisson_spike_train(rate, T)      → Poisson spike times (ms)
    inter_spike_intervals(spikes)     → ISI vector
    cv_isi(spikes)                    → coefficient of variation

  Signal analysis
    compute_hurst(signal)              → Hurst exponent H ∈ [0.2, 0.8]
    compute_hawkes(signal)             → Hawkes intensity proxy
    compute_gbm_surprise(signal)       → GBM surprise Z-score
    snn_features(signal)              → NamedTuple(hurst, hawkes, gbm_surprise)

  Kelly criterion
    kelly_fraction(win_rate, avg_win, avg_loss)  → half-Kelly fraction
    half_kelly(p, b)                   → half-Kelly given p and payoff b
    full_kelly(p, b)                   → full-Kelly (educational)
    expected_log_growth(p, b, f)       → G(f) = p·log(1+bf) + (1-p)·log(1-f)
    simulate_kelly_growth(p, b, f, n)  → bankroll path
    confidence_to_kelly(c)             → SNN confidence → half-Kelly fraction

Run welcome() to show the banner again.
""")
end

# Print the banner automatically when the module is loaded interactively.
function __init__()
    if isinteractive()
        println(_BANNER)
    end
end

end # module NeuroMathPlayground
