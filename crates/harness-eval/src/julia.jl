using Base64

const HARNESS_WIRE = Base.fdio(reinterpret(Int32, Base.Libc.dup(Base.RawFD(1))), true)
const HARNESS_INPUT = Base.PipeEndpoint(Base.Libc.dup(Base.RawFD(0)))
const HARNESS_OUTPUT = map(["stdout", "stderr"]) do name
    path, created = mktemp(get(ENV, "HARNESS_EVAL_CAPTURE_DIR", tempdir()))
    close(created)
    writer, reader = open(path, "a"), open(path, "r")
    (name == "stdout" ? redirect_stdout : redirect_stderr)(writer)
    (name, writer, reader, UInt8[])
end
redirect_stdin(devnull)
const HARNESS_REQUESTS = Channel{Any}(16)
const HARNESS_PENDING = Dict{String,Channel{Any}}()
const HARNESS_LOCK = ReentrantLock()
const HARNESS_OPTIONS = Dict{String,Any}()
const HARNESS_ACTIVE = Ref("")
const HARNESS_SEQUENCE = Ref(0)

function harness_json(value)
    value === nothing && return "null"
    value isa Bool && return value ? "true" : "false"
    value isa Number && return isfinite(value) ? string(value) : "null"
    if value isa AbstractDict
        return "{" * join((harness_json(string(key)) * ":" * harness_json(item) for (key,item) in value), ",") * "}"
    end
    if value isa AbstractArray || value isa Tuple
        return "[" * join(harness_json.(collect(value)), ",") * "]"
    end
    buffer = IOBuffer()
    Base.write(buffer, '"')
    for character in string(value)
        if !isvalid(character)
            Base.write(buffer, '\ufffd')
        elseif character == '"' || character == '\\'
            Base.write(buffer, '\\', character)
        elseif Int(character) < 32
            Base.write(buffer, "\\u" * lpad(string(Int(character), base=16), 4, '0'))
        else
            Base.write(buffer, character)
        end
    end
    Base.write(buffer, '"')
    String(take!(buffer))
end

function harness_emit(kind; fields...)
    cell = get(task_local_storage(), :harness_eval_cell, "")
    kind == "ready" || (!isempty(cell) && cell == HARNESS_ACTIVE[]) || return nothing
    frame = Dict{String,Any}(string(key)=>value for (key,value) in fields)
    frame["type"], frame["cellId"] = kind, cell
    lock(HARNESS_LOCK) do
        harness_drain_native(cell)
        Base.write(HARNESS_WIRE, harness_json(frame) * "\n")
        flush(HARNESS_WIRE)
    end
    nothing
end

function harness_drain_native(cell)
    for (name, writer, reader, pending) in HARNESS_OUTPUT
        flush(writer)
        while !eof(reader)
            append!(pending, Base.read(reader, 8192))
            last = length(pending)
            lead = last
            while lead > 0 && 0x80 <= pending[lead] <= 0xbf
                lead -= 1
            end
            if lead > 0
                byte = pending[lead]
                needed = byte >= 0xf0 ? 4 : byte >= 0xe0 ? 3 : byte >= 0xc0 ? 2 : 1
                last - lead + 1 < needed && (last = lead - 1)
            end
            data = String(pending[1:last])
            deleteat!(pending, 1:last)
            isempty(cell) || isempty(data) || Base.write(HARNESS_WIRE, harness_json(Dict("type"=>"text", "cellId"=>cell, "stream"=>name, "data"=>data)) * "\n")
        end
    end
    flush(HARNESS_WIRE)
end

function harness_receive()
    try
        for line in eachline(HARNESS_INPUT)
            ncodeunits(line) > 32 * 1024 * 1024 && break
            # Rust serializes protocol values as Julia literals, with strings escaped
            # including interpolation. User code remains a string until run().
            frame = Core.eval(Main, Meta.parse(line))
            kind = get(frame, "type", "")
            if kind == "reply"
                reply = pop!(HARNESS_PENDING, frame["id"], nothing)
                reply === nothing || put!(reply, frame)
            elseif kind == "shutdown"
                break
            elseif kind != "cancel"
                put!(HARNESS_REQUESTS, frame)
            end
        end
    finally
        put!(HARNESS_REQUESTS, nothing)
    end
end

struct HarnessHelperError <: Exception
    message::String
    code::Any
end
Base.showerror(io::IO, error::HarnessHelperError) = Base.print(io, error.message)

function harness_call(operation, args)
    cell = get(task_local_storage(), :harness_eval_cell, "")
    !isempty(cell) && cell == HARNESS_ACTIVE[] || error("eval cell is no longer active")
    HARNESS_SEQUENCE[] += 1
    id = string(HARNESS_SEQUENCE[])
    reply = HARNESS_PENDING[id] = Channel{Any}(1)
    harness_emit("call"; id=id, operation=operation, args=args)
    try
        response = take!(reply)
        if haskey(response, "error")
            fields = response["error"]
            fields isa AbstractDict && throw(HarnessHelperError(get(fields, "message", "eval helper failed"), get(fields, "code", nothing)))
            error(string(fields))
        end
        get(response, "result", nothing)
    finally
        pop!(HARNESS_PENDING, id, nothing)
    end
end

struct HarnessTools end
function Base.getproperty(::HarnessTools, name::Symbol)
    function invoke(args=Dict{String,Any}(); kwargs...)
        args isa AbstractDict || error("tool arguments must be a dictionary")
        parameters = Dict{String,Any}(string(key)=>value for (key,value) in args)
        merge!(parameters, Dict(string(key)=>value for (key,value) in kwargs))
        harness_call("tool", Dict("name"=>string(name), "parameters"=>parameters))
    end
end
Base.getindex(tools::HarnessTools, name) = getproperty(tools, Symbol(name))
const tool = HarnessTools()
const tools = tool
tool_schema(name=nothing) = harness_call("schema", name === nothing ? Dict() : Dict("name"=>name))
completion(prompt; kwargs...) = harness_call("completion", Dict("prompt"=>prompt, "opts"=>Dict(string(key)=>value for (key,value) in kwargs)))
agent(prompt; kwargs...) = harness_call("agent", merge(Dict{String,Any}("prompt"=>prompt), Dict(string(key)=>value for (key,value) in kwargs)))
function output(ids...; kwargs...)
    values = length(ids) == 1 && ids[1] isa AbstractVector ? ids[1] : ids
    names = [replace(string(value isa AbstractDict ? get(value, "id", get(value, "handle", "")) : value), r"^agent://"=>"") for value in values]
    harness_call("output", merge(Dict{String,Any}("ids"=>names), Dict(string(key)=>value for (key,value) in kwargs)))
end

struct HarnessWorkpool
    pool_id::String
end
function Base.getproperty(pool::HarnessWorkpool, name::Symbol)
    id = getfield(pool, :pool_id)
    name == :pool_id && return id
    name == :push && return items -> harness_call("workpool", Dict("op"=>"push", "pool_id"=>id, "items"=>items))
    name in (:close, :inspect, :cancel) || error("unknown workpool operation")
    () -> harness_call("workpool", Dict("op"=>string(name), "pool_id"=>id))
end
function workpool(agent, name; mode=nothing, width=nothing, tools=nothing)
    args = Dict{String,Any}("op"=>"create", "agent"=>agent, "name"=>name)
    mode === nothing || (args["mode"] = mode)
    width === nothing || (args["width"] = width)
    tools === nothing || (args["tools"] = tools)
    result = harness_call("workpool", args)
    id = get(get(result, "details", Dict()), "pool_id", nothing)
    get(result, "hasError", false) || !(id isa String) ? error(get(result, "text", "workpool creation failed")) : HarnessWorkpool(id)
end
function wait(handles; mode="all", timeout=60)
    harness_call("wait", Dict("handles" => handles isa AbstractVector ? handles : [handles], "mode" => mode, "timeout" => timeout))
end

text(value) = harness_emit("text"; stream="stdout", data=string(value))
print(values...) = text(join(string.(values)))
println(values...) = text(join(string.(values)) * "\n")
log(value) = harness_emit("log"; message=string(value))
phase(value) = harness_emit("phase"; title=string(value))
display_image(data, mime="image/png") = harness_emit("display"; mimeType=mime, dataBase64=data)
function display(value)
    if value isa AbstractDict
        fields = Dict(string(key)=>item for (key,item) in value)
        if get(fields, "type", "") == "markdown"
            return harness_emit("display"; mimeType="text/markdown", dataBase64=base64encode(string(fields["text"])))
        elseif startswith(get(fields, "mimeType", ""), "image/")
            return display_image(get(fields, "dataBase64", get(fields, "data", "")), fields["mimeType"])
        end
    end
    structured = value isa AbstractDict || value isa AbstractVector || value isa Tuple
    harness_emit("display"; mimeType=structured ? "application/json" : "text/plain", dataBase64=base64encode(structured ? harness_json(value) : string(value)))
end

function harness_path(name)
    raw = string(name)
    matched = match(r"^([a-z][a-z0-9+.-]*)://(.*)$"i, raw)
    matched === nothing && return abspath(raw)
    lowercase(matched[1]) == "local" || error("unsupported file protocol")
    encoded = replace(matched[2], '\\'=>'/')
    decoded = IOBuffer()
    bytes = codeunits(encoded)
    index = 1
    while index <= length(bytes)
        hex = bytes[index] == UInt8('%') && index + 2 <= length(bytes) ? tryparse(UInt8, String(bytes[index+1:index+2]); base=16) : nothing
        Base.write(decoded, hex === nothing ? bytes[index] : hex)
        index += hex === nothing ? 1 : 3
    end
    relative = String(take!(decoded))
    isabspath(relative) || ".." in split(relative, '/') ? error("local:// path must remain within its session directory") : joinpath(HARNESS_OPTIONS["localRoot"], relative)
end
function read(name; offset=1, limit=nothing)
    path = harness_path(name)
    data = Base.read(path, String)
    if offset > 1 || limit !== nothing
        lines = split(data, '\n'; keepempty=true)
        start = max(1, offset)
        stop = limit === nothing ? length(lines) : min(length(lines), start + limit - 1)
        data = start > length(lines) ? "" : join(lines[start:stop], "\n")
    end
    harness_emit("status"; event=Dict("op"=>"read", "path"=>path, "chars"=>length(data)))
    data
end
function write(name, content)
    path = harness_path(name)
    mkpath(dirname(path))
    Base.write(path, string(content))
    harness_emit("status"; event=Dict("op"=>"write", "path"=>path, "chars"=>length(string(content))))
    path
end
function env(key=nothing, value=nothing)
    key === nothing && return Dict(ENV)
    value === nothing || (ENV[string(key)] = string(value))
    result = get(ENV, string(key), nothing)
    harness_emit("status"; event=Dict("op"=>"env", "key"=>string(key), "value"=>result, "action"=>value === nothing ? "get" : "set"))
    result
end

function parallel(thunks)
    functions = collect(thunks)
    results, errors = Vector{Any}(undef, length(functions)), fill(nothing, length(functions))
    failures = Any[errors...]
    next = Ref(1)
    cell = get(task_local_storage(), :harness_eval_cell, "")
    @sync for _ in 1:min(get(HARNESS_OPTIONS, "parallelPoolWidth", 4), length(functions))
        @async begin
            task_local_storage(:harness_eval_cell, cell)
            while next[] <= length(functions)
                index = next[]
                next[] += 1
                try
                    results[index] = functions[index]()
                catch error
                    failures[index] = error
                end
            end
        end
    end
    failed = findfirst(error -> error !== nothing, failures)
    failed === nothing || throw(failures[failed])
    results
end
function pipeline(items, stages...)
    values = collect(items)
    for stage in stages
        values = parallel([() -> stage(value) for value in values])
    end
    values
end

function harness_run(frame)
    kind = frame["type"]
    if kind == "init"
        merge!(HARNESS_OPTIONS, frame)
        harness_emit("ready"; runtime=Dict("name"=>"Julia", "version"=>string(VERSION)))
        return
    end
    kind == "run" || return
    for (_, writer, reader, pending) in HARNESS_OUTPUT
        truncate(writer, 0)
        seekstart(reader)
        empty!(pending)
    end
    HARNESS_ACTIVE[] = frame["id"]
    task_local_storage(:harness_eval_cell, HARNESS_ACTIVE[])
    started = time_ns()
    try
        parsed = Meta.parseall(frame["code"]; filename="<eval>")
        value = Core.eval(Main, parsed)
        expressions = filter(item -> !(item isa LineNumberNode), parsed.args)
        last = isempty(expressions) ? nothing : expressions[end]
        declaration = last isa Expr && last.head in (:(=), :function, :struct, :using, :import, :const, :global, :local, :macro)
        harness_emit("result"; ok=true, valueRepr=value === nothing || declaration ? nothing : harness_json(value), durationMs=div(time_ns()-started, 1_000_000))
    catch error
        harness_emit("result"; ok=false, error=Dict("message"=>sprint(showerror, error)), durationMs=div(time_ns()-started, 1_000_000))
    finally
        HARNESS_ACTIVE[] = ""
        task_local_storage(:harness_eval_cell, "")
    end
end
function harness_loop()
    @async harness_receive()
    @async while true
        sleep(0.05)
        lock(HARNESS_LOCK) do
            harness_drain_native(HARNESS_ACTIVE[])
        end
    end
    while true
        frame = take!(HARNESS_REQUESTS)
        frame === nothing && break
        harness_run(frame)
    end
end
harness_loop()
