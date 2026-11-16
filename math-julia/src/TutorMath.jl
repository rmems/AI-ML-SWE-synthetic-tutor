"""
    TutorMath

Julia math worker for the synthetic tutoring pipeline.

The Rust pipeline (`crates/tutor-wire`) drives this package over ZMQ REQ/REP with JSON
payloads. Julia only does math here: it generates problems together with their exact
answers. It never writes dataset rows; verification and assembly stay in Rust.

- `numerics.jl`: finite-difference calculus and vector helpers carried over from the
  pre-pivot playground (`legacy/NeuroMathPlayground/src/math.jl`).
- `generators.jl`: seeded problem generators that emit `problem.schema.json` payloads.
- `protocol.jl`: the `tutor-math/1` request/reply envelope, socket-free.
- `server.jl`: the REP worker loop.
"""
module TutorMath

using JSON
using Random
using ZMQ

include("numerics.jl")
include("generators.jl")
include("protocol.jl")
include("server.jl")

export gradient_approx, hessian_approx, softmax, normalize_vec
export polynomial_derivative_at_point, derivative_at, format_polynomial, GENERATORS, MAX_SEED
export PROTOCOL, MAX_REQUEST_BYTES, handle_request, worker_info
export DEFAULT_ENDPOINT, serve

end
