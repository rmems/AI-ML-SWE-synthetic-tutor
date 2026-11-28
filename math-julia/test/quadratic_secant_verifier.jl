# Local, test-only verifier for Rust quadratic fixtures. Deliberately independent
# of the expansion used by Rust; uses polynomial evaluations in exact rationals.
# A frame is a b c x limit linear_term then eight rows of sign denominator slope_num error_num.

function verify_quadratic_secant(frame)
    length(frame) == 38 || return "invalid_frame"
    a, b, c, x, limit, linear_term = frame[1:6]
    iszero(a) && return "not_quadratic"
    poly(t) = (a * t + b) * t + c
    before, at, after = poly(x - 1), poly(x), poly(x + 1)

    # These central differences determine the constant and linear coefficients
    # of the quotient for any quadratic, without expanding Rust's formula.
    derivative = (after - before) // 2
    curvature = (after - 2 * at + before) // 2
    limit == derivative || return "wrong_limit"
    linear_term == curvature || return "wrong_h_coefficient"

    previous = big(0)
    for offset in 7:8:length(frame)
        first_row = view(frame, offset:offset+3)
        second_row = view(frame, offset+4:offset+7)
        denominator = first_row[2]
        first_row[1] == -1 && second_row[1] == 1 || return "invalid_grid"
        denominator == second_row[2] && denominator > previous || return "invalid_grid"
        for row in (first_row, second_row)
            sign, q, slope_numerator, error_numerator = row
            h = sign // q
            quotient = (poly(x + h) - at) / h
            slope_numerator // q == quotient || return "wrong_slope"
            error_numerator // q == abs(quotient - derivative) || return "wrong_absolute_error"
        end
        previous = denominator
    end
    return "ok"
end

if abspath(PROGRAM_FILE) == @__FILE__
    for line in eachline(stdin)
        frame = parse.(BigInt, split(line))
        println(verify_quadratic_secant(frame))
    end
end
