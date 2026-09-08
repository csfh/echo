//! Grammar and syntax metadata facts for tooling generators.

#![forbid(unsafe_code)]

pub mod escapes;
pub mod leaders;
pub mod tree_sitter;

pub use escapes::{decode_escape, skip_bad_escape, EscapeError};
pub use leaders::{is_leader_char, LeaderFamily, LeaderKind, LEADERS};
pub use tree_sitter::{
    leader_glyphs, leader_token_names, tree_sitter_package_files, write_tree_sitter_grammar,
    GrammarFile,
};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}
