# server.jl — ZMQ REP worker loop.

"""
Default worker endpoint; override with `TUTOR_MATH_ENDPOINT`. Must match
`tutor_wire::DEFAULT_ENDPOINT`.
"""
const DEFAULT_ENDPOINT = "tcp://127.0.0.1:5557"

"""
    serve(endpoint=DEFAULT_ENDPOINT; max_requests=nothing, on_ready=() -> nothing) -> Int

Bind a REP socket to `endpoint` and answer requests with [`handle_request`](@ref)
until `max_requests` have been handled (forever when `nothing`). `on_ready` runs once
the socket is bound. Returns the number of requests handled.
"""
function serve(
    endpoint::AbstractString = DEFAULT_ENDPOINT;
    max_requests::Union{Nothing,Integer} = nothing,
    on_ready = () -> nothing,
)::Int
    context = ZMQ.Context()
    socket = ZMQ.Socket(context, ZMQ.REP)
    handled = 0
    try
        socket.linger = 0
        ZMQ.bind(socket, endpoint)
        on_ready()
        while max_requests === nothing || handled < max_requests
            raw = ZMQ.recv(socket, String)
            ZMQ.send(socket, handle_request(raw))
            handled += 1
        end
    finally
        close(socket)
        close(context)
    end
    return handled
end
