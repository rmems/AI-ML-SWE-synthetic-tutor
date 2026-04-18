# NeuroMathPlayground.jl

A personal Julia playground for mathematics and neuromorphic computing.
Practice Julia in the REPL while exploring the same math that powers
[Limen-Neural](https://github.com/Limen-Neural)'s spiking neural network
ecosystem — from LIF neurons and STDP to GPU-accelerated kernels.

---

## What is this?

| Area | What you get |
|------|-------------|
| **REPL playground** | Load the module, see a banner, call `repl_help()` for a live reference sheet |
| **Neuromorphic math** | LIF neuron ODE, STDP learning rule, spike kernels, rate/phase coding |
| **General math** | Numerical gradient & Hessian, softmax, L2 normalise, matrix demos |
| **Stochastic processes** | GBM, Ornstein-Uhlenbeck, Poisson spike trains |
| **Signal analysis** | Hurst exponent, Hawkes intensity, GBM surprise Z-score |
| **Kelly criterion** | Optimal position sizing from SNN confidence scores |
| **GPU acceleration** | CUDA.jl integration with graceful CPU fallback |
| **Plotting** | Plots.jl examples (optional) |
| **Notebooks** | Pluto.jl starter notebook |

---

## Quick Start

```julia
# 1. Clone and open a project REPL
git clone https://github.com/rmems/NeuroMathPlayground.jl
cd NeuroMathPlayground.jl
julia --project=.

# 2. Instantiate dependencies (first time only)
julia> using Pkg; Pkg.instantiate()

# 3. Load the module — banner prints automatically
julia> using NeuroMathPlayground

# 4. Explore
julia> repl_help()      # full function reference
julia> lif_demo()       # LIF neuron walkthrough
julia> math_demo()      # gradient, softmax, normalise
julia> neuro_demo()     # spike functions and rate coding
julia> gpu_info()       # GPU detection
```

---

## REPL Mode

When you `using NeuroMathPlayground`, a welcome banner prints automatically:

```
╔══════════════════════════════════════════════════════════════╗
║         NeuroMathPlayground.jl  —  v0.1                     ║
║   Julia playground for math & neuromorphic computing        ║
╠══════════════════════════════════════════════════════════════╣
║  📐 Math:   mat_demo()   gradient_approx(f,x)  softmax(v)  ║
║  🧠 Neuro:  lif_demo()   spike_threshold(x)    neuro_demo() ║
║  🖥️  GPU:   gpu_info()   cuda_kernel_demo()                 ║
║  ❓ Help:   repl_help()   (lists everything)                 ║
╚══════════════════════════════════════════════════════════════╝
```

Useful REPL shortcuts:

```julia
welcome()               # reprint the banner
repl_help()             # full function reference (grouped by topic)

lif_demo()              # step-by-step LIF walkthrough with F-I curve
neuro_demo()            # spike functions, kernels, rate/phase coding
math_demo()             # gradient, softmax, normalise demos
mat_demo()              # matrix multiply, SVD, eigendecomposition
gpu_info()              # detect and describe GPU
cuda_kernel_demo()      # vector-add on GPU (or CPU fallback)
```

---

## Running GPU Examples

The playground uses **CUDA.jl** for GPU acceleration. It must be installed
separately so the core module works offline without a GPU.

```julia
# 1. Install CUDA.jl (once)
julia --project=. -e 'using Pkg; Pkg.add("CUDA")'

# 2. Load in a REPL session
julia> using CUDA
julia> using NeuroMathPlayground
julia> gpu_info()           # shows RTX 5080 info
julia> cuda_kernel_demo()   # vector-add kernel on GPU
```

Run the GPU example script:

```bash
julia --project=. examples/01_gpu_example.jl
```

Broadcasting standard Julia operations over GPU arrays works out of the box:

```julia
using CUDA
n    = 1_000_000
V    = CUDA.fill(-65f0, n)   # membrane potentials on GPU
I    = CUDA.ones(Float32, n) .* 3.5f0
V  .+= 0.1f0 .* (-65f0 .- V .+ 10f0 .* I) ./ 20f0
```

For custom CUDA kernels:

```julia
function lif_kernel!(V, I, dt, tau_m, v_rest, r_m, v_thresh, v_reset)
    idx = (blockIdx().x - 1) * blockDim().x + threadIdx().x
    if idx <= length(V)
        dv       = (dt / tau_m) * (-(V[idx] - v_rest) + r_m * I[idx])
        V[idx]  += dv
        V[idx] >= v_thresh && (V[idx] = v_reset)
    end
    return
end

n = 1_000_000
V = CUDA.fill(-65f0, n)
I = CUDA.ones(Float32, n) .* 3.5f0
@cuda threads=256 blocks=cld(n,256) lif_kernel!(V, I, 0.1f0, 20f0, -65f0, 10f0, -50f0, -65f0)
CUDA.synchronize()
```

---

## Example Scripts

Run any script from the project root with `julia --project=. examples/<file>`:

| File | Topic |
|------|-------|
| `examples/01_gpu_example.jl` | GPU detection and CUDA kernel demo |
| `examples/02_math_demo.jl`  | Matrix ops, gradient descent, Jacobian |
| `examples/03_neuro_demo.jl` | LIF, STDP, spike encoding, SNN features |
| `examples/04_plotting_demo.jl` | Plots.jl visualisations (requires Plots.jl) |
| `examples/05_repl_quickstart.jl` | Full REPL walkthrough (paste into REPL) |

---

## Sandbox Scripts

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

## Pluto Notebook

An interactive starter notebook is in `notebooks/playground.jl`.

```julia
# Install Pluto.jl once
julia --project=. -e 'using Pkg; Pkg.add("Pluto")'

# Launch
julia --project=. -e 'using Pluto; Pluto.run()'
# Then open notebooks/playground.jl in the browser
```

---

## Optional Dependencies

Install these for the full experience:

```julia
using Pkg
Pkg.add("CUDA")       # GPU acceleration (RTX 5080)
Pkg.add("Plots")      # visualisation
Pkg.add("Symbolics")  # symbolic differentiation
Pkg.add("Pluto")      # interactive notebooks
```

Using Symbolics:

```julia
using Symbolics
@variables x y
D = Differential(x)
expand_derivatives(D(x^2 + 2*y^2 + x*y))   # 2x + y
```

---

## Module Structure

```
NeuroMathPlayground
├── src/
│   ├── NeuroMathPlayground.jl  — main module, banner, REPL helpers
│   ├── neurons.jl              — LIF neuron ODE, STDP learning rule
│   ├── neuro.jl                — spike functions, kernels, rate coding
│   ├── stochastic.jl           — GBM, OU, Poisson spike trains
│   ├── signals.jl              — Hurst, Hawkes, GBM surprise
│   ├── kelly.jl                — Kelly criterion, position sizing
│   ├── math.jl                 — gradient, Hessian, softmax, normalise
│   └── gpu.jl                  — GPU detection, CUDA kernel demo
├── examples/                   — runnable demo scripts
├── notebooks/                  — Pluto starter notebook
├── sandbox/                    — step-by-step worked examples
└── test/                       — test suite
```

---

## Running the Tests

```julia
julia --project=. test/runtests.jl
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

### Exponential PSP Kernel
```
K(Δt) = exp(-Δt / τ)   for Δt ≥ 0
```

### Rate Coding
```
rate(x) = x_norm · rate_max    where x_norm = (x - lo) / (hi - lo)
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

## Part of the Limen-Neural Ecosystem

This playground uses the same mathematical building blocks as:

| Library | Purpose |
|---------|---------|
| [SpikenautSignals.jl](https://github.com/Limen-Neural/SpikenautSignals.jl) | Streaming feature extraction (Hurst, Hawkes, GBM) |
| [SpikenautKelly.jl](https://github.com/Limen-Neural/SpikenautKelly.jl) | Half-Kelly position sizing from SNN confidence |
| [SpikenautLSM.jl](https://github.com/Limen-Neural/SpikenautLSM.jl) | GPU-accelerated Liquid State Machine |
| [SpikenautNero.jl](https://github.com/Limen-Neural/SpikenautNero.jl) | Multi-lobe relevance scoring |
| [neuromod](https://github.com/Limen-Neural/neuromod) | LIF + Izhikevich + STDP + dopamine (Rust) |
