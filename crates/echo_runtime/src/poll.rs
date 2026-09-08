//! Shared **mio** helpers for the runtime event loop (ADR 0013).
//!
//! The live loop owns `mio::Poll` in [`crate::sched`]. Interest/token helpers
//! are for future nonblocking socket registration on that registry.

use mio::{Interest, Token};

/// Interest flags for registering sockets with the loop registry.
#[must_use]
#[allow(dead_code)] // used once nonblocking net parks on the mio registry
pub fn interest(readable: bool, writable: bool) -> Interest {
    match (readable, writable) {
        (true, true) => Interest::READABLE | Interest::WRITABLE,
        (true, false) => Interest::READABLE,
        (false, true) => Interest::WRITABLE,
        (false, false) => Interest::READABLE,
    }
}

/// Build a mio token from a runtime id (non-zero; 0 is reserved for the waker).
#[must_use]
#[allow(dead_code)] // used once nonblocking net parks on the mio registry
pub fn token(id: usize) -> Token {
    debug_assert_ne!(id, 0, "token 0 is the event-loop waker");
    Token(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interest_readable() {
        assert!(interest(true, false).is_readable());
        assert!(!interest(true, false).is_writable());
    }

    #[test]
    fn token_nonzero() {
        assert_eq!(token(1), Token(1));
    }
}

