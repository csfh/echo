//! Control-flow leaders: `?` `:` `!` `^` `*` `<` `>` `|` `+` `-` `&`.

use super::LeaderKind;

/// Control-family leaders in syntax.md order.
pub const LEADERS: &[LeaderKind] = &[
    LeaderKind::Question, // if
    LeaderKind::Colon,    // else-if / else / match default
    LeaderKind::Bang,     // error return (result err)
    LeaderKind::Caret,    // return
    LeaderKind::Star,     // loop
    LeaderKind::Lt,       // break
    LeaderKind::Gt,       // continue
    LeaderKind::Pipe,     // match
    LeaderKind::Plus,     // task spawn
    LeaderKind::Minus,    // task join / immediate block
    LeaderKind::Ampersand, // effect block (auto-unwrap result/option)
];
