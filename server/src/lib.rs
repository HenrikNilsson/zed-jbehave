//! `jbehave-language-server`: a standalone, native LSP server for JBehave
//! `.story` files, consumed by the Zed extension `crates/zed-jbehave`.
//!
//! This library target exists so the server can be exercised end to end from
//! `tests/`; `src/main.rs` is a thin wrapper around `lsp::run`.

pub mod analysis;
pub mod index;
pub mod java;
pub mod lsp;
pub mod matcher;
pub mod positions;
pub mod story;
