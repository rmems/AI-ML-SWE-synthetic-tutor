# math-julia/

`TutorMath`, the Julia math worker. It generates seeded problems with exact answers and serves them to the Rust pipeline over ZMQ REQ/REP using the `tutor-math/1` JSON protocol (see `src/protocol.jl`); it never writes dataset rows, and Rust re-verifies every answer. Run it with `julia --project=math-julia math-julia/bin/serve.jl` (binds `tcp://127.0.0.1:5557`, override with `TUTOR_MATH_ENDPOINT`) and test it with `julia --project=math-julia -e 'using Pkg; Pkg.test()'`.
