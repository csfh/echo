//! Bind leaders: `~` `$` `#`.

use super::LeaderKind;

/// Bind-family leaders in syntax.md order.
pub const LEADERS: &[LeaderKind] = &[
    LeaderKind::Tilde,  // mutable bind / reassign
    LeaderKind::Dollar, // immutable runtime bind
    LeaderKind::Hash,   // compile-time constant
];
