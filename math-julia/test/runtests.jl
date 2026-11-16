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
        @test ping["result"]["generators"] == ["polynomial_derivative_at_point"]
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

    @testset "REP worker over ZMQ" begin
        endpoint = "tcp://127.0.0.1:55571"
        ready = Channel{Nothing}(1)
        worker = @async serve(endpoint; max_requests = 2, on_ready = () -> put!(ready, nothing))
        bind(ready, worker)  # a failing worker closes the channel instead of hanging take!
        take!(ready)
        context = ZMQ.Context()
        client = ZMQ.Socket(context, ZMQ.REQ)
        try
            client.rcvtimeo = 10_000
            ZMQ.connect(client, endpoint)
            ZMQ.send(client, request("op" => "ping"))
            @test JSON.parse(ZMQ.recv(client, String))["status"] == "ok"
            ZMQ.send(client, request("op" => "generate_problem", "generator" => "polynomial_derivative_at_point", "seed" => 3))
            problem = JSON.parse(ZMQ.recv(client, String))["result"]
            @test problem["answer"]["value"] == derivative_at(Int.(problem["parameters"]["coefficients"]), problem["parameters"]["point"])
        finally
            close(client)
            close(context)
        end
        @test fetch(worker) == 2
    end
end
