# sandbox/05_kelly_sizing.jl
#
# Kelly Criterion and Optimal Position Sizing
#
# The Kelly criterion gives the fraction of your bankroll that maximises
# long-run expected log-wealth — the same math behind SpikenautKelly.jl
# which sizes positions from SNN neuron confidence scores.
#
# Run with:   julia --project=. sandbox/05_kelly_sizing.jl

using MathSandbox
using Printf
using Random
using Statistics

println("=" ^ 60)
println("  MathSandbox — 05 Kelly Criterion and Position Sizing")
println("=" ^ 60)
println()

# ── 1. The Kelly Formula ──────────────────────────────────────────────────────
println("─── 1. The Kelly Formula ───")
println("""
Given:
  p  = probability of winning a bet
  q  = 1 - p = probability of losing
  b  = net payoff ratio (win b for every 1 risked)

The full-Kelly fraction that maximises expected log-growth is:

   F* = (p·b − q) / b  =  p − q/b

In practice the half-Kelly fraction F = F*/2 is preferred because it
dramatically reduces variance while sacrificing only ~13% of expected
growth rate.

References:
  Kelly (1956) — Bell System Technical Journal 35(4), 917-926
  Thorp (1969) — optimal gambling systems (introduced half-Kelly)
""")

# Simple example
p = 0.55   # 55% win rate
b = 1.5    # 1.5× payoff
fk = full_kelly(p, b)
hk = half_kelly(p, b)
println("Example: p=$p, b=$b")
println("  Full Kelly:  $(round(fk * 100, digits=2))% of bankroll")
println("  Half Kelly:  $(round(hk * 100, digits=2))% of bankroll")
println()

# ── 2. Expected Log-Growth Surface ───────────────────────────────────────────
println("─── 2. Expected Log-Growth G(f) ───")
println("""
G(f) = p·log(1 + b·f) + (1−p)·log(1−f)

This is strictly concave and maximised at f = F*.  Over-betting (f > F*)
actually *decreases* long-run wealth — even with an edge!
""")

p2, b2 = 0.55, 1.5
println("f (%)  | G(f)       | vs optimal")
println("-" ^ 40)
g_opt = expected_log_growth(p2, b2, full_kelly(p2, b2))
for f_val in [0.0, 0.05, 0.10, full_kelly(p2, b2), 0.30, 0.50, 0.70]
    g = expected_log_growth(p2, b2, f_val)
    fstar_marker = abs(f_val - full_kelly(p2, b2)) < 0.001 ? " ← F*" : ""
    if isfinite(g)
        @printf("  %5.1f%% | %+10.6f | %.1f%%%s\n", f_val * 100, g, g / g_opt * 100, fstar_marker)
    else
        @printf("  %5.1f%% | %+10s | -∞%s\n", f_val * 100, string(round(g, digits=6)), fstar_marker)
    end
end
println()

# ── 3. Kelly Fraction vs. Win Rate ────────────────────────────────────────────
println("─── 3. Kelly Fraction vs. Win Rate ───")
println("Payoff ratio b=1.5 fixed, varying win probability p:")
println()
println("  p     | Full F* | Half F | Expected edge")
println("-" ^ 48)
for p_val in [0.40, 0.45, 0.50, 0.52, 0.55, 0.60, 0.65, 0.70]
    fk2 = full_kelly(p_val, 1.5)
    hk2 = half_kelly(p_val, 1.5)
    edge = p_val * 1.5 - (1.0 - p_val)      # expected value per unit
    @printf("  %.2f  | %6.2f%% | %6.2f%% | %+.4f\n",
            p_val, fk2 * 100, hk2 * 100, edge)
end
println()

# ── 4. Bankroll Simulation: Full vs. Half Kelly ───────────────────────────────
println("─── 4. Bankroll Growth Simulation ───")
println("Comparing full-Kelly vs. half-Kelly over 1,000 bets (p=0.55, b=1.5):")
println()

Random.seed!(42)
p3, b3 = 0.55, 1.5
fk3 = full_kelly(p3, b3)
hk3 = half_kelly(p3, b3)

full_path = simulate_kelly_growth(p3, b3, fk3, 1000)
half_path = simulate_kelly_growth(p3, b3, hk3, 1000)

println("  Strategy   | Final bankroll | Max drawdown | Growth rate")
println("-" ^ 58)
for (name, path) in [("Full Kelly", full_path), ("Half Kelly", half_path)]
    final  = path[end]
    # Max drawdown
    peak = path[1]
    dd   = 0.0
    for v in path
        peak = max(peak, v)
        dd   = max(dd, (peak - v) / peak)
    end
    log_growth = log(final) / 1000
    @printf("  %-10s | %-14.2f | %12.1f%% | %.6f\n",
            name, final, dd * 100, log_growth)
end
println()
println("  → Half-Kelly has lower variance and smaller drawdowns,")
println("    at the cost of slightly slower growth.")
println()

# ── 5. SNN Confidence → Kelly Fraction ───────────────────────────────────────
println("─── 5. SNN Confidence → Kelly Position Size ───")
println("""
In Spikenaut, a 16-neuron LIF layer produces a confidence score in [0,1].
This score is treated as the win probability p and mapped to a Kelly fraction.

The payoff_ratio should reflect the expected price move (e.g. 0.5 = 50%
average win per unit risked, matching a typical 1.5× payoff ratio).

High confidence → larger position.
Low confidence  → minimal position (floor = 0).
""")

println("  SNN Confidence | Kelly Fraction | Position (\$10k account, asset \$65k)")
println("-" ^ 68)
account = 10_000.0
price   = 65_000.0

for conf in [0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95]
    f    = confidence_to_kelly(conf, payoff_ratio=1.5)   # 1.5× payoff ratio
    units = f * account / price
    @printf("  %-14.2f | %-14.4f | %.6f BTC (\$%.2f)\n",
            conf, f, units, units * price)
end
println()

# ── 6. Kelly from Trade Statistics ───────────────────────────────────────────
println("─── 6. Kelly from Historical Trade Statistics ───")
println("Typical usage: compute Kelly fraction from your actual trade log.")
println()

for (win_rate, avg_win, avg_loss) in [
        (0.55, 8.50, 5.20),
        (0.60, 6.00, 6.00),
        (0.45, 15.0, 5.00),
        (0.70, 4.00, 8.00),
    ]
    f = kelly_fraction(win_rate=win_rate, avg_win=avg_win, avg_loss=avg_loss)
    b_val = avg_win / avg_loss
    edge  = win_rate * b_val - (1.0 - win_rate)
    @printf("  p=%.2f, b=%.2f, edge=%+.3f → Kelly = %.2f%%\n",
            win_rate, b_val, edge, f * 100)
end
println()

println("=" ^ 60)
println("  Proceed to 06_stdp_learning.jl")
println("=" ^ 60)
