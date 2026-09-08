//! Source identity, maps, and file text/path metadata.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// Opaque id for a registered source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceId(u32);

impl SourceId {
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self.0
    }

    /// Reconstruct from a previously serialized id (cache / tools only).
    #[must_use]
    pub fn from_u32(id: u32) -> Self {
        Self(id)
    }
}

/// Byte offset into a source file (UTF-8 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BytePos(pub u32);

impl BytePos {
    #[must_use]
    pub fn as_usize(self) -> usize {
        self.0 as usize
    }
}

/// Half-open byte range `[start, end)` in a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    pub source: SourceId,
    pub start: BytePos,
    pub end: BytePos,
}

impl Span {
    #[must_use]
    pub fn new(source: SourceId, start: BytePos, end: BytePos) -> Self {
        Self {
            source,
            start,
            end,
        }
    }

    #[must_use]
    pub fn len(self) -> u32 {
        self.end.0.saturating_sub(self.start.0)
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// Byte offset of the start of each line (line 0 at offset 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMap {
    /// `line_starts[i]` = UTF-8 byte offset of the first byte of line `i` (0-based).
    line_starts: Vec<u32>,
    /// Total text length in bytes.
    len: u32,
}

impl LineMap {
    /// Build a line map from UTF-8 source text.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                let next = (i + 1) as u32;
                if next as usize <= text.len() {
                    line_starts.push(next);
                }
            }
        }
        Self {
            line_starts,
            len: text.len() as u32,
        }
    }

    /// 0-based line and column (UTF-8 byte column within the line) for `pos`.
    #[must_use]
    pub fn line_col(&self, pos: BytePos) -> (u32, u32) {
        let off = pos.0.min(self.len);
        let line = match self.line_starts.binary_search(&off) {
            Ok(i) => i as u32,
            Err(i) => i.saturating_sub(1) as u32,
        };
        let start = self.line_starts.get(line as usize).copied().unwrap_or(0);
        (line, off.saturating_sub(start))
    }

    /// 1-based line and column (common for user-facing diagnostics).
    #[must_use]
    pub fn line_col_1based(&self, pos: BytePos) -> (u32, u32) {
        let (l, c) = self.line_col(pos);
        (l + 1, c + 1)
    }

    /// Format `path:line:col` (1-based) for a byte position.
    #[must_use]
    pub fn format_location(&self, path: &Path, pos: BytePos) -> String {
        let (line, col) = self.line_col_1based(pos);
        format!("{}:{line}:{col}", path.display())
    }

    /// Format `path:line:col-endline:endcol` for a span (1-based).
    #[must_use]
    pub fn format_span_location(&self, path: &Path, span: Span) -> String {
        let (sl, sc) = self.line_col_1based(span.start);
        let (el, ec) = self.line_col_1based(span.end);
        if sl == el {
            format!("{}:{sl}:{sc}-{ec}", path.display())
        } else {
            format!("{}:{sl}:{sc}-{el}:{ec}", path.display())
        }
    }

    #[must_use]
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }
}

/// One Echo source unit (path + UTF-8 text).
#[derive(Debug, Clone)]
pub struct SourceFile {
    id: SourceId,
    path: PathBuf,
    text: String,
    line_map: LineMap,
}

impl SourceFile {
    #[must_use]
    pub fn new(id: SourceId, path: impl Into<PathBuf>, text: impl Into<String>) -> Self {
        let text = text.into();
        let line_map = LineMap::from_text(&text);
        Self {
            id,
            path: path.into(),
            text,
            line_map,
        }
    }

    #[must_use]
    pub fn id(&self) -> SourceId {
        self.id
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.text.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Precomputed line starts for this file.
    #[must_use]
    pub fn line_map(&self) -> &LineMap {
        &self.line_map
    }

    /// 0-based line and UTF-8 byte column for a position in this file.
    #[must_use]
    pub fn line_col(&self, pos: BytePos) -> (u32, u32) {
        self.line_map.line_col(pos)
    }

    /// 1-based line and column for diagnostics.
    #[must_use]
    pub fn line_col_1based(&self, pos: BytePos) -> (u32, u32) {
        self.line_map.line_col_1based(pos)
    }

    /// Slice of this file covered by `span`. Panics if `span` is for another file
    /// or out of bounds.
    #[must_use]
    pub fn slice(&self, span: Span) -> &str {
        assert_eq!(span.source, self.id, "span source mismatch");
        &self.text[span.start.as_usize()..span.end.as_usize()]
    }
}

/// Registry of source files for a compilation session.
#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `path` with `text`. Returns the file id.
    pub fn add(&mut self, path: impl Into<PathBuf>, text: impl Into<String>) -> SourceId {
        let id = SourceId(self.files.len() as u32);
        self.files.push(SourceFile::new(id, path, text));
        id
    }

    /// Load a path from the filesystem and register it.
    pub fn load(&mut self, path: impl Into<PathBuf>) -> std::io::Result<SourceId> {
        let path = path.into();
        let text = std::fs::read_to_string(&path)?;
        Ok(self.add(path, text))
    }

    #[must_use]
    pub fn get(&self, id: SourceId) -> Option<&SourceFile> {
        self.files.get(id.0 as usize)
    }

    #[must_use]
    pub fn get_mut(&mut self, id: SourceId) -> Option<&mut SourceFile> {
        self.files.get_mut(id.0 as usize)
    }

    #[must_use]
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_map_assigns_ids() {
        let mut map = SourceMap::new();
        let a = map.add("a.echo", "$ x = 1\n");
        let b = map.add("b.echo", "% user { }\n");
        assert_eq!(a.as_u32(), 0);
        assert_eq!(b.as_u32(), 1);
        assert_eq!(map.get(a).unwrap().path(), Path::new("a.echo"));
        assert_eq!(map.get(b).unwrap().text(), "% user { }\n");
    }

    #[test]
    fn line_map_basic() {
        let text = "ab\nc\n\nd";
        let lm = LineMap::from_text(text);
        assert_eq!(lm.line_col(BytePos(0)), (0, 0));
        assert_eq!(lm.line_col(BytePos(2)), (0, 2)); // '\n' of line 0
        assert_eq!(lm.line_col(BytePos(3)), (1, 0)); // 'c'
        assert_eq!(lm.line_col_1based(BytePos(3)), (2, 1));
        let mut map = SourceMap::new();
        let id = map.add("t.echo", text);
        let f = map.get(id).unwrap();
        assert_eq!(f.line_col(BytePos(3)), (1, 0));
        let span = Span::new(id, BytePos(3), BytePos(4));
        assert_eq!(
            lm.format_span_location(Path::new("t.echo"), span),
            "t.echo:2:1-2"
        );
    }

    #[test]
    fn span_slice() {
        let mut map = SourceMap::new();
        let id = map.add("t.echo", "~ n = 1");
        let file = map.get(id).unwrap();
        let span = Span::new(id, BytePos(0), BytePos(1));
        assert_eq!(file.slice(span), "~");
    }
}
