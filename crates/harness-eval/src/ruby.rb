# Interpreter glue. Scheduling, retention, deadlines and host calls belong to Rust.
require "json"
require "fileutils"
require "uri"
require "tempfile"

module HarnessEval
  WIRE = STDOUT.dup
  INPUT = STDIN.dup
  LOCK = Mutex.new
  NATIVE = ["stdout", "stderr"].each_with_index.map do |name, index|
    writer = Tempfile.new(name, ENV["HARNESS_EVAL_CAPTURE_DIR"])
    writer.binmode
    writer.reopen(writer.path, "ab")
    reader = File.open(writer.path, "rb")
    (index == 0 ? STDOUT : STDERR).reopen(writer)
    [name, writer, reader, Encoding::Converter.new("UTF-8", "UTF-16LE", invalid: :replace, undef: :replace)]
  end
  STDIN.reopen(File::NULL, "r")
  REQUESTS = Queue.new
  PENDING = {}
  USER = Object.new.instance_eval("binding")
  @active = nil
  @sequence = 0
  @options = {}

  class HelperError < StandardError
    attr_reader :code
    def initialize(fields)
      @code = fields["code"]
      super(fields["message"])
    end
  end

  class << self
    attr_reader :options

    def emit(frame)
      cell = Thread.current[:harness_eval_cell]
      return unless frame[:type] == "ready" || cell && cell == @active
      LOCK.synchronize { drain_native(cell); WIRE.write(JSON.generate(frame.merge(cellId: cell || "")) + "\n"); WIRE.flush }
      nil
    end

    def drain_native(cell)
      NATIVE.each do |name, _, reader, decoder|
        while (bytes = reader.read(8192))
          text = decoder.convert(bytes).encode("UTF-8")
          WIRE.write(JSON.generate(type: "text", cellId: cell, stream: name, data: text) + "\n") if cell && !text.empty?
        end
      end
      WIRE.flush
    end

    def poll_native
      loop do
        sleep 0.05
        LOCK.synchronize { drain_native(@active) }
      end
    end

    def call(operation, args)
      raise "eval cell is no longer active" unless Thread.current[:harness_eval_cell] == @active && @active
      id, reply = LOCK.synchronize do
        @sequence += 1
        id = @sequence.to_s
        [id, PENDING[id] = Queue.new]
      end
      emit(type: "call", id: id, operation: operation, args: args)
      result = reply.pop
      raise(result["error"].is_a?(Hash) ? HelperError.new(result["error"]) : result["error"].to_s) if result.key?("error")
      result["result"]
    ensure
      LOCK.synchronize { PENDING.delete(id) } if id
    end

    def receive
      INPUT.each_line do |line|
        break if line.bytesize > 32 * 1024 * 1024
        frame = JSON.parse(line)
        case frame["type"]
        when "reply"
          reply = LOCK.synchronize { PENDING.delete(frame["id"]) }
          reply << frame if reply
        when "cancel", "shutdown"
          if @active && (frame["type"] == "shutdown" || frame["id"] == @active)
            replies = LOCK.synchronize { pending = PENDING.values; PENDING.clear; pending }
            replies.each { |reply| reply << { "error" => "Ruby cell interrupted" } }
            Thread.main.raise(Interrupt, "Ruby cell interrupted")
          end
          break if frame["type"] == "shutdown"
        else
          REQUESTS << frame
        end
      end
    rescue JSON::ParserError, IOError
      nil
    ensure
      Process.kill("KILL", -Process.getpgrp) if Process.getpgrp == Process.pid
      REQUESTS << nil
    end

    def run(frame)
      if frame["type"] == "init"
        @options = frame
        emit(type: "ready", runtime: {name: "Ruby", version: RUBY_VERSION})
        return
      end
      return unless frame["type"] == "run"
      LOCK.synchronize do
        NATIVE.each do |stream|
          stream[1].truncate(0)
          stream[2].rewind
          stream[3] = Encoding::Converter.new("UTF-8", "UTF-16LE", invalid: :replace, undef: :replace)
        end
      end
      @active = frame["id"]
      Thread.current[:harness_eval_cell] = @active
      started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      begin
        value = USER.eval(frame["code"], "<eval>", 1)
        result = {type: "result", ok: true, valueRepr: value_repr(value, frame["code"])}
      rescue Exception => error # Serialize user exceptions, exits and interrupts at the cell boundary.
        result = {type: "result", ok: false, error: {message: "#{error.class}: #{error.message}"}}
      end
      result[:durationMs] = ((Process.clock_gettime(Process::CLOCK_MONOTONIC) - started) * 1000).to_i
      emit(result)
    ensure
      @active = nil
      Thread.current[:harness_eval_cell] = nil
    end

    def value_repr(value, source)
      return if value.nil?
      if defined?(RubyVM::AbstractSyntaxTree)
        node = RubyVM::AbstractSyntaxTree.parse(source)
        while [:SCOPE, :BLOCK].include?(node&.type)
          node = node.type == :SCOPE ? node.children[2] : node.children.compact.last
        end
        return if node && %i[LASGN IASGN GASGN CVASGN DASGN OP_ASGN OP_CDECL CDECL MASGN CASGN DEFN DEFS CLASS MODULE SCLASS ALIAS UNDEF].include?(node.type)
      end
      JSON.generate(value)
    rescue JSON::GeneratorError
      value.inspect
    end
  end

  class Stream
    def initialize(name) = @name = name
    def write(*values)
      text = values.join
      HarnessEval.emit(type: "text", stream: @name, data: text)
      text.bytesize
    end
    def print(*values)
      write(*values)
      nil
    end
    def puts(*values)
      values = [""] if values.empty?
      values.each do |value|
        value.is_a?(Array) ? puts(*value) : write(value.to_s.delete_suffix("\n") + "\n")
      end
      nil
    end
    def flush = self
    def tty? = false
    def sync = true
    def sync=(_value); end
  end

  class Tools < BasicObject
    def method_missing(name, args = {}, **kwargs)
      ::Kernel.raise ::ArgumentError, "tool arguments must be a Hash" unless args.is_a?(::Hash)
      ::HarnessEval.call("tool", {name: name.to_s, parameters: args.merge(kwargs)})
    end
    def [](name) = ::Object.new.tap { |callable| callable.define_singleton_method(:call) { |args = {}, **kwargs| ::HarnessEval.call("tool", {name: name.to_s, parameters: args.merge(kwargs)}) } }
    def respond_to_missing?(_name, _private = false) = true
  end

  class Workpool
    attr_reader :pool_id
    def initialize(pool_id) = @pool_id = pool_id
    def push(items) = HarnessEval.call("workpool", {op: "push", pool_id: @pool_id, items: items})
    def close = HarnessEval.call("workpool", {op: "close", pool_id: @pool_id})
    def inspect = HarnessEval.call("workpool", {op: "inspect", pool_id: @pool_id})
    def cancel = HarnessEval.call("workpool", {op: "cancel", pool_id: @pool_id})
  end
  TOOLS = Tools.new
end

def tool = HarnessEval::TOOLS
def tools = tool
def tool_schema(name = nil) = HarnessEval.call("schema", name.nil? ? {} : {name: name})
def completion(prompt, **options) = HarnessEval.call("completion", {prompt: prompt, opts: options})
def agent(prompt, **options) = HarnessEval.call("agent", {prompt: prompt}.merge(options))
def output(*ids, **options)
  names = ids.flatten.map { |value| value.is_a?(Hash) ? value["id"] || value[:id] || value["handle"] || value[:handle] : value }
  HarnessEval.call("output", {ids: names.map { |name| name.to_s.delete_prefix("agent://") }}.merge(options))
end
def workpool(agent, name, mode: nil, width: nil, tools: nil)
  options = {mode: mode, width: width, tools: tools}.compact
  result = HarnessEval.call("workpool", {op: "create", agent: agent, name: name}.merge(options))
  id = result.dig("details", "pool_id")
  raise result.fetch("text", "workpool creation failed") if result["hasError"] || !id.is_a?(String)
  HarnessEval::Workpool.new(id)
end
def wait(handles, mode: "all", timeout: 60)
  HarnessEval.call("wait", {handles: handles.is_a?(Array) ? handles : [handles], mode: mode, timeout: timeout})
end

def text(value) = HarnessEval.emit(type: "text", stream: "stdout", data: value.to_s)
def print(*values) = text(values.join)
def log(message) = HarnessEval.emit(type: "log", message: message.to_s)
def phase(title) = HarnessEval.emit(type: "phase", title: title.to_s)
def display_image(data, mime_type: "image/png") = HarnessEval.emit(type: "display", mimeType: mime_type, dataBase64: data)
def display(value)
  fields = value.is_a?(Hash) ? value.transform_keys(&:to_s) : {}
  if fields["type"] == "markdown"
    mime, data = "text/markdown", fields["text"].to_s
  elsif fields.fetch("mimeType", "").start_with?("image/")
    return display_image(fields["dataBase64"] || fields["data"], mime_type: fields["mimeType"])
  elsif value.is_a?(Hash) || value.is_a?(Array)
    mime, data = "application/json", JSON.generate(value)
  else
    mime, data = "text/plain", value.to_s
  end
  HarnessEval.emit(type: "display", mimeType: mime, dataBase64: [data].pack("m0"))
end

def __harness_path(name)
  raw = name.to_s
  match = raw.match(/\A([a-z][a-z0-9+.-]*):\/\/(.*)\z/i)
  return File.expand_path(raw) unless match
  raise "unsupported file protocol" unless match[1].downcase == "local"
  relative = URI::DEFAULT_PARSER.unescape(match[2].tr("\\", "/"))
  raise "local:// path must remain within its session directory" if relative.start_with?("/") || relative.split("/").include?("..")
  File.expand_path(relative, HarnessEval.options.fetch("localRoot"))
end
def read(name, offset = 1, limit = nil)
  path = __harness_path(name)
  data = File.read(path, encoding: "UTF-8")
  data = data.lines.drop([offset.to_i - 1, 0].max).take(limit || data.lines.length).join if offset > 1 || limit
  HarnessEval.emit(type: "status", event: {op: "read", path: path, chars: data.length, preview: data[0, 500]})
  data
end
def write(name, content)
  path = __harness_path(name)
  FileUtils.mkdir_p(File.dirname(path))
  File.write(path, content.to_s)
  HarnessEval.emit(type: "status", event: {op: "write", path: path, chars: content.to_s.length})
  path
end
def env(key = nil, value = nil)
  return ENV.to_h.sort.to_h if key.nil?
  ENV[key.to_s] = value.to_s unless value.nil?
  result = ENV[key.to_s]
  HarnessEval.emit(type: "status", event: {op: "env", key: key.to_s, value: result, action: value.nil? ? "get" : "set"})
  result
end

def parallel(thunks)
  functions = thunks.to_a
  pending = Queue.new
  functions.each_index { |index| pending << index }
  results, errors = Array.new(functions.length), Array.new(functions.length)
  cell = Thread.current[:harness_eval_cell]
  workers = [HarnessEval.options.fetch("parallelPoolWidth", 4), functions.length].min.times.map do
    Thread.new do
      Thread.current[:harness_eval_cell] = cell
      loop do
        begin
          index = pending.pop(true)
        rescue ThreadError
          break
        end
        begin
          results[index] = functions[index].call
        rescue Exception => error
          errors[index] = error
        end
      end
    end
  end
  workers.each(&:join)
  error = errors.find { |item| !item.nil? }
  raise error if error
  results
ensure
  workers&.each { |worker| worker.kill if worker.alive? }
end
def pipeline(items, *stages)
  stages.reduce(items.to_a) { |values, stage| parallel(values.map { |value| -> { stage.call(value) } }) }
end

$stdout = HarnessEval::Stream.new("stdout")
$stderr = HarnessEval::Stream.new("stderr")
Thread.new { HarnessEval.receive }
Thread.new { HarnessEval.poll_native }
while (request = HarnessEval::REQUESTS.pop)
  HarnessEval.run(request)
end
