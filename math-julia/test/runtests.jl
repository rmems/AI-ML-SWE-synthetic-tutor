using Test
using JSON
using TutorMath
using ZMQ

reply(raw) = JSON.parse(handle_request(raw))
request(fields...) = JSON.json(Dict("protocol" => PROTOCOL, "request_id" => "t-1", fields...))

@testset "TutorMath" begin
    # Carried over from legacy/NeuroMathPlayground/test/runtests.jl ("Math utilities").
    @testset "numerics" begin
        @test gradient_approx(x -> x[1]^2 + 2 * x[2]^2, [1.0, 1.0]) ≈ [2.0, 4.0] atol = 1e-5
        H = hessian_approx(x -> x[1]^2 + 3 * x[2]^2, [0.0, 0.0])
        @test H[1, 1] ≈ 2.0 atol = 1e-3
        @test H[2, 2] ≈ 6.0 atol = 1e-3
        @test H[1, 2] ≈ 0.0 atol = 1e-3
        p = softmax([1.0, 2.0, 3.0])
        @test sum(p) ≈ 1.0 atol = 1e-10
        @test all(p .> 0.0)
        @test argmax(p) == 3
        @test normalize_vec([3.0, 4.0]) ≈ [0.6, 0.8] atol = 1e-10
        @test normalize_vec([0.0, 0.0]) == [0.0, 0.0]
    end

    @testset "derivative_at and format_polynomial" begin
        @test derivative_at([5, -2, 3], 2) == 10      # f = 3x^2 - 2x + 5
        @test derivative_at([7], 3) == 0
        @test derivative_at([0, 0, 0, 1], -2) == 12   # f = x^3
        @test_throws OverflowError derivative_at([0, 0, typemax(Int)], 2)
        @test format_polynomial([5, -2, 3]) == "3x^2 - 2x + 5"
        @test format_polynomial([0, 1]) == "x"
        @test format_polynomial([-1, 0, 0, -1]) == "-x^3 - 1"
        @test format_polynomial([0, -4, 1]) == "x^2 - 4x"
        @test_throws ArgumentError format_polynomial([1, 0])
        @test_throws ArgumentError format_polynomial(Int[])
    end

    @testset "polynomial_derivative_at_point" begin
        for seed in 0:300
            problem = polynomial_derivative_at_point(seed)
            c = problem.parameters.coefficients
            @test 2 <= length(c) <= 5
            @test !iszero(c[end])
            @test all(-9 .<= c .<= 9)
            @test -5 <= problem.parameters.point <= 5
            @test problem.answer.value == derivative_at(c, problem.parameters.point)
            @test problem.generator.seed == seed
        end
        @test JSON.json(polynomial_derivative_at_point(42)) ==
              JSON.json(polynomial_derivative_at_point(42))
        @test polynomial_derivative_at_point(MAX_SEED).generator.seed == MAX_SEED
        @test_throws DomainError polynomial_derivative_at_point(-1)
        @test_throws DomainError polynomial_derivative_at_point(MAX_SEED + 1)
        @test_throws DomainError polynomial_derivative_at_point(true)
    end

    @testset "protocol" begin
        ping = reply(request("op" => "ping"))
        @test ping["status"] == "ok"
        @test ping["request_id"] == "t-1"
        @test ping["protocol"] == PROTOCOL
        @test ping["result"]["generators"] ==
              ["chain_rule_at_point", "polynomial_derivative_at_point", "product_rule_at_point"]
        @test ping["result"]["julia_version"] == string(VERSION)

        ok = reply(request("op" => "generate_problem", "generator" => "polynomial_derivative_at_point", "seed" => 7))
        @test ok["status"] == "ok"
        @test ok["result"]["problem_id"] == "polynomial_derivative_at_point/v1/seed-7"
        @test !haskey(ok, "error")

        code(raw) = reply(raw)["error"]["code"]
        @test code("not json") == "malformed_json"
        @test reply("not json")["request_id"] === nothing
        @test code("[1, 2]") == "malformed_request"
        @test code(JSON.json(Dict("protocol" => PROTOCOL, "op" => "ping"))) == "malformed_request"
        @test code(JSON.json(Dict("protocol" => "tutor-math/0", "request_id" => "x", "op" => "ping"))) ==
              "unsupported_protocol"
        @test code(request("op" => "shutdown")) == "unknown_op"
        @test code(request("op" => "ping", "extra" => 1)) == "malformed_request"
        gen(seed) = request("op" => "generate_problem", "generator" => "polynomial_derivative_at_point", "seed" => seed)
        @test code(gen(-1)) == "invalid_seed"
        @test code(gen(MAX_SEED + 1)) == "invalid_seed"
        @test code(gen(1.5)) == "invalid_seed"
        @test code(gen(true)) == "invalid_seed"
        @test code(gen("1")) == "invalid_seed"
        @test code(request("op" => "generate_problem", "generator" => "nope", "seed" => 1)) == "unknown_generator"
        @test code(request("op" => "generate_problem", "generator" => "polynomial_derivative_at_point")) ==
              "malformed_request"
        @test reply(gen(-1))["request_id"] == "t-1"
        @test code(" "^(MAX_REQUEST_BYTES + 1)) == "request_too_large"
    end

    @testset "product_rule_at_point and chain_rule_at_point" begin
        for seed in 0:100
            problem = product_rule_at_point(seed)
            params = problem.parameters
            @test params.kind == "product_rule_at_point"
            @test -5 <= params.point <= 5
            x = params.point
            @test problem.answer.value ==
                  derivative_at(params.p, x) * polynomial_at(params.q, x) +
                  polynomial_at(params.p, x) * derivative_at(params.q, x)
            @test length(problem.steps) == 9
            @test problem.misconception_ids ==
                  ["derivatives.product_rule", "algebra.sign_error"]

            problem = chain_rule_at_point(seed)
            params = problem.parameters
            @test params.kind == "chain_rule_at_point"
            u = polynomial_at(params.inner, params.point)
            @test problem.answer.value ==
                  polynomial_at(derivative_coefficients(params.outer), u) *
                  derivative_at(params.inner, params.point)
            @test length(problem.steps) == 6
            @test problem.misconception_ids ==
                  ["derivatives.chain_rule", "algebra.sign_error"]
        end
        @test JSON.json(product_rule_at_point(9)) == JSON.json(product_rule_at_point(9))
        @test JSON.json(chain_rule_at_point(9)) == JSON.json(chain_rule_at_point(9))
        @test_throws DomainError product_rule_at_point(-1)
        @test_throws DomainError chain_rule_at_point(-1)
    end

    @testset "verify_answer" begin
        # Every generator's own output must pass the Julia verifier.
        for seed in 0:100
            for generator in (polynomial_derivative_at_point, product_rule_at_point, chain_rule_at_point)
                verdict = verify_answer(JSON.parse(JSON.json(generator(seed))))
                @test verdict.verified
                @test verdict.reason === nothing
            end
        end

        # A wrong answer is rejected, loudly, with a reason code.
        problem = JSON.parse(JSON.json(product_rule_at_point(5)))
        problem["answer"]["value"] += 1
        verdict = verify_answer(problem)
        @test !verdict.verified
        @test verdict.reason == "verify_answer_mismatch"
        @test verdict.expected !== nothing

        # A tampered step is rejected.
        problem = JSON.parse(JSON.json(product_rule_at_point(5)))
        problem["steps"][end]["value"] += 1
        @test verify_answer(problem).reason == "verify_step_replay"

        # A fabricated step input is rejected.
        problem = JSON.parse(JSON.json(product_rule_at_point(5)))
        problem["steps"][1]["input"] = [99, 1]
        @test verify_answer(problem).reason == "verify_step_replay"

        # An unknown parameter kind cannot be verified at all.
        problem = JSON.parse(JSON.json(product_rule_at_point(5)))
        problem["parameters"]["kind"] = "exotic"
        @test verify_answer(problem).reason == "verify_unknown_parameter_kind"

        # Over the wire.
        good = reply(request("op" => "verify_answer", "problem" => JSON.parse(JSON.json(chain_rule_at_point(3)))))
        @test good["status"] == "ok"
        @test good["result"]["verified"] == true
        bad = reply(request("op" => "verify_answer", "problem" => 5))
        @test bad["error"]["code"] == "malformed_request"
    end

    @testset "REP worker over ZMQ" begin
        endpoint = "tcp://127.0.0.1:55571"
        ready = Channel{Nothing}(1)
        worker = @async serve(endpoint; max_requests = 3, on_ready = () -> put!(ready, nothing))
        bind(ready, worker)  # a failing worker closes the channel instead of hanging take!
        take!(ready)
        context = ZMQ.Context()
        client = ZMQ.Socket(context, ZMQ.REQ)
        try
            client.rcvtimeo = 10_000
            ZMQ.connect(client, endpoint)
            ZMQ.send(client, request("op" => "ping"); more = true)
            ZMQ.send(client, "extra frame")
            multipart = JSON.parse(ZMQ.recv(client, String))
            @test multipart["status"] == "error"
            @test multipart["error"]["code"] == "malformed_request"
            ZMQ.send(client, request("op" => "ping"))
            @test JSON.parse(ZMQ.recv(client, String))["status"] == "ok"
            ZMQ.send(client, request("op" => "generate_problem", "generator" => "polynomial_derivative_at_point", "seed" => 3))
            problem = JSON.parse(ZMQ.recv(client, String))["result"]
            @test problem["answer"]["value"] == derivative_at(Int.(problem["parameters"]["coefficients"]), problem["parameters"]["point"])
        finally
            close(client)
            close(context)
        end
        @test fetch(worker) == 3
    end
end
