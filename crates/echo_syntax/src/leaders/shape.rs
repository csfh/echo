//! Shape leaders: `%` `@`.

use super::LeaderKind;

/// Shape-family leaders in syntax.md order.
pub const LEADERS: &[LeaderKind] = &[
    LeaderKind::Percent, // struct shape
    LeaderKind::At,      // additional struct members
];
