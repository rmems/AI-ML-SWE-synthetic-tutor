# protocol.jl — the `tutor-math/1` JSON envelope.
#
# Request:  {"protocol": "tutor-math/1", "request_id": "<string>", "op": "ping"}
#           {"protocol": "tutor-math/1", "request_id": "<string>", "op": "generate_problem",
#            "generator": "<name>", "seed": <0..2^53-1>}
#           {"protocol", "request_id", "op": "verify_answer", "problem": {...}}
# Reply:    {"protocol", "request_id", "status": "ok", "result": {...}}
#           {"protocol", "request_id", "status": "error", "error": {"code", "message"}}
#
# Every request gets exactly one reply (REP lockstep). Unknown keys, unknown ops, and
# out-of-range values are rejected with a reason code instead of being ignored.

"""
Wire protocol identifier shared with `crates/tutor-wire`.
"""
const PROTOCOL = "tutor-math/1"

"""
Largest request accepted, in bytes. `handle_request` answers larger input with a
`request_too_large` error; over the wire, `serve` sets `ZMQ_MAXMSGSIZE` to this value, so
libzmq drops oversized frames and the peer's connection without a reply, and the client
sees a receive timeout instead.
"""
const MAX_REQUEST_BYTES = 1 << 20

struct RequestError <: Exception
    code::String
    message::String
end

const PING_KEYS = Set(["protocol", "request_id", "op"])
const GENERATE_KEYS = Set(["protocol", "request_id", "op", "generator", "seed"])
const VERIFY_KEYS = Set(["protocol", "request_id", "op", "problem"])

function check_keys(request::AbstractDict, allowed::Set{String})
    extra = sort([String(k) for k in keys(request) if !(String(k) in allowed)])
    isempty(extra) ||
        throw(RequestError("malformed_request", "unexpected keys: $(join(extra, ", "))"))
    missing_keys = sort([k for k in allowed if !haskey(request, k)])
    isempty(missing_keys) ||
        throw(RequestError("malformed_request", "missing keys: $(join(missing_keys, ", "))"))
    return nothing
end

"""
    worker_info() -> NamedTuple

Result of a `ping`: worker identity and versions, for provenance.
"""
function worker_info()
    return (
        worker = "TutorMath",
        worker_version = string(pkgversion(@__MODULE__)),
        julia_version = string(VERSION),
        protocol = PROTOCOL,
        generators = sort(collect(keys(GENERATORS))),
    )
end

function dispatch(request::AbstractDict)
    op = get(request, "op", nothing)
    if op == "ping"
        check_keys(request, PING_KEYS)
        return worker_info()
    elseif op == "generate_problem"
        check_keys(request, GENERATE_KEYS)
        name = request["generator"]
        name isa AbstractString ||
            throw(RequestError("malformed_request", "generator must be a string"))
        generator = get(GENERATORS, name, nothing)
        generator === nothing &&
            throw(RequestError("unknown_generator", "no generator named $(repr(name))"))
        seed = request["seed"]
        (seed isa Integer && !(seed isa Bool) && 0 <= seed <= MAX_SEED) ||
            throw(RequestError("invalid_seed", "seed must be an integer in 0:$(MAX_SEED)"))
        return generator(seed)
    elseif op == "verify_answer"
        check_keys(request, VERIFY_KEYS)
        problem = request["problem"]
        problem isa AbstractDict ||
            throw(RequestError("malformed_request", "problem must be a JSON object"))
        return verify_answer(problem)
    else
        throw(RequestError("unknown_op", "unsupported op $(repr(op))"))
    end
end

"""
    handle_request(raw) -> String

Decode one request, run it, and encode the reply. Never throws for bad input: every
failure becomes an `"error"` reply with a reason code (`malformed_json`,
`request_too_large`, `malformed_request`, `unsupported_protocol`, `unknown_op`, `unknown_generator`,
`invalid_seed`, `internal_error`).
"""
function handle_request(raw::AbstractString)::String
    request_id = nothing
    try
        ncodeunits(raw) <= MAX_REQUEST_BYTES || throw(
            RequestError("request_too_large", "request exceeds $(MAX_REQUEST_BYTES) bytes"),
        )
        request = try
            JSON.parse(raw)
        catch err
            err isa InterruptException && rethrow()
            throw(RequestError("malformed_json", "request is not valid JSON"))
        end
        request isa AbstractDict ||
            throw(RequestError("malformed_request", "request must be a JSON object"))
        id = get(request, "request_id", nothing)
        (id isa AbstractString && !isempty(id)) ||
            throw(RequestError("malformed_request", "request_id must be a non-empty string"))
        request_id = String(id)
        protocol = get(request, "protocol", nothing)
        protocol == PROTOCOL || throw(
            RequestError(
                "unsupported_protocol",
                "expected protocol $(PROTOCOL), got $(repr(protocol))",
            ),
        )
        result = dispatch(request)
        return JSON.json((
            protocol = PROTOCOL,
            request_id = request_id,
            status = "ok",
            result = result,
        ))
    catch err
        err isa InterruptException && rethrow()
        failure =
            err isa RequestError ? err : RequestError("internal_error", sprint(showerror, err))
        return error_reply(failure, request_id)
    end
end

function error_reply(failure::RequestError, request_id = nothing)::String
    return JSON.json((
        protocol = PROTOCOL,
        request_id = request_id,
        status = "error",
        error = (code = failure.code, message = failure.message),
    ))
end
