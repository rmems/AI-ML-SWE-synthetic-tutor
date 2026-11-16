# Start the TutorMath ZMQ worker.
#
#   julia --project=math-julia math-julia/bin/serve.jl
#
# TUTOR_MATH_ENDPOINT overrides the bind address (default tcp://127.0.0.1:5557).

using TutorMath

const endpoint = get(ENV, "TUTOR_MATH_ENDPOINT", DEFAULT_ENDPOINT)

function announce()
    println("TutorMath worker listening on $(endpoint) (protocol $(PROTOCOL))")
    flush(stdout)
end

serve(endpoint; on_ready = announce)
