globalThis.__harness_create_registry = native => {
  const entries = new Map();
  let generation = 1, hostNames = [];
  const key = name => String(name).replace(/[^a-zA-Z0-9_-]/g, "_").slice(0, 64).replaceAll("-", "_");
  const fail = (code, message, fields = {}) => { throw Object.assign(new Error(message), { name: "KernelToolError", code, ...fields }); };
  const plain = value => value !== null && typeof value === "object" && [Object.prototype, null].includes(Object.getPrototypeOf(value));
  const clone = value => {
    const seen = new Set();
    const check = value => {
      if (value === null || typeof value === "string" || typeof value === "boolean" || typeof value === "number" && Number.isFinite(value)) return;
      if ((!plain(value) && !Array.isArray(value)) || seen.has(value)) fail("invalid_tool_definition", "metadata must contain only JSON values");
      seen.add(value);
      Object.values(value).forEach(check);
      seen.delete(value);
    };
    check(value);
    return JSON.parse(JSON.stringify(value));
  };
  const descriptor = entry => ({ name: entry.name, description: entry.description, input_schema: clone(entry.schema), language: "js", kernel_generation: generation, definition_revision: entry.revision });
  return {
    configure(options) { generation = options.generation ?? generation; hostNames = (options.tools ?? []).map(tool => key(tool.name)); },
    refresh(tools) { hostNames = [...new Set([...hostNames, ...tools.map(tool => key(tool.name))])]; },
    define(fn, metadata = {}) {
      if (typeof fn !== "function") fail("invalid_tool_definition", "tool() requires a named function");
      let parsed;
      try { parsed = native.parseFunction(Function.prototype.toString.call(fn)); }
      catch (error) { fail("invalid_tool_definition", error.message); }
      const { name, params } = parsed;
      if (!/^[a-zA-Z0-9_-]{1,64}$/.test(name)) fail("invalid_tool_definition", "Kernel tool name must match MCP name grammar");
      if (["__agent__", "__output__", "__schema__"].includes(key(name))) fail("reserved_tool_name", `Kernel tool name is reserved: ${name}`);
      if (hostNames.includes(key(name))) fail("tool_name_collision", `Kernel tool name collides: ${name}`);
      if (!plain(metadata)) fail("invalid_tool_definition", "tool metadata must be an object");
      metadata = clone(metadata);
      const description = metadata.description ?? "";
      if (typeof description !== "string") fail("invalid_tool_definition", "tool description must be a string");
      const schema = metadata.schema ?? { type: "object", properties: Object.fromEntries(params.map(name => [name, {}])), required: params, additionalProperties: false };
      if (!plain(schema) || schema.type !== "object" || !plain(schema.properties)) fail("invalid_tool_definition", "input_schema must describe an object with properties");
      const properties = Object.keys(schema.properties);
      if (properties.length !== params.length || params.some(name => !properties.includes(name))) fail("invalid_tool_definition", "schema properties must match the function parameters");
      const previous = entries.get(key(name));
      if (previous && previous.name !== name) fail("tool_name_collision", `Kernel tool name collides: ${name}`);
      const entry = { name, params, fn, description, schema, revision: (previous?.revision ?? 0) + 1 };
      entries.set(key(name), entry);
      return descriptor(entry);
    },
    describe(names) {
      return { results: names.map(name => {
        const entry = entries.get(key(name));
        return entry ? { name, ok: true, descriptor: descriptor(entry) } : { name, ok: false, error: { code: "kernel_tool_missing", message: `Kernel tool is not defined: ${name}` } };
      }) };
    },
    async invoke(request) {
      if (request.kernel_generation !== generation) fail("kernel_tool_stale", "Kernel tool descriptor generation is stale");
      const entry = entries.get(key(request.name));
      if (!entry) fail("kernel_tool_missing", `Kernel tool is not defined: ${request.name}`);
      if (entry.revision !== request.definition_revision) fail("kernel_tool_stale", "Kernel tool descriptor revision is stale");
      if (!plain(request.args)) fail("invalid_tool_definition", "kernel tool arguments must be an object");
      const required = Array.isArray(entry.schema.required) ? entry.schema.required : [];
      if (required.some(name => !Object.hasOwn(request.args, name)) || entry.schema.additionalProperties === false && Object.keys(request.args).some(name => !entry.params.includes(name))) fail("invalid_tool_definition", "kernel tool arguments do not match the schema");
      try {
        const result = await entry.fn(...entry.params.map(name => request.args[name]));
        if (generation !== request.kernel_generation || entries.get(key(request.name)) !== entry) fail("kernel_tool_stale", "Kernel tool descriptor is stale");
        return result;
      } catch (error) {
        if (typeof error?.code === "string") throw error;
        fail("kernel_tool_failed", error instanceof Error ? error.message : String(error));
      }
    },
  };
};
