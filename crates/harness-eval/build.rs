use deno_runtime::snapshot::{create_runtime_snapshot, LazyExtensionFileKind};
use std::{collections::BTreeSet, path::PathBuf};

deno_core::extension!(eval_warmup, js = [dir "src/javascript", "warmup.js"]);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Preloaded lazy modules need compiled function bodies in the snapshot.
    deno_core::v8::V8::set_flags_from_string("--no-lazy --no-lazy-eval --no-lazy-streaming");
    let output = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR is unavailable")?);
    let snapshot = create_runtime_snapshot(
        output.join("eval-v8.bin"),
        Default::default(),
        vec![eval_warmup::init()],
    );
    let consumed: BTreeSet<_> = snapshot.consumed_lazy_specifiers.into_iter().collect();
    let mut js = Vec::new();
    let mut esm = Vec::new();
    for file in snapshot.lazy_extension_files {
        if consumed.contains(&file.specifier) {
            continue;
        }
        let text = std::fs::read_to_string(&file.path)?;
        let (text, _) = deno_runtime::transpile::maybe_transpile_source(
            file.specifier.clone().into(),
            text.into(),
        )?;
        let source = format!("({:?}, {:?})", file.specifier, text.as_str());
        match file.kind {
            LazyExtensionFileKind::Js => js.push(source),
            LazyExtensionFileKind::Esm => esm.push(source),
        }
    }
    std::fs::write(
        output.join("eval-sources.rs"),
        format!(
            "pub const JS: &[(&str, &str)] = &[{}];\npub const ESM: &[(&str, &str)] = &[{}];\n",
            js.join(",\n"),
            esm.join(",\n")
        ),
    )?;
    Ok(())
}
