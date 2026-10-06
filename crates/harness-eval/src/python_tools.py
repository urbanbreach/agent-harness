"""Python kernel tool definitions. The host owns publication and invocation scope."""
import types
import typing


class _KernelToolError(RuntimeError):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def _tool_error(code, message):
    raise _KernelToolError(code, message)


def _annotation_schema(annotation):
    if annotation in (inspect.Parameter.empty, typing.Any):
        return {}
    if annotation in (str, int, float, bool, type(None)):
        return {"type": {str: "string", int: "integer", float: "number", bool: "boolean", type(None): "null"}[annotation]}
    origin, args = typing.get_origin(annotation), typing.get_args(annotation)
    if origin in (typing.Union, types.UnionType):
        return {"anyOf": [_annotation_schema(arg) for arg in args]}
    if origin is typing.Literal:
        return {"enum": list(args)}
    if annotation is list or origin is list:
        return {"type": "array", "items": _annotation_schema(args[0]) if args else {}}
    if annotation is dict or origin is dict:
        if args and args[0] is not str:
            _tool_error("invalid_tool_definition", "Dictionary tool arguments require string keys")
        return {"type": "object", "additionalProperties": _annotation_schema(args[1]) if args else {}}
    _tool_error("invalid_tool_definition", f"Unsupported tool annotation: {annotation!r}; supply an explicit schema")


class _KernelTools:
    def __init__(self):
        self.entries = {}
        self.revisions = {}
        self.host_names = set()

    @staticmethod
    def key(name):
        return re.sub(r"[^a-zA-Z0-9_-]", "_", name)[:64].replace("-", "_")

    def refresh(self, tools):
        self.host_names.update(self.key(t["name"]) for t in tools)

    def descriptor(self, entry):
        return {"name": entry["name"], "description": entry["description"], "input_schema": entry["schema"],
                "language": "py", "kernel_generation": _options["generation"], "definition_revision": entry["revision"]}

    def define(self, fn, *, description=None, schema=None):
        if not inspect.isfunction(fn) or not re.fullmatch(r"[a-zA-Z0-9_-]{1,64}", fn.__name__):
            _tool_error("invalid_tool_definition", "tool() requires a named Python function")
        name, signature = fn.__name__, inspect.signature(fn)
        key = self.key(name)
        if key in ("__agent__", "__output__", "__schema__"):
            _tool_error("reserved_tool_name", f"Kernel tool name is reserved: {name}")
        if key in self.host_names or key in self.entries and self.entries[key]["name"] != name:
            _tool_error("tool_name_collision", f"Kernel tool name collides: {name}")
        if any(p.kind not in (p.POSITIONAL_OR_KEYWORD, p.KEYWORD_ONLY) for p in signature.parameters.values()):
            _tool_error("invalid_tool_definition", "Kernel tools require explicit named parameters")
        if schema is None:
            annotations = typing.get_type_hints(fn)
            schema = {"type": "object", "properties": {name: _annotation_schema(annotations.get(name, p.annotation))
                      for name, p in signature.parameters.items()},
                      "required": [name for name, p in signature.parameters.items() if p.default is p.empty],
                      "additionalProperties": False}
        try:
            schema = json.loads(json.dumps(schema, allow_nan=False))
        except (ValueError, TypeError):
            _tool_error("invalid_tool_definition", "Tool schema must contain JSON values")
        if not isinstance(schema, dict) or schema.get("type") != "object" or not isinstance(schema.get("properties"), dict) or set(schema["properties"]) != set(signature.parameters):
            _tool_error("invalid_tool_definition", "Schema properties must match the function parameters")
        description = inspect.getdoc(fn) or "" if description is None else description
        if not isinstance(description, str):
            _tool_error("invalid_tool_definition", "Tool description must be text")
        revision = self.revisions.get(key, 0) + 1
        self.revisions[key] = revision
        self.entries[key] = {"name": name, "fn": fn, "signature": signature, "schema": schema,
                             "description": description, "revision": revision}
        return fn

    def describe(self, names):
        return {"results": [{"name": name, "ok": True, "descriptor": self.descriptor(self.entries[self.key(name)])}
                            if self.key(name) in self.entries else {"name": name, "ok": False, "error": {
                                "code": "kernel_tool_missing", "message": f"Kernel tool is not defined: {name}"}}
                            for name in names]}

    def invoke(self, request):
        entry = self.entries.get(self.key(request["name"]))
        if request["kernel_generation"] != _options["generation"] or entry and entry["revision"] != request["definition_revision"]:
            _tool_error("kernel_tool_stale", "Kernel tool descriptor is stale")
        if entry is None:
            _tool_error("kernel_tool_missing", "Kernel tool is no longer defined")
        if not isinstance(request["args"], dict):
            _tool_error("invalid_tool_definition", "Kernel tool arguments must be an object")
        try:
            bound = entry["signature"].bind(**request["args"])
        except TypeError as error:
            _tool_error("invalid_tool_definition", str(error))
        value = entry["fn"](*bound.args, **bound.kwargs)
        if inspect.isawaitable(value):
            value = asyncio.run(value)
        if self.entries.get(self.key(request["name"])) is not entry:
            _tool_error("kernel_tool_stale", "Kernel tool was redefined during invocation")
        return value
