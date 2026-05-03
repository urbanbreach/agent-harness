# Rust Language

Rust is a systems programming language that emphasizes safety, speed, and concurrency. Its type system and borrow checker catch many bugs before code runs, which makes it a strong fit for reliable software.

## What Rust is good at

Memory safety without a garbage collector
High-performance services and command-line tools
Reliable concurrent and parallel code
Embedded, networking, and systems software
Cross-platform development with predictable performance

## Core ideas

### Ownership and borrowing

Each value in Rust has a single owner. When the owner goes out of scope, the value is dropped automatically. Borrowing lets code access data without taking ownership, and the compiler checks that references stay valid.

### Types and pattern matching

Rust leans on enums, structs, traits, and pattern matching. `match`, `if let`, and `while let` make it easy to express control flow clearly and handle every case explicitly.

### Error handling

Rust favors `Option<T>` and `Result<T, E>` instead of exceptions. The `?` operator keeps error propagation concise while still making failure paths visible in the type system.

### Cargo and crates

Most Rust projects are built with `cargo`, which handles building, testing, formatting, dependency management, and publishing. Reusable code is packaged as crates and shared through `Cargo.toml`.

## Example

This example shows borrowing, pattern matching, and `Result`-based error handling together:

```rust
fn main() {
let name = String::from("Rust");

if let Err(err) = greet(&name) {
eprintln!("error: {err}");
}
}

fn greet(name: &str) -> Result<(), &'static str> {
if name.is_empty() {
return Err("name cannot be empty");
}

println!("Hello, {name}!");
Ok(())
}
```

## Practical workflow

Run `cargo fmt` to keep formatting consistent
Use `cargo check` for fast feedback while iterating
Run `cargo clippy` to catch common mistakes early
Run `cargo test` often to protect behavior
Read compiler errors carefully; they are usually specific and actionable

## Learning path

1. Learn variables, functions, and control flow
2. Understand ownership and borrowing
3. Practice structs, enums, and traits
4. Build projects with Cargo
5. Explore async, macros, and unsafe Rust

## Summary

Rust combines low-level control with strong compile-time guarantees. That makes it a popular choice for performance-critical software where correctness matters.
