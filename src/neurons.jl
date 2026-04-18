# neurons.jl — Spiking Neuron Mathematics
#
# Leaky Integrate-and-Fire (LIF) neuron dynamics and Spike-Timing-Dependent
# Plasticity (STDP) — the two foundational building blocks of every Spikenaut
# network.

"""
    LIFParams

Parameters for a Leaky Integrate-and-Fire neuron.

# Fields
- `tau_m`:    membrane time constant (ms), typical 10–20 ms
- `v_rest`:   resting membrane potential (mV), typical –65 mV
- `v_thresh`: spike threshold (mV), typical –50 mV
- `v_reset`:  reset potential after spike (mV), typical –65 mV
- `r_m`:      membrane resistance (MΩ), typical 10 MΩ
"""
struct LIFParams
    tau_m::Float64     # membrane time constant (ms)
    v_rest::Float64    # resting potential (mV)
    v_thresh::Float64  # spike threshold (mV)
    v_reset::Float64   # reset potential (mV)
    r_m::Float64       # membrane resistance (MΩ)
end

"""
    LIFParams(; tau_m=20.0, v_rest=-65.0, v_thresh=-50.0, v_reset=-65.0, r_m=10.0)

Construct `LIFParams` with typical cortical neuron defaults.
"""
function LIFParams(;
    tau_m::Float64    = 20.0,
    v_rest::Float64   = -65.0,
    v_thresh::Float64 = -50.0,
    v_reset::Float64  = -65.0,
    r_m::Float64      = 10.0,
)
    return LIFParams(tau_m, v_rest, v_thresh, v_reset, r_m)
end

"""
    lif_step(v, I_ext, p, dt) -> (v_new, spiked)

Advance a LIF neuron one time-step using the Euler method.

The governing ODE is:

```
τ_m · dV/dt = -(V - V_rest) + R_m · I(t)
```

Discretised as:

```
V(t+dt) = V(t) + dt/τ_m · (-(V(t) - V_rest) + R_m · I)
```

If `V(t+dt) ≥ V_thresh`, a spike is emitted and `V` resets to `V_reset`.

# Arguments
- `v`:     current membrane potential (mV)
- `I_ext`: external input current (nA)
- `p`:     `LIFParams`
- `dt`:    time step (ms)

# Returns
`(v_new, spiked)` — updated potential and spike flag.

# Example
```julia
p   = LIFParams()
v   = p.v_rest
dt  = 0.1
I   = 3.5
for t in 1:300
    v, spiked = lif_step(v, I, p, dt)
    spiked && println("spike at t = \$(t * dt) ms")
end
```
"""
function lif_step(v::Float64, I_ext::Float64, p::LIFParams, dt::Float64)::Tuple{Float64,Bool}
    dv    = (dt / p.tau_m) * (-(v - p.v_rest) + p.r_m * I_ext)
    v_new = v + dv
    if v_new >= p.v_thresh
        return p.v_reset, true
    else
        return v_new, false
    end
end

"""
    simulate_lif(I_ext, T, p; dt=0.1) -> (times, voltages, spike_times)

Simulate a LIF neuron driven by constant current `I_ext` for `T` milliseconds.

Returns the full membrane voltage trace and the times at which spikes fired.

# Arguments
- `I_ext`: constant input current (nA)
- `T`:     simulation duration (ms)
- `p`:     `LIFParams`
- `dt`:    time step (ms, default 0.1)

# Example
```julia
p  = LIFParams()
ts, vs, spks = simulate_lif(3.5, 200.0, p)
println("Fired \$(length(spks)) spikes; first ISI = \$(spks[2]-spks[1]) ms")
```
"""
function simulate_lif(
    I_ext::Float64,
    T::Float64,
    p::LIFParams;
    dt::Float64 = 0.1,
)::Tuple{Vector{Float64},Vector{Float64},Vector{Float64}}
    steps   = round(Int, T / dt)
    times   = [(i - 1) * dt for i in 1:steps]
    voltages   = Vector{Float64}(undef, steps)
    spike_times = Float64[]

    v = p.v_rest
    for i in 1:steps
        v, spiked = lif_step(v, I_ext, p, dt)
        voltages[i] = v
        spiked && push!(spike_times, (i - 1) * dt)
    end
    return times, voltages, spike_times
end

"""
    firing_rate(spike_times, T) -> Float64

Compute the mean firing rate (Hz) from a list of spike times (ms) over
duration `T` ms.

# Example
```julia
rate = firing_rate(spike_times, 200.0)   # spikes/s
```
"""
function firing_rate(spike_times::Vector{Float64}, T::Float64)::Float64
    return length(spike_times) / (T * 1e-3)   # convert ms → s
end

# ── STDP — Spike-Timing-Dependent Plasticity ──────────────────────────────────

"""
    STDPParams

Parameters for an asymmetric Hebbian STDP learning window.

# Fields
- `A_plus`:   potentiation amplitude (> 0), typical 0.01
- `A_minus`:  depression amplitude (> 0), typical 0.0105
- `tau_plus`: potentiation time constant (ms), typical 20 ms
- `tau_minus`:depression time constant (ms), typical 20 ms
- `w_min`:    minimum synaptic weight
- `w_max`:    maximum synaptic weight
"""
struct STDPParams
    A_plus::Float64
    A_minus::Float64
    tau_plus::Float64
    tau_minus::Float64
    w_min::Float64
    w_max::Float64
end

"""
    STDPParams(; A_plus=0.01, A_minus=0.0105, tau_plus=20.0, tau_minus=20.0,
               w_min=0.0, w_max=1.0)

Construct `STDPParams` with standard asymmetric Hebbian defaults.

The slight asymmetry (`A_minus > A_plus`) prevents runaway potentiation and
produces stable weight distributions, following Bi & Poo (1998).
"""
function STDPParams(;
    A_plus::Float64   = 0.01,
    A_minus::Float64  = 0.0105,
    tau_plus::Float64 = 20.0,
    tau_minus::Float64 = 20.0,
    w_min::Float64    = 0.0,
    w_max::Float64    = 1.0,
)
    return STDPParams(A_plus, A_minus, tau_plus, tau_minus, w_min, w_max)
end

"""
    stdp_delta_w(delta_t, p) -> Float64

Compute the STDP weight change for spike-time difference `Δt = t_post − t_pre`.

```
ΔW =  A_+ · exp(-|Δt| / τ_+)   if Δt ≥ 0  (pre fires before post → LTP)
ΔW = -A_- · exp(-|Δt| / τ_-)   if Δt < 0  (post fires before pre → LTD)
```

# Arguments
- `delta_t`: `t_post − t_pre` in milliseconds
- `p`:       `STDPParams`

# Example
```julia
p  = STDPParams()
dw = stdp_delta_w(10.0, p)   # positive: pre fired 10 ms before post → LTP
dw = stdp_delta_w(-10.0, p)  # negative: post fired 10 ms before pre → LTD
```
"""
function stdp_delta_w(delta_t::Float64, p::STDPParams)::Float64
    if delta_t >= 0.0
        return p.A_plus * exp(-delta_t / p.tau_plus)
    else
        return -p.A_minus * exp(delta_t / p.tau_minus)
    end
end

"""
    apply_stdp(w, delta_t, p) -> Float64

Apply an STDP weight update to synapse `w`, clamped to `[w_min, w_max]`.

# Example
```julia
p = STDPParams()
w = 0.5
w = apply_stdp(w, 5.0, p)   # pre fired 5 ms before post → slight potentiation
```
"""
function apply_stdp(w::Float64, delta_t::Float64, p::STDPParams)::Float64
    return clamp(w + stdp_delta_w(delta_t, p), p.w_min, p.w_max)
end
