# Contributing to module_info

Thank you for your interest in contributing to module_info! This document provides guidelines and instructions for contributing.

## Contributor License Agreement (CLA)

This project welcomes contributions and suggestions. Most contributions require you to
agree to a Contributor License Agreement (CLA) declaring that you have the right to,
and actually do, grant us the rights to use your contribution. For details, visit
https://cla.opensource.microsoft.com.

When you submit a pull request, a CLA-bot will automatically determine whether you need
to provide a CLA and decorate the PR appropriately (e.g., label, comment). Simply follow the
instructions provided by the bot. You will only need to do this once across all repositories using our CLA.

## Code of Conduct

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/). For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/) or contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

## How to Contribute

### Reporting Bugs

Bug reports help make module_info better for everyone. When reporting a bug, please use the bug report template and include:

1. A clear, descriptive title
2. A detailed description of the issue
3. Steps to reproduce the behavior
4. Expected vs. actual behavior
5. Your environment (OS, Rust version, etc.)

### Suggesting Features

We welcome feature suggestions! Please use the feature request template and provide:

1. A clear description of the problem your feature solves
2. How you envision the solution working
3. Alternative approaches you've considered

### Pull Requests

1. Fork the repository
2. Create a new branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Run tests locally (`cargo test`)
5. Commit your changes (`git commit -m 'Add some amazing feature'`)
6. Push to the branch (`git push origin feature/amazing-feature`)
7. Open a Pull Request using the PR template

### Development Workflow

1. Make sure you have Rust installed (we recommend using [rustup](https://rustup.rs/)).
   The minimum supported Rust version (MSRV) is **1.74**, matching
   `rust-version` in `Cargo.toml`.
2. Clone the repository
3. Run `cargo build` to build the project
4. Run `cargo test` to run the tests

## Testing

- All new code should include tests
- Run the test suite with `cargo test`
- For Linux-specific functionality, ensure tests are conditionally compiled

### Pre-PR checks

Run these locally before opening a PR. CI runs the same set:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features embed-module-info -- -D warnings
cargo test --all-targets
cargo test --all-targets --features embed-module-info
cargo doc  --no-deps --features embed-module-info
```

Each example under `examples/` is a standalone Cargo package, so build and
test them from their own directories:

```sh
cargo build --manifest-path examples/sample_elf_bin/Cargo.toml
cargo test  --manifest-path examples/sample_elf_bin/Cargo.toml
```

## Style Guidelines

- Follow the standard Rust style guidelines.
- Run `cargo fmt --check` before submitting; CI rejects unformatted code.
- Run `cargo clippy --all-targets -- -D warnings` and
  `cargo clippy --all-targets --features embed-module-info -- -D warnings`.
  CI treats clippy warnings as errors under both feature configurations.
- In production code (`src/`), avoid `.unwrap()`, `.expect()`, and direct
  slice indexing (`slice[i]`); use pattern matching, `?`, or `.get()` so
  that build-script and runtime failures surface as `ModuleInfoError`
  instead of panics. Tests, doctests, and examples may use `.unwrap()` /
  `.expect()` freely.

## Documentation

- All public APIs should be documented
- Examples should be included when appropriate
- README should be updated for significant changes

Thank you for contributing to module_info!
