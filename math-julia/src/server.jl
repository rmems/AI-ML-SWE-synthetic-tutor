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
const ZMQ_MAXMSGSIZE = Cint(22)

# ZMQ.jl's `socket.maxmsgsize` setter passes the wrong option width (EINVAL), so call
# libzmq directly. With the limit set, libzmq drops oversized frames before allocating them.
function set_max_message_size!(socket::ZMQ.Socket, bytes::Integer)
    value = Ref{Int64}(bytes)
    rc = ccall((:zmq_setsockopt, ZMQ.libzmq), Cint,
               (Ptr{Cvoid}, Cint, Ptr{Cvoid}, Csize_t),
               socket, ZMQ_MAXMSGSIZE, value, sizeof(Int64))
    rc == 0 || throw(ZMQ.StateError("zmq_setsockopt(ZMQ_MAXMSGSIZE) failed"))
    return nothing
end

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
        set_max_message_size!(socket, MAX_REQUEST_BYTES)
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
