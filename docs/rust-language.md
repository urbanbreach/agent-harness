# Rust Language

Rust is a modern systems programming language focused on safety, speed, and concurrency. It helps developers build reliable software with strong compile-time guarantees.

Most Rust projects are organized as crates and built with Cargo, Rust's build tool and package manager.

## What Rust is good at

- Memory safety without a garbage collector
- High-performance applications
- Reliable concurrent and parallel code
- Command-line tools, services, and embedded software
- Cross-platform development
- Zero-cost abstractions

## Core ideas

### Ownership

Rust uses ownership to manage memory without a garbage collector. Each value has a single owner, and it is dropped automatically when it leaves scope.

### Borrowing

Instead of copying data, Rust often lets you borrow references:

- `&T` for shared reads
- `&mut T` for exclusive mutation

### Lifetimes

Lifetimes describe how long references are valid. The compiler checks them so borrowed data stays safe.

### String types

`String` owns heap-allocated UTF-8 text, while `&str` is a borrowed string slice.

### Pattern matching

Rust has powerful pattern matching with `match`, `if let`, and `while let`.

### Error handling

Rust encourages explicit error handling with `Option<T>` and `Result<T, E>`, so failure paths stay visible and intentional.
The `?` operator keeps propagation concise in functions that return `Result` or `Option`.

## Example

This example combines borrowing with `Result`-based error handling:

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

## Common tooling

`cargo` — build, test, package, and manage Rust projects
`cargo check` — quickly validate code without producing a binary
`cargo fmt` — format code consistently
`cargo clippy` — lint for common mistakes
`cargo test` — run tests
`rustc` — Rust compiler

## Editions and ecosystem

Rust uses editions to introduce language changes without breaking older code. Common editions include 2018, 2021, and 2024.
Most reusable Rust code is published as a crate on [crates.io](https://crates.io/), and dependencies are usually managed through `Cargo.toml`.

## Practical tips

- Run `cargo fmt` before sharing code
- Use `cargo check` for fast feedback while iterating
- Use `cargo clippy` to catch common mistakes early
- Run `cargo test` often while iterating
- Read compiler errors carefully; they are usually specific and actionable
- Start with small, focused functions and let the compiler guide refactoring

## First commands to try

- `cargo new hello-rust` — create a new project
- `cargo run` — build and run the current package
- `cargo test` — run tests

## Why people like Rust

Rust combines low-level control with strong compile-time guarantees. That makes it popular for performance-critical software where correctness matters.

## Learning path

1. Learn variables, functions, and control flow
2. Understand ownership and borrowing
3. Practice structs, enums, and traits
4. Build projects with Cargo
5. Explore async, macros, and unsafe Rust

## Summary

Rust is a fast, safe, and expressive language that helps developers build robust software with confidence.
