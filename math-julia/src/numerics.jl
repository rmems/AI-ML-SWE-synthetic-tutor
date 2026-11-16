# numerics.jl — numerical calculus and vector helpers.
#
# Carried over unchanged (minus the REPL demos) from the pre-pivot playground,
# legacy/NeuroMathPlayground/src/math.jl at a9dcb94. Generators use these as an
# independent numeric cross-check on their exact answers.

"""
    gradient_approx(f, x; h=1e-5) -> Vector{Float64}

Gradient of `f: ℝⁿ → ℝ` at `x` by central finite differences:
`∂f/∂xᵢ ≈ (f(x + h·eᵢ) - f(x - h·eᵢ)) / (2h)`.
"""
function gradient_approx(f, x::AbstractVector{<:Real}; h::Float64 = 1e-5)::Vector{Float64}
    x = Float64.(x)
    n = length(x)
    grad = Vector{Float64}(undef, n)
    for i in 1:n
        xp = copy(x)
        xp[i] += h
        xm = copy(x)
        xm[i] -= h
        grad[i] = (f(xp) - f(xm)) / (2h)
    end
    return grad
end

"""
    hessian_approx(f, x; h=1e-4) -> Matrix{Float64}

Hessian of `f: ℝⁿ → ℝ` at `x` by central finite differences.
"""
function hessian_approx(f, x::AbstractVector{<:Real}; h::Float64 = 1e-4)::Matrix{Float64}
    x = Float64.(x)
    n = length(x)
    H = Matrix{Float64}(undef, n, n)
    for i in 1:n, j in 1:n
        xpp = copy(x)
        xpp[i] += h
        xpp[j] += h
        xpm = copy(x)
        xpm[i] += h
        xpm[j] -= h
        xmp = copy(x)
        xmp[i] -= h
        xmp[j] += h
        xmm = copy(x)
        xmm[i] -= h
        xmm[j] -= h
        H[i, j] = (f(xpp) - f(xpm) - f(xmp) + f(xmm)) / (4h^2)
    end
    return H
end

"""
    softmax(v) -> Vector{Float64}

Max-shifted softmax: `exp(vᵢ - max(v)) / Σⱼ exp(vⱼ - max(v))`.
"""
function softmax(v::AbstractVector{<:Real})::Vector{Float64}
    v = Float64.(v)
    e = exp.(v .- maximum(v))
    return e ./ sum(e)
end

"""
    normalize_vec(v) -> Vector{Float64}

L2-normalise `v`. A vector with norm below `1e-12` is returned unchanged.
"""
function normalize_vec(v::AbstractVector{<:Real})::Vector{Float64}
    v = Float64.(v)
    n = sqrt(sum(abs2, v))
    n < 1e-12 && return v
    return v ./ n
end
