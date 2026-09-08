//! Language server presentation layer.
//!
//! Document model + IDE features over the **shared** resolver/check pipeline and
//! the same `.xo` artifact cache as `xo check` ([`docs/incremental.md`]).
//! Does not reimplement language semantics.
//!
//! Protocol state lives in [`session::LspSession`] (testable without stdio);
//! [`server::run_stdio`] is the Content-Length wire loop used by `xo lsp`.

#![forbid(unsafe_code)]

mod analysis;
mod document;
mod features;
mod names;
mod position;
mod server;
mod session;

pub use analysis::{analyze_buffer, analyze_path, LspDiagnostic, LspSeverity};
pub use document::{
    diagnostic_matches_doc, path_to_uri, paths_equal, uri_to_path, ContentChange, DocumentStore,
    OpenDocument,
};
pub use features::{
    analysis_product, completion, definition, document_symbols, format_document, format_edits,
    hover, references, rename, semantic_tokens, semantic_tokens_with_ast, signature_help,
    workspace_symbols, CompletionItem, HoverInfo, Location, RenameResult, SymbolInfo,
};
pub use position::{byte_to_position, position_to_byte, Position};
pub use server::run_stdio;
pub use session::{outgoing_to_json, LspSession, Outgoing};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}
