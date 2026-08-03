# math.jl — General Math Utilities
#
# Numerical calculus, linear algebra helpers, and activation functions.
# All routines use only stdlib packages (LinearAlgebra, Statistics).

# ── Numerical Calculus ────────────────────────────────────────────────────────

"""
    gradient_approx(f, x; h=1e-5) -> Vector{Float64}

Compute the gradient of `f` at point `x` using central finite differences.

```
∂f/∂xᵢ ≈ (f(x + h·eᵢ) - f(x - h·eᵢ)) / (2h)
```

# Arguments
- `f`: function `ℝⁿ → ℝ`
- `x`: point at which to evaluate the gradient
- `h`: step size (default `1e-5`)

# Example
```julia
f = x -> x[1]^2 + 2*x[2]^2
g = gradient_approx(f, [1.0, 1.0])   # ≈ [2.0, 4.0]
```
"""
function gradient_approx(f, x::AbstractVector{<:Real}; h::Float64 = 1e-5)::Vector{Float64}
    x = Float64.(x)
    n = length(x)
    grad = Vector{Float64}(undef, n)
    for i in 1:n
        xp = copy(x); xp[i] += h
        xm = copy(x); xm[i] -= h
        grad[i] = (f(xp) - f(xm)) / (2h)
    end
    return grad
end

"""
    hessian_approx(f, x; h=1e-4) -> Matrix{Float64}

Compute the Hessian of `f` at point `x` using central finite differences.

```
∂²f/∂xᵢ∂xⱼ ≈ (f(x+heᵢ+heⱼ) - f(x+heᵢ-heⱼ) - f(x-heᵢ+heⱼ) + f(x-heᵢ-heⱼ)) / (4h²)
```

# Example
```julia
f = x -> x[1]^2 + 3*x[2]^2
H = hessian_approx(f, [0.0, 0.0])   # ≈ [2 0; 0 6]
```
"""
function hessian_approx(f, x::AbstractVector{<:Real}; h::Float64 = 1e-4)::Matrix{Float64}
    x = Float64.(x)
    n = length(x)
    H = Matrix{Float64}(undef, n, n)
    for i in 1:n, j in 1:n
        xpp = copy(x); xpp[i] += h; xpp[j] += h
        xpm = copy(x); xpm[i] += h; xpm[j] -= h
        xmp = copy(x); xmp[i] -= h; xmp[j] += h
        xmm = copy(x); xmm[i] -= h; xmm[j] -= h
        H[i, j] = (f(xpp) - f(xpm) - f(xmp) + f(xmm)) / (4h^2)
    end
    return H
end

# ── Activation Functions ──────────────────────────────────────────────────────

"""
    softmax(v) -> Vector{Float64}

Compute the softmax of vector `v`, returning a probability distribution.

```
softmax(v)ᵢ = exp(vᵢ - max(v)) / Σⱼ exp(vⱼ - max(v))
```

The max-shift is applied for numerical stability.

# Example
```julia
p = softmax([1.0, 2.0, 3.0])   # ≈ [0.09, 0.24, 0.67]
sum(p) ≈ 1.0                    # true
```
"""
function softmax(v::AbstractVector{<:Real})::Vector{Float64}
    v = Float64.(v)
    e = exp.(v .- maximum(v))
    return e ./ sum(e)
end

"""
    normalize_vec(v) -> Vector{Float64}

L2-normalise a vector to unit length. Returns the zero vector unchanged.

# Example
```julia
u = normalize_vec([3.0, 4.0])   # [0.6, 0.8]
norm(u) ≈ 1.0                   # true
```
"""
function normalize_vec(v::AbstractVector{<:Real})::Vector{Float64}
    v = Float64.(v)
    n = norm(v)
    n < 1e-12 && return v
    return v ./ n
end

# ── Demo Functions ────────────────────────────────────────────────────────────

"""
    mat_demo()

Demonstrate basic linear algebra operations: matrix multiplication, SVD, and
eigendecomposition.

# Example
```julia
mat_demo()
```
"""
function mat_demo()
    println("── Matrix multiply ───────────────────────────────────────")
    A = [1.0 2.0; 3.0 4.0]
    B = [5.0 6.0; 7.0 8.0]
    C = A * B
    println("A ="); display(A)
    println("B ="); display(B)
    println("A * B ="); display(C)

    println()
    println("── SVD decomposition ─────────────────────────────────────")
    M = randn(4, 3)
    U, S, V = svd(M)
    println("M (4×3) singular values: ", round.(S, digits=4))
    println("Reconstruction error:    ", round(norm(U * Diagonal(S) * V' - M), digits=12))

    println()
    println("── Eigendecomposition ────────────────────────────────────")
    W = Symmetric(randn(4, 4))   # guaranteed real eigenvalues
    vals, vecs = eigen(W)
    println("Symmetric 4×4 eigenvalues: ", round.(vals, digits=4))

    println()
    println("── Random matrix statistics ──────────────────────────────")
    X = randn(100, 5)
    Σ = cov(X)
    println("Sample covariance (5×5), trace = ", round(tr(Σ), digits=4))
end

"""
    math_demo()

Demonstrate gradient approximation, softmax, and normalisation.

# Example
```julia
math_demo()
```
"""
function math_demo()
    println("── Gradient approximation ────────────────────────────────")
    f = x -> x[1]^2 + 2 * x[2]^2 + x[1] * x[2]
    x₀ = [1.0, 2.0]
    g  = gradient_approx(f, x₀)
    println("f(x)  = x₁² + 2x₂² + x₁x₂")
    println("∇f at [1, 2]  = ", round.(g, digits=6))
    println("Exact answer   = [$(2*x₀[1]+x₀[2]), $(4*x₀[2]+x₀[1])]")

    println()
    println("── Hessian ────────────────────────────────────────────────")
    H = hessian_approx(f, x₀)
    println("H ="); display(round.(H, digits=4))

    println()
    println("── Softmax ────────────────────────────────────────────────")
    logits = [1.0, 2.0, 3.0, 0.5]
    p = softmax(logits)
    println("logits = $logits")
    println("softmax = ", round.(p, digits=4))
    println("sum = $(round(sum(p), digits=10))")

    println()
    println("── Normalisation ──────────────────────────────────────────")
    v = [3.0, 4.0]
    u = normalize_vec(v)
    println("v = $v   ‖v‖ = $(norm(v))")
    println("û = $u   ‖û‖ = $(round(norm(u), digits=10))")

    println()
    println("── Symbolics tip ──────────────────────────────────────────")
    println("For symbolic differentiation, install Symbolics.jl:")
    println("  using Pkg; Pkg.add(\"Symbolics\")")
    println("  using Symbolics")
    println("  @variables x y")
    println("  D = Differential(x)")
    println("  expand_derivatives(D(x^2 + 2y^2 + x*y))")
end
