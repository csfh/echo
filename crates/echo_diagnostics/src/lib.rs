//! Shared diagnostic model used across compiler layers.

#![forbid(unsafe_code)]

use echo_source::Span;

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

/// Structured diagnostic. Codes stabilize as user-visible surfaces land.
///
/// `code` is owned so diagnostics can be serialized (artifact cache) without
/// `'static` lifetime tricks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
    /// Optional stable code, e.g. `lex-leader-ws`.
    pub code: Option<String>,
}

impl Diagnostic {
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            span: None,
            code: None,
        }
    }

    #[must_use]
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            span: None,
            code: None,
        }
    }

    #[must_use]
    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}

/// Ordered bag of diagnostics from one pipeline stage.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, d: Diagnostic) {
        self.items.push(d);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    #[must_use]
    pub fn error_count(&self) -> usize {
        self.items
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    pub fn extend(&mut self, other: Diagnostics) {
        self.items.extend(other.items);
    }
}

/// Serialize diagnostics for the artifact cache (one record per line).
///
/// Format: `severity\tcode\tsource_id\tstart\tend\tmessage` with tabs in
/// message escaped as `\t` and newlines as `\n`.
#[must_use]
pub fn encode_diagnostics(diags: &Diagnostics) -> Vec<u8> {
    let mut out = String::new();
    for d in diags.items() {
        let sev = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        };
        let code = d.code.as_deref().unwrap_or("");
        let (sid, start, end) = match d.span {
            Some(s) => (s.source.as_u32(), s.start.0, s.end.0),
            None => (u32::MAX, 0, 0),
        };
        let msg = d
            .message
            .replace('\\', "\\\\")
            .replace('\t', "\\t")
            .replace('\n', "\\n");
        out.push_str(sev);
        out.push('\t');
        out.push_str(code);
        out.push('\t');
        out.push_str(&sid.to_string());
        out.push('\t');
        out.push_str(&start.to_string());
        out.push('\t');
        out.push_str(&end.to_string());
        out.push('\t');
        out.push_str(&msg);
        out.push('\n');
    }
    out.into_bytes()
}

/// Inverse of [`encode_diagnostics`]. Invalid lines are skipped.
#[must_use]
pub fn decode_diagnostics(bytes: &[u8]) -> Diagnostics {
    let mut diags = Diagnostics::new();
    let text = String::from_utf8_lossy(bytes);
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(6, '\t').collect();
        if parts.len() != 6 {
            continue;
        }
        let severity = match parts[0] {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            "note" => Severity::Note,
            _ => continue,
        };
        let code = if parts[1].is_empty() {
            None
        } else {
            Some(parts[1].to_string())
        };
        let sid: u32 = parts[2].parse().unwrap_or(u32::MAX);
        let start: u32 = parts[3].parse().unwrap_or(0);
        let end: u32 = parts[4].parse().unwrap_or(0);
        let message = parts[5]
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\\\", "\\");
        let mut d = Diagnostic {
            severity,
            message,
            span: None,
            code,
        };
        if sid != u32::MAX {
            d.span = Some(Span::new(
                echo_source::SourceId::from_u32(sid),
                echo_source::BytePos(start),
                echo_source::BytePos(end),
            ));
        }
        diags.push(d);
    }
    diags
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use echo_source::{BytePos, SourceId, Span};

    #[test]
    fn encode_decode_roundtrip() {
        let mut d = Diagnostics::new();
        d.push(
            Diagnostic::error("hi")
                .with_code("sem-unbound")
                .with_span(Span::new(SourceId::from_u32(0), BytePos(1), BytePos(2))),
        );
        let bytes = encode_diagnostics(&d);
        let back = decode_diagnostics(&bytes);
        assert_eq!(back.items().len(), 1);
        assert_eq!(back.items()[0].code.as_deref(), Some("sem-unbound"));
        assert_eq!(back.items()[0].message, "hi");
        assert_eq!(back.items()[0].span.unwrap().start.0, 1);
    }
}
