//! Diagnostics via shared `echo_pipeline::analyze` (same meaning as `xo check`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use echo_diagnostics::Severity;
use echo_pipeline::{analyze, AnalyzeOptions};

use crate::document::{path_to_uri, paths_equal};
use crate::position::{byte_to_position, Position};

/// LSP diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspSeverity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

/// One diagnostic ready for `textDocument/publishDiagnostics`.
#[derive(Debug, Clone)]
pub struct LspDiagnostic {
    pub uri: String,
    pub severity: LspSeverity,
    pub code: Option<String>,
    pub message: String,
    pub start: Position,
    pub end: Position,
}

/// Run the shared analysis pipeline for `entry`, applying `overlays` (dirty buffers).
#[must_use]
pub fn analyze_path(
    entry: &Path,
    overlays: &HashMap<PathBuf, String>,
    use_cache: bool,
) -> Vec<LspDiagnostic> {
    let product = analyze(
        entry,
        &AnalyzeOptions {
            use_cache,
            overlays: overlays.clone(),
        },
    );

    let mut texts: HashMap<PathBuf, String> = HashMap::new();
    for m in &product.modules {
        if let Some(t) = overlays.get(&m.path) {
            texts.insert(m.path.clone(), t.clone());
        } else if let Ok(canon) = m.path.canonicalize() {
            if let Some(t) = overlays.get(&canon) {
                texts.insert(m.path.clone(), t.clone());
            }
        }
        if !texts.contains_key(&m.path) {
            if let Ok(t) = std::fs::read_to_string(&m.path) {
                texts.insert(m.path.clone(), t);
            }
        }
    }

    // Prefer AST on product for span mapping when present.
    let mut out = Vec::new();
    for d in product.diagnostics.items() {
        let severity = match d.severity {
            Severity::Error => LspSeverity::Error,
            Severity::Warning => LspSeverity::Warning,
            Severity::Note => LspSeverity::Information,
        };
        let (uri, start, end) = if let Some(span) = d.span {
            // Attribute by SourceId → module path (never by filename substring).
            let path = product
                .modules
                .iter()
                .find(|m| {
                    m.file
                        .as_ref()
                        .map(|f| f.source == span.source)
                        .unwrap_or(false)
                })
                .map(|m| m.path.clone())
                .unwrap_or_else(|| entry.to_path_buf());
            // Overlay text may be keyed by canonicalize; try both.
            let text = texts
                .get(&path)
                .or_else(|| {
                    path.canonicalize()
                        .ok()
                        .and_then(|c| texts.get(&c).map(|s| s))
                })
                .map(String::as_str)
                .unwrap_or("");
            // If still empty, try any text whose path equals this module.
            let text = if text.is_empty() {
                texts
                    .iter()
                    .find(|(p, _)| paths_equal(p, &path))
                    .map(|(_, t)| t.as_str())
                    .unwrap_or("")
            } else {
                text
            };
            let start = byte_to_position(text, span.start.0);
            let end = byte_to_position(text, span.end.0);
            (path_to_uri(&path), start, end)
        } else {
            (
                path_to_uri(entry),
                Position {
                    line: 0,
                    character: 0,
                },
                Position {
                    line: 0,
                    character: 0,
                },
            )
        };
        out.push(LspDiagnostic {
            uri,
            severity,
            code: d.code.clone(),
            message: d.message.clone(),
            start,
            end,
        });
    }
    out
}

/// Analyze using an in-memory buffer overlay on `path`.
#[must_use]
pub fn analyze_buffer(path: &Path, text: &str, use_cache: bool) -> Vec<LspDiagnostic> {
    let mut overlays = HashMap::new();
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    overlays.insert(key, text.to_string());
    analyze_path(path, &overlays, use_cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn analyze_clean_program() {
        let mut root = std::env::temp_dir();
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("echo-lsp-an-{t}"));
        fs::create_dir_all(&root).unwrap();
        let src = root.join("ok.echo");
        fs::write(&src, "$ x = 1\n").unwrap();
        let diags = analyze_path(&src, &HashMap::new(), false);
        assert!(
            diags.iter().all(|d| d.severity != LspSeverity::Error),
            "{diags:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn analyze_overlay_dirty() {
        let mut root = std::env::temp_dir();
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("echo-lsp-ov-{t}"));
        fs::create_dir_all(&root).unwrap();
        let src = root.join("t.echo");
        fs::write(&src, "$ x = 1\n").unwrap();
        let diags = analyze_buffer(&src, "$ x = 1\n$ x = 2\n", false);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("sem-shadow")),
            "{diags:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn shadow_diag_matches_shared_code() {
        // Same error code as `xo check` for no-shadowing (shared pipeline).
        let mut root = std::env::temp_dir();
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("echo-lsp-sh-{t}"));
        fs::create_dir_all(&root).unwrap();
        let src = root.join("bad.echo");
        let text = "$ x = 1\n$ x = 2\n";
        fs::write(&src, text).unwrap();
        let diags = analyze_path(&src, &HashMap::new(), false);
        let shadow: Vec<_> = diags
            .iter()
            .filter(|d| d.code.as_deref() == Some("sem-shadow"))
            .collect();
        assert!(!shadow.is_empty(), "{diags:?}");
        assert!(
            shadow[0].message.to_ascii_lowercase().contains("shadow")
                || shadow[0].message.contains("reintroduce"),
            "{}",
            shadow[0].message
        );
        // Span on second bind name (line 1 in 0-based).
        assert_eq!(shadow[0].start.line, 1, "{shadow:?}");
        let _ = fs::remove_dir_all(&root);
    }
}
