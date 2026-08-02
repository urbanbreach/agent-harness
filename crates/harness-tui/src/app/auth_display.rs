pub(super) fn auth_status_banner(args: &[String]) -> String {
    format!(
        "auth backend requested: harness auth {}",
        display_auth_args_for_status(args)
    )
}

fn display_auth_args_for_status(args: &[String]) -> String {
    let mut display = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for arg in args {
        if redact_next {
            display.push("<redacted>".to_string());
            redact_next = false;
            continue;
        }
        if auth_arg_redacts_next(arg) {
            display.push(arg.clone());
            redact_next = true;
            continue;
        }
        if let Some(redacted) = redact_auth_arg_value(arg) {
            display.push(redacted);
            continue;
        }
        display.push(arg.clone());
    }
    display.join(" ")
}

fn auth_arg_redacts_next(arg: &str) -> bool {
    matches!(
        arg,
        "--mock-token" | "--mock-refresh-token" | "--enterprise-url"
    )
}

fn redact_auth_arg_value(arg: &str) -> Option<String> {
    [
        "--mock-token=",
        "--mock-refresh-token=",
        "--enterprise-url=",
    ]
    .into_iter()
    .find_map(|prefix| {
        arg.strip_prefix(prefix)
            .map(|_| format!("{prefix}<redacted>"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth_args(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_string()).collect()
    }

    #[test]
    fn auth_status_banner_preserves_non_secret_args() {
        // arrange
        // act
        // assert
        let banner = auth_status_banner(&auth_args(&["login", "codex"]));

        assert_eq!(banner, "auth backend requested: harness auth login codex");
    }

    #[test]
    fn auth_status_banner_redacts_inline_secret_args() {
        // arrange
        // act
        // assert
        let banner = auth_status_banner(&auth_args(&[
            "login",
            "codex",
            "--mock-token=secret123",
            "--mock-refresh-token=refresh123",
        ]));

        assert_eq!(
            banner,
            "auth backend requested: harness auth login codex --mock-token=<redacted> --mock-refresh-token=<redacted>"
        );
    }

    #[test]
    fn auth_status_banner_redacts_next_arg_after_secret_flags() {
        // arrange
        // act
        // assert
        let banner = auth_status_banner(&auth_args(&[
            "login",
            "--enterprise-url",
            "https://harness.ai",
            "--api-key-stdin",
        ]));

        assert_eq!(
            banner,
            "auth backend requested: harness auth login --enterprise-url <redacted> --api-key-stdin"
        );
    }
}
