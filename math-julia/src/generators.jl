# generators.jl — seeded problem generators.
#
# Each generator takes a seed and returns a NamedTuple that serialises to one
# `schemas/problem.schema.json` object (without a `verification` block; Rust adds
# that after it has checked the answer independently).
#
# Steps are emitted in the order a learner would work them: differentiate, then
# evaluate; rule families decompose into explicit sub-steps so the verifier can
# replay each one.

"""
Largest accepted seed, `2^53 - 1`, so seeds survive any JSON reader as exact integers.
"""
const MAX_SEED = 2^53 - 1

const POLYNOMIAL_DERIVATIVE_GENERATOR = "polynomial_derivative_at_point"
const PRODUCT_RULE_GENERATOR = "product_rule_at_point"
const CHAIN_RULE_GENERATOR = "chain_rule_at_point"
const GENERATOR_VERSION = 1

const POWER_RULE_MISCONCEPTIONS =
    ["derivatives.power_rule_overgeneralised", "algebra.sign_error"]
const PRODUCT_RULE_MISCONCEPTIONS =
    ["derivatives.product_rule", "algebra.sign_error"]
const CHAIN_RULE_MISCONCEPTIONS =
    ["derivatives.chain_rule", "algebra.sign_error"]

"""
    derivative_coefficients(coefficients) -> Vector{Int}

Coefficients of `p'(x)` (ascending) for `p(x) = Σₖ coefficients[k+1] xᵏ`.
"""
function derivative_coefficients(coefficients::AbstractVector{<:Integer})::Vector{Int}
    length(coefficients) > 1 || return [0]  # derivative of a constant
    return [Base.checked_mul(Int(coefficients[k + 1]), k) for k in 1:(length(coefficients) - 1)]
end

"""
    derivative_at(coefficients, x) -> Int

Exact value of `p'(x)` for the integer polynomial `p(x) = Σₖ coefficients[k+1] xᵏ`
(coefficients in ascending order). Throws `OverflowError` on `Int` overflow.
"""
function derivative_at(coefficients::AbstractVector{<:Integer}, x::Integer)::Int
    return polynomial_at(derivative_coefficients(coefficients), x)
end

"""
    polynomial_at(coefficients, x) -> Int

Exact value of `p(x)` for the integer polynomial `p(x) = Σₖ coefficients[k+1] xᵏ`
(coefficients in ascending order). Throws `OverflowError` on `Int` overflow.
"""
function polynomial_at(coefficients::AbstractVector{<:Integer}, x::Integer)::Int
    acc = 0
    for a in Iterators.reverse(coefficients)
        acc = Base.checked_add(Base.checked_mul(acc, Int(x)), Int(a))
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

function draw_polynomial(rng::Xoshiro, degree_range, bound)::Vector{Int}
    degree = rand(rng, degree_range)
    coefficients = [rand(rng, -bound:bound) for _ in 0:degree]
    while iszero(coefficients[end])
        coefficients[end] = rand(rng, -bound:bound)
    end
    return coefficients
end

function problem_shell(name, version, topic, seed, statement, parameters, answer)
    return (
        schema_version = 1,
        problem_id = "$(name)/v$(version)/seed-$(seed)",
        topic = topic,
        generator = (
            name = name,
            version = version,
            language = "julia",
            runtime_version = string(VERSION),
            seed = Int(seed),
        ),
        statement = statement,
        parameters = parameters,
        answer = (kind = "integer", value = answer),
    )
end

check_seed(seed) = (
    (seed isa Bool || !(0 <= seed <= MAX_SEED)) &&
    throw(DomainError(seed, "seed must be an integer in 0:$(MAX_SEED)"))
)

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
    check_seed(seed)
    rng = Xoshiro(seed)
    coefficients = draw_polynomial(rng, 1:4, 9)
    point = rand(rng, -5:5)
    derivative = derivative_coefficients(coefficients)
    answer = polynomial_at(derivative, point)

    p = x -> evalpoly(x[1], Float64.(coefficients))
    numeric = only(gradient_approx(p, [Float64(point)]))
    isapprox(numeric, answer; rtol = 1e-6, atol = 1e-4) || error(
        "exact derivative $(answer) disagrees with finite difference $(numeric) for seed $(seed)",
    )

    problem = problem_shell(
        POLYNOMIAL_DERIVATIVE_GENERATOR,
        GENERATOR_VERSION,
        "calculus.derivatives.power_rule",
        seed,
        "Let f(x) = $(format_polynomial(coefficients)). Find f'($(point)).",
        (kind = POLYNOMIAL_DERIVATIVE_GENERATOR, coefficients = coefficients, point = point),
        answer,
    )
    return (
        problem...,
        difficulty = (max_degree = 4, coefficient_bound = 9, point_bound = 5),
        steps = [
            (kind = "differentiate_polynomial", input = coefficients, output = derivative),
            (
                kind = "evaluate_polynomial",
                coefficients = derivative,
                point = point,
                value = answer,
            ),
        ],
        misconception_ids = POWER_RULE_MISCONCEPTIONS,
    )
end

"""
    product_rule_at_point(seed) -> NamedTuple

Generate "Let h(x) = p(x)·q(x). Compute h'(a)." for integer polynomials p,q of
degree 1–2, coefficients `-9:9`, `a` in `-5:5`. Steps decompose the product rule
so each sub-result is replayable: p'(x), q'(x), p(a), q(a), p'(a), q'(a), the two
products, and the final sum.
"""
function product_rule_at_point(seed::Integer)
    check_seed(seed)
    rng = Xoshiro(seed)
    p = draw_polynomial(rng, 1:2, 9)
    q = draw_polynomial(rng, 1:2, 9)
    point = rand(rng, -5:5)
    p′ = derivative_coefficients(p)
    q′ = derivative_coefficients(q)
    pa = polynomial_at(p, point)
    qa = polynomial_at(q, point)
    p′a = polynomial_at(p′, point)
    q′a = polynomial_at(q′, point)
    t1 = Base.checked_mul(p′a, qa)
    t2 = Base.checked_mul(pa, q′a)
    answer = Base.checked_add(t1, t2)

    problem = problem_shell(
        PRODUCT_RULE_GENERATOR,
        GENERATOR_VERSION,
        "calculus.derivatives.product_rule",
        seed,
        "Let p(x) = $(format_polynomial(p)) and q(x) = $(format_polynomial(q)). " *
        "If h(x) = p(x)·q(x), compute h'($(point)).",
        (kind = PRODUCT_RULE_GENERATOR, p = p, q = q, point = point),
        answer,
    )
    return (
        problem...,
        difficulty = (max_degree = 2, coefficient_bound = 9, point_bound = 5),
        steps = [
            (kind = "differentiate_polynomial", input = p, output = p′),
            (kind = "differentiate_polynomial", input = q, output = q′),
            (
                kind = "evaluate_polynomial",
                coefficients = p,
                point = point,
                value = pa,
            ),
            (
                kind = "evaluate_polynomial",
                coefficients = q,
                point = point,
                value = qa,
            ),
            (
                kind = "evaluate_polynomial",
                coefficients = p′,
                point = point,
                value = p′a,
            ),
            (
                kind = "evaluate_polynomial",
                coefficients = q′,
                point = point,
                value = q′a,
            ),
            (kind = "multiply", factors = [p′a, qa], value = t1),
            (kind = "multiply", factors = [pa, q′a], value = t2),
            (kind = "add", addends = [t1, t2], value = answer),
        ],
        misconception_ids = PRODUCT_RULE_MISCONCEPTIONS,
    )
end

"""
    chain_rule_at_point(seed) -> NamedTuple

Generate "Let h(x) = p(q(x)). Compute h'(a)." for integer polynomials p,q of
degree 1–2 (q of degree ≥1 so the chain is real), coefficients `-9:9`, `a` in
`-5:5`. Steps replay the chain rule: u = q(a), p'(x), p'(u), q'(x), q'(a), then
p'(u)·q'(a). The inner value `u` is clamped implicitly by the small draw bounds,
keeping `p'(u)` inside `Int`.
"""
function chain_rule_at_point(seed::Integer)
    check_seed(seed)
    rng = Xoshiro(seed)
    p = draw_polynomial(rng, 1:2, 9)
    q = draw_polynomial(rng, 1:2, 9)
    point = rand(rng, -5:5)
    u = polynomial_at(q, point)
    p′ = derivative_coefficients(p)
    q′ = derivative_coefficients(q)
    p′u = polynomial_at(p′, u)
    q′a = polynomial_at(q′, point)
    answer = Base.checked_mul(p′u, q′a)

    problem = problem_shell(
        CHAIN_RULE_GENERATOR,
        GENERATOR_VERSION,
        "calculus.derivatives.chain_rule",
        seed,
        "Let p(x) = $(format_polynomial(p)) and q(x) = $(format_polynomial(q)). " *
        "If h(x) = p(q(x)), compute h'($(point)).",
        (kind = CHAIN_RULE_GENERATOR, outer = p, inner = q, point = point),
        answer,
    )
    return (
        problem...,
        difficulty = (max_degree = 2, coefficient_bound = 9, point_bound = 5),
        steps = [
            (
                kind = "evaluate_polynomial",
                coefficients = q,
                point = point,
                value = u,
            ),
            (kind = "differentiate_polynomial", input = p, output = p′),
            (kind = "differentiate_polynomial", input = q, output = q′),
            (
                kind = "evaluate_polynomial",
                coefficients = p′,
                point = u,
                value = p′u,
            ),
            (
                kind = "evaluate_polynomial",
                coefficients = q′,
                point = point,
                value = q′a,
            ),
            (kind = "multiply", factors = [p′u, q′a], value = answer),
        ],
        misconception_ids = CHAIN_RULE_MISCONCEPTIONS,
    )
end

"""
Generators the worker serves, by name.
"""
const GENERATORS = Dict{String,Function}(
    POLYNOMIAL_DERIVATIVE_GENERATOR => polynomial_derivative_at_point,
    PRODUCT_RULE_GENERATOR => product_rule_at_point,
    CHAIN_RULE_GENERATOR => chain_rule_at_point,
)
