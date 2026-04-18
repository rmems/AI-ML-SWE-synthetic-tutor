# examples/04_plotting_demo.jl
#
# Visualisation examples using Plots.jl.
#
# Requires Plots.jl — install once with:
#   julia --project=. -e 'using Pkg; Pkg.add("Plots")'
#
# Then run:
#   julia --project=. examples/04_plotting_demo.jl
#
# Plots are saved as PNG files in the current directory.

using NeuroMathPlayground

# ── Load Plots (optional dependency) ─────────────────────────────────────────
try
    @eval using Plots
catch
    println("""
Plots.jl is not installed.
Install it with:
  using Pkg; Pkg.add("Plots")
Then re-run this script.
""")
    exit(0)
end

println("Generating plots — this may take a moment on first run (precompilation)...")
println()

# ── 1. LIF membrane voltage trace ─────────────────────────────────────────────
p = LIFParams()
ts, vs, spks = simulate_lif(3.5, 200.0, p; dt=0.1)

plt1 = plot(ts, vs;
    xlabel = "Time (ms)", ylabel = "Voltage (mV)",
    title  = "LIF Neuron — I = 3.5 nA",
    label  = "V(t)", lw = 1.5, color = :steelblue,
    legend = :topright,
)
hline!(plt1, [p.v_thresh]; label = "V_thresh", ls = :dash, color = :red, lw = 1)
scatter!(plt1, spks, fill(p.v_thresh, length(spks));
    label = "spikes", marker = :circle, ms = 4, color = :orange,
)
savefig(plt1, "lif_trace.png")
println("Saved: lif_trace.png  ($(length(spks)) spikes at $(round(firing_rate(spks,200.0),digits=1)) Hz)")

# ── 2. F-I curve ──────────────────────────────────────────────────────────────
currents = 0.5:0.25:6.0
rates    = [firing_rate(simulate_lif(Float64(I), 500.0, p; dt=0.1)[3], 500.0) for I in currents]

plt2 = plot(collect(currents), rates;
    xlabel = "Input current I (nA)", ylabel = "Firing rate (Hz)",
    title  = "F-I Curve", label = "firing rate",
    lw = 2, marker = :circle, ms = 3, color = :purple,
)
savefig(plt2, "fi_curve.png")
println("Saved: fi_curve.png")

# ── 3. GBM price path ─────────────────────────────────────────────────────────
times, prices = simulate_gbm(100.0, 0.08, 0.20, 1.0, 1/252)

plt3 = plot(times, prices;
    xlabel = "Time (years)", ylabel = "Price",
    title  = "Geometric Brownian Motion  (μ=8%, σ=20%)",
    label  = "S(t)", lw = 1.5, color = :forestgreen,
)
savefig(plt3, "gbm_path.png")
println("Saved: gbm_path.png  (final = $(round(prices[end], digits=2)))")

# ── 4. STDP learning window ───────────────────────────────────────────────────
sp     = STDPParams()
dts    = -80.0:1.0:80.0
dws    = [stdp_delta_w(Float64(dt), sp) for dt in dts]

plt4 = plot(dts, dws;
    xlabel = "Δt = t_post − t_pre (ms)", ylabel = "ΔW",
    title  = "STDP Learning Window",
    label  = "ΔW(Δt)", lw = 2, color = :crimson,
)
hline!(plt4, [0.0]; label = nothing, ls = :dot, color = :black)
vline!(plt4, [0.0]; label = nothing, ls = :dot, color = :black)
savefig(plt4, "stdp_window.png")
println("Saved: stdp_window.png")

# ── 5. Spike kernel ───────────────────────────────────────────────────────────
kernel_t = 0.0:0.5:100.0
kernel_v = spike_kernel.(kernel_t, 20.0)

plt5 = plot(kernel_t, kernel_v;
    xlabel = "Time after spike (ms)", ylabel = "K(t)",
    title  = "Exponential PSP Kernel  (τ = 20 ms)",
    label  = "K(t) = exp(-t/τ)", lw = 2, color = :darkorange,
    fill   = (0, 0.15, :darkorange),
)
savefig(plt5, "spike_kernel.png")
println("Saved: spike_kernel.png")

println()
println("All plots saved.  Open them with any image viewer.")
