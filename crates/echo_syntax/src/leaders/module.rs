//! Module leaders: `/` `\`.

use super::LeaderKind;

/// Module-family leaders in syntax.md order.
pub const LEADERS: &[LeaderKind] = &[
    LeaderKind::Slash,     // import
    LeaderKind::Backslash, // export
];
