# examples/02_math_demo.jl
#
# Matrix operations, numerical calculus, and activation function demos.
#
# Run from the project root:
#   julia --project=. examples/02_math_demo.jl

using NeuroMathPlayground
using LinearAlgebra

println("=" ^ 60)
println("  Math Demo")
println("=" ^ 60)
println()

# ── Linear Algebra ────────────────────────────────────────────────────────────
mat_demo()

println()
println("-" ^ 60)
println()

# ── Numerical Calculus ────────────────────────────────────────────────────────
math_demo()

println()
println("-" ^ 60)
println()

# ── Jacobian of a vector-valued function ──────────────────────────────────────
println("── Jacobian (vector-valued f : ℝ² → ℝ²) ─────────────────────")
f2 = x -> [x[1]^2 - x[2]^2, 2 * x[1] * x[2]]
x0 = [1.0, 1.0]
n_out = 2
J = hcat([gradient_approx(x -> f2(x)[i], x0) for i in 1:n_out]...)'
println("f(x) = [x₁² - x₂², 2x₁x₂]   at x = $x0")
println("J ≈"); display(round.(J, digits=4))
println("Exact: [2 -2; 2 2]")

println()
println("-" ^ 60)
println()

# ── Softmax probability distribution ─────────────────────────────────────────
println("── Softmax as attention weights ──────────────────────────────")
scores = [0.2, 1.5, 0.3, 2.1, 0.8]
weights = softmax(scores)
println("Raw scores:   ", round.(scores, digits=2))
println("Attention:    ", round.(weights, digits=4))
println("Max attends to index $(argmax(weights)) (score = $(scores[argmax(weights)]))")

println()

# ── Gradient descent toy example ──────────────────────────────────────────────
println("── Gradient descent on f(x,y) = x² + 5y² ───────────────────")
f_bowl = x -> x[1]^2 + 5 * x[2]^2
x = [3.0, 2.0]
lr = 0.1
println("  Starting at: $x   f = $(f_bowl(x))")
for iter in 1:20
    g = gradient_approx(f_bowl, x)
    x = x .- lr .* g
end
println("  After 20 steps: $(round.(x, digits=6))   f = $(round(f_bowl(x), digits=8))")
println("  Minimum is at [0, 0]   f = 0")
