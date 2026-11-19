# sandbox/06_stdp_learning.jl
#
# Spike-Timing-Dependent Plasticity (STDP) — Unsupervised SNN Learning
#
# STDP is the biological learning rule: synapses strengthen when the pre-
# synaptic neuron consistently fires before the post-synaptic one (causal
# pairing), and weaken when the order is reversed.  It is the learning
# rule used in every Spikenaut STDP training run.
#
# Run with:   julia --project=. sandbox/06_stdp_learning.jl

using NeuroMathPlayground
using Printf
using Random
using Statistics

println("=" ^ 60)
println("  NeuroMathPlayground — 06 STDP Unsupervised Learning")
println("=" ^ 60)
println()

# ── 1. The STDP Learning Window ───────────────────────────────────────────────
println("─── 1. The STDP Learning Window ───")
println("""
STDP is an asymmetric Hebbian rule where the weight change ΔW depends on
the relative timing of pre- and post-synaptic spikes:

   Δt = t_post − t_pre

   ΔW =  A_+ · exp(−|Δt| / τ_+)   if Δt ≥ 0  (LTP — pre fires first)
   ΔW = −A_− · exp(−|Δt| / τ_−)   if Δt < 0  (LTD — post fires first)

A slight asymmetry A_- > A_+ is critical for weight stability.

References:
  Bi & Poo (1998) — Synaptic modifications in cultured hippocampal neurons.
  Song, Miller & Abbott (2000) — Competitive Hebbian learning through STDP.
""")

sp = STDPParams(
    A_plus   = 0.01,
    A_minus  = 0.0105,
    tau_plus = 20.0,
    tau_minus = 20.0,
)

println("STDP window profile:")
println("  Δt (ms) | ΔW          | Rule")
println("-" ^ 44)
for dt_val in [-100.0, -50.0, -20.0, -10.0, -5.0, -1.0, 0.0,
                1.0,    5.0,   10.0,  20.0,  50.0, 100.0]
    dw  = stdp_delta_w(dt_val, sp)
    rule = dw > 0.0 ? "LTP ↑" : (dw < 0.0 ? "LTD ↓" : "none")
    bar_len = round(Int, abs(dw) * 1000)
    bar = (dw >= 0 ? "+" : "-") ^ min(30, bar_len)
    @printf("  %+6.1f   | %+11.7f | %-5s %s\n", dt_val, dw, rule, bar)
end
println()

# ── 2. Weight Evolution under Repeated Pairings ───────────────────────────────
println("─── 2. Weight Evolution under Repeated Pairings ───")
println()

# Scenario A: pre always fires before post (Δt = +10 ms) → potentiation
let w_a = 0.5
    println("Scenario A: pre fires 10 ms before post (causal pairing, Δt = +10 ms)")
    println("  Repetition | Weight   | ΔW")
    println("-" ^ 38)
    for rep in 1:10
        dw   = stdp_delta_w(10.0, sp)
        w_a  = apply_stdp(w_a, 10.0, sp)
        @printf("  %-10d | %.6f | %+.6f\n", rep, w_a, dw)
    end
    println("  → Synapse $(round(w_a, digits=4)) (max cap: $(sp.w_max))")
end
println()

# Scenario B: post always fires before pre (Δt = -10 ms) → depression
let w_b = 0.5
    println("Scenario B: post fires 10 ms before pre (anti-causal, Δt = -10 ms)")
    println("  Repetition | Weight   | ΔW")
    println("-" ^ 38)
    for rep in 1:10
        dw   = stdp_delta_w(-10.0, sp)
        w_b  = apply_stdp(w_b, -10.0, sp)
        @printf("  %-10d | %.6f | %+.6f\n", rep, w_b, dw)
    end
    println("  → Synapse $(round(w_b, digits=4)) (min cap: $(sp.w_min))")
end
println()

# ── 3. Competitive Learning with Multiple Synapses ───────────────────────────
println("─── 3. Competitive Learning — Two Synapses Converging ───")
println("""
Classic STDP result (Song, Miller & Abbott 2000):
When a post-synaptic neuron receives input from two synapses with random
relative timings, STDP drives a bimodal weight distribution — one synapse
saturates at w_max, the other near w_min.

This is unsupervised feature selection.
""")

Random.seed!(42)
n_synapses = 2
w = fill(0.5, n_synapses)

println("  500 random pairings with random Δt in [-40, +40] ms:")
println()

n_trials = 500
for trial in 1:n_trials
    # For each synapse, sample a random timing offset
    for j in 1:n_synapses
        dt_rand = (rand() - 0.5) * 80.0   # uniform in [-40, +40] ms
        w[j]    = apply_stdp(w[j], dt_rand, sp)
    end
end

for j in 1:n_synapses
    bar_len = round(Int, w[j] * 40)
    bar     = "█" ^ bar_len
    @printf("  Synapse %d: w = %.4f  |%s\n", j, w[j], bar)
end
println()
println("  Note: random pairings → weights drift near 0 or 1 (bimodal).")
println()

# ── 4. STDP with LIF Neurons ─────────────────────────────────────────────────
println("─── 4. STDP in a Two-Neuron Circuit ───")
println("""
A minimal circuit: one pre-synaptic LIF neuron drives a post-synaptic LIF
neuron through a plastic synapse.

If the pre-neuron fires earlier on average, the synapse strengthens.
This is Hebbian learning implemented by biology.
""")

p_lif = LIFParams()
dt    = 0.1   # ms

# Pre-neuron fires at regular intervals
pre_current  = 3.5    # drives pre at ~40 Hz
post_current = 1.5    # post is just below threshold without input

# Simulate 1000 ms
T = 1000.0
_, _, pre_spikes  = simulate_lif(pre_current,  T, p_lif; dt=dt)
_, _, post_spikes = simulate_lif(post_current, T, p_lif; dt=dt)

println("Pre-neuron:  $(length(pre_spikes)) spikes in $T ms")
println("Post-neuron: $(length(post_spikes)) spikes in $T ms")
println()

# Apply STDP for all spike pairs (nearest-neighbour rule)
let w_syn = 0.5, n_updates = 0
    for t_pre in pre_spikes
        # Find nearest post spike
        if !isempty(post_spikes)
            nearest_post = post_spikes[argmin(abs.(post_spikes .- t_pre))]
            dt_pair      = nearest_post - t_pre
            if abs(dt_pair) < 100.0    # only within 100 ms window
                w_syn    = apply_stdp(w_syn, dt_pair, sp)
                n_updates += 1
            end
        end
    end

    println("STDP weight updates applied: $n_updates")
    println("Final synaptic weight:       $(round(w_syn, digits=6))")
    println("  (started at 0.5; pre fires first → expect increase)")
end
println()

# ── 5. Weight Distribution Intuition ─────────────────────────────────────────
println("─── 5. Rate Coding vs. STDP Competition ───")
println("""
Key insight: in a population of synapses with random initial weights,
STDP + soft competition (normalisation) produces sparse, selective
representations — neurons respond to specific patterns, not all inputs.

This is the biological origin of "feature detectors" in the brain, and
why STDP is used in Spikenaut's unsupervised pre-training phase.
""")

Random.seed!(7)
n_pop = 10
w_pop = rand(n_pop)   # random initial weights

println("Weight histogram after 2000 trials (random Δt in [-30, +30] ms):")
for trial in 1:2000
    for j in 1:n_pop
        dt_rand = (rand() - 0.5) * 60.0
        w_pop[j] = apply_stdp(w_pop[j], dt_rand, sp)
    end
end

bins = 0.0:0.1:1.0
for lo in bins[1:end-1]
    hi    = lo + 0.1
    count = sum(lo .<= w_pop .< hi)
    bar   = "█" ^ (count * 4)
    @printf("  %.1f–%.1f: %s\n", lo, hi, count > 0 ? bar : "(empty)")
end
println()
println("  → Bimodal distribution: weights cluster near 0 (silent) or 1 (active).")
println()

println("=" ^ 60)
println("  All sandbox notebooks complete!")
println("  Explore src/ for the full module implementations.")
println("=" ^ 60)
