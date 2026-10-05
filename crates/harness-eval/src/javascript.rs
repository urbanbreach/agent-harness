mod memory;
mod modules;
mod protocol;
mod runtime;
mod stdio;
mod tools;
mod transform;

pub fn worker_main() -> std::process::ExitCode {
    let outcome = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())
        .and_then(|runtime| runtime.block_on(runtime::run()));
    match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("eval JavaScript worker: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
