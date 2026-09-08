//! Byte offset ↔ LSP position (UTF-16 code units, 0-based).

/// LSP-style position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// Convert a UTF-8 byte offset into an LSP position (UTF-16 columns).
#[must_use]
pub fn byte_to_position(text: &str, byte_offset: u32) -> Position {
    let target = byte_offset as usize;
    let mut line = 0u32;
    let mut character = 0u32;
    let mut i = 0usize;
    for ch in text.chars() {
        if i >= target {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
        i += ch.len_utf8();
    }
    Position { line, character }
}

/// Convert LSP position to a UTF-8 byte offset (clamped to text length).
#[must_use]
pub fn position_to_byte(text: &str, pos: Position) -> u32 {
    let mut line = 0u32;
    let mut character = 0u32;
    let mut i = 0usize;
    for ch in text.chars() {
        if line == pos.line && character >= pos.character {
            return i as u32;
        }
        if ch == '\n' {
            if line == pos.line {
                return i as u32;
            }
            line += 1;
            character = 0;
        } else {
            if line == pos.line {
                character += ch.len_utf16() as u32;
            }
        }
        i += ch.len_utf8();
    }
    i as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_positions() {
        let t = "ab\ncd\n";
        assert_eq!(byte_to_position(t, 0), Position { line: 0, character: 0 });
        assert_eq!(byte_to_position(t, 3), Position { line: 1, character: 0 });
        assert_eq!(byte_to_position(t, 4), Position { line: 1, character: 1 });
        assert_eq!(position_to_byte(t, Position { line: 1, character: 1 }), 4);
    }

    #[test]
    fn utf16_bmp_and_emoji() {
        // "a" + emoji (U+1F600, two UTF-16 units) + "b"
        let t = "a😀b";
        // byte offset of 'b': "a" (1) + emoji (4) = 5
        let pos = byte_to_position(t, 5);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 3); // 1 + 2 (surrogate pair)
        assert_eq!(position_to_byte(t, Position { line: 0, character: 3 }), 5);
        assert_eq!(position_to_byte(t, Position { line: 0, character: 1 }), 1);
    }

    #[test]
    fn position_clamps_past_eol() {
        let t = "hi\n";
        // character past end of line → byte of newline
        assert_eq!(
            position_to_byte(
                t,
                Position {
                    line: 0,
                    character: 99
                }
            ),
            2
        );
    }
}
