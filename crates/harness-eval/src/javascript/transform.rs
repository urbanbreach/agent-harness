use deno_ast::{
    swc::{
        ast::*,
        common::{Span, Spanned},
        ecma_visit::{Visit, VisitWith},
    },
    MediaType, ParseParams, ParsedSource, ProgramRef,
};
use deno_core::ModuleSpecifier;
use std::collections::BTreeSet;

fn parse(source: &str, specifier: &ModuleSpecifier) -> Result<ParsedSource, String> {
    deno_ast::parse_program(ParseParams {
        specifier: specifier.clone(),
        text: source.into(),
        media_type: MediaType::JavaScript,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    })
    .map_err(|error| error.to_string())
}

/// Parse syntax before editing it so comments, templates, regular expressions,
/// destructuring and nested functions keep their language semantics.
pub(super) fn cell(
    source: &str,
    specifier: &ModuleSpecifier,
    protected: &BTreeSet<String>,
) -> Result<String, String> {
    let parsed = parse(source, specifier)?;
    let mut edits = Vec::new();
    if let ProgramRef::Module(module) = parsed.program_ref() {
        for item in &module.body {
            if let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item {
                edits.push((import.span, rewrite_import(import, source)));
            }
        }
    }
    let source = apply(source, edits);
    let parsed = parse(&source, specifier)?;
    let statements = statements(&parsed);
    let mut return_scan = Returns::default();
    for statement in &statements {
        statement.visit_with(&mut return_scan);
    }
    let mut edits = Vec::new();
    let last = statements.last().map(|statement| statement.span());
    for statement in statements {
        let capture = !return_scan.found && Some(statement.span()) == last;
        match statement {
            Stmt::Decl(Decl::Var(declaration)) => {
                let mut expressions = Vec::new();
                for variable in &declaration.decls {
                    let mut bindings = Bindings::default();
                    variable.name.visit_with(&mut bindings);
                    if let Some(name) = bindings
                        .names
                        .into_iter()
                        .find(|name| protected.contains(name))
                    {
                        return Err(format!("eval cell declares top-level `{name}`, which would replace an existing kernel global; rename the binding or assign globalThis.{name} explicitly"));
                    }
                    let initializer = variable
                        .init
                        .as_ref()
                        .map_or("undefined", |value| text(&source, value.span()));
                    expressions.push(format!(
                        "({} = {initializer})",
                        pattern(&variable.name, &source)?
                    ));
                }
                let mut replacement = String::new();
                let count = expressions.len();
                for (index, expression) in expressions.into_iter().enumerate() {
                    if capture && index + 1 == count {
                        replacement.push_str("return ");
                    }
                    replacement.push_str(&expression);
                    replacement.push_str(";\n");
                }
                edits.push((declaration.span, replacement));
            }
            Stmt::Expr(expression) if capture => {
                edits.push((
                    statement.span(),
                    format!("return ({});", text(&source, expression.expr.span())),
                ));
            }
            _ => {}
        }
    }
    Ok(format!("(async () => {{\n{}\n}})()", apply(&source, edits)))
}

fn rewrite_import(import: &ImportDecl, source: &str) -> String {
    let arguments = format!(
        "{}{}",
        literal(&import.src.value.to_string_lossy()),
        import
            .with
            .as_ref()
            .map_or_else(String::new, |attributes| format!(
                ", {{with: {}}}",
                text(source, attributes.span)
            ))
    );
    let namespace = import.specifiers.iter().find_map(|binding| match binding {
        ImportSpecifier::Namespace(binding) => Some(binding.local.sym.to_string()),
        _ => None,
    });
    let mut named = Vec::new();
    for binding in &import.specifiers {
        let (local, value) = match binding {
            ImportSpecifier::Default(binding) => (&binding.local.sym, "default".to_owned()),
            ImportSpecifier::Namespace(_) => continue,
            ImportSpecifier::Named(binding) => {
                let name = binding.imported.as_ref().map_or_else(
                    || binding.local.sym.to_string(),
                    |name| match name {
                        ModuleExportName::Ident(name) => name.sym.to_string(),
                        ModuleExportName::Str(name) => name.value.to_string_lossy().into_owned(),
                    },
                );
                (&binding.local.sym, name)
            }
        };
        named.push(format!("{}: {local}", literal(&value)));
    }
    if let Some(namespace) = namespace {
        let default = if named.is_empty() {
            String::new()
        } else {
            format!("const {{{}}} = {namespace};", named.join(","))
        };
        format!("const {namespace} = await import({arguments}); {default}")
    } else if named.is_empty() {
        format!("await import({arguments});")
    } else {
        format!("const {{{}}} = await import({arguments});", named.join(","))
    }
}

fn statements(source: &ParsedSource) -> Vec<&Stmt> {
    match source.program_ref() {
        ProgramRef::Script(script) => script.body.iter().collect(),
        ProgramRef::Module(module) => module
            .body
            .iter()
            .filter_map(|item| match item {
                ModuleItem::Stmt(statement) => Some(statement),
                _ => None,
            })
            .collect(),
    }
}

fn literal(value: &str) -> String {
    serde_json::Value::String(value.to_owned()).to_string()
}

fn text(source: &str, span: Span) -> &str {
    // deno_ast starts each source at byte position one.
    &source[span.lo.0 as usize - 1..span.hi.0 as usize - 1]
}

fn apply(source: &str, mut edits: Vec<(Span, String)>) -> String {
    edits.sort_by_key(|(span, _)| std::cmp::Reverse(span.lo.0));
    let mut source = source.to_owned();
    for (span, replacement) in edits {
        source.replace_range(span.lo.0 as usize - 1..span.hi.0 as usize - 1, &replacement);
    }
    source
}

fn pattern(value: &Pat, source: &str) -> Result<String, String> {
    Ok(match value {
        Pat::Ident(binding) => format!("globalThis[{}]", literal(&binding.id.sym)),
        Pat::Array(array) => format!(
            "[{}{}]",
            array
                .elems
                .iter()
                .map(|value| value
                    .as_ref()
                    .map_or_else(|| Ok(String::new()), |value| pattern(value, source)))
                .collect::<Result<Vec<_>, _>>()?
                .join(","),
            if array.elems.last().is_some_and(Option::is_none) {
                ","
            } else {
                ""
            }
        ),
        Pat::Object(object) => {
            let mut fields = Vec::new();
            for property in &object.props {
                fields.push(match property {
                    ObjectPatProp::KeyValue(property) => format!(
                        "{}: {}",
                        text(source, property.key.span()),
                        pattern(&property.value, source)?
                    ),
                    ObjectPatProp::Assign(property) => format!(
                        "{}: globalThis[{}]{}",
                        property.key.id.sym,
                        literal(&property.key.id.sym),
                        property
                            .value
                            .as_ref()
                            .map_or_else(String::new, |value| format!(
                                " = {}",
                                text(source, value.span())
                            ))
                    ),
                    ObjectPatProp::Rest(rest) => format!("...{}", pattern(&rest.arg, source)?),
                });
            }
            format!("{{{}}}", fields.join(","))
        }
        Pat::Assign(assign) => format!(
            "{} = {}",
            pattern(&assign.left, source)?,
            text(source, assign.right.span())
        ),
        Pat::Rest(rest) => format!("...{}", pattern(&rest.arg, source)?),
        _ => return Err("unsupported declaration binding".into()),
    })
}

#[derive(Default)]
struct Bindings {
    names: Vec<String>,
}
impl Visit for Bindings {
    fn visit_function(&mut self, _function: &Function) {}
    fn visit_arrow_expr(&mut self, _function: &ArrowExpr) {}
    fn visit_binding_ident(&mut self, binding: &BindingIdent) {
        self.names.push(binding.id.sym.to_string());
    }
}

#[derive(Default)]
struct Returns {
    found: bool,
}
impl Visit for Returns {
    fn visit_return_stmt(&mut self, _statement: &ReturnStmt) {
        self.found = true;
    }
    fn visit_function(&mut self, _function: &Function) {}
    fn visit_arrow_expr(&mut self, _function: &ArrowExpr) {}
    fn visit_class(&mut self, _class: &Class) {}
}
