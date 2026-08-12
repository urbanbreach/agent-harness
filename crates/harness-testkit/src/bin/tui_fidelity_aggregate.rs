use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tui_fidelity_aggregate: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1).peekable();
    let profile = if args.peek().is_some_and(|value| value == "--profile") {
        let _ = args.next();
        match args
            .next()
            .and_then(|value| value.into_string().ok())
            .as_deref()
        {
            Some("packet2-scheduling") => {
                harness_testkit::tui_fidelity_compare::AcceptanceProfile::Packet2Scheduling
            }
            Some(value) => return Err(format!("unknown profile: {value}").into()),
            None => return Err("--profile requires a value".into()),
        }
    } else {
        harness_testkit::tui_fidelity_compare::AcceptanceProfile::FullParity
    };
    let roots = args.map(PathBuf::from).collect::<Vec<_>>();
    let summary = harness_testkit::tui_fidelity_aggregate::aggregate_with_profile(&roots, profile)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
