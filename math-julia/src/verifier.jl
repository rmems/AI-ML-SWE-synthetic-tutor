# verifier.jl — the `verify_answer` op.
#
# Julia re-checks a problem that may have been produced by a Rust generator: the
# check lives on the opposite side of the wire from the generator, so the answer
# cannot grade itself. Mirrors the logic in crates/tutor-verify: recompute the
# expected answer from `parameters`, replay `steps`, and compare with
# `answer.value`. Returns a NamedTuple
#   (verified = Bool, reason = String-or-nothing, detail = String, expected = Int-or-nothing)
# which Rust turns into a pass or a `Rejection` with the same reason codes.

const VERIFY_KINDS = Set([
    "polynomial_derivative_at_point",
    "binomial_derivative_at_point",
    "product_rule_at_point",
    "chain_rule_at_point",
])

function dense_binomial(a::Integer, m::Integer, b::Integer, n::Integer)::Vector{Int}
    coeffs = zeros(Int, m + 1)
    coeffs[m + 1] = a
    coeffs[n + 1] = b
    return coeffs
end

"""
    expected_answer(problem) -> Int

Recompute the expected answer from `problem["parameters"]` alone. Throws
`RequestError` for a kind this verifier does not know.
"""
function expected_answer(problem::AbstractDict)::Int
    parameters = get(problem, "parameters", nothing)
    parameters isa AbstractDict ||
        throw(RequestError("malformed_problem", "parameters must be an object"))
    kind = get(parameters, "kind", nothing)
    if kind == "polynomial_derivative_at_point"
        return derivative_at(
            [Int(c) for c in parameters["coefficients"]],
            Int(parameters["point"]),
        )
    elseif kind == "binomial_derivative_at_point"
        a = Int(parameters["a"])
        m = Int(parameters["m"])
        b = Int(parameters["b"])
        n = Int(parameters["n"])
        return derivative_at(dense_binomial(a, m, b, n), Int(parameters["point"]))
    elseif kind == "product_rule_at_point"
        p = [Int(c) for c in parameters["p"]]
        q = [Int(c) for c in parameters["q"]]
        x = Int(parameters["point"])
        return derivative_at(p, x) * polynomial_at(q, x) +
               polynomial_at(p, x) * derivative_at(q, x)
    elseif kind == "chain_rule_at_point"
        outer = [Int(c) for c in parameters["outer"]]
        inner = [Int(c) for c in parameters["inner"]]
        x = Int(parameters["point"])
        u = polynomial_at(inner, x)
        return polynomial_at(derivative_coefficients(outer), u) * derivative_at(inner, x)
    else
        throw(RequestError("verify_unknown_parameter_kind", "cannot verify kind $(repr(kind))"))
    end
end

"""
    replay_steps(problem) -> (ok, detail)

Replay `problem["steps"]`: every step output is recomputed and compared; step
inputs must be parameter polynomials or earlier differentiations, and integer
operands must be declared parameters or earlier outputs (each produced once).
"""
function replay_steps(problem::AbstractDict)
    parameters = problem["parameters"]
    steps = get(problem, "steps", nothing)
    steps isa AbstractVector ||
        throw(RequestError("malformed_problem", "steps must be an array"))

    # Polynomials that steps may reference: parameter polys + produced outputs.
    parameter_polynomials = Set{Vector{Int}}()
    for key in ("coefficients", "p", "q", "outer", "inner")
        haskey(parameters, key) &&
            push!(parameter_polynomials, [Int(c) for c in parameters[key]])
    end
    kind = parameters["kind"]
    if kind == "binomial_derivative_at_point"
        push!(
            parameter_polynomials,
            dense_binomial(
                Int(parameters["a"]),
                Int(parameters["m"]),
                Int(parameters["b"]),
                Int(parameters["n"]),
            ),
        )
    end

    declared = Set{Int}()
    for (key, value) in parameters
        key == "kind" && continue
        if value isa Integer
            push!(declared, Int(value))
        end
    end

    produced = Dict{Int,Int}()  # value => remaining uses
    polynomials = Set{Vector{Int}}()
    last_value = nothing
    for (i, step) in enumerate(steps)
        step isa AbstractDict ||
            throw(RequestError("malformed_problem", "step $i must be an object"))
        skind = get(step, "kind", nothing)
        if skind == "differentiate_polynomial"
            input = [Int(c) for c in step["input"]]
            output = [Int(c) for c in step["output"]]
            if !(input in parameter_polynomials) && !(input in polynomials)
                return (false, "step $i differentiates an undeclared polynomial")
            end
            derivative_coefficients(input) == output ||
                return (false, "step $i: wrong derivative")
            push!(polynomials, output)
        elseif skind == "evaluate_polynomial"
            coeffs = [Int(c) for c in step["coefficients"]]
            x = Int(step["point"])
            value = Int(step["value"])
            if !(coeffs in parameter_polynomials) && !(coeffs in polynomials)
                return (false, "step $i evaluates an undeclared polynomial")
            end
            use_integer!(produced, declared, x) ||
                return (false, "step $i: evaluation point not produced")
            polynomial_at(coeffs, x) == value ||
                return (false, "step $i: wrong evaluation")
            produced[value] = get(produced, value, 0) + 1
            last_value = value
        elseif skind == "multiply"
            factors = [Int(v) for v in step["factors"]]
            value = Int(step["value"])
            for f in factors
                use_integer!(produced, declared, f) ||
                    return (false, "step $i: factor $f not produced")
            end
            prod(factors) == value || return (false, "step $i: wrong product")
            produced[value] = get(produced, value, 0) + 1
            last_value = value
        elseif skind == "add"
            addends = [Int(v) for v in step["addends"]]
            value = Int(step["value"])
            for d in addends
                use_integer!(produced, declared, d) ||
                    return (false, "step $i: addend $d not produced")
            end
            sum(addends) == value || return (false, "step $i: wrong sum")
            produced[value] = get(produced, value, 0) + 1
            last_value = value
        else
            return (false, "step $i has unknown kind $(repr(skind))")
        end
    end
    last_value === nothing &&
        return (false, "steps produce no integer answer")
    return (true, last_value)
end

# Consume one use of a produced integer; declared integers are always allowed.
function use_integer!(produced::Dict{Int,Int}, declared::Set{Int}, v::Int)::Bool
    v in declared && return true
    remaining = get(produced, v, 0)
    remaining <= 0 && return false
    produced[v] = remaining - 1
    return true
end

"""
    verify_answer(problem) -> NamedTuple

Independent Julia-side check of one problem payload. Never throws for bad input:
a `RequestError` becomes `verified = false` with the code as `reason`.
"""
function verify_answer(problem::AbstractDict)
    get(problem, "verification", nothing) === nothing || return (
        verified = false,
        reason = "verify_already_verified",
        detail = "problem already carries a verification block",
        expected = nothing,
    )
    try
        expected = expected_answer(problem)
        answer = get(problem, "answer", nothing)
        answer isa AbstractDict ||
            throw(RequestError("malformed_problem", "answer must be an object"))
        answer["kind"] == "integer" ||
            throw(RequestError("malformed_problem", "answer.kind must be integer"))
        claimed = Int(answer["value"])
        if claimed != expected
            return (
                verified = false,
                reason = "verify_answer_mismatch",
                detail = "answer $claimed, expected $expected",
                expected = expected,
            )
        end
        (ok, last) = replay_steps(problem)
        ok || return (verified = false, reason = "verify_step_replay", detail = string(last), expected = expected)
        last == expected || return (
            verified = false,
            reason = "verify_step_replay",
            detail = "steps end at $last but expected $expected",
            expected = expected,
        )
        return (
            verified = true,
            reason = nothing,
            detail = "answer and steps replay verified",
            expected = expected,
        )
    catch err
        err isa InterruptException && rethrow()
        failure =
            err isa RequestError ? err :
            RequestError(
                err isa OverflowError ? "verify_overflow" : "internal_error",
                sprint(showerror, err),
            )
        return (
            verified = false,
            reason = failure.code,
            detail = failure.message,
            expected = nothing,
        )
    end
end
