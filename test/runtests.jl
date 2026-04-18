using Test
using Random
using MathSandbox
using Statistics

@testset "MathSandbox" begin

    # ── LIF Neuron ─────────────────────────────────────────────────────────────
    @testset "LIF Neuron" begin
        p = LIFParams()

        @testset "LIFParams defaults" begin
            @test p.tau_m    == 20.0
            @test p.v_rest   == -65.0
            @test p.v_thresh == -50.0
            @test p.v_reset  == -65.0
            @test p.r_m      == 10.0
        end

        @testset "lif_step — no spike below threshold" begin
            v, spiked = lif_step(-65.0, 0.0, p, 0.1)
            @test !spiked
            @test v ≈ -65.0 atol=1e-10   # no current → stays at rest
        end

        @testset "lif_step — spike when above threshold" begin
            # Force a spike by starting above threshold
            v, spiked = lif_step(p.v_thresh + 1.0, 0.0, p, 0.1)
            @test spiked
            @test v == p.v_reset
        end

        @testset "simulate_lif — fires spikes with sufficient current" begin
            ts, vs, spks = simulate_lif(3.5, 200.0, p; dt=0.1)
            @test length(ts) == length(vs)
            @test length(spks) > 0
        end

        @testset "simulate_lif — no spikes with zero current" begin
            _, _, spks = simulate_lif(0.0, 200.0, p; dt=0.1)
            @test length(spks) == 0
        end

        @testset "firing_rate" begin
            spike_times = [0.0, 25.0, 50.0, 75.0, 100.0]
            rate = firing_rate(spike_times, 200.0)
            @test rate ≈ 25.0 atol=1.0   # 5 spikes / 0.2 s = 25 Hz
        end
    end

    # ── STDP ───────────────────────────────────────────────────────────────────
    @testset "STDP" begin
        sp = STDPParams()

        @testset "LTP for positive Δt" begin
            dw = stdp_delta_w(10.0, sp)
            @test dw > 0.0
            @test dw ≈ sp.A_plus * exp(-10.0 / sp.tau_plus)
        end

        @testset "LTD for negative Δt" begin
            dw = stdp_delta_w(-10.0, sp)
            @test dw < 0.0
            @test dw ≈ -sp.A_minus * exp(-10.0 / sp.tau_minus)
        end

        @testset "zero Δt → LTP" begin
            dw = stdp_delta_w(0.0, sp)
            @test dw == sp.A_plus
        end

        @testset "apply_stdp clamps to [w_min, w_max]" begin
            # Potentiate from near w_max
            w = apply_stdp(0.999, 0.0, sp)
            @test w <= sp.w_max

            # Depress from near w_min
            w = apply_stdp(0.001, -1.0, sp)
            @test w >= sp.w_min
        end

        @testset "repeated LTP raises weight" begin
            w = 0.5
            for _ in 1:20
                w = apply_stdp(w, 10.0, sp)
            end
            @test w > 0.5
        end
    end

    # ── GBM ────────────────────────────────────────────────────────────────────
    @testset "GBM" begin
        @testset "gbm_expected_value" begin
            ev = gbm_expected_value(100.0, 0.05, 1.0)
            @test ev ≈ 100.0 * exp(0.05) atol=1e-10
        end

        @testset "simulate_gbm — correct length" begin
            ts, prices = simulate_gbm(100.0, 0.05, 0.20, 1.0, 1/252)
            @test length(ts) == length(prices)
            @test prices[1] == 100.0
            @test all(prices .> 0.0)   # GBM stays positive
        end

        @testset "gbm_step — positive price" begin
            for _ in 1:100
                S = gbm_step(100.0, 0.05, 0.20, 1/252)
                @test S > 0.0
            end
        end
    end

    # ── Ornstein-Uhlenbeck ─────────────────────────────────────────────────────
    @testset "Ornstein-Uhlenbeck" begin
        @testset "ou_stationary_std" begin
            p = OUParams(mu=0.0, kappa=1.0, sigma=0.2)
            @test ou_stationary_std(p) ≈ 0.2 / sqrt(2.0) atol=1e-10
        end

        @testset "simulate_ou — reverts to mean" begin
            p = OUParams(mu=0.0, kappa=5.0, sigma=0.1)
            _, vals = simulate_ou(10.0, p, 5.0, 0.01)   # start far from mean
            @test abs(vals[end]) < 2.0                   # should be near 0
        end

        @testset "simulate_ou — correct length" begin
            p = OUParams()
            ts, vals = simulate_ou(0.0, p, 1.0, 0.01)
            @test length(ts) == length(vals)
        end
    end

    # ── Poisson Spike Trains ──────────────────────────────────────────────────
    @testset "Poisson Spike Trains" begin
        @testset "poisson_spike_train — approximately correct rate" begin
            # Average over many trials to reduce randomness
            counts = [length(poisson_spike_train(50.0, 1000.0)) for _ in 1:100]
            @test mean(counts) ≈ 50 atol=10   # 50 Hz × 1 s ≈ 50 spikes
        end

        @testset "inter_spike_intervals — correct length" begin
            spks = [10.0, 20.0, 35.0, 55.0]
            isis = inter_spike_intervals(spks)
            @test length(isis) == 3
            @test isis[1] ≈ 10.0
        end

        @testset "inter_spike_intervals — empty for < 2 spikes" begin
            @test inter_spike_intervals(Float64[]) == Float64[]
            @test inter_spike_intervals([5.0]) == Float64[]
        end

        @testset "cv_isi — Poisson ≈ 1" begin
            spks = poisson_spike_train(50.0, 10_000.0)
            cv   = cv_isi(spks)
            @test cv ≈ 1.0 atol=0.3   # Poisson ISI CV = 1
        end
    end

    # ── Hurst Exponent ────────────────────────────────────────────────────────
    @testset "Hurst Exponent" begin
        @testset "returns 0.5 for short series" begin
            @test compute_hurst([1.0, 2.0, 3.0]) == 0.5
        end

        @testset "returns 0.5 for constant series" begin
            @test compute_hurst(fill(5.0, 100)) == 0.5
        end

        @testset "output in [0.2, 0.8]" begin
            Random.seed!(1)
            for _ in 1:20
                sig = cumsum(randn(200))
                h = compute_hurst(sig)
                @test 0.2 <= h <= 0.8
            end
        end
    end

    # ── Hawkes Intensity ──────────────────────────────────────────────────────
    @testset "Hawkes Intensity" begin
        @testset "returns 1.0 for short series" begin
            @test compute_hawkes([100.0]) == 1.0
        end

        @testset "output in [0.5, 3.0]" begin
            for sig in [
                    [100.0, 100.0, 100.0, 100.0],   # zero movement
                    [100.0, 200.0, 50.0, 300.0],    # huge swings
                ]
                λ = compute_hawkes(sig)
                @test 0.5 <= λ <= 3.0
            end
        end

        @testset "volatile > calm" begin
            calm     = [100.0 + 0.01*i for i in 1:50]
            volatile = [100.0 + 5.0*randn() for _ in 1:50]
            @test compute_hawkes(volatile) > compute_hawkes(calm)
        end
    end

    # ── GBM Surprise ─────────────────────────────────────────────────────────
    @testset "GBM Surprise" begin
        @testset "returns 0.0 for short series" begin
            @test compute_gbm_surprise([100.0, 101.0]) == 0.0
        end

        @testset "output in [-3.0, 3.0]" begin
            Random.seed!(2)
            for _ in 1:30
                sig = [100.0 * exp(0.01 * randn()) for _ in 1:20]
                z = compute_gbm_surprise(sig)
                @test -3.0 <= z <= 3.0
            end
        end
    end

    # ── snn_features ─────────────────────────────────────────────────────────
    @testset "snn_features" begin
        sig = cumsum(randn(100)) .+ 100.0
        f = snn_features(sig)
        @test haskey(NamedTuple(pairs(f)), :hurst)
        @test haskey(NamedTuple(pairs(f)), :hawkes)
        @test haskey(NamedTuple(pairs(f)), :gbm_surprise)
        @test 0.2 <= f.hurst <= 0.8
        @test 0.5 <= f.hawkes <= 3.0
        @test -3.0 <= f.gbm_surprise <= 3.0
    end

    # ── Kelly Criterion ───────────────────────────────────────────────────────
    @testset "Kelly Criterion" begin
        @testset "full_kelly is correct" begin
            p, b = 0.55, 1.5
            fk = full_kelly(p, b)
            @test fk ≈ (p * b - (1 - p)) / b atol=1e-10
        end

        @testset "half_kelly is full_kelly / 2" begin
            p, b = 0.60, 2.0
            @test half_kelly(p, b) ≈ full_kelly(p, b) / 2 atol=1e-10
        end

        @testset "full_kelly in [0, 1]" begin
            @test 0.0 <= full_kelly(0.55, 1.5) <= 1.0
            @test full_kelly(0.01, 1.0) >= 0.0   # negative edge → 0
        end

        @testset "kelly_fraction respects bounds [0.02, 0.20]" begin
            # Very high win rate
            f = kelly_fraction(win_rate=0.99, avg_win=10.0, avg_loss=1.0)
            @test f <= 0.20

            # Very low win rate
            f2 = kelly_fraction(win_rate=0.01, avg_win=1.0, avg_loss=10.0)
            @test f2 >= 0.02
        end

        @testset "expected_log_growth maximised at full-Kelly" begin
            p, b = 0.55, 1.5
            fstar = full_kelly(p, b)
            g_star = expected_log_growth(p, b, fstar)
            for f in [0.05, 0.10, fstar * 1.5, 0.40]
                f = clamp(f, 0.001, 0.999)
                @test expected_log_growth(p, b, f) <= g_star + 1e-6
            end
        end

        @testset "simulate_kelly_growth — correct length" begin
            path = simulate_kelly_growth(0.55, 1.5, 0.10, 100)
            @test length(path) == 101
            @test path[1] == 1.0
        end

        @testset "confidence_to_kelly — in [0, 1]" begin
            for conf in [0.3, 0.5, 0.7, 0.9]
                f = confidence_to_kelly(conf)
                @test 0.0 <= f <= 1.0
            end
        end
    end

end
