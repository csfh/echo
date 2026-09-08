//! Open document store (editor buffers).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::position::{position_to_byte, Position};

/// One open text document (LSP `TextDocumentItem` subset).
#[derive(Debug, Clone)]
pub struct OpenDocument {
    pub uri: String,
    /// Filesystem path when `uri` is `file://…`; otherwise unset.
    pub path: Option<PathBuf>,
    pub version: i32,
    pub text: String,
}

/// One `textDocument/didChange` content change (full or incremental).
#[derive(Debug, Clone)]
pub struct ContentChange {
    /// When set, replace this UTF-16 range; when `None`, replace the whole buffer.
    pub range: Option<(Position, Position)>,
    pub text: String,
}

/// In-memory open documents keyed by URI.
#[derive(Debug, Default)]
pub struct DocumentStore {
    docs: HashMap<String, OpenDocument>,
}

impl DocumentStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self, uri: String, version: i32, text: String) {
        let path = uri_to_path(&uri);
        self.docs.insert(
            uri.clone(),
            OpenDocument {
                uri,
                path,
                version,
                text,
            },
        );
    }

    /// Full-buffer replace (TextDocumentSyncKind Full, or last resort).
    pub fn change(&mut self, uri: &str, version: i32, text: String) -> bool {
        if let Some(doc) = self.docs.get_mut(uri) {
            doc.version = version;
            doc.text = text;
            true
        } else {
            false
        }
    }

    /// Apply an ordered list of content changes (incremental or full).
    ///
    /// Full changes (`range == None`) replace the entire buffer. Incremental
    /// ranges use LSP UTF-16 positions, matching [`crate::position`].
    pub fn apply_changes(&mut self, uri: &str, version: i32, changes: &[ContentChange]) -> bool {
        let Some(doc) = self.docs.get_mut(uri) else {
            return false;
        };
        for ch in changes {
            match ch.range {
                None => {
                    doc.text = ch.text.clone();
                }
                Some((start, end)) => {
                    let start_b = position_to_byte(&doc.text, start) as usize;
                    let end_b = position_to_byte(&doc.text, end) as usize;
                    let start_b = start_b.min(doc.text.len());
                    let end_b = end_b.min(doc.text.len()).max(start_b);
                    let mut next = String::with_capacity(
                        doc.text.len() - (end_b - start_b) + ch.text.len(),
                    );
                    next.push_str(&doc.text[..start_b]);
                    next.push_str(&ch.text);
                    next.push_str(&doc.text[end_b..]);
                    doc.text = next;
                }
            }
        }
        doc.version = version;
        true
    }

    pub fn close(&mut self, uri: &str) -> Option<OpenDocument> {
        self.docs.remove(uri)
    }

    #[must_use]
    pub fn get(&self, uri: &str) -> Option<&OpenDocument> {
        self.docs.get(uri)
    }

    /// Find open document by filesystem path (canonicalize-aware).
    #[must_use]
    pub fn get_by_path(&self, path: &Path) -> Option<&OpenDocument> {
        self.docs.values().find(|d| {
            d.path
                .as_ref()
                .map(|p| paths_equal(p, path))
                .unwrap_or(false)
        })
    }

    /// Overlay map for resolver: canonical path → buffer text.
    #[must_use]
    pub fn overlays(&self) -> HashMap<PathBuf, String> {
        let mut map = HashMap::new();
        for doc in self.docs.values() {
            if let Some(path) = &doc.path {
                let key = path.canonicalize().unwrap_or_else(|_| path.clone());
                map.insert(key, doc.text.clone());
            }
        }
        map
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Iterate open documents (order unspecified).
    pub fn iter(&self) -> impl Iterator<Item = &OpenDocument> {
        self.docs.values()
    }
}

/// Path equality that prefers canonicalize when both paths exist.
#[must_use]
pub fn paths_equal(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// Convert `file://` URI to a path (best-effort; non-file URIs → `None`).
#[must_use]
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // file:///home/… or file://localhost/home/…
    let path = if rest.starts_with('/') {
        PathBuf::from(rest)
    } else if let Some(stripped) = rest.strip_prefix("localhost") {
        PathBuf::from(stripped)
    } else {
        PathBuf::from(rest)
    };
    Some(percent_decode_path(path))
}

fn percent_decode_path(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if !s.contains('%') {
        return path;
    }
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((h << 4 | l) as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    PathBuf::from(out)
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Path → `file://` URI for locations / publishDiagnostics.
///
/// Percent-encodes path bytes that are not unreserved / path-safe so client
/// URIs with spaces or non-ASCII match decode→path round-trips.
#[must_use]
pub fn path_to_uri(path: &Path) -> String {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = abs.to_string_lossy();
    let mut out = String::with_capacity(s.len() + 16);
    out.push_str("file://");
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'/'
            | b'-'
            | b'_'
            | b'.'
            | b'~' => out.push(b as char),
            // Keep colon for Windows drive letters after absolute form if any.
            b':' => out.push(':'),
            _ => {
                out.push('%');
                out.push(hex_digit(b >> 4));
                out.push(hex_digit(b & 0xf));
            }
        }
    }
    out
}

fn hex_digit(n: u8) -> char {
    char::from(if n < 10 { b'0' + n } else { b'A' + (n - 10) })
}

/// Whether an LSP diagnostic URI refers to the same document as `doc`.
#[must_use]
pub fn diagnostic_matches_doc(diag_uri: &str, doc: &OpenDocument) -> bool {
    if diag_uri == doc.uri {
        return true;
    }
    let Some(doc_path) = doc.path.as_ref() else {
        return false;
    };
    let Some(diag_path) = uri_to_path(diag_uri) else {
        return false;
    };
    paths_equal(&diag_path, doc_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_roundtrip_linux() {
        let p = uri_to_path("file:///tmp/foo.echo").unwrap();
        assert_eq!(p, PathBuf::from("/tmp/foo.echo"));
    }

    #[test]
    fn path_to_uri_encodes_space() {
        let p = PathBuf::from("/tmp/my file.echo");
        let uri = path_to_uri(&p);
        assert!(uri.contains("%20"), "{uri}");
        assert!(!uri.contains(' '), "{uri}");
        let back = uri_to_path(&uri).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn open_change_close() {
        let mut s = DocumentStore::new();
        s.open("file:///tmp/a.echo".into(), 1, "$ x = 1\n".into());
        assert_eq!(s.len(), 1);
        assert!(s.change("file:///tmp/a.echo", 2, "$ x = 2\n".into()));
        assert_eq!(s.get("file:///tmp/a.echo").unwrap().version, 2);
        assert!(s.close("file:///tmp/a.echo").is_some());
        assert!(s.is_empty());
    }

    #[test]
    fn incremental_apply_middle() {
        let mut s = DocumentStore::new();
        s.open("file:///tmp/i.echo".into(), 1, "$ ab = 1\n".into());
        let changes = [ContentChange {
            range: Some((
                Position {
                    line: 0,
                    character: 2,
                },
                Position {
                    line: 0,
                    character: 4,
                },
            )),
            text: "xy".into(),
        }];
        assert!(s.apply_changes("file:///tmp/i.echo", 2, &changes));
        assert_eq!(s.get("file:///tmp/i.echo").unwrap().text, "$ xy = 1\n");
        assert_eq!(s.get("file:///tmp/i.echo").unwrap().version, 2);
    }

    #[test]
    fn incremental_then_full() {
        let mut s = DocumentStore::new();
        s.open("file:///tmp/f.echo".into(), 1, "old\n".into());
        let changes = [
            ContentChange {
                range: Some((
                    Position {
                        line: 0,
                        character: 0,
                    },
                    Position {
                        line: 0,
                        character: 3,
                    },
                )),
                text: "mid".into(),
            },
            ContentChange {
                range: None,
                text: "full\n".into(),
            },
        ];
        assert!(s.apply_changes("file:///tmp/f.echo", 3, &changes));
        assert_eq!(s.get("file:///tmp/f.echo").unwrap().text, "full\n");
    }

    #[test]
    fn paths_equal_same_string() {
        assert!(paths_equal(Path::new("/tmp/a"), Path::new("/tmp/a")));
    }

    #[test]
    fn diagnostic_matches_same_uri() {
        let mut s = DocumentStore::new();
        s.open("file:///tmp/a.echo".into(), 1, "".into());
        let doc = s.get("file:///tmp/a.echo").unwrap();
        assert!(diagnostic_matches_doc("file:///tmp/a.echo", doc));
    }
}
