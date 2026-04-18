# sandbox/03_stochastic_processes.jl
#
# Stochastic Processes: GBM, Ornstein-Uhlenbeck, and Poisson Spike Trains
#
# These processes appear throughout Spikenaut:
#   • GBM — models asset price dynamics fed into the SNN encoder
#   • OU  — models membrane adaptation currents and neuromodulator drift
#   • Poisson — generates synthetic spike trains for SNN training
#
# Run with:   julia --project=. sandbox/03_stochastic_processes.jl

using MathSandbox
using Printf
using Random
using Statistics

println("=" ^ 60)
println("  MathSandbox — 03 Stochastic Processes")
println("=" ^ 60)
println()

# ── 1. Geometric Brownian Motion ──────────────────────────────────────────────
println("─── 1. Geometric Brownian Motion (GBM) ───")
println("""
GBM is the standard model for asset prices and many continuous signals.

SDE:      dS = μ·S·dt + σ·S·dW
Solution: S(T) = S₀ · exp((μ − σ²/2)·T + σ·√T·Z)   Z ~ N(0,1)

Key insight: log-returns are normally distributed,
             but prices themselves are log-normally distributed.
""")

S0    = 100.0    # initial price
mu    = 0.05     # 5% annual drift
sigma = 0.20     # 20% annual volatility
T     = 1.0      # 1 year
dt    = 1.0/252  # daily steps (252 trading days/year)

# Analytical expected value
ev = gbm_expected_value(S0, mu, T)
println("Analytical expected value after 1 year: $(round(ev, digits=2))")
println("  E[S(T)] = S₀ · exp(μ·T) = $S0 · exp($mu · $T)")
println()

# Simulate multiple paths
println("Simulating 5 GBM paths (S₀=$S0, μ=$mu, σ=$sigma, T=$T yr):")
println("-" ^ 55)
Random.seed!(42)
final_prices = Float64[]
for trial in 1:5
    _, prices = simulate_gbm(S0, mu, sigma, T, dt)
    S_end     = round(prices[end], digits=2)
    pct       = round((S_end/S0 - 1) * 100, digits=1)
    push!(final_prices, S_end)
    bar       = pct >= 0 ? "+" ^ min(30, round(Int, abs(pct) / 2)) :
                           "-" ^ min(30, round(Int, abs(pct) / 2))
    @printf("  Path %d: S_T = %-8.2f (%+.1f%%) %s\n", trial, S_end, pct, bar)
end
println("  Monte Carlo mean: $(round(mean(final_prices), digits=2))  (expected: $(round(ev, digits=2)))")
println()

# Log-return distribution
println("Log-Return Distribution (50 simulated paths):")
all_log_rets = Float64[]
for _ in 1:50
    _, prices = simulate_gbm(S0, mu, sigma, T, dt)
    push!(all_log_rets, log(prices[end] / S0))
end
theoretical_mean = mu - 0.5 * sigma^2
theoretical_std  = sigma * sqrt(T)
println("  Theoretical mean of log-returns: $(round(theoretical_mean, digits=4))")
println("  Simulated   mean of log-returns: $(round(mean(all_log_rets), digits=4))")
println("  Theoretical std  of log-returns: $(round(theoretical_std, digits=4))")
println("  Simulated   std  of log-returns: $(round(std(all_log_rets), digits=4))")
println()

# ── 2. Ornstein-Uhlenbeck Process ─────────────────────────────────────────────
println("─── 2. Ornstein-Uhlenbeck (OU) Process ───")
println("""
OU is the continuous-time analogue of an AR(1) process.
Unlike GBM it is mean-reverting — ideal for membrane adaptation currents
and neuromodulator dynamics.

SDE:  dX = κ·(μ − X)·dt + σ·dW

Stationary distribution: N(μ, σ²/(2κ))
Half-life of deviation:  t₁/₂ = log(2) / κ
""")

p_ou = OUParams(mu=0.0, kappa=1.0, sigma=0.2)
println("OU Parameters:")
println("  μ (mean)  = $(p_ou.mu)")
println("  κ (speed) = $(p_ou.kappa)")
println("  σ (vol)   = $(p_ou.sigma)")
println("  Stationary std = $(round(ou_stationary_std(p_ou), digits=4))")
println("  Half-life      = $(round(log(2)/p_ou.kappa, digits=4)) time units")
println()

# Simulate from an extreme starting point
times, vals = simulate_ou(2.0, p_ou, 10.0, 0.01)
halfway = round(Int, length(vals) / 2)
println("Simulating from x₀ = 2.0 (far from mean μ=0):")
println("  x₀      = $(round(vals[1], digits=4))")
println("  x(5.0)  = $(round(vals[halfway], digits=4))  (expect closer to 0)")
println("  x(10.0) = $(round(vals[end], digits=4))")
println()

# Effect of kappa on mean reversion speed
println("Mean reversion speed comparison (from x₀=1.0, T=5, same σ=0.3):")
println("  κ      | x(1.0) | x(5.0)")
println("  " ^ 5 * "-")
for kappa in [0.5, 1.0, 2.0, 5.0]
    p2 = OUParams(mu=0.0, kappa=kappa, sigma=0.3)
    _, v2 = simulate_ou(1.0, p2, 5.0, 0.01)
    idx1 = round(Int, 1.0/0.01) + 1
    @printf("  %-5.1f  | %-6.3f | %-6.3f\n", kappa, v2[idx1], v2[end])
end
println()

# ── 3. Poisson Spike Trains ───────────────────────────────────────────────────
println("─── 3. Poisson Spike Trains ───")
println("""
A Poisson spike train has spikes distributed randomly in time, with a
fixed probability p = rate·Δt per bin.  The inter-spike interval (ISI)
follows an exponential distribution.

P(ISI > t) = exp(−rate·t)
E[ISI]     = 1/rate
""")

# Single neuron at 40 Hz for 1 second
rate_hz = 40.0
T_ms    = 1000.0
spikes  = poisson_spike_train(rate_hz, T_ms)
isis    = inter_spike_intervals(spikes)
cv      = cv_isi(spikes)

println("Poisson spike train — $rate_hz Hz, $(T_ms) ms:")
println("  Expected spikes: $(round(Int, rate_hz * T_ms/1000))")
println("  Actual spikes:   $(length(spikes))")
println("  Expected ISI:    $(round(1000.0/rate_hz, digits=1)) ms")
if !isempty(isis)
    println("  Mean ISI:        $(round(mean(isis), digits=1)) ms")
    println("  CV(ISI):         $(round(cv, digits=3))  (≈1.0 for Poisson)")
end
println()

# Compare multiple neurons at different rates
println("Multi-neuron population (1 second simulation):")
println("  Rate (Hz) | Spikes | Mean ISI (ms) | CV(ISI)")
println("-" ^ 52)
for r in [10.0, 20.0, 40.0, 80.0, 100.0]
    spks = poisson_spike_train(r, 1000.0)
    is   = inter_spike_intervals(spks)
    m    = isempty(is) ? NaN : mean(is)
    cv2  = cv_isi(spks)
    @printf("  %-9.0f | %-6d | %-13.2f | %.3f\n", r, length(spks), m, cv2)
end
println()

# ISI distribution for a Poisson neuron (exponential)
println("ISI histogram (80 Hz, 5 seconds):")
spks_long = poisson_spike_train(80.0, 5000.0)
is_long   = inter_spike_intervals(spks_long)
if length(is_long) >= 2
    bins = 0.0:2.5:60.0
    for lo in bins[1:end-1]
        hi    = lo + 2.5
        count = sum(lo .<= is_long .< hi)
        bar   = "█" ^ min(40, count)
        @printf("  %5.1f–%-5.1f ms: %s (%d)\n", lo, hi, bar, count)
    end
end
println()

println("=" ^ 60)
println("  Proceed to 04_signal_analysis.jl")
println("=" ^ 60)
