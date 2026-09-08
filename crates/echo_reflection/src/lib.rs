//! Callable and reflection metadata (tools only).
//!
//! Status: stub. See `docs/reflection.md`. Future APIs will mirror
//! index/resolver/semantics — never a parallel type system.

#![forbid(unsafe_code)]

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}
