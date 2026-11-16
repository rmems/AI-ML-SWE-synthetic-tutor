# examples/03_neuro_demo.jl
#
# Neuromorphic computing: LIF neurons, STDP, spike encoding, and SNN features.
#
# Run from the project root:
#   julia --project=. examples/03_neuro_demo.jl

using NeuroMathPlayground
using Statistics
using Random

Random.seed!(42)

println("=" ^ 60)
println("  Neuromorphic Demo")
println("=" ^ 60)
println()

# ── LIF walkthrough ───────────────────────────────────────────────────────────
lif_demo()

println()
println("-" ^ 60)
println()

# ── Spike functions and kernels ───────────────────────────────────────────────
neuro_demo()

println()
println("-" ^ 60)
println()

# ── SNN feature extraction ────────────────────────────────────────────────────
println("── SNN Feature Extraction from a price signal ────────────────")
signal = cumsum(randn(200)) .+ 100.0
f = snn_features(signal)
println("  Signal length : $(length(signal))")
println("  Hurst H       : $(round(f.hurst, digits=4))  ($(f.hurst > 0.5 ? "trending" : "mean-reverting"))")
println("  Hawkes λ      : $(round(f.hawkes, digits=4))")
println("  GBM surprise z: $(round(f.gbm_surprise, digits=4))")

println()
println("-" ^ 60)
println()

# ── Poisson spike train statistics ───────────────────────────────────────────
println("── Poisson Spike Train Analysis (40 Hz, 2 s) ─────────────────")
spikes = poisson_spike_train(40.0, 2000.0)
isis   = inter_spike_intervals(spikes)
cv     = cv_isi(spikes)
println("  Spikes fired  : $(length(spikes))  (expected ≈ 80)")
println("  Mean ISI      : $(round(mean(isis), digits=1)) ms  (expected ≈ 25 ms)")
println("  CV_ISI        : $(round(cv, digits=3))  (Poisson → 1.0)")

println()
println("-" ^ 60)
println()

# ── Rate coding pipeline ──────────────────────────────────────────────────────
println("── Rate Coding Pipeline ──────────────────────────────────────")
println("  Encoding sensor values in [0, 1] as spike trains (1 second)")
println()
for val in [0.0, 0.25, 0.5, 0.75, 1.0]
    spks  = encode_rate(val, 0.0, 1.0, 1000.0)
    phase = rate_to_phase(val * 200.0)
    act   = lif_activation(val * 2.0)
    println("  x=$(val)  → $(lpad(length(spks), 3)) spikes  " *
            "phase=$(lpad(round(phase, digits=1), 7)) ms  " *
            "lif=$(round(act, digits=3))")
end

println()
println("-" ^ 60)
println()

# ── Spike kernel convolution ──────────────────────────────────────────────────
println("── PSP Kernel: effect of a spike over time ───────────────────")
println("  τ = 20 ms (standard PSP decay)")
dt_values = [0, 5, 10, 20, 30, 50, 100]
for dt in dt_values
    k = spike_kernel(Float64(dt))
    bar = "█" ^ round(Int, k * 30)
    println("  t+$(lpad(dt, 3)) ms  K=$(round(k, digits=3))  $bar")
end
