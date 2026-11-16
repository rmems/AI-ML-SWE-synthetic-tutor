### A Pluto.jl notebook ###
# v0.19.36
#
# Starter notebook for NeuroMathPlayground.jl
#
# Run with:
#   julia --project=. -e 'using Pkg; Pkg.add("Pluto"); using Pluto; Pluto.run()'
# Then open notebooks/playground.jl in the Pluto interface.

using Markdown
using InteractiveUtils

# ╔═╡ 00000000-0000-0000-0000-000000000001
md"""
# NeuroMathPlayground.jl — Interactive Notebook

A personal Julia playground for mathematics and neuromorphic computing.

## Contents
1. [Loading the module](#section-1)
2. [LIF Neuron Simulation](#section-2)
3. [Stochastic Processes](#section-3)
4. [Spike Encoding](#section-4)
5. [Math Utilities](#section-5)
6. [GPU Setup](#section-6)
"""

# ╔═╡ 00000000-0000-0000-0000-000000000002
# Section 1 — Load the module
# anchor: section-1
begin
    using NeuroMathPlayground
    using Statistics
    using Random
    Random.seed!(42)
    md"✅ NeuroMathPlayground loaded — call `repl_help()` for the function list."
end

# ╔═╡ 00000000-0000-0000-0000-000000000003
# Section 2 — LIF Neuron Simulation
# anchor: section-2
md"""
## LIF Neuron Simulation

The **Leaky Integrate-and-Fire** (LIF) neuron is the simplest spiking neuron model.

```
τ_m · dV/dt = -(V - V_rest) + R_m · I(t)
```

When V reaches V_thresh a spike is emitted and V resets to V_reset.
"""

# ╔═╡ 00000000-0000-0000-0000-000000000004
begin
    p  = LIFParams()        # default cortical parameters
    I  = 3.5                # input current (nA)
    T  = 300.0              # duration (ms)
    ts, vs, spks = simulate_lif(I, T, p; dt=0.1)

    md"""
    **Simulation results:**
    - Input current: $(I) nA
    - Duration: $(T) ms
    - Spikes fired: **$(length(spks))**
    - Firing rate: **$(round(firing_rate(spks, T), digits=1)) Hz**
    - First spike: $(isempty(spks) ? "none" : "$(spks[1]) ms")
    """
end

# ╔═╡ 00000000-0000-0000-0000-000000000005
# F-I curve — firing rate as a function of input current
begin
    fi_currents = 0.5:0.5:6.0
    fi_rates = [
        firing_rate(simulate_lif(Float64(I), 500.0, p; dt=0.1)[3], 500.0)
        for I in fi_currents
    ]
    md"""
    **F-I curve** (firing rate vs input current):

    | I (nA) | Rate (Hz) |
    |--------|-----------|
    $(join(["| $(fi_currents[i]) | $(round(fi_rates[i], digits=1)) |" for i in 1:length(fi_currents)], "\n"))
    """
end

# ╔═╡ 00000000-0000-0000-0000-000000000006
# Section 3 — Stochastic Processes
# anchor: section-3
md"""
## Stochastic Processes

**Geometric Brownian Motion** — price/signal model:
```
dS = μS dt + σS dW
```

**Ornstein-Uhlenbeck** — mean-reverting noise:
```
dX = κ(μ - X) dt + σ dW
```
"""

# ╔═╡ 00000000-0000-0000-0000-000000000007
begin
    # GBM simulation
    gbm_times, gbm_prices = simulate_gbm(100.0, 0.08, 0.20, 1.0, 1/252)
    ev = gbm_expected_value(100.0, 0.08, 1.0)
    md"""
    **GBM** (S₀=100, μ=8%, σ=20%, T=1 year, 252 steps):
    - Final price: **$(round(gbm_prices[end], digits=2))**
    - Expected value E[S(T)]: **$(round(ev, digits=2))**
    """
end

# ╔═╡ 00000000-0000-0000-0000-000000000008
begin
    # OU simulation
    ou_p = OUParams(mu=0.0, kappa=2.0, sigma=0.3)
    ou_t, ou_v = simulate_ou(2.0, ou_p, 5.0, 0.01)
    md"""
    **OU process** (x₀=2.0, κ=2.0, σ=0.3, T=5 s):
    - Start: 2.0  →  End: **$(round(ou_v[end], digits=3))**
    - Stationary σ: **$(round(ou_stationary_std(ou_p), digits=3))**
    - (Process reverts toward μ=0)
    """
end

# ╔═╡ 00000000-0000-0000-0000-000000000009
# Section 4 — Spike Encoding
# anchor: section-4
md"""
## Spike Encoding

Converting scalar values to spike trains for neuromorphic computing.
"""

# ╔═╡ 00000000-0000-0000-0000-000000000010
begin
    enc_results = [
        (val=v, spikes=encode_rate(v, 0.0, 1.0, 1000.0),
         phase=rate_to_phase(v * 200.0))
        for v in [0.0, 0.25, 0.5, 0.75, 1.0]
    ]
    md"""
    **Rate coding** (scalar → Poisson spike train over 1 second):

    | Value | Spikes | Phase (ms) |
    |-------|--------|------------|
    $(join(["| $(r.val) | $(length(r.spikes)) | $(round(r.phase, digits=1)) |" for r in enc_results], "\n"))

    Higher values → more spikes (rate coding) and shorter latency (phase coding).
    """
end

# ╔═╡ 00000000-0000-0000-0000-000000000011
# Section 5 — Math Utilities
# anchor: section-5
md"""
## Math Utilities

Numerical gradient, Hessian, softmax, and L2 normalisation.
"""

# ╔═╡ 00000000-0000-0000-0000-000000000012
begin
    # Numerical gradient of f(x,y) = x² + 2y²
    f_quad = x -> x[1]^2 + 2*x[2]^2
    x_pt   = [1.0, 2.0]
    grad   = gradient_approx(f_quad, x_pt)
    hess   = hessian_approx(f_quad, x_pt)
    md"""
    **f(x,y) = x² + 2y²** at x = $(x_pt):
    - ∇f ≈ $(round.(grad, digits=6))  (exact: [2, 8])
    - H  ≈ $(round.(hess, digits=4))  (exact: [2 0; 0 4])
    """
end

# ╔═╡ 00000000-0000-0000-0000-000000000013
begin
    logits  = [1.0, 2.5, 0.3, 3.1, 0.8]
    probs   = softmax(logits)
    normed  = normalize_vec([3.0, 4.0])
    using LinearAlgebra
    md"""
    **Softmax** [$(join(logits, ", "))]:
    - probs = $(round.(probs, digits=3))
    - argmax = $(argmax(probs))

    **normalize([3, 4])** = $(round.(normed, digits=3))  (‖u‖ = $(round(norm(normed), digits=10)))
    """
end

# ╔═╡ 00000000-0000-0000-0000-000000000014
# Section 6 — GPU Setup
# anchor: section-6
md"""
## GPU Setup

To use your RTX 5080 GPU, install and load CUDA.jl:

```julia
using Pkg
Pkg.add("CUDA")
```

Then in a new Julia session:
```julia
using CUDA
using NeuroMathPlayground

gpu_info()          # shows your GPU
cuda_kernel_demo()  # vector-add on GPU
```

For large-scale simulations:
```julia
# Simulate 10,000 LIF neurons in parallel on GPU
n = 10_000
V    = CUDA.fill(Float32(-65.0), n)    # membrane potentials
I    = CUDA.ones(Float32, n) .* 3.5f0  # input currents
V   .= V .+ 0.1f0 .* ((-65f0 .- V) .+ 10f0 .* I) ./ 20f0
```

No custom kernels needed — Julia broadcasting works directly on GPU arrays!
"""

# ╔═╡ 00000000-0000-0000-0000-000000000015
begin
    gpu_status = gpu_available() ? "✅ GPU available" : "⚠️ No GPU (CPU mode)"
    md"""
    **Current GPU status:** $(gpu_status)

    GPU info:
    """
end

# ╔═╡ Cell order:
# ╠═00000000-0000-0000-0000-000000000001
# ╠═00000000-0000-0000-0000-000000000002
# ╠═00000000-0000-0000-0000-000000000003
# ╠═00000000-0000-0000-0000-000000000004
# ╠═00000000-0000-0000-0000-000000000005
# ╠═00000000-0000-0000-0000-000000000006
# ╠═00000000-0000-0000-0000-000000000007
# ╠═00000000-0000-0000-0000-000000000008
# ╠═00000000-0000-0000-0000-000000000009
# ╠═00000000-0000-0000-0000-000000000010
# ╠═00000000-0000-0000-0000-000000000011
# ╠═00000000-0000-0000-0000-000000000012
# ╠═00000000-0000-0000-0000-000000000013
# ╠═00000000-0000-0000-0000-000000000014
# ╠═00000000-0000-0000-0000-000000000015
