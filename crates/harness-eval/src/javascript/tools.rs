use deno_ast::{
    swc::ast::{Decl, Pat, Program, Stmt},
    MediaType, ParseParams,
};
use serde_json::{json, Value};

pub(super) fn parse(source: &str) -> Result<Value, String> {
    let parsed = deno_ast::parse_program(ParseParams {
        specifier: deno_core::ModuleSpecifier::parse("file:///kernel-tool.js")
            .map_err(|error| error.to_string())?,
        text: source.to_owned().into(),
        media_type: MediaType::JavaScript,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    })
    .map_err(|error| error.to_string())?;
    let program = parsed.program();
    let Program::Script(script) = program.as_ref() else {
        return Err("tool() requires a named function".into());
    };
    let [Stmt::Decl(Decl::Fn(function))] = script.body.as_slice() else {
        return Err("tool() requires a named function".into());
    };
    if function.function.is_generator {
        return Err("generator tools are unsupported".into());
    }
    let mut names = Vec::new();
    for parameter in &function.function.params {
        let Pat::Ident(name) = &parameter.pat else {
            return Err(
                "tool parameters must be simple identifiers without defaults or rest".into(),
            );
        };
        let name = name.sym.to_string();
        if names.contains(&name) {
            return Err("tool parameters must have distinct names".into());
        }
        names.push(name);
    }
    Ok(json!({"name":function.ident.sym.as_ref(),"params":names}))
}
