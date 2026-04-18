# stochastic.jl — Stochastic Process Mathematics
#
# Geometric Brownian Motion, Ornstein-Uhlenbeck dynamics, and Poisson spike
# trains — the probabilistic engines driving Spikenaut market models and SNN
# sensory encoding.

# ── Geometric Brownian Motion ─────────────────────────────────────────────────

"""
    gbm_step(S, mu, sigma, dt) -> Float64

Advance a Geometric Brownian Motion by one step using the exact log-normal
solution (no Euler error).

The GBM SDE is `dS = μ·S·dt + σ·S·dW`, solved exactly as:

```
S(t+dt) = S(t) · exp((μ − σ²/2)·dt + σ·√dt·Z)   where Z ~ N(0,1)
```

# Arguments
- `S`:     current value
- `mu`:    drift (per unit time)
- `sigma`: volatility (per √time)
- `dt`:    time step

# Example
```julia
S = 100.0
for _ in 1:10
    S = gbm_step(S, 0.05, 0.20, 1/252)
end
```
"""
function gbm_step(S::Float64, mu::Float64, sigma::Float64, dt::Float64)::Float64
    Z = randn()
    return S * exp((mu - 0.5 * sigma^2) * dt + sigma * sqrt(dt) * Z)
end

"""
    simulate_gbm(S0, mu, sigma, T, dt) -> (times, prices)

Simulate a GBM path from `S0` over `T` time units with step `dt`.

# Arguments
- `S0`:    initial value
- `mu`:    drift (annualised if `T` and `dt` are in years/trading-days)
- `sigma`: volatility
- `T`:     total time
- `dt`:    time step

# Example
```julia
times, prices = simulate_gbm(100.0, 0.05, 0.20, 1.0, 1/252)
println("Final price: \$(round(prices[end], digits=2))")
```
"""
function simulate_gbm(
    S0::Float64, mu::Float64, sigma::Float64, T::Float64, dt::Float64
)::Tuple{Vector{Float64},Vector{Float64}}
    steps  = round(Int, T / dt)
    times  = [(i - 1) * dt for i in 1:(steps + 1)]
    prices = Vector{Float64}(undef, steps + 1)
    prices[1] = S0
    for i in 2:(steps + 1)
        prices[i] = gbm_step(prices[i-1], mu, sigma, dt)
    end
    return times, prices
end

"""
    gbm_expected_value(S0, mu, T) -> Float64

Analytical expected value of a GBM at time `T`.

```
E[S(T)] = S0 · exp(μ·T)
```

# Example
```julia
ev = gbm_expected_value(100.0, 0.05, 1.0)   # ≈ 105.13
```
"""
function gbm_expected_value(S0::Float64, mu::Float64, T::Float64)::Float64
    return S0 * exp(mu * T)
end

# ── Ornstein-Uhlenbeck Process ────────────────────────────────────────────────

"""
    OUParams

Parameters for an Ornstein-Uhlenbeck mean-reverting process.

# Fields
- `mu`:    long-run mean (θ in some texts)
- `kappa`: mean-reversion speed (κ)
- `sigma`: volatility
"""
struct OUParams
    mu::Float64
    kappa::Float64
    sigma::Float64
end

"""
    OUParams(; mu=0.0, kappa=1.0, sigma=0.1)

Construct `OUParams` with sensible defaults.
"""
OUParams(; mu::Float64=0.0, kappa::Float64=1.0, sigma::Float64=0.1) =
    OUParams(mu, kappa, sigma)

"""
    ou_step(x, p, dt) -> Float64

Advance an Ornstein-Uhlenbeck process by one time step using the Euler-Maruyama
method.

The OU SDE is `dX = κ·(μ − X)·dt + σ·dW`, discretised as:

```
X(t+dt) = X(t) + κ·(μ − X(t))·dt + σ·√dt·Z   where Z ~ N(0,1)
```

OU processes are used in Spikenaut for:
- Membrane adaptation currents (spike-frequency adaptation)
- Synaptic noise modelling
- Neuromodulator baseline drift

# Arguments
- `x`: current value
- `p`: `OUParams`
- `dt`: time step

# Example
```julia
p = OUParams(mu=0.0, kappa=2.0, sigma=0.3)
x = 1.0
for _ in 1:200
    x = ou_step(x, p, 0.01)
end
# x has drifted toward 0.0
```
"""
function ou_step(x::Float64, p::OUParams, dt::Float64)::Float64
    return x + p.kappa * (p.mu - x) * dt + p.sigma * sqrt(dt) * randn()
end

"""
    simulate_ou(x0, p, T, dt) -> (times, values)

Simulate an OU process path from initial value `x0`.

# Example
```julia
p = OUParams(mu=0.0, kappa=1.0, sigma=0.2)
times, vals = simulate_ou(1.0, p, 5.0, 0.01)
```
"""
function simulate_ou(
    x0::Float64, p::OUParams, T::Float64, dt::Float64
)::Tuple{Vector{Float64},Vector{Float64}}
    steps  = round(Int, T / dt)
    times  = [(i - 1) * dt for i in 1:(steps + 1)]
    values = Vector{Float64}(undef, steps + 1)
    values[1] = x0
    for i in 2:(steps + 1)
        values[i] = ou_step(values[i-1], p, dt)
    end
    return times, values
end

"""
    ou_stationary_std(p) -> Float64

Analytical standard deviation of the OU stationary distribution.

```
σ_∞ = σ / √(2κ)
```

# Example
```julia
p = OUParams(mu=0.0, kappa=1.0, sigma=0.2)
println(ou_stationary_std(p))   # ≈ 0.141
```
"""
function ou_stationary_std(p::OUParams)::Float64
    return p.sigma / sqrt(2 * p.kappa)
end

# ── Poisson Spike Trains ──────────────────────────────────────────────────────

"""
    poisson_spike_train(rate_hz, T_ms; dt_ms=1.0) -> Vector{Float64}

Generate a homogeneous Poisson spike train with firing rate `rate_hz` (spikes/s)
over duration `T_ms` milliseconds.

Each time bin of width `dt_ms` ms has an independent spike probability:

```
P(spike in dt) = rate_hz · dt_ms · 1e-3
```

Returns the spike times in milliseconds.

# Arguments
- `rate_hz`: firing rate in Hz (spikes per second)
- `T_ms`:    duration in milliseconds
- `dt_ms`:   bin width (ms, default 1.0)

# Example
```julia
spikes = poisson_spike_train(50.0, 1000.0)   # 50 Hz, 1 second
println("Expected ≈50 spikes, got: \$(length(spikes))")
```
"""
function poisson_spike_train(
    rate_hz::Float64, T_ms::Float64; dt_ms::Float64 = 1.0
)::Vector{Float64}
    p_spike = rate_hz * dt_ms * 1e-3
    steps   = round(Int, T_ms / dt_ms)
    spikes  = Float64[]
    for i in 1:steps
        rand() < p_spike && push!(spikes, (i - 1) * dt_ms)
    end
    return spikes
end

"""
    inter_spike_intervals(spike_times) -> Vector{Float64}

Compute inter-spike intervals (ISIs) from a vector of spike times.

For a Poisson neuron the ISI distribution is exponential with mean `1/rate`.

# Example
```julia
spikes = poisson_spike_train(40.0, 2000.0)
isis   = inter_spike_intervals(spikes)
println("Mean ISI: \$(mean(isis)) ms  (expected ≈25 ms for 40 Hz)")
```
"""
function inter_spike_intervals(spike_times::Vector{Float64})::Vector{Float64}
    length(spike_times) < 2 && return Float64[]
    return diff(spike_times)
end

"""
    cv_isi(spike_times) -> Float64

Coefficient of variation of the inter-spike interval distribution.

```
CV_ISI = std(ISI) / mean(ISI)
```

- `CV ≈ 1.0` → Poisson (random) spiking
- `CV < 1.0` → regular spiking (clock-like)
- `CV > 1.0` → bursty spiking

# Example
```julia
spikes = poisson_spike_train(40.0, 5000.0)
cv     = cv_isi(spikes)   # ≈ 1.0 for Poisson
```
"""
function cv_isi(spike_times::Vector{Float64})::Float64
    isis = inter_spike_intervals(spike_times)
    length(isis) < 2 && return 0.0
    m = mean(isis)
    m < 1e-12 && return 0.0
    return std(isis) / m
end
