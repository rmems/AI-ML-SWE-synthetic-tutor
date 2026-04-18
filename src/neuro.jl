# neuro.jl — Neuromorphic Math Utilities
#
# Spike activation functions, kernels, rate coding, and phase coding —
# the building blocks used in spiking neural network inference and learning.

# ── Spike Activation Functions ────────────────────────────────────────────────

"""
    spike_threshold(x, θ=0.0) -> Float64

Heaviside threshold activation: returns `1.0` if `x ≥ θ`, else `0.0`.

Models an all-or-none spike: once the input exceeds the threshold, the neuron
fires a unit spike.

# Example
```julia
spike_threshold(0.5)        # 1.0
spike_threshold(-0.1)       # 0.0
spike_threshold(0.3, 0.5)   # 0.0  (threshold = 0.5)
```
"""
function spike_threshold(x::Real, θ::Real = 0.0)::Float64
    return x >= θ ? 1.0 : 0.0
end

"""
    lif_activation(x; gain=1.0) -> Float64

Soft LIF activation: a rectified-log function that approximates the
steady-state firing rate of a Leaky Integrate-and-Fire neuron.

```
f(x) = gain · log(1 + max(x, 0))
```

Unlike the hard Heaviside, this is differentiable everywhere, making it
useful for gradient-based analyses or as a smooth spike-rate proxy.

# Example
```julia
lif_activation(0.0)          # 0.0
lif_activation(1.0)          # ≈ 0.693
lif_activation(2.0; gain=2)  # ≈ 2.197
```
"""
function lif_activation(x::Real; gain::Real = 1.0)::Float64
    return gain * log1p(max(Float64(x), 0.0))
end

# ── Synaptic Kernels ──────────────────────────────────────────────────────────

"""
    spike_kernel(dt, τ=20.0) -> Float64

Evaluate the exponential post-synaptic potential (PSP) kernel at time `dt`
after a spike, with time constant `τ` (ms).

```
K(Δt) = exp(-Δt / τ)   for Δt ≥ 0, else 0
```

The kernel models the decaying influence of a pre-synaptic spike on the
post-synaptic membrane potential.

# Arguments
- `dt`: time since the spike (ms)
- `τ`:  time constant (ms, default 20 ms)

# Example
```julia
spike_kernel(0.0)    # 1.0   (immediately after spike)
spike_kernel(20.0)   # ≈ 0.368 (one time constant)
spike_kernel(-1.0)   # 0.0   (before the spike)
```
"""
function spike_kernel(dt::Real, τ::Real = 20.0)::Float64
    dt < 0.0 && return 0.0
    return exp(-Float64(dt) / Float64(τ))
end

# ── Rate Coding ───────────────────────────────────────────────────────────────

"""
    encode_rate(x, lo, hi, T=1000.0; dt=1.0) -> Vector{Float64}

Rate-encode the scalar `x` ∈ [lo, hi] as a Poisson spike train.

The value is mapped linearly to a firing rate in [0, 200] Hz, then a
Poisson spike train of duration `T` ms is generated.

Returns the spike times in milliseconds.

# Example
```julia
spikes = encode_rate(0.75, 0.0, 1.0)   # targets ≈150 Hz → ~150 spikes on average over 1 s
println("encoded \$(length(spikes)) spikes (target rate: \$(150) Hz, Poisson so count varies)")
```
"""
function encode_rate(
    x::Real, lo::Real, hi::Real, T::Real = 1000.0;
    dt::Real = 1.0, max_rate::Real = 200.0,
)::Vector{Float64}
    lo, hi = Float64(lo), Float64(hi)
    lo == hi && return Float64[]
    frac     = clamp((Float64(x) - lo) / (hi - lo), 0.0, 1.0)
    rate_hz  = frac * Float64(max_rate)
    return poisson_spike_train(rate_hz, Float64(T); dt_ms = Float64(dt))
end

"""
    rate_to_phase(rate_hz, T=1000.0) -> Float64

Convert a firing rate (Hz) to a first-spike latency (ms) using a reciprocal
time-to-first-spike (TTFS) code.

```
phase = T / (1 + rate · T · 1e-3)
```

Higher firing rates produce shorter latencies (earlier first spikes).
Returns `T` (end of window) when the rate is zero.

# Example
```julia
rate_to_phase(100.0)   # ≈ 9.09 ms  (fast response)
rate_to_phase(10.0)    # ≈ 50.0 ms  (slow response)
rate_to_phase(0.0)     # 1000.0 ms  (no spike)
```
"""
function rate_to_phase(rate_hz::Real, T::Real = 1000.0)::Float64
    rate_hz = max(Float64(rate_hz), 0.0)
    T       = Float64(T)
    return T / (1.0 + rate_hz * T * 1e-3)
end

# ── Demo Functions ────────────────────────────────────────────────────────────

"""
    lif_demo()

Walk through a Leaky Integrate-and-Fire neuron simulation step by step,
printing key results to the REPL.

# Example
```julia
lif_demo()
```
"""
function lif_demo()
    println("── LIF Neuron Demo ──────────────────────────────────────────")
    p = LIFParams()
    println("Parameters:")
    println("  τ_m = $(p.tau_m) ms   (membrane time constant)")
    println("  V_rest = $(p.v_rest) mV   V_thresh = $(p.v_thresh) mV")
    println("  R_m = $(p.r_m) MΩ")

    println()
    println("── Single step at rest (I=0) ─────────────────────────────")
    v, spiked = lif_step(p.v_rest, 0.0, p, 0.1)
    println("  V stays at rest: $v mV  (spiked = $spiked)")

    println()
    println("── Simulate 200 ms at I = 3.5 nA ───────────────────────")
    ts, vs, spks = simulate_lif(3.5, 200.0, p; dt=0.1)
    rate = firing_rate(spks, 200.0)
    println("  Spikes fired   : $(length(spks))")
    println("  Firing rate    : $(round(rate, digits=1)) Hz")
    isempty(spks) || println("  First spike at : $(spks[1]) ms")

    println()
    println("── F-I curve (firing rate vs input current) ─────────────")
    println("  I (nA)  →  rate (Hz)")
    for I in [0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0]
        _, _, s = simulate_lif(Float64(I), 500.0, p; dt=0.1)
        r = firing_rate(s, 500.0)
        bar = "█" ^ round(Int, r / 5)
        println("  $(lpad(I, 5))  →  $(lpad(round(r, digits=1), 7)) Hz  $bar")
    end

    println()
    println("── STDP weight update ────────────────────────────────────")
    sp = STDPParams()
    dw_ltp = stdp_delta_w(10.0, sp)
    dw_ltd = stdp_delta_w(-10.0, sp)
    println("  Δt = +10 ms (pre→post): ΔW = $(round(dw_ltp, digits=6))  (LTP)")
    println("  Δt = -10 ms (post→pre): ΔW = $(round(dw_ltd, digits=6))  (LTD)")
end

"""
    neuro_demo()

Demonstrate spike activation functions, kernels, and rate encoding.

# Example
```julia
neuro_demo()
```
"""
function neuro_demo()
    println("── Spike Threshold Activation ────────────────────────────")
    for x in [-0.5, 0.0, 0.1, 0.5, 1.0]
        println("  spike_threshold($x) = $(spike_threshold(x))")
    end

    println()
    println("── Soft LIF Activation ───────────────────────────────────")
    for x in [0.0, 0.5, 1.0, 2.0, 5.0]
        println("  lif_activation($x) = $(round(lif_activation(x), digits=4))")
    end

    println()
    println("── Exponential Spike Kernel (τ=20 ms) ───────────────────")
    for dt in [0.0, 5.0, 20.0, 40.0, 100.0]
        k = spike_kernel(dt)
        bar = "█" ^ round(Int, k * 20)
        println("  K($(lpad(dt, 5)) ms) = $(round(k, digits=3))  $bar")
    end

    println()
    println("── Rate Encoding (scalar → spike train) ──────────────────")
    for val in [0.0, 0.25, 0.5, 0.75, 1.0]
        spks = encode_rate(val, 0.0, 1.0, 1000.0)
        phase = rate_to_phase(val * 200.0)
        println("  x=$(val) → $(length(spks)) spikes  phase=$(round(phase, digits=1)) ms")
    end
end
