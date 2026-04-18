# sandbox/04_signal_analysis.jl
#
# Time-Series Signal Analysis: Hurst, Hawkes, and GBM Surprise
#
# These three features form the core of SpikenautSignals.jl — they transform
# raw time-series data into SNN-compatible scalar signals that capture
# trend conviction, event clustering, and anomaly magnitude.
#
# Run with:   julia --project=. sandbox/04_signal_analysis.jl

using MathSandbox
using Printf
using Random
using Statistics

println("=" ^ 60)
println("  MathSandbox — 04 Signal Analysis")
println("=" ^ 60)
println()

# ── 1. Hurst Exponent ─────────────────────────────────────────────────────────
println("─── 1. Hurst Exponent (R/S Analysis) ───")
println("""
The Hurst exponent measures long-range dependence (memory) in a time series.

R/S Analysis:
  1. Compute mean-adjusted increments Δx_i = x_i - x_{i-1} - mean(Δx)
  2. Form cumulative deviation Z_t = Σ Δx_i
  3. R = max(Z) - min(Z)  (range of cumulative deviation)
  4. S = std(increments)
  5. H = log(R/S) / log(n)

H > 0.5 → trending (persistent memory)
H ≈ 0.5 → random walk (no memory)
H < 0.5 → mean-reverting (anti-persistent)
""")

# Synthetic signals with known properties
Random.seed!(1)
n = 500

# 1. Random walk (H ≈ 0.5)
rw = cumsum(randn(n))
# 2. Strongly trending signal (built by summing positively-biased increments)
trend = cumsum(abs.(randn(n)) .+ 0.1)
# 3. Mean-reverting OU process (H < 0.5)
p_ou  = OUParams(mu=0.0, kappa=5.0, sigma=1.0)
_, ou_vals = simulate_ou(0.0, p_ou, 50.0, 0.1)
ou_signal  = ou_vals[1:n]

println("Signal type            | Hurst H | Interpretation")
println("-" ^ 58)
for (name, sig) in [
        ("Random walk",      rw),
        ("Trending (biased cumsum)", trend),
        ("OU mean-reverting", ou_signal),
    ]
    h = compute_hurst(sig)
    interp = h > 0.55 ? "Trending ↗" : (h < 0.45 ? "Mean-reverting ↔" : "Random walk ~")
    @printf("%-24s | %-7.4f | %s\n", name, h, interp)
end
println()

# Hurst over different window sizes (shows consistency)
println("Hurst stability over window sizes (random walk):")
println("  Window | H")
println("-" ^ 22)
for w in [50, 100, 200, 500]
    h = compute_hurst(rw[1:w])
    @printf("  %-6d | %.4f\n", w, h)
end
println()

# ── 2. Hawkes Self-Exciting Intensity ─────────────────────────────────────────
println("─── 2. Hawkes Self-Exciting Intensity ───")
println("""
The Hawkes process models clustered, self-exciting events (trade bursts,
neural spike avalanches).  Each event temporarily increases the rate of
future events.

Intensity function:
   λ(t) = μ + Σ_{t_i < t} α·exp(−β·(t − t_i))

Our proxy uses mean absolute increments as an intensity estimate:
   λ̂ = 1 + 10 · mean(|ΔS|)

Range [0.5, 3.0].  Value of 1.0 = neutral (no self-excitation).
""")

calm_prices    = [100.0 + 0.02 * i + 0.05 * randn() for i in 1:50]
volatile_prices = [100.0 .+ cumsum(randn(50) .* 2.0)...]
burst_prices   = [fill(100.0, 30); 100.0 .+ cumsum(randn(20) .* 5.0)]

println("Signal type     | Hawkes λ | Interpretation")
println("-" ^ 52)
for (name, sig) in [
        ("Calm (low vol)",  calm_prices),
        ("Volatile (high vol)", volatile_prices),
        ("Burst (spike cluster)", burst_prices),
    ]
    λ = compute_hawkes(sig)
    interp = λ > 2.0 ? "High excitement !" : (λ > 1.3 ? "Moderate activity" : "Calm baseline")
    @printf("%-22s | %-8.4f | %s\n", name, λ, interp)
end
println()

# Real-time update simulation
println("Streaming Hawkes update (simulating 20-step window):")
window = [100.0]
println("  Step | Last price | Hawkes λ")
println("-" ^ 36)
for step in 1:20
    # Alternate between calm and burst phases
    if step in 8:12
        push!(window, window[end] + randn() * 3.0)   # burst
    else
        push!(window, window[end] + randn() * 0.1)   # calm
    end
    λ = compute_hawkes(window)
    @printf("  %-4d | %-10.3f | %.4f %s\n", step, window[end], λ,
            step in 8:12 ? "← burst" : "")
end
println()

# ── 3. GBM Surprise Z-Score ───────────────────────────────────────────────────
println("─── 3. GBM Surprise Z-Score ───")
println("""
The GBM surprise measures how anomalous the last price move was relative
to the local drift and volatility estimated from the recent history.

  log-return: r_t = log(S_t / S_{t-1})
  drift:      μ̂ = mean(r₁,...,r_{t-1})
  volatility: σ̂ = std(r₁,...,r_{t-1})
  surprise:   z = (r_t − μ̂) / σ̂

|z| > 2 → anomalous (less than ~5% expected under GBM)
|z| > 3 → extreme event (less than ~0.3% expected)

Clamped to [−3.0, 3.0].
""")

# Normal moves
normal_prices = [100.0 * exp(0.001 * i + 0.01 * randn()) for i in 1:20]
# Upward flash crash
crash_up      = copy(normal_prices); crash_up[end] *= 1.25
# Downward flash crash
crash_down    = copy(normal_prices); crash_down[end] *= 0.75

println("Signal         | GBM Surprise z | Interpretation")
println("-" ^ 55)
for (name, sig) in [
        ("Normal drift",     normal_prices),
        ("Flash up +25%",    crash_up),
        ("Flash down -25%",  crash_down),
    ]
    z = compute_gbm_surprise(sig)
    interp = abs(z) > 2.5 ? "EXTREME EVENT" : (abs(z) > 1.5 ? "Anomalous" : "Normal")
    @printf("%-15s | %+14.4f | %s\n", name, z, interp)
end
println()

# ── 4. Combined SNN Feature Vector ────────────────────────────────────────────
println("─── 4. Combined SNN Feature Vector ───")
println("""
In the Spikenaut pipeline, all three features are computed together and fed
as a three-channel spike encoding to the SNN.

  [H, λ, z] → Poisson encoder → SNN layer
""")

Random.seed!(7)
signal = cumsum(randn(100)) .+ 100.0
signal[end] += 20.0    # inject a large surprise at the end

f = snn_features(signal)
println("Feature extraction from a 100-point signal:")
println("  Hurst exponent:   $(round(f.hurst, digits=4))")
println("  Hawkes intensity: $(round(f.hawkes, digits=4))")
println("  GBM surprise:     $(round(f.gbm_surprise, digits=4))")
println()

# Streaming simulation
println("Streaming feature update (appending one new price each step):")
println("  t   | Hurst | Hawkes | GBM z")
println("-" ^ 42)
stream = copy(signal[1:50])
for step in 51:60
    push!(stream, signal[step])
    f2 = snn_features(stream)
    @printf("  %-3d | %.3f | %-6.3f | %+.3f\n",
            step, f2.hurst, f2.hawkes, f2.gbm_surprise)
end
println()

println("=" ^ 60)
println("  Proceed to 05_kelly_sizing.jl")
println("=" ^ 60)
