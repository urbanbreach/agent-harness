use std::fs;

fn main() {
    let path = "crates/harness-tools/src/shell_safety.rs";
    let content = fs::read_to_string(path).unwrap();

    // First, let's look at the current code in validate_shell_path_arguments
    let search = r#"        let candidate = if token.starts_with('-') {
            if let Some((_, value)) = token.split_once('=') {
                value
            } else {
                continue;
            }
        } else {
            token
        };"#;

    let replace = r#"        let candidate = if token.starts_with('-') {
            if let Some((_, value)) = token.split_once('=') {
                value
            } else if token.starts_with("--") {
                continue;
            } else {
                let mut chars = token.chars();
                chars.next(); // skip '-'
                chars.next(); // skip flag character
                chars.as_str()
            }
        } else {
            token
        };"#;

    let new_content = content.replace(search, replace);
    fs::write(path, new_content).unwrap();
}
