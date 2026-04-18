# MathSandbox.jl

A hands-on Julia learning sandbox built around the mathematics that powers
[Limen-Neural](https://github.com/Limen-Neural)'s neuromorphic computing ecosystem.
Learn Julia programming while working through the same mathematical foundations
used in production spiking neural network systems.

---

## What You Will Learn

| Area | Math | Julia concepts |
|------|------|----------------|
| **LIF Neurons** | ODE integration, threshold dynamics | structs, multiple dispatch, `@printf` |
| **STDP** | Hebbian learning rule, exponential decay | keyword functions, broadcasting |
| **Stochastic Processes** | GBM, Ornstein-Uhlenbeck, Itô calculus | random numbers, comprehensions |
| **Spike Trains** | Poisson processes, ISI statistics | vectors, `push!`, `filter` |
| **Signal Analysis** | Hurst exponent, Hawkes intensity, Z-scores | `Statistics` stdlib, closures |
| **Kelly Criterion** | Expected log-growth, optimal sizing | anonymous functions, `map` |

---

## Quick Start

```julia
# 1. Clone and activate the project
julia --project=.

# 2. Instantiate dependencies
julia> using Pkg; Pkg.instantiate()

# 3. Use the library directly
julia> using MathSandbox

# Simulate a LIF neuron for 200 ms with 3.5 nA input current
julia> p  = LIFParams()
julia> ts, vs, spks = simulate_lif(3.5, 200.0, p)
julia> println("Fired $(length(spks)) spikes at $(round(firing_rate(spks, 200.0))) Hz")

# Generate a Poisson spike train at 40 Hz for 1 second
julia> spikes = poisson_spike_train(40.0, 1000.0)

# Extract SNN features from a signal
julia> signal = cumsum(randn(200)) .+ 100.0
julia> f = snn_features(signal)
julia> println("H=$(f.hurst)  λ=$(f.hawkes)  z=$(f.gbm_surprise)")

# Compute optimal Kelly position size
julia> frac = kelly_fraction(win_rate=0.55, avg_win=8.50, avg_loss=5.20)
```

---

## Sandbox Notebooks

Step-by-step worked examples. Run each with `julia --project=. sandbox/NN_name.jl`:

| File | Topic |
|------|-------|
| `sandbox/01_julia_basics.jl` | Julia syntax: types, functions, structs, dispatch |
| `sandbox/02_lif_neuron.jl` | LIF dynamics, F-I curve, STDP learning rule |
| `sandbox/03_stochastic_processes.jl` | GBM paths, OU mean-reversion, Poisson trains |
| `sandbox/04_signal_analysis.jl` | Hurst, Hawkes, GBM surprise — SNN feature pipeline |
| `sandbox/05_kelly_sizing.jl` | Kelly criterion, bankroll growth, SNN confidence mapping |
| `sandbox/06_stdp_learning.jl` | STDP window, competitive learning, LIF+STDP circuit |

---

## Module Reference

```
MathSandbox
├── neurons.jl      — LIF neuron ODE, spike generation
│   LIFParams · lif_step · simulate_lif · firing_rate
│   STDPParams · stdp_delta_w · apply_stdp
│
├── stochastic.jl   — Stochastic process math
│   gbm_step · simulate_gbm · gbm_expected_value
│   OUParams · ou_step · simulate_ou · ou_stationary_std
│   poisson_spike_train · inter_spike_intervals · cv_isi
│
├── signals.jl      — Time-series signal analysis
│   compute_hurst · compute_hawkes · compute_gbm_surprise
│   snn_features
│
└── kelly.jl        — Optimal position sizing
    kelly_fraction · half_kelly · full_kelly
    expected_log_growth · simulate_kelly_growth
    confidence_to_kelly
```

---

## Mathematical Foundations

### LIF Neuron ODE
```
τ_m · dV/dt = -(V - V_rest) + R_m · I(t)
```
Euler discretisation: `V(t+Δt) = V(t) + Δt/τ_m · [-(V-V_rest) + R_m·I]`

### STDP Learning Rule (Bi & Poo 1998)
```
ΔW =  A_+ · exp(-|Δt| / τ_+)   if Δt ≥ 0  (LTP)
ΔW = -A_- · exp(-|Δt| / τ_-)   if Δt < 0  (LTD)
```

### Geometric Brownian Motion
```
dS = μ·S·dt + σ·S·dW
S(T) = S₀ · exp((μ - σ²/2)·T + σ·√T·Z)   Z ~ N(0,1)
```

### Ornstein-Uhlenbeck Process
```
dX = κ·(μ - X)·dt + σ·dW
Stationary: X_∞ ~ N(μ, σ²/(2κ))
```

### Hurst Exponent (R/S Analysis)
```
H = log(R/S) / log(n)
H > 0.5 → trending  |  H ≈ 0.5 → random walk  |  H < 0.5 → mean-reverting
```

### Kelly Criterion (Kelly 1956)
```
F* = (p·b - (1-p)) / b        full Kelly
F  = F*/2                       half Kelly (practical)
G(f) = p·log(1+b·f) + (1-p)·log(1-f)   expected log-growth
```

---

## Running the Tests

```julia
julia --project=. test/runtests.jl
```

---

## Part of the Limen-Neural Ecosystem

This sandbox uses the same mathematical building blocks as:

| Library | Purpose |
|---------|---------|
| [SpikenautSignals.jl](https://github.com/Limen-Neural/SpikenautSignals.jl) | Streaming feature extraction (Hurst, Hawkes, GBM) |
| [SpikenautKelly.jl](https://github.com/Limen-Neural/SpikenautKelly.jl) | Half-Kelly position sizing from SNN confidence |
| [SpikenautLSM.jl](https://github.com/Limen-Neural/SpikenautLSM.jl) | GPU-accelerated Liquid State Machine |
| [SpikenautNero.jl](https://github.com/Limen-Neural/SpikenautNero.jl) | Multi-lobe relevance scoring |
| [neuromod](https://github.com/Limen-Neural/neuromod) | LIF + Izhikevich + STDP + dopamine (Rust) |
