//! Locked rich-string / rich-bytes / rich-locator escape set.
//!
//! Authority: `docs/syntax.md` — `\n` `\t` `\r` `\\` `\"` `\{` `\}` `\xHH`.
//! Unknown escapes must not be rewritten (`\q` is not `q`).

use std::fmt;

/// Why a `\` sequence is not a locked escape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscapeError {
    /// `\` is the last byte of the literal payload.
    LoneBackslash,
    /// `\` followed by a character outside the locked set.
    Unknown(u8),
    /// `\x` without two hex digits.
    HexIncomplete,
    /// `\x` plus two bytes that are not hex digits.
    HexInvalid,
}

impl fmt::Display for EscapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LoneBackslash => write!(f, "rich string ends with lone backslash"),
            Self::Unknown(b) => {
                if b.is_ascii_graphic() {
                    write!(f, "unknown escape `\\{}`", *b as char)
                } else {
                    write!(f, "unknown escape `\\x{b:02X}`")
                }
            }
            Self::HexIncomplete => write!(f, "incomplete `\\x` escape (need two hex digits)"),
            Self::HexInvalid => write!(f, "invalid hex escape"),
        }
    }
}

/// Decode one escape whose first byte is the character **after** `\`.
///
/// On success, returns `(decoded_byte, bytes_consumed_from_rest)`.
pub fn decode_escape(rest: &[u8]) -> Result<(u8, usize), EscapeError> {
    let Some(&b) = rest.first() else {
        return Err(EscapeError::LoneBackslash);
    };
    match b {
        b'n' => Ok((b'\n', 1)),
        b't' => Ok((b'\t', 1)),
        b'r' => Ok((b'\r', 1)),
        b'\\' => Ok((b'\\', 1)),
        b'"' => Ok((b'"', 1)),
        b'{' => Ok((b'{', 1)),
        b'}' => Ok((b'}', 1)),
        b'x' | b'X' => {
            if rest.len() < 3 {
                return Err(EscapeError::HexIncomplete);
            }
            let h = &rest[1..3];
            if !h[0].is_ascii_hexdigit() || !h[1].is_ascii_hexdigit() {
                return Err(EscapeError::HexInvalid);
            }
            let s = std::str::from_utf8(h).map_err(|_| EscapeError::HexInvalid)?;
            let byte = u8::from_str_radix(s, 16).map_err(|_| EscapeError::HexInvalid)?;
            Ok((byte, 3))
        }
        other => Err(EscapeError::Unknown(other)),
    }
}

/// How many bytes of `rest` to skip after a failed [`decode_escape`] so the
/// scanner can keep looking for the closing quote.
#[must_use]
pub fn skip_bad_escape(rest: &[u8], err: &EscapeError) -> usize {
    match err {
        EscapeError::LoneBackslash => 0,
        EscapeError::Unknown(_) => rest.first().map(|_| 1).unwrap_or(0),
        EscapeError::HexIncomplete | EscapeError::HexInvalid => {
            let mut n = 0;
            if matches!(rest.first(), Some(b'x' | b'X')) {
                n = 1;
                if rest
                    .get(1)
                    .is_some_and(|c| *c != b'"' && *c != b'\n' && *c != b'\r')
                {
                    n += 1;
                }
                if rest.get(2).is_some_and(|c| c.is_ascii_hexdigit()) {
                    n += 1;
                }
            } else if !rest.is_empty() {
                n = 1;
            }
            n
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_set_decodes() {
        assert_eq!(decode_escape(b"nxyz"), Ok((b'\n', 1)));
        assert_eq!(decode_escape(b"t"), Ok((b'\t', 1)));
        assert_eq!(decode_escape(b"r"), Ok((b'\r', 1)));
        assert_eq!(decode_escape(b"\\"), Ok((b'\\', 1)));
        assert_eq!(decode_escape(b"\""), Ok((b'"', 1)));
        assert_eq!(decode_escape(b"{"), Ok((b'{', 1)));
        assert_eq!(decode_escape(b"}"), Ok((b'}', 1)));
        assert_eq!(decode_escape(b"x41!"), Ok((b'A', 3)));
        assert_eq!(decode_escape(b"X0a"), Ok((0x0a, 3)));
    }

    #[test]
    fn unknown_is_not_the_following_char() {
        assert_eq!(decode_escape(b"q"), Err(EscapeError::Unknown(b'q')));
        assert_eq!(decode_escape(b""), Err(EscapeError::LoneBackslash));
        assert_eq!(decode_escape(b"x"), Err(EscapeError::HexIncomplete));
        assert_eq!(decode_escape(b"xG"), Err(EscapeError::HexIncomplete));
        assert_eq!(decode_escape(b"xGG"), Err(EscapeError::HexInvalid));
        assert_eq!(decode_escape(b"xZZ"), Err(EscapeError::HexInvalid));
    }
}
