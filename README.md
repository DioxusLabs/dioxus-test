# An experimental library for testing Dioxus apps

This allows writing tests in Rust which interact with and assert on the Dioxus
component model.

More details to come.

## Development

This repo ships a pre-commit hook that runs `cargo fmt --check`, `cargo
clippy`, and `cargo doc`. To enable it, run once after cloning:

```
git config core.hooksPath .githooks
```
