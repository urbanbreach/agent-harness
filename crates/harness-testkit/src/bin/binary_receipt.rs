use harness_testkit::binary_receipt::{read_receipt, ReceiptExpectations, BINARY_RECEIPT_SCHEMA};
use std::env;
use std::path::PathBuf;

const REFERENCE_REVISION: &str = "eb267feff13129e568df38fb6fdf0ceb65f735d6";
const REFERENCE_PACKAGE: &str = "xai-grok-pager-bin";
const REFERENCE_EXECUTABLE: &str = "xai-grok-pager";

fn main() {
    match run(env::args_os().skip(1)) {
        Ok(()) => println!("binary receipt verified ({BINARY_RECEIPT_SCHEMA})"),
        Err(error) => {
            eprintln!("binary receipt verification failed: {error}");
            std::process::exit(1);
        }
    }
}

fn run(mut args: impl Iterator<Item = std::ffi::OsString>) -> Result<(), String> {
    let command = args
        .next()
        .ok_or_else(|| "missing command; expected `verify`".to_owned())?;
    if command != "verify" {
        return Err(format!("unknown command `{}`", command.to_string_lossy()));
    }
    let receipt = required_path(&mut args, "--receipt")?;
    let harness_revision = required_text(&mut args, "--harness-revision")?;
    if args.next().is_some() {
        return Err("unexpected argument after --harness-revision".to_owned());
    }

    let receipt = read_receipt(&receipt).map_err(|error| error.to_string())?;
    let expectations = ReceiptExpectations {
        reference_revision: REFERENCE_REVISION.to_owned(),
        harness_revision,
        reference_clean_pre: true,
        reference_clean_post: true,
        harness_clean_pre: false,
        harness_clean_post: false,
        reference_package: REFERENCE_PACKAGE.to_owned(),
        reference_executable: REFERENCE_EXECUTABLE.to_owned(),
        harness_package: "harness".to_owned(),
        harness_executable: "harness".to_owned(),
    };
    receipt
        .verify(&expectations)
        .map_err(|error| error.to_string())?;
    receipt
        .verify_binary_digests()
        .map_err(|error| error.to_string())
}

fn required_path(
    args: &mut impl Iterator<Item = std::ffi::OsString>,
    flag: &str,
) -> Result<PathBuf, String> {
    required_text(args, flag).map(PathBuf::from)
}

fn required_text(
    args: &mut impl Iterator<Item = std::ffi::OsString>,
    flag: &str,
) -> Result<String, String> {
    let observed_flag = args
        .next()
        .ok_or_else(|| format!("missing flag {flag}"))?
        .into_string()
        .map_err(|_| format!("flag {flag} must be valid UTF-8"))?;
    if observed_flag != flag {
        return Err(format!("expected flag {flag}, got {observed_flag}"));
    }
    let value = args
        .next()
        .ok_or_else(|| format!("missing value for {flag}"))?;
    let value = value
        .into_string()
        .map_err(|_| format!("value for {flag} must be valid UTF-8"))?;
    if value.trim().is_empty() {
        return Err(format!("value for {flag} must be non-empty"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::required_path;
    use std::ffi::OsString;

    #[test]
    fn required_path_rejects_unexpected_flag() {
        let mut args = ["--unexpected", "/tmp/receipt"]
            .into_iter()
            .map(OsString::from);

        assert!(required_path(&mut args, "--receipt").is_err());
    }
}
