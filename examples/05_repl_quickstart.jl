# examples/05_repl_quickstart.jl
#
# REPL quick-start script — paste snippets into the Julia REPL to explore
# NeuroMathPlayground interactively.
#
# Start a project REPL:
#   julia --project=.
#
# Then include this file to run all snippets, or copy-paste sections you want:
#   include("examples/05_repl_quickstart.jl")

# ── Load the module ───────────────────────────────────────────────────────────
using NeuroMathPlayground   # banner prints automatically

println()
println("Module loaded. Type repl_help() for the full function list.")
println()

# ── Explore LIF dynamics ──────────────────────────────────────────────────────
lif_demo()

println()
println("-" ^ 60)
println()

# ── Neuromorphic spike functions ──────────────────────────────────────────────
neuro_demo()

println()
println("-" ^ 60)
println()

# ── Math utilities ────────────────────────────────────────────────────────────
math_demo()

println()
println("-" ^ 60)
println()

# ── GPU check ─────────────────────────────────────────────────────────────────
println("── GPU status ────────────────────────────────────────────────")
println("  gpu_available() = $(gpu_available())")
println()
println("  To enable GPU support on your RTX 5080:")
println("    using Pkg; Pkg.add(\"CUDA\")")
println("    using CUDA         # loads CUDA.jl into the session")
println("    gpu_info()         # now shows your GPU details")
println("    cuda_kernel_demo() # runs on the GPU")

println()
println("-" ^ 60)
println()

# ── SNN feature pipeline ──────────────────────────────────────────────────────
println("── SNN feature pipeline on a random signal ───────────────────")
import Random; Random.seed!(7)
sig = cumsum(randn(200)) .+ 100.0
f   = snn_features(sig)
println("  hurst        = $(round(f.hurst, digits=4))")
println("  hawkes       = $(round(f.hawkes, digits=4))")
println("  gbm_surprise = $(round(f.gbm_surprise, digits=4))")

println()
println("-" ^ 60)
println()

# ── Kelly sizing ──────────────────────────────────────────────────────────────
println("── Kelly position sizing ─────────────────────────────────────")
fk = full_kelly(0.55, 1.5)
hk = half_kelly(0.55, 1.5)
kf = kelly_fraction(win_rate=0.55, avg_win=8.5, avg_loss=5.2)
println("  full_kelly(p=0.55, b=1.5)                        = $(round(fk, digits=4))")
println("  half_kelly(p=0.55, b=1.5)                        = $(round(hk, digits=4))")
println("  kelly_fraction(wr=0.55, aw=8.5, al=5.2)          = $(round(kf, digits=4))")

println()
println("=" ^ 60)
println("  Type repl_help() for the full reference at any time.")
println("=" ^ 60)
