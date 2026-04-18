# kelly.jl — Kelly Criterion and Position Sizing
#
# Optimal capital allocation under uncertainty — the same Kelly math that
# powers the SpikenautKelly.jl position sizing module in production.

# ── Core Kelly Formula ────────────────────────────────────────────────────────

"""
    kelly_fraction(; win_rate, avg_win, avg_loss) -> Float64

Compute the half-Kelly position fraction from historical trade statistics.

```
b  = avg_win / avg_loss
F* = (p·b − (1−p)) / b    # full Kelly
F  = F* / 2                # half-Kelly (practical variance reduction)
F  = clamp(F, 0.02, 0.20)  # conservative bounds
```

# Arguments
- `win_rate`: fraction of trades that were profitable (0–1)
- `avg_win`:  average profit on winning trades
- `avg_loss`: average loss on losing trades (absolute value)

# References
- Kelly, J.L. (1956). A New Interpretation of Information Rate.
  *Bell System Technical Journal*, 35(4), 917–926.

# Example
```julia
f = kelly_fraction(win_rate=0.55, avg_win=8.50, avg_loss=5.20)
println("Bet \$(round(f*100, digits=1))% of bankroll")
```
"""
function kelly_fraction(; win_rate::Float64, avg_win::Float64, avg_loss::Float64)::Float64
    p        = clamp(win_rate, 0.01, 0.99)
    avg_win  = max(avg_win,  1e-9)
    avg_loss = max(avg_loss, 1e-9)
    b        = avg_win / avg_loss
    q        = 1.0 - p
    full     = (p * b - q) / b
    return clamp(full * 0.5, 0.02, 0.20)
end

"""
    half_kelly(p, b) -> Float64

Compute the half-Kelly fraction given win probability `p` and payoff ratio `b`.

```
F = (p·b − (1−p)) / b / 2
```

Returns a value clamped to `[0.0, 1.0]`.

# Example
```julia
f = half_kelly(0.60, 1.5)   # 60% win rate, 1.5× payoff
```
"""
function half_kelly(p::Float64, b::Float64)::Float64
    p = clamp(p, 0.01, 0.99)
    b = max(b, 1e-9)
    q = 1.0 - p
    return clamp((p * b - q) / b * 0.5, 0.0, 1.0)
end

"""
    full_kelly(p, b) -> Float64

Compute the unrestricted full-Kelly fraction (for educational comparison).

```
F* = (p·b − (1−p)) / b
```

In practice, half-Kelly (`F*/2`) is preferred because it reduces variance
while sacrificing only a small fraction of expected growth.

# Example
```julia
fk = full_kelly(0.60, 1.5)
hk = half_kelly(0.60, 1.5)
println("Full: \$fk  Half: \$hk  (variance penalty halved)")
```
"""
function full_kelly(p::Float64, b::Float64)::Float64
    p = clamp(p, 0.01, 0.99)
    b = max(b, 1e-9)
    q = 1.0 - p
    return clamp((p * b - q) / b, 0.0, 1.0)
end

"""
    expected_log_growth(p, b, f) -> Float64

Compute the expected logarithmic growth rate for a fraction `f` under win
probability `p` and payoff ratio `b`.

```
G(f) = p·log(1 + b·f) + (1−p)·log(1 − f)
```

This is maximised at the full-Kelly fraction. Plotting `G(f)` for different
`f` values is an excellent way to visualise why Kelly is optimal.

# Example
```julia
p, b = 0.55, 1.5
fs   = 0.0:0.01:0.5
gs   = [expected_log_growth(p, b, f) for f in fs]
fstar = fs[argmax(gs)]
println("Optimal f ≈ \$fstar  (full-Kelly = \$(full_kelly(p, b)))")
```
"""
function expected_log_growth(p::Float64, b::Float64, f::Float64)::Float64
    (f <= 0.0 || f >= 1.0) && return -Inf
    return p * log(1.0 + b * f) + (1.0 - p) * log(1.0 - f)
end

# ── SNN Confidence Mapping ────────────────────────────────────────────────────

"""
    confidence_to_kelly(confidence; payoff_ratio=0.01) -> Float64

Map an SNN neuron confidence score `[0, 1]` to a half-Kelly fraction.

The confidence is treated directly as the win probability `p`.

# Arguments
- `confidence`:   SNN output in `[0, 1]`
- `payoff_ratio`: expected move as fraction (default 1 %)

# Example
```julia
f = confidence_to_kelly(0.85, payoff_ratio=0.015)
println("Position size: \$(round(f*100, digits=1))%")
```
"""
function confidence_to_kelly(confidence::Float64; payoff_ratio::Float64 = 0.01)::Float64
    p = clamp(confidence, 0.01, 0.99)
    b = max(payoff_ratio, 1e-9)
    q = 1.0 - p
    return clamp((p * b - q) / b * 0.5, 0.0, 1.0)
end

# ── Bankroll Simulation ───────────────────────────────────────────────────────

"""
    simulate_kelly_growth(p, b, f, n_bets; bankroll=1.0) -> Vector{Float64}

Simulate bankroll growth over `n_bets` independent bets using fraction `f`.

Each bet either wins (`+b·f·bankroll`) or loses (`-f·bankroll`) according to
win probability `p`.

Returns the full bankroll history (length `n_bets + 1`, starting from `bankroll`).

# Example
```julia
# Compare full-Kelly vs half-Kelly over 500 bets
p, b = 0.55, 1.5
hk   = half_kelly(p, b)
fk   = full_kelly(p, b)
Random.seed!(42)
h_path = simulate_kelly_growth(p, b, hk, 500)
f_path = simulate_kelly_growth(p, b, fk, 500)
println("Half-Kelly final: \$(round(h_path[end], digits=2))")
println("Full-Kelly final: \$(round(f_path[end], digits=2))")
```
"""
function simulate_kelly_growth(
    p::Float64, b::Float64, f::Float64, n_bets::Int; bankroll::Float64 = 1.0
)::Vector{Float64}
    history    = Vector{Float64}(undef, n_bets + 1)
    history[1] = bankroll
    for i in 1:n_bets
        if rand() < p
            history[i+1] = history[i] * (1.0 + b * f)
        else
            history[i+1] = history[i] * (1.0 - f)
        end
    end
    return history
end
