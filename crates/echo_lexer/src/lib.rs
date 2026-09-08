//! Tokenization and lexer diagnostics.
//!
//! Phase focus: **statement leaders** (roadmap §2.1) with dual-use glyphs
//! distinguished by statement-start position. Also emits identifiers, numbers,
//! strings, comments-skipped, and expression punctuation so leader lines can be
//! fully tokenized.

#![forbid(unsafe_code)]

use echo_diagnostics::{Diagnostic, Diagnostics};
use echo_source::{BytePos, SourceFile, Span};
use echo_syntax::{LeaderKind, decode_escape, skip_bad_escape};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// Lexical token kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// Statement leader (only emitted in leader position).
    Leader(LeaderKind),

    Ident,
    /// Integer or float text (validated lightly; further rules in later phases).
    Number,
    /// Pure `'...'` string including delimiters.
    StringPure,
    /// Rich `"..."` string including delimiters.
    StringRich,
    /// Pure `b'...'` bytes including prefix and delimiters.
    BytesPure,
    /// Rich `b"..."` bytes including prefix and delimiters.
    BytesRich,
    /// Pure `p'...'` locator (path/URI).
    LocatorPure,
    /// Rich `p"..."` locator (path/URI).
    LocatorRich,
    /// Number with duration suffix (`5s`, `10ms`, `100us`, …).
    Duration,

    // Expression / punctuation (non-leader uses of dual-use glyphs included)
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    EqEqEq,
    NotEqEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    AndAnd,
    OrOr,
    /// Bitwise AND (`&`).
    Ampersand,
    /// Bitwise XOR (`^`) — dual-use with return leader.
    Caret,
    /// Bitwise NOT (`~`) — dual-use with mutable-bind leader.
    Tilde,
    /// Shift left (`<<`).
    LtLt,
    /// Arithmetic shift right (`>>`).
    GtGt,
    Bang,
    Dot,
    /// Inclusive range operator `..` (expr `lo..hi`).
    DotDot,
    Comma,
    Colon,
    Eq,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    /// Expression `|` (true bool) or bitwise OR between ints — distinct from `Leader(Pipe)` match.
    Pipe,
    /// Expression `_` (false bool) or start of `_name` idents handled separately.
    Underscore,
    /// End of file.
    Eof,
}

impl TokenKind {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Leader(k) => match k {
                LeaderKind::Tilde => "leader_tilde",
                LeaderKind::Dollar => "leader_dollar",
                LeaderKind::Hash => "leader_hash",
                LeaderKind::Percent => "leader_percent",
                LeaderKind::At => "leader_at",
                LeaderKind::Question => "leader_question",
                LeaderKind::Colon => "leader_colon",
                LeaderKind::Bang => "leader_bang",
                LeaderKind::Caret => "leader_caret",
                LeaderKind::Star => "leader_star",
                LeaderKind::Lt => "leader_lt",
                LeaderKind::Gt => "leader_gt",
                LeaderKind::Pipe => "leader_pipe",
                LeaderKind::Plus => "leader_plus",
                LeaderKind::Minus => "leader_minus",
                LeaderKind::Ampersand => "leader_ampersand",
                LeaderKind::Slash => "leader_slash",
                LeaderKind::Backslash => "leader_backslash",
            },
            Self::Ident => "ident",
            Self::Number => "number",
            Self::StringPure => "string_pure",
            Self::StringRich => "string_rich",
            Self::BytesPure => "bytes_pure",
            Self::BytesRich => "bytes_rich",
            Self::LocatorPure => "locator_pure",
            Self::LocatorRich => "locator_rich",
            Self::Duration => "duration",
            Self::Plus => "plus",
            Self::Minus => "minus",
            Self::Star => "star",
            Self::Slash => "slash",
            Self::Percent => "percent",
            Self::EqEq => "eq_eq",
            Self::NotEq => "not_eq",
            Self::EqEqEq => "eq_eq_eq",
            Self::NotEqEq => "not_eq_eq",
            Self::Lt => "lt",
            Self::Gt => "gt",
            Self::LtEq => "lt_eq",
            Self::GtEq => "gt_eq",
            Self::AndAnd => "and_and",
            Self::OrOr => "or_or",
            Self::Ampersand => "ampersand",
            Self::Caret => "caret",
            Self::Tilde => "tilde",
            Self::LtLt => "lt_lt",
            Self::GtGt => "gt_gt",
            Self::Bang => "bang",
            Self::Dot => "dot",
            Self::DotDot => "dot_dot",
            Self::Comma => "comma",
            Self::Colon => "colon",
            Self::Eq => "eq",
            Self::LParen => "l_paren",
            Self::RParen => "r_paren",
            Self::LBracket => "l_bracket",
            Self::RBracket => "r_bracket",
            Self::LBrace => "l_brace",
            Self::RBrace => "r_brace",
            Self::Pipe => "pipe",
            Self::Underscore => "underscore",
            Self::Eof => "eof",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lexed {
    pub tokens: Vec<Token>,
    pub diagnostics: Diagnostics,
}

/// Tokenize an entire source file.
#[must_use]
pub fn lex(file: &SourceFile) -> Lexed {
    Lexer::new(file).run()
}

struct Lexer<'a> {
    file: &'a SourceFile,
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    /// Next non-trivia token is at statement start (after newline / BOF / indent).
    at_statement_start: bool,
    tokens: Vec<Token>,
    diagnostics: Diagnostics,
}

impl<'a> Lexer<'a> {
    fn new(file: &'a SourceFile) -> Self {
        Self {
            file,
            text: file.text(),
            bytes: file.text().as_bytes(),
            pos: 0,
            at_statement_start: true,
            tokens: Vec::new(),
            diagnostics: Diagnostics::new(),
        }
    }

    fn run(mut self) -> Lexed {
        while self.pos < self.bytes.len() {
            self.skip_spaces_and_tabs();
            if self.pos >= self.bytes.len() {
                break;
            }

            let b = self.bytes[self.pos];

            // Newline resets statement start.
            if b == b'\n' {
                self.pos += 1;
                self.at_statement_start = true;
                continue;
            }
            if b == b'\r' {
                self.pos += 1;
                if self.pos < self.bytes.len() && self.bytes[self.pos] == b'\n' {
                    self.pos += 1;
                }
                self.at_statement_start = true;
                continue;
            }

            // Line comment: `;` … EOL (not emitted).
            if b == b';' {
                self.skip_line_comment();
                continue;
            }

            // Statement leader position.
            if self.at_statement_start {
                if let Some(kind) = LeaderKind::from_char(b as char) {
                    self.lex_leader(kind);
                    continue;
                }
            }

            self.lex_non_leader();
        }

        let eof_pos = BytePos(self.bytes.len() as u32);
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span::new(self.file.id(), eof_pos, eof_pos),
        });

        Lexed {
            tokens: self.tokens,
            diagnostics: self.diagnostics,
        }
    }

    fn skip_spaces_and_tabs(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn skip_line_comment(&mut self) {
        // `;` already at pos
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'\n' || b == b'\r' {
                break;
            }
            self.pos += 1;
        }
        // Do not consume newline here — let main loop handle it so
        // at_statement_start is set correctly.
    }

    fn span_from(&self, start: usize) -> Span {
        Span::new(
            self.file.id(),
            BytePos(start as u32),
            BytePos(self.pos as u32),
        )
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        self.tokens.push(Token {
            kind,
            span: self.span_from(start),
        });
        // After any real token on a line, we are no longer at statement start.
        self.at_statement_start = false;
    }

    fn lex_leader(&mut self, kind: LeaderKind) {
        let start = self.pos;
        self.pos += 1; // consume glyph (all leaders are single ASCII bytes)

        if kind.requires_whitespace_after() {
            let ok = match self.peek_byte() {
                None => true, // EOF after leader
                Some(b) if b == b' ' || b == b'\t' => true,
                Some(b) if b == b'\n' || b == b'\r' => true,
                Some(b) if b == b';' => true, // `;` comment
                _ => false,
            };
            if !ok {
                let span = self.span_from(start);
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "statement leader '{}' requires whitespace after it",
                        kind.glyph()
                    ))
                    .with_span(span)
                    .with_code("lex-leader-ws"),
                );
            }
        }

        self.push(TokenKind::Leader(kind), start);
        // Spaces after leader are skipped by the main loop on the next iteration.
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn lex_non_leader(&mut self) {
        let start = self.pos;
        let b = self.bytes[self.pos];

        // Bytes / locator prefixes before general idents
        if b == b'b' {
            match self.bytes.get(self.pos + 1).copied() {
                Some(b'\'') => {
                    self.lex_prefixed_string(start, TokenKind::BytesPure, b'\'', "lex-bytes-pure");
                    return;
                }
                Some(b'"') => {
                    self.lex_prefixed_string(start, TokenKind::BytesRich, b'"', "lex-bytes-rich");
                    return;
                }
                _ => {}
            }
        }
        if b == b'p' {
            match self.bytes.get(self.pos + 1).copied() {
                Some(b'\'') => {
                    self.lex_prefixed_string(
                        start,
                        TokenKind::LocatorPure,
                        b'\'',
                        "lex-locator-pure",
                    );
                    return;
                }
                Some(b'"') => {
                    self.lex_prefixed_string(
                        start,
                        TokenKind::LocatorRich,
                        b'"',
                        "lex-locator-rich",
                    );
                    return;
                }
                _ => {}
            }
        }

        // Ident or lone `_` (false bool)
        if b == b'_' || b.is_ascii_alphabetic() {
            self.lex_ident_or_underscore(start);
            return;
        }

        // Number (and duration with unit suffix)
        if b.is_ascii_digit() {
            self.lex_number(start);
            return;
        }

        // Strings
        if b == b'\'' {
            self.lex_pure_string(start);
            return;
        }
        if b == b'"' {
            self.lex_rich_string(start);
            return;
        }

        // Multi-char operators first
        if self.try_multi_ops(start) {
            return;
        }

        // Single-char
        let kind = match b {
            b'+' => TokenKind::Plus,
            b'-' => TokenKind::Minus,
            b'*' => TokenKind::Star,
            b'/' => TokenKind::Slash,
            b'%' => TokenKind::Percent,
            b'<' => TokenKind::Lt,
            b'>' => TokenKind::Gt,
            b'!' => TokenKind::Bang,
            b'.' => TokenKind::Dot,
            b',' => TokenKind::Comma,
            b':' => TokenKind::Colon,
            b'=' => TokenKind::Eq,
            b'(' => TokenKind::LParen,
            b')' => TokenKind::RParen,
            b'[' => TokenKind::LBracket,
            b']' => TokenKind::RBracket,
            b'{' => TokenKind::LBrace,
            b'}' => TokenKind::RBrace,
            b'|' => TokenKind::Pipe,
            b'&' => TokenKind::Ampersand,
            b'^' => TokenKind::Caret,
            b'~' => TokenKind::Tilde,
            b'$' | b'#' | b'@' | b'?' | b'\\' => {
                // These glyphs are only leaders at statement start; elsewhere error.
                self.pos += 1;
                let span = self.span_from(start);
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "unexpected '{}' outside statement-leader position",
                        b as char
                    ))
                    .with_span(span)
                    .with_code("lex-unexpected-leader-glyph"),
                );
                return;
            }
            _ => {
                self.pos += 1;
                let span = self.span_from(start);
                let ch = self.text[start..self.pos]
                    .chars()
                    .next()
                    .unwrap_or('\u{FFFD}');
                self.diagnostics.push(
                    Diagnostic::error(format!("unexpected character {ch:?}"))
                        .with_span(span)
                        .with_code("lex-unexpected"),
                );
                return;
            }
        };
        self.pos += 1;
        self.push(kind, start);
    }

    fn try_multi_ops(&mut self, start: usize) -> bool {
        let rest = &self.text[self.pos..];
        let (kind, len) = if rest.starts_with("===") {
            (TokenKind::EqEqEq, 3)
        } else if rest.starts_with("!==") {
            (TokenKind::NotEqEq, 3)
        } else if rest.starts_with("==") {
            (TokenKind::EqEq, 2)
        } else if rest.starts_with("!=") {
            (TokenKind::NotEq, 2)
        } else if rest.starts_with("<<") {
            (TokenKind::LtLt, 2)
        } else if rest.starts_with(">>") {
            (TokenKind::GtGt, 2)
        } else if rest.starts_with("<=") {
            (TokenKind::LtEq, 2)
        } else if rest.starts_with(">=") {
            (TokenKind::GtEq, 2)
        } else if rest.starts_with("&&") {
            (TokenKind::AndAnd, 2)
        } else if rest.starts_with("||") {
            (TokenKind::OrOr, 2)
        } else if rest.starts_with("..") {
            (TokenKind::DotDot, 2)
        } else {
            return false;
        };
        self.pos += len;
        self.push(kind, start);
        true
    }

    fn lex_ident_or_underscore(&mut self, start: usize) {
        // Lone `_` is false bool; `_foo` is an ident.
        if self.bytes[start] == b'_' {
            let next = self.bytes.get(start + 1).copied();
            if next.is_none_or(|n| !is_ident_continue(n)) {
                self.pos = start + 1;
                self.push(TokenKind::Underscore, start);
                return;
            }
        }
        self.pos = start + 1;
        while self.pos < self.bytes.len() && is_ident_continue(self.bytes[self.pos]) {
            self.pos += 1;
        }
        self.push(TokenKind::Ident, start);
    }

    fn lex_number(&mut self, start: usize) {
        // 0x / 0b
        if self.bytes[start] == b'0' {
            if let Some(b'x' | b'X') = self.bytes.get(start + 1) {
                self.pos = start + 2;
                while self.pos < self.bytes.len() {
                    let b = self.bytes[self.pos];
                    if b.is_ascii_hexdigit() || b == b'_' {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                self.finish_number_or_duration(start, false);
                return;
            }
            if let Some(b'b' | b'B') = self.bytes.get(start + 1) {
                self.pos = start + 2;
                while self.pos < self.bytes.len() {
                    let b = self.bytes[self.pos];
                    if b == b'0' || b == b'1' || b == b'_' {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                self.finish_number_or_duration(start, false);
                return;
            }
        }

        self.pos = start + 1;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b.is_ascii_digit() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }

        // Fractional part
        if self.pos < self.bytes.len()
            && self.bytes[self.pos] == b'.'
            && self
                .bytes
                .get(self.pos + 1)
                .is_some_and(|d| d.is_ascii_digit())
        {
            self.pos += 1;
            while self.pos < self.bytes.len() {
                let b = self.bytes[self.pos];
                if b.is_ascii_digit() || b == b'_' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }

        // Exponent
        if self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b'e' | b'E') {
            let save = self.pos;
            self.pos += 1;
            if matches!(self.peek_byte(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.peek_byte().is_some_and(|d| d.is_ascii_digit()) {
                while self.pos < self.bytes.len() {
                    let b = self.bytes[self.pos];
                    if b.is_ascii_digit() || b == b'_' {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            } else {
                self.pos = save;
            }
        }

        let is_float = self.text[start..self.pos].contains('.')
            || self.text[start..self.pos].contains('e')
            || self.text[start..self.pos].contains('E');
        self.finish_number_or_duration(start, is_float);
    }

    /// After scanning digits, optionally take duration unit `us|ms|s|m|h`.
    fn finish_number_or_duration(&mut self, start: usize, is_float: bool) {
        if !is_float {
            let rest = &self.text[self.pos..];
            let unit = if rest.starts_with("us") {
                Some(2)
            } else if rest.starts_with("ms") {
                Some(2)
            } else if rest.starts_with('s')
                && !rest
                    .as_bytes()
                    .get(1)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                Some(1)
            } else if rest.starts_with('m')
                && !rest
                    .as_bytes()
                    .get(1)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                Some(1)
            } else if rest.starts_with('h')
                && !rest
                    .as_bytes()
                    .get(1)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                Some(1)
            } else {
                None
            };
            if let Some(len) = unit {
                self.pos += len;
                self.push(TokenKind::Duration, start);
                return;
            }
        }
        self.push(TokenKind::Number, start);
    }

    fn lex_prefixed_string(
        &mut self,
        start: usize,
        kind: TokenKind,
        quote: u8,
        err_code: &'static str,
    ) {
        // start at 'b' or 'p'; quote at start+1
        self.pos = start + 2;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == quote {
                self.pos += 1;
                self.push(kind, start);
                return;
            }
            if b == b'\\' && quote == b'"' {
                self.consume_rich_escape();
                continue;
            }
            if b == b'\n' || b == b'\r' {
                break;
            }
            self.pos += 1;
        }
        let span = self.span_from(start);
        self.diagnostics.push(
            Diagnostic::error("unterminated literal")
                .with_span(span)
                .with_code(err_code),
        );
    }

    fn lex_pure_string(&mut self, start: usize) {
        self.pos = start + 1;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'\'' {
                self.pos += 1;
                self.push(TokenKind::StringPure, start);
                return;
            }
            if b == b'\n' || b == b'\r' {
                break;
            }
            self.pos += 1;
        }
        let span = self.span_from(start);
        self.diagnostics.push(
            Diagnostic::error("unterminated pure string")
                .with_span(span)
                .with_code("lex-string-pure"),
        );
    }

    fn lex_rich_string(&mut self, start: usize) {
        self.pos = start + 1;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'"' {
                self.pos += 1;
                self.push(TokenKind::StringRich, start);
                return;
            }
            if b == b'\\' {
                self.consume_rich_escape();
                continue;
            }
            if b == b'\n' || b == b'\r' {
                break;
            }
            self.pos += 1;
        }
        let span = self.span_from(start);
        self.diagnostics.push(
            Diagnostic::error("unterminated rich string")
                .with_span(span)
                .with_code("lex-string-rich"),
        );
    }

    /// `pos` is on `\`. Validate the locked escape set; recover so we still
    /// find the closing quote.
    fn consume_rich_escape(&mut self) {
        let slash = self.pos;
        self.pos += 1;
        let rest = &self.bytes[self.pos..];
        match decode_escape(rest) {
            Ok((_, n)) => self.pos += n,
            Err(err) => {
                let skip = skip_bad_escape(rest, &err);
                self.pos += skip;
                let span = Span::new(
                    self.file.id(),
                    BytePos(slash as u32),
                    BytePos(self.pos as u32),
                );
                self.diagnostics.push(
                    Diagnostic::error(err.to_string())
                        .with_span(span)
                        .with_code("lex-escape"),
                );
            }
        }
    }
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Stable kind-only dump for fixtures (`echo26`): one token name per line.
#[must_use]
pub fn format_token_kinds(tokens: &[Token]) -> String {
    let mut out = String::new();
    for tok in tokens {
        out.push_str(tok.kind.name());
        out.push('\n');
    }
    out
}

/// Format tokens for `xo lex` dumps (one token per line with spans/text).
pub fn format_tokens(file: &SourceFile, tokens: &[Token]) -> String {
    let mut out = String::new();
    for tok in tokens {
        if tok.kind == TokenKind::Eof {
            out.push_str("eof\n");
            continue;
        }
        let text = file.slice(tok.span);
        let display = text.replace('\n', "\\n").replace('\r', "\\r");
        out.push_str(&format!(
            "{:<18} {:>4}..{:<4}  {:?}\n",
            tok.kind.name(),
            tok.span.start.0,
            tok.span.end.0,
            display
        ));
    }
    out
}

/// Diagnostic codes only, one per line (fixture `.diag` files).
#[must_use]
pub fn format_diag_codes(diagnostics: &Diagnostics) -> String {
    let mut out = String::new();
    for d in diagnostics.items() {
        out.push_str(d.code.as_deref().unwrap_or("-"));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use echo_source::SourceMap;
    use echo_syntax::LeaderKind;

    fn lex_str(src: &str) -> Lexed {
        let mut map = SourceMap::new();
        let id = map.add("test.echo", src);
        lex(map.get(id).unwrap())
    }

    fn kinds(lexed: &Lexed) -> Vec<TokenKind> {
        lexed
            .tokens
            .iter()
            .map(|t| t.kind)
            .filter(|k| *k != TokenKind::Eof)
            .collect()
    }

    #[test]
    fn all_leaders_at_line_start() {
        let src = "\
~ a = 1
$ b = 2
# C = 3
% user {
@ user {
? true {
: {
! \"x\"
^ 1
* {
<
>
| x {
+ {
- {
& {
/ std/io
\\ name
";
        let lexed = lex_str(src);
        assert!(
            lexed.diagnostics.is_empty(),
            "{:?}",
            lexed.diagnostics.items()
        );
        let leaders: Vec<LeaderKind> = kinds(&lexed)
            .into_iter()
            .filter_map(|k| match k {
                TokenKind::Leader(l) => Some(l),
                _ => None,
            })
            .collect();
        assert_eq!(
            leaders,
            vec![
                LeaderKind::Tilde,
                LeaderKind::Dollar,
                LeaderKind::Hash,
                LeaderKind::Percent,
                LeaderKind::At,
                LeaderKind::Question,
                LeaderKind::Colon,
                LeaderKind::Bang,
                LeaderKind::Caret,
                LeaderKind::Star,
                LeaderKind::Lt,
                LeaderKind::Gt,
                LeaderKind::Pipe,
                LeaderKind::Plus,
                LeaderKind::Minus,
                LeaderKind::Ampersand,
                LeaderKind::Slash,
                LeaderKind::Backslash,
            ]
        );
    }

    #[test]
    fn dual_use_star_slash_bang() {
        // After `$ x =`, `*` `/` `!` are expression ops, not leaders.
        let lexed = lex_str("$ x = 2 * 3 / 1\n$ y = ! |\n");
        assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
        let k = kinds(&lexed);
        assert_eq!(k[0], TokenKind::Leader(LeaderKind::Dollar));
        assert!(k.contains(&TokenKind::Star));
        assert!(k.contains(&TokenKind::Slash));
        assert!(k.contains(&TokenKind::Bang));
        assert!(k.contains(&TokenKind::Pipe));
        // Second line also has Dollar leader, then bang as expr.
        let leader_count = k
            .iter()
            .filter(|t| matches!(t, TokenKind::Leader(LeaderKind::Dollar)))
            .count();
        assert_eq!(leader_count, 2);
        assert!(
            !k.iter()
                .any(|t| matches!(t, TokenKind::Leader(LeaderKind::Star)))
        );
        assert!(
            !k.iter()
                .any(|t| matches!(t, TokenKind::Leader(LeaderKind::Slash)))
        );
    }

    #[test]
    fn bitwise_ops_tokens() {
        let lexed = lex_str("$ x = 1 & 2 | 3 ^ 4 << 1 >> 2\n$ y = ~ 0\n");
        assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
        let k = kinds(&lexed);
        assert!(k.contains(&TokenKind::Ampersand));
        assert!(k.contains(&TokenKind::Pipe));
        assert!(k.contains(&TokenKind::Caret));
        assert!(k.contains(&TokenKind::LtLt));
        assert!(k.contains(&TokenKind::GtGt));
        assert!(k.contains(&TokenKind::Tilde));
        assert!(
            !k.iter()
                .any(|t| matches!(t, TokenKind::Leader(LeaderKind::Caret)))
        );
        assert!(
            !k.iter()
                .any(|t| matches!(t, TokenKind::Leader(LeaderKind::Tilde)))
        );
    }

    #[test]
    fn leader_requires_whitespace() {
        let lexed = lex_str("$x = 1\n");
        assert_eq!(lexed.diagnostics.error_count(), 1);
        assert_eq!(
            lexed.diagnostics.items()[0].code.as_deref(),
            Some("lex-leader-ws")
        );
        assert_eq!(kinds(&lexed)[0], TokenKind::Leader(LeaderKind::Dollar));
    }

    #[test]
    fn bare_break_continue_no_ws() {
        let lexed = lex_str("<\n>\n");
        assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
        assert_eq!(
            kinds(&lexed),
            vec![
                TokenKind::Leader(LeaderKind::Lt),
                TokenKind::Leader(LeaderKind::Gt),
            ]
        );
    }

    #[test]
    fn indent_still_statement_start() {
        let lexed = lex_str("  $ x = 1\n");
        assert!(lexed.diagnostics.is_empty());
        assert_eq!(kinds(&lexed)[0], TokenKind::Leader(LeaderKind::Dollar));
    }

    #[test]
    fn comment_does_not_block_next_line_leader() {
        let lexed = lex_str("; note\n$ x = 1\n");
        assert!(lexed.diagnostics.is_empty());
        assert_eq!(kinds(&lexed)[0], TokenKind::Leader(LeaderKind::Dollar));
    }

    #[test]
    fn comparison_not_break_in_expr() {
        let lexed = lex_str("$ ok = n < 10\n");
        assert!(lexed.diagnostics.is_empty());
        let k = kinds(&lexed);
        assert!(k.contains(&TokenKind::Lt));
        assert!(
            !k.iter()
                .any(|t| matches!(t, TokenKind::Leader(LeaderKind::Lt)))
        );
    }

    #[test]
    fn dollar_bind_line_shape() {
        let lexed = lex_str("$ name = 42\n");
        assert!(lexed.diagnostics.is_empty());
        assert_eq!(
            kinds(&lexed),
            vec![
                TokenKind::Leader(LeaderKind::Dollar),
                TokenKind::Ident,
                TokenKind::Eq,
                TokenKind::Number,
            ]
        );
    }

    #[test]
    fn duration_suffix() {
        let lexed = lex_str("$ t = 5s\n");
        assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
        assert!(kinds(&lexed).contains(&TokenKind::Duration));
    }

    #[test]
    fn bytes_and_locator() {
        let lexed = lex_str("$ a = b\"hi\"\n$ p = p'/tmp'\n");
        assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
        let k = kinds(&lexed);
        assert!(k.contains(&TokenKind::BytesRich));
        assert!(k.contains(&TokenKind::LocatorPure));
    }

    #[test]
    fn legal_rich_escapes_are_silent() {
        let lexed = lex_str("$ s = \"a\\nb\\t\\{x\\}\\x41\"\n");
        assert!(
            lexed.diagnostics.is_empty(),
            "{:?}",
            lexed.diagnostics.items()
        );
        assert!(kinds(&lexed).contains(&TokenKind::StringRich));
    }

    #[test]
    fn unknown_rich_escape_is_lex_escape() {
        let lexed = lex_str("$ s = \"hello\\q\"\n");
        let codes: Vec<_> = lexed
            .diagnostics
            .items()
            .iter()
            .filter_map(|d| d.code.as_deref())
            .collect();
        assert_eq!(codes, ["lex-escape"], "{:?}", lexed.diagnostics.items());
        assert!(kinds(&lexed).contains(&TokenKind::StringRich));
    }

    #[test]
    fn bad_hex_and_prefixed_rich_escape() {
        let lexed = lex_str("$ a = \"\\xGG\"\n$ b = b\"\\q\"\n$ c = p\"\\q\"\n");
        let n = lexed
            .diagnostics
            .items()
            .iter()
            .filter(|d| d.code.as_deref() == Some("lex-escape"))
            .count();
        assert_eq!(n, 3, "{:?}", lexed.diagnostics.items());
    }

    #[test]
    fn pure_backslash_is_not_an_escape() {
        let lexed = lex_str("$ s = '\\q'\n");
        assert!(
            lexed.diagnostics.is_empty(),
            "{:?}",
            lexed.diagnostics.items()
        );
    }

    #[test]
    fn unexpected_leader_glyph_in_expr() {
        let lexed = lex_str("$ x = @\n");
        let codes: Vec<_> = lexed
            .diagnostics
            .items()
            .iter()
            .filter_map(|d| d.code.as_deref())
            .collect();
        assert!(codes.contains(&"lex-unexpected-leader-glyph"), "{codes:?}");
    }

    #[test]
    fn unexpected_character() {
        let lexed = lex_str("$ x = `\n");
        let codes: Vec<_> = lexed
            .diagnostics
            .items()
            .iter()
            .filter_map(|d| d.code.as_deref())
            .collect();
        assert!(codes.contains(&"lex-unexpected"), "{codes:?}");
    }

    #[test]
    fn unterminated_pure_and_rich() {
        let p = lex_str("$ s = 'hi\n");
        assert!(
            p.diagnostics
                .items()
                .iter()
                .any(|d| d.code.as_deref() == Some("lex-string-pure")),
            "{:?}",
            p.diagnostics.items()
        );
        let r = lex_str("$ s = \"hi\n");
        assert!(
            r.diagnostics
                .items()
                .iter()
                .any(|d| d.code.as_deref() == Some("lex-string-rich")),
            "{:?}",
            r.diagnostics.items()
        );
    }
}
