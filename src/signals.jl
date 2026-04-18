# signals.jl — Time-Series Signal Analysis
#
# Hurst exponent, Hawkes self-exciting intensity, and GBM surprise Z-score —
# the three streaming feature primitives used throughout the Spikenaut ecosystem
# to encode market and sensor data into SNN-compatible signals.

# ── Hurst Exponent ────────────────────────────────────────────────────────────

"""
    compute_hurst(signal::AbstractVector{<:Real}) -> Float64

Estimate the Hurst exponent `H` via Rescaled Range (R/S) analysis.

```
H = log(R/S) / log(n)
```

where `R` is the range of cumulative mean-adjusted increments and `S` is
their standard deviation.

| H value  | Interpretation         |
|----------|------------------------|
| > 0.5    | Trending (persistent)  |
| ≈ 0.5    | Random walk            |
| < 0.5    | Mean-reverting         |

Returns a value clamped to `[0.2, 0.8]`.
Returns `0.5` (random walk) for series shorter than 20 elements or with
zero variance.

# References
- Hurst, H.E. (1951). Long-term storage capacity of reservoirs.
  *Trans. American Society of Civil Engineers*, 116, 770–799.

# Example
```julia
signal = cumsum(randn(500)) .+ 50.0
h = compute_hurst(signal)
h > 0.5 ? println("Trending (H=\$h)") : println("Mean-reverting (H=\$h)")
```
"""
function compute_hurst(signal::AbstractVector{<:Real})::Float64
    length(signal) < 20 && return 0.5

    diffs   = diff(Float64.(signal))
    m       = mean(diffs)
    s       = std(diffs)
    s < 1e-10 && return 0.5

    cum_dev = cumsum(diffs .- m)
    r       = maximum(cum_dev) - minimum(cum_dev)
    h       = log(r / s) / log(length(signal))
    return clamp(h, 0.2, 0.8)
end

# ── Hawkes Self-Exciting Intensity ────────────────────────────────────────────

"""
    compute_hawkes(signal::AbstractVector{<:Real}) -> Float64

Estimate the Hawkes self-exciting intensity proxy from recent absolute
increments of `signal`.

```
λ̂ = 1 + 10 · mean(|ΔS|)
```

Higher values indicate bursty, self-exciting behaviour (clustered events).
Returns a value clamped to `[0.5, 3.0]`.
Returns `1.0` (neutral) for series shorter than 2 elements.

# References
- Hawkes, A.G. (1971). Spectra of some self-exciting and mutually exciting
  point processes. *Biometrika*, 58(1), 83–90.

# Example
```julia
volatile = [100.0, 102.5, 98.0, 104.0, 99.0]
calm     = [100.0, 100.1, 100.2, 100.1, 100.0]
println(compute_hawkes(volatile))  # > 1.0
println(compute_hawkes(calm))      # ≈ 1.0
```
"""
function compute_hawkes(signal::AbstractVector{<:Real})::Float64
    length(signal) < 2 && return 1.0
    diffs     = abs.(diff(Float64.(signal)))
    intensity = 1.0 + mean(diffs) * 10.0
    return clamp(intensity, 0.5, 3.0)
end

# ── GBM Surprise Z-Score ──────────────────────────────────────────────────────

"""
    compute_gbm_surprise(signal::AbstractVector{<:Real}) -> Float64

Compute the GBM surprise Z-score — the deviation of the last log-return from
the local GBM drift and volatility estimate.

```
dS = μS dt + σS dW
z  = (log(S_t/S_{t-1}) − μ̂) / σ̂
```

Positive values indicate an upward surprise; negative values a downward
surprise. Useful for anomaly detection and adaptive SNN threshold modulation.

Returns a value clamped to `[-3.0, 3.0]`.
Returns `0.0` for series shorter than 5 elements or with zero variance.

# Example
```julia
signal = [100.0, 101.0, 100.5, 102.0, 115.0]   # big last jump
z = compute_gbm_surprise(signal)   # strongly positive
```
"""
function compute_gbm_surprise(signal::AbstractVector{<:Real})::Float64
    length(signal) < 5 && return 0.0

    sv         = Float64.(signal)
    log_rets   = log.(sv[2:end] ./ sv[1:end-1])
    drift      = mean(log_rets)
    vol        = std(log_rets)

    vol < 1e-10 && return 0.0

    surprise = (log_rets[end] - drift) / vol
    return clamp(surprise, -3.0, 3.0)
end

# ── Combined Feature Vector ───────────────────────────────────────────────────

"""
    snn_features(signal) -> NamedTuple{(:hurst, :hawkes, :gbm_surprise)}

Compute all three SNN-compatible features from a signal vector in one call.

This mirrors the three-channel encoding used in the Spikenaut ecosystem.

# Example
```julia
signal = [100.0 + 0.5*randn() for _ in 1:100]
f = snn_features(signal)
println("H=\$(f.hurst)  λ=\$(f.hawkes)  z=\$(f.gbm_surprise)")
```
"""
function snn_features(signal::AbstractVector{<:Real})
    return (
        hurst        = compute_hurst(signal),
        hawkes       = compute_hawkes(signal),
        gbm_surprise = compute_gbm_surprise(signal),
    )
end
