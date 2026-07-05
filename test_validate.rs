use std::fs;

fn main() {
    let path = "crates/harness-tools/src/shell_safety.rs";
    let content = fs::read_to_string(path).unwrap();
    println!("{}", content.contains("let mut chars = token.chars();"));
}
