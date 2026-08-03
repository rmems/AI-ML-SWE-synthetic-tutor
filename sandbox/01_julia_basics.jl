# sandbox/01_julia_basics.jl
#
# Julia Language Fundamentals — a hands-on tour of the syntax and features
# you will encounter in every Limen-Neural repository.
#
# Run this file with:
#   julia sandbox/01_julia_basics.jl
#
# Or paste sections into an interactive REPL:
#   julia --project=.

println("=" ^ 60)
println("  NeuroMathPlayground — 01 Julia Basics")
println("=" ^ 60)
println()

# ── 1. Variables and Types ────────────────────────────────────────────────────
println("─── 1. Variables and Types ───")

x = 3.14          # Float64 by default
n = 42            # Int64 by default
s = "hello"       # String
b = true          # Bool

println("x = $x  ($(typeof(x)))")
println("n = $n  ($(typeof(n)))")
println("s = $s  ($(typeof(s)))")
println("b = $b  ($(typeof(b)))")

# Explicit type annotation
tau_m::Float64 = 20.0   # membrane time constant (ms) — like in LIF neurons
println("tau_m = $tau_m ms")
println()

# ── 2. Arithmetic and Math ────────────────────────────────────────────────────
println("─── 2. Arithmetic and Math ───")

# Standard operators
println("2^10 = $(2^10)")
println("sqrt(2) = $(sqrt(2))")
println("exp(1) = $(exp(1))")
println("log(exp(3)) = $(log(exp(3)))")

# Unicode math — Julia accepts UTF-8 identifiers
σ = 0.20          # volatility (GBM)
μ = 0.05          # drift
τ = 20.0          # time constant
println("σ = $σ,  μ = $μ,  τ = $τ")

# Broadcasting with dot syntax (element-wise operations)
xs = [1.0, 2.0, 3.0, 4.0]
println("exp.(xs) = $(exp.(xs))")
println("xs .^ 2  = $(xs .^ 2)")
println()

# ── 3. Vectors and Ranges ─────────────────────────────────────────────────────
println("─── 3. Vectors and Ranges ───")

# Range (lazy, memory-efficient)
ts = 0.0:0.1:1.0          # time axis 0..1 ms with 0.1 step
println("First 5 times: $(collect(ts)[1:5])")

# Vector construction
v = zeros(5)              # 5-element zero vector
v[3] = 42.0
println("v = $v")

# Comprehension (very common in Julia numerical code)
spikes = [t for t in 0:10:100]   # spike times at 10 ms intervals
println("Spike times: $spikes")

# push! and append! for dynamic vectors
trace = Float64[]
for t in 1:5
    push!(trace, t * 2.0)
end
println("Trace: $trace")
println()

# ── 4. Functions ──────────────────────────────────────────────────────────────
println("─── 4. Functions ───")

# Standard function definition
function membrane_update(v, I, tau, v_rest, R, dt)
    return v + (dt / tau) * (-(v - v_rest) + R * I)
end

v_new = membrane_update(-65.0, 3.5, 20.0, -65.0, 10.0, 0.1)
println("V after one step: $(round(v_new, digits=4)) mV")

# One-liner ("short-form") function
lif_leak(v, v_rest, tau) = -(v - v_rest) / tau

# Keyword arguments
function kelly(; win_rate, payoff)
    q = 1.0 - win_rate
    return (win_rate * payoff - q) / payoff
end

println("Kelly fraction: $(round(kelly(win_rate=0.55, payoff=1.5), digits=4))")

# Multiple return values (tuple)
function minmax_pair(xs)
    return minimum(xs), maximum(xs)
end
lo, hi = minmax_pair([3, 1, 4, 1, 5, 9, 2, 6])
println("min=$lo, max=$hi")
println()

# ── 5. Control Flow ───────────────────────────────────────────────────────────
println("─── 5. Control Flow ───")

# if / elseif / else
function classify_hurst(h)
    if h > 0.5
        return "Trending (persistent)"
    elseif h < 0.5
        return "Mean-reverting (anti-persistent)"
    else
        return "Random walk"
    end
end

for h in [0.3, 0.5, 0.7]
    println("H=$h → $(classify_hurst(h))")
end

# for loop
total_spikes = 0
for rate in [10, 20, 40, 80]   # Hz
    global total_spikes
    expected = rate * 1          # 1 second
    total_spikes += expected
end
println("Total expected spikes (1s): $total_spikes")

# while loop
let v = -65.0, t = 0.0
    while v < -50.0 && t < 100.0
        v = membrane_update(v, 3.5, 20.0, -65.0, 10.0, 0.1)
        t += 0.1
    end
    println("Threshold reached at t ≈ $(round(t, digits=1)) ms")
end
println()

# ── 6. Structs ────────────────────────────────────────────────────────────────
println("─── 6. Structs ───")

struct Neuron
    id::Int
    tau_m::Float64      # membrane time constant (ms)
    v_thresh::Float64   # spike threshold (mV)
end

n1 = Neuron(1, 20.0, -50.0)
n2 = Neuron(2, 15.0, -52.0)
println("Neuron 1: τ=$(n1.tau_m) ms, threshold=$(n1.v_thresh) mV")
println("Neuron 2: τ=$(n2.tau_m) ms, threshold=$(n2.v_thresh) mV")
println()

# ── 7. Multiple Dispatch ──────────────────────────────────────────────────────
println("─── 7. Multiple Dispatch ───")

# Julia selects the method based on all argument types (not just the first)
describe(x::Float64)  = "real number $x"
describe(x::Int)      = "integer $x"
describe(x::String)   = "string \"$x\""
describe(v::Vector)   = "vector of length $(length(v))"

println(describe(3.14))
println(describe(42))
println(describe("hello"))
println(describe([1.0, 2.0, 3.0]))
println()

# ── 8. Anonymous Functions and map / filter ───────────────────────────────────
println("─── 8. Higher-order Functions ───")

rates = [10.0, 25.0, 40.0, 80.0, 120.0]    # Hz

# map: apply a function to every element
isis = map(r -> 1000.0 / r, rates)    # expected ISI (ms)
println("Firing rates (Hz): $rates")
println("Expected ISIs (ms): $(round.(isis, digits=1))")

# filter: keep elements satisfying a predicate
fast_neurons = filter(r -> r > 50.0, rates)
println("Fast neurons (>50 Hz): $fast_neurons")

# Chaining with anonymous functions
log_returns = map(x -> log(x / 100.0), [98.0, 101.0, 99.5, 103.0])
println("Log-returns: $(round.(log_returns, digits=4))")
println()

# ── 9. Using Statistics stdlib ────────────────────────────────────────────────
println("─── 9. Statistics ───")

using Statistics

data = randn(1000)
println("mean = $(round(mean(data), digits=3))")
println("std  = $(round(std(data), digits=3))")
println("var  = $(round(var(data), digits=3))")

# Correlation between two series
x_data = randn(100)
y_data = 0.8 * x_data + 0.2 * randn(100)
r = cor(x_data, y_data)
println("Correlation (should be ≈0.8): $(round(r, digits=3))")
println()

println("=" ^ 60)
println("  Basics complete — proceed to 02_lif_neuron.jl")
println("=" ^ 60)
