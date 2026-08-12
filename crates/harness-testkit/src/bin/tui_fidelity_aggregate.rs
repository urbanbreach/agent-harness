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
    let roots = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let summary = harness_testkit::tui_fidelity_aggregate::aggregate(&roots)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
