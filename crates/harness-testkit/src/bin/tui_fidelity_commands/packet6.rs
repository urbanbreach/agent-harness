use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use harness_testkit::tui_fidelity_packet6::build_capability_receipt;

pub(super) fn execute(arguments: Vec<OsString>) -> Result<(), String> {
    let args = parse(arguments)?;
    let input = fs::read_to_string(&args.input)
        .map_err(|error| format!("{}: {error}", args.input.display()))?;
    let receipt = build_capability_receipt(&input, &args.evidence_root, &args.authority_digest)
        .map_err(|error| error.to_string())?;
    fs::write(&args.output, receipt)
        .map_err(|error| format!("{}: {error}", args.output.display()))?;
    println!(
        "tui-fidelity packet6-capability PASS: {}",
        args.output.display()
    );
    Ok(())
}

struct Args {
    input: PathBuf,
    evidence_root: PathBuf,
    output: PathBuf,
    authority_digest: String,
}

fn parse(arguments: Vec<OsString>) -> Result<Args, String> {
    let mut values = arguments.into_iter();
    if values.next().as_deref() != Some(OsStr::new("packet6-capability")) {
        return Err("usage: packet6-capability --input PATH --evidence-root PATH --output PATH --authority-digest SHA256".to_owned());
    }
    let mut input = None;
    let mut evidence_root = None;
    let mut output = None;
    let mut authority_digest = None;
    while let Some(flag) = values.next() {
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {}", flag.to_string_lossy()))?;
        match flag.to_str() {
            Some("--input") => input = Some(value.into()),
            Some("--evidence-root") => evidence_root = Some(value.into()),
            Some("--output") => output = Some(value.into()),
            Some("--authority-digest") => {
                authority_digest = Some(value.to_string_lossy().into_owned());
            }
            _ => return Err(format!("unknown argument: {}", flag.to_string_lossy())),
        }
    }
    Ok(Args {
        input: input.ok_or("missing --input")?,
        evidence_root: evidence_root.ok_or("missing --evidence-root")?,
        output: output.ok_or("missing --output")?,
        authority_digest: authority_digest.ok_or("missing --authority-digest")?,
    })
}
