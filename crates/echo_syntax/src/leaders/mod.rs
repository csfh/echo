//! Statement leaders — keyword-free statement introducers.
//!
//! Families: [`bind`], [`shape`], [`control`], [`module`].

pub mod bind;
pub mod control;
pub mod module;
pub mod shape;

/// Statement leader character and its role (keyword-free control/bind/define).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeaderKind {
    // bind
    /// `~` mutable bind / reassign
    Tilde,
    /// `$` immutable runtime bind
    Dollar,
    /// `#` compile-time constant
    Hash,
    // shape
    /// `%` struct shape
    Percent,
    /// `@` additional struct members
    At,
    // control
    /// `?` if
    Question,
    /// `:` else-if / else / match default
    Colon,
    /// `!` error return (language `result` err path)
    Bang,
    /// `^` return
    Caret,
    /// `*` loop
    Star,
    /// `<` break
    Lt,
    /// `>` continue
    Gt,
    /// `|` match
    Pipe,
    /// `+` task spawn (schedule on event loop immediately)
    Plus,
    /// `-` task join / immediate block
    Minus,
    /// `&` effect block (auto-unwrap result/option)
    Ampersand,
    // module
    /// `/` import
    Slash,
    /// `\` export
    Backslash,
}

impl LeaderKind {
    /// Which coarse family this leader belongs to.
    #[must_use]
    pub fn family(self) -> LeaderFamily {
        match self {
            Self::Tilde | Self::Dollar | Self::Hash => LeaderFamily::Bind,
            Self::Percent | Self::At => LeaderFamily::Shape,
            Self::Question
            | Self::Colon
            | Self::Bang
            | Self::Caret
            | Self::Star
            | Self::Lt
            | Self::Gt
            | Self::Pipe
            | Self::Plus
            | Self::Minus
            | Self::Ampersand => LeaderFamily::Control,
            Self::Slash | Self::Backslash => LeaderFamily::Module,
        }
    }

    /// Glyph as it appears in source.
    #[must_use]
    pub fn glyph(self) -> char {
        match self {
            Self::Tilde => '~',
            Self::Dollar => '$',
            Self::Hash => '#',
            Self::Percent => '%',
            Self::At => '@',
            Self::Question => '?',
            Self::Colon => ':',
            Self::Bang => '!',
            Self::Caret => '^',
            Self::Star => '*',
            Self::Lt => '<',
            Self::Gt => '>',
            Self::Pipe => '|',
            Self::Plus => '+',
            Self::Minus => '-',
            Self::Ampersand => '&',
            Self::Slash => '/',
            Self::Backslash => '\\',
        }
    }

    /// Short role name for diagnostics and dumps.
    #[must_use]
    pub fn role(self) -> &'static str {
        match self {
            Self::Tilde => "mutable bind",
            Self::Dollar => "immutable bind",
            Self::Hash => "compile-time constant",
            Self::Percent => "struct shape",
            Self::At => "struct members",
            Self::Question => "if",
            Self::Colon => "else-if / else / match default",
            Self::Bang => "error return",
            Self::Caret => "return",
            Self::Star => "loop",
            Self::Lt => "break",
            Self::Gt => "continue",
            Self::Pipe => "match",
            Self::Plus => "task spawn",
            Self::Minus => "task join",
            Self::Ampersand => "effect block",
            Self::Slash => "import",
            Self::Backslash => "export",
        }
    }

    /// Bare `<` and `>` do not require whitespace after the leader.
    #[must_use]
    pub fn requires_whitespace_after(self) -> bool {
        !matches!(self, Self::Lt | Self::Gt)
    }

    /// Map a character to a leader kind, if it is a statement leader glyph.
    #[must_use]
    pub fn from_char(c: char) -> Option<Self> {
        Some(match c {
            '~' => Self::Tilde,
            '$' => Self::Dollar,
            '#' => Self::Hash,
            '%' => Self::Percent,
            '@' => Self::At,
            '?' => Self::Question,
            ':' => Self::Colon,
            '!' => Self::Bang,
            '^' => Self::Caret,
            '*' => Self::Star,
            '<' => Self::Lt,
            '>' => Self::Gt,
            '|' => Self::Pipe,
            '+' => Self::Plus,
            '-' => Self::Minus,
            '&' => Self::Ampersand,
            '/' => Self::Slash,
            '\\' => Self::Backslash,
            _ => return None,
        })
    }

    /// Stable token name (matches `echo_lexer` leader dumps: `leader_tilde`, …).
    #[must_use]
    pub fn token_name(self) -> &'static str {
        match self {
            Self::Tilde => "leader_tilde",
            Self::Dollar => "leader_dollar",
            Self::Hash => "leader_hash",
            Self::Percent => "leader_percent",
            Self::At => "leader_at",
            Self::Question => "leader_question",
            Self::Colon => "leader_colon",
            Self::Bang => "leader_bang",
            Self::Caret => "leader_caret",
            Self::Star => "leader_star",
            Self::Lt => "leader_lt",
            Self::Gt => "leader_gt",
            Self::Pipe => "leader_pipe",
            Self::Plus => "leader_plus",
            Self::Minus => "leader_minus",
            Self::Ampersand => "leader_ampersand",
            Self::Slash => "leader_slash",
            Self::Backslash => "leader_backslash",
        }
    }

    /// Dual-use: valid as an expression/operator token outside leader position.
    ///
    /// Leader-only glyphs (`$ # @ ? \`) error outside statement start
    /// (`lex-unexpected-leader-glyph`). Dual-use glyphs are scanned as
    /// operators/punctuation in expression position (see `docs/lexer.md`).
    ///
    /// `~` (bit-not) and `^` (bit-xor) are dual-use with bind/return leaders.
    #[must_use]
    pub fn is_dual_use(self) -> bool {
        !matches!(
            self,
            Self::Dollar | Self::Hash | Self::At | Self::Question | Self::Backslash
        )
    }
}

/// Coarse grouping of statement leaders (one module per family).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeaderFamily {
    Bind,
    Shape,
    Control,
    Module,
}

impl LeaderFamily {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Bind => "bind",
            Self::Shape => "shape",
            Self::Control => "control",
            Self::Module => "module",
        }
    }

    /// Leaders in this family (syntax.md order within the family).
    #[must_use]
    pub fn leaders(self) -> &'static [LeaderKind] {
        match self {
            Self::Bind => bind::LEADERS,
            Self::Shape => shape::LEADERS,
            Self::Control => control::LEADERS,
            Self::Module => module::LEADERS,
        }
    }
}

/// All statement leaders in declaration order (syntax.md table).
pub const LEADERS: &[LeaderKind] = &[
    // bind
    LeaderKind::Tilde,
    LeaderKind::Dollar,
    LeaderKind::Hash,
    // shape
    LeaderKind::Percent,
    LeaderKind::At,
    // control
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
    // module
    LeaderKind::Slash,
    LeaderKind::Backslash,
];

/// True if `c` can introduce a statement when in leader position.
#[must_use]
pub fn is_leader_char(c: char) -> bool {
    LeaderKind::from_char(c).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eighteen_leaders() {
        assert_eq!(LEADERS.len(), 18);
        for kind in LEADERS {
            assert_eq!(LeaderKind::from_char(kind.glyph()), Some(*kind));
        }
    }

    #[test]
    fn families_partition_all_leaders() {
        let mut from_families = Vec::new();
        for fam in [
            LeaderFamily::Bind,
            LeaderFamily::Shape,
            LeaderFamily::Control,
            LeaderFamily::Module,
        ] {
            from_families.extend_from_slice(fam.leaders());
        }
        assert_eq!(from_families.as_slice(), LEADERS);
    }

    #[test]
    fn bare_break_continue_no_ws_required() {
        assert!(!LeaderKind::Lt.requires_whitespace_after());
        assert!(!LeaderKind::Gt.requires_whitespace_after());
        assert!(LeaderKind::Dollar.requires_whitespace_after());
        assert!(LeaderKind::Star.requires_whitespace_after());
    }

    #[test]
    fn family_of_each_leader() {
        assert_eq!(LeaderKind::Dollar.family(), LeaderFamily::Bind);
        assert_eq!(LeaderKind::Percent.family(), LeaderFamily::Shape);
        assert_eq!(LeaderKind::Star.family(), LeaderFamily::Control);
        assert_eq!(LeaderKind::Slash.family(), LeaderFamily::Module);
    }

    #[test]
    fn dual_use_and_token_names() {
        assert!(!LeaderKind::Dollar.is_dual_use());
        assert!(LeaderKind::Tilde.is_dual_use());
        assert!(LeaderKind::Caret.is_dual_use());
        assert!(LeaderKind::Star.is_dual_use());
        assert!(LeaderKind::Percent.is_dual_use());
        assert_eq!(LeaderKind::Dollar.token_name(), "leader_dollar");
        assert_eq!(LeaderKind::Backslash.token_name(), "leader_backslash");
        assert!(LeaderKind::Ampersand.is_dual_use());
        assert_eq!(LEADERS.iter().filter(|k| k.is_dual_use()).count(), 13);
        assert_eq!(LEADERS.iter().filter(|k| !k.is_dual_use()).count(), 5);
    }
}
