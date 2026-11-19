# generators.jl — seeded problem generators.
#
# Each generator takes a seed and returns a NamedTuple that serialises to one
# `schemas/problem.schema.json` object (without a `verification` block; Rust adds
# that after it has checked the answer independently).

"""
Largest accepted seed, `2^53 - 1`, so seeds survive any JSON reader as exact integers.
"""
const MAX_SEED = 2^53 - 1

const POLYNOMIAL_DERIVATIVE_GENERATOR = "polynomial_derivative_at_point"
const POLYNOMIAL_DERIVATIVE_VERSION = 1
const POLYNOMIAL_DERIVATIVE_TOPIC = "calculus.derivatives.power_rule"

"""
    derivative_at(coefficients, x) -> Int

Exact value of `p'(x)` for the integer polynomial `p(x) = Σₖ coefficients[k+1] xᵏ`
(coefficients in ascending order). Throws `OverflowError` on `Int` overflow.
"""
function derivative_at(coefficients::AbstractVector{<:Integer}, x::Integer)::Int
    acc = 0
    for k in (length(coefficients) - 1):-1:1
        term = Base.checked_mul(k, Int(coefficients[k + 1]))
        acc = Base.checked_add(Base.checked_mul(acc, Int(x)), term)
    end
    return acc
end

"""
    format_polynomial(coefficients) -> String

Render an integer polynomial (ascending coefficients, nonzero leading term) in
descending powers, e.g. `[5, -2, 3]` → `"3x^2 - 2x + 5"`.
"""
function format_polynomial(coefficients::AbstractVector{<:Integer})::String
    isempty(coefficients) && throw(ArgumentError("polynomial has no coefficients"))
    iszero(coefficients[end]) && throw(ArgumentError("leading coefficient must be nonzero"))
    parts = String[]
    for k in (length(coefficients) - 1):-1:0
        a = coefficients[k + 1]
        iszero(a) && continue
        power = k == 0 ? "" : k == 1 ? "x" : "x^$k"
        magnitude = abs(a)
        body = (magnitude == 1 && k > 0) ? power : string(magnitude, power)
        if isempty(parts)
            push!(parts, a < 0 ? "-" * body : body)
        else
            push!(parts, (a < 0 ? "- " : "+ ") * body)
        end
    end
    return join(parts, " ")
end

"""
    polynomial_derivative_at_point(seed) -> NamedTuple

Generate "Let f(x) = … . Find f'(a)." for an integer polynomial of degree 1–4 with
coefficients in `-9:9` (nonzero leading term) and `a` in `-5:5`, with its exact
integer answer.

The draw is fully determined by `seed` through `Random.Xoshiro`; the Julia version
is recorded in `generator.runtime_version` because Julia does not promise identical
random streams across releases. The exact answer is cross-checked against
`gradient_approx` before it is returned; disagreement is an error, never a row.
"""
function polynomial_derivative_at_point(seed::Integer)
    (seed isa Bool || !(0 <= seed <= MAX_SEED)) &&
        throw(DomainError(seed, "seed must be an integer in 0:$(MAX_SEED)"))
    rng = Xoshiro(seed)
    degree = rand(rng, 1:4)
    coefficients = [rand(rng, -9:9) for _ in 0:degree]
    while iszero(coefficients[end])
        coefficients[end] = rand(rng, -9:9)
    end
    point = rand(rng, -5:5)
    answer = derivative_at(coefficients, point)

    p = x -> evalpoly(x[1], Float64.(coefficients))
    numeric = only(gradient_approx(p, [Float64(point)]))
    isapprox(numeric, answer; rtol = 1e-6, atol = 1e-4) || error(
        "exact derivative $(answer) disagrees with finite difference $(numeric) for seed $(seed)",
    )

    return (
        schema_version = 1,
        problem_id = "$(POLYNOMIAL_DERIVATIVE_GENERATOR)/v$(POLYNOMIAL_DERIVATIVE_VERSION)/seed-$(seed)",
        topic = POLYNOMIAL_DERIVATIVE_TOPIC,
        generator = (
            name = POLYNOMIAL_DERIVATIVE_GENERATOR,
            version = POLYNOMIAL_DERIVATIVE_VERSION,
            language = "julia",
            runtime_version = string(VERSION),
            seed = Int(seed),
        ),
        statement = "Let f(x) = $(format_polynomial(coefficients)). Find f'($(point)).",
        parameters = (
            kind = POLYNOMIAL_DERIVATIVE_GENERATOR,
            coefficients = coefficients,
            point = point,
        ),
        answer = (kind = "integer", value = answer),
    )
end

"""
Generators the worker serves, by name.
"""
const GENERATORS = Dict{String,Function}(
    POLYNOMIAL_DERIVATIVE_GENERATOR => polynomial_derivative_at_point,
)
