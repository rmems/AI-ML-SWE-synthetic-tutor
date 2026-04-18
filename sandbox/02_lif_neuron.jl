# sandbox/02_lif_neuron.jl
#
# Leaky Integrate-and-Fire (LIF) Neuron Dynamics
#
# The LIF model is the simplest spiking neuron that captures the essential
# dynamics of biological neurons:  passive membrane leak, threshold crossing,
# and reset.  It forms the backbone of every Spikenaut spiking layer.
#
# Run with:   julia --project=. sandbox/02_lif_neuron.jl

using MathSandbox
using Printf
using Statistics

println("=" ^ 60)
println("  MathSandbox — 02 LIF Neuron Dynamics")
println("=" ^ 60)
println()

# ── The Mathematics ───────────────────────────────────────────────────────────
println("""
─── The LIF Governing ODE ───

   τ_m · dV/dt = -(V - V_rest) + R_m · I(t)

where
   V       membrane potential (mV)
   τ_m     membrane time constant — how fast the membrane "forgets" (ms)
   V_rest  resting potential (mV)
   R_m     membrane resistance (MΩ)
   I(t)    injected current (nA)

When V reaches V_thresh a spike is emitted and V resets to V_reset.

Euler discretisation (Δt small):
   V(t+Δt) = V(t) + Δt/τ_m · [-(V(t) - V_rest) + R_m · I]
""")

# ── Parameter Setup ───────────────────────────────────────────────────────────
p = LIFParams(
    tau_m    = 20.0,   # ms — typical cortical neuron
    v_rest   = -65.0,  # mV
    v_thresh = -50.0,  # mV
    v_reset  = -65.0,  # mV
    r_m      = 10.0,   # MΩ
)

println("LIF Parameters:")
println("  τ_m      = $(p.tau_m) ms")
println("  V_rest   = $(p.v_rest) mV")
println("  V_thresh = $(p.v_thresh) mV")
println("  V_reset  = $(p.v_reset) mV")
println("  R_m      = $(p.r_m) MΩ")
println()

# ── Single Step Walkthrough ───────────────────────────────────────────────────
println("─── Manual Step-by-Step (first 10 steps) ───")
let v = p.v_rest, I = 3.5, dt = 0.1
    println("t (ms) | V (mV)  | dV (mV) | spiked")
    println("-" ^ 45)
    for step in 1:10
        t   = (step - 1) * dt
        dv  = (dt / p.tau_m) * (-(v - p.v_rest) + p.r_m * I)
        v_new, spiked = lif_step(v, I, p, dt)
        @printf("%-6.1f | %-7.3f | %-7.4f | %s\n", t, v, dv, spiked ? "YES" : "")
        v = v_new
    end
end
println()

# ── Full Simulation ───────────────────────────────────────────────────────────
println("─── Full Simulation (200 ms, I = 3.5 nA) ───")
times, voltages, spike_times = simulate_lif(3.5, 200.0, p; dt=0.1)

println("Duration:    200 ms")
println("Spike count: $(length(spike_times))")
println("Firing rate: $(round(firing_rate(spike_times, 200.0), digits=1)) Hz")

if length(spike_times) >= 2
    isis = diff(spike_times)
    println("Mean ISI:    $(round(mean(isis), digits=2)) ms")
    println("CV(ISI):     $(round(std(isis)/mean(isis), digits=3))  (0 = regular clock)")
end
println()

# ── F-I Curve (Firing Rate vs. Input Current) ─────────────────────────────────
println("─── F-I Curve (Firing Rate vs. Input Current) ───")
println("The F-I curve shows how strongly a neuron responds to different inputs.")
println()
println("I (nA) | Rate (Hz) | Spike count")
println("-" ^ 40)
for I_val in [1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 5.0]
    _, _, spks = simulate_lif(I_val, 500.0, p; dt=0.1)
    rate = firing_rate(spks, 500.0)
    bar  = "█" ^ min(40, round(Int, rate / 5))
    @printf("%-6.1f | %-9.1f | %s\n", I_val, rate, bar)
end
println()

# ── Rheobase: minimum current to produce spikes ───────────────────────────────
println("─── Rheobase Current ───")
println("Scanning for the minimum current that drives spiking...")
for I_val in 0.1:0.1:5.0
    _, _, spks = simulate_lif(I_val, 500.0, p; dt=0.1)
    if length(spks) > 0
        println("Rheobase ≈ $I_val nA  ($(length(spks)) spikes in 500 ms)")
        break
    end
end
println()

# ── STDP Learning Rule ────────────────────────────────────────────────────────
println("─── STDP: Spike-Timing-Dependent Plasticity ───")
println("""
When a pre-synaptic spike arrives BEFORE the post-synaptic spike
(Δt = t_post − t_pre > 0), the synapse is potentiated (LTP).
When post fires before pre (Δt < 0), the synapse is depressed (LTD).

   ΔW =  A_+ · exp(−|Δt| / τ_+)   if Δt ≥ 0  (LTP)
   ΔW = −A_− · exp(−|Δt| / τ_−)   if Δt < 0  (LTD)
""")

sp = STDPParams()
println("STDP Parameters:")
println("  A_+   = $(sp.A_plus)  (potentiation amplitude)")
println("  A_-   = $(sp.A_minus)  (depression amplitude)")
println("  τ_+   = $(sp.tau_plus) ms")
println("  τ_-   = $(sp.tau_minus) ms")
println()

println("Δt (ms) | ΔW        | Direction")
println("-" ^ 40)
for dt_val in [-50.0, -20.0, -10.0, -5.0, -1.0, 0.0, 1.0, 5.0, 10.0, 20.0, 50.0]
    dw  = stdp_delta_w(dt_val, sp)
    dir = dw > 0 ? "LTP ↑" : (dw < 0 ? "LTD ↓" : "neutral")
    @printf("%-7.1f | %+9.6f | %s\n", dt_val, dw, dir)
end
println()

println("─── Applying STDP to a Synapse ───")
let w = 0.5
    println("Initial weight: $w")
    for (event, dt_val) in [("pre before post", 10.0), ("pre before post", 5.0),
                             ("post before pre", -8.0), ("post before pre", -3.0)]
        w_old = w
        w     = apply_stdp(w, dt_val, sp)
        @printf("  %s (Δt=%+.0f ms): w %+.6f → %.6f\n", event, dt_val, w - w_old, w)
    end
end
println()

println("=" ^ 60)
println("  Proceed to 03_stochastic_processes.jl")
println("=" ^ 60)
