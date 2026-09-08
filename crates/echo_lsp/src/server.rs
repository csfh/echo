//! LSP over stdio (JSON-RPC 2.0, Content-Length framing).
//!
//! Protocol meaning lives in [`crate::session::LspSession`]; this module only
//! reads/writes the wire format.

use std::io::{self, BufRead, Write};

use serde_json::Value;

use crate::session::{outgoing_to_json, LspSession};

/// Run the language server on stdin/stdout until `exit` (or stdin EOF).
pub fn run_stdio() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let mut session = LspSession::new();

    loop {
        let Some(msg) = read_message(&mut stdin)? else {
            break;
        };
        let method = msg.get("method").and_then(|m| m.as_str());
        let outgoing = session.handle(&msg);
        for m in &outgoing {
            write_message(&mut stdout, &outgoing_to_json(m))?;
        }
        if method == Some("exit") {
            break;
        }
    }
    Ok(())
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().ok();
        }
        // Ignore other headers (Content-Type, etc.).
    }
    let Some(len) = content_length else {
        // Missing header: protocol error — surface clearly; session stays usable
        // only when framing is well-formed.
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing Content-Length",
        ));
    };
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    match serde_json::from_slice::<Value>(&buf) {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            // Soft-fail parse: return a synthetic invalid message the session
            // will ignore (no method). Keep the process alive for the client.
            let _ = e;
            Ok(Some(serde_json::json!({
                "jsonrpc": "2.0",
                "method": "$/invalidJson",
            })))
        }
    }
}

fn write_message(out: &mut impl Write, value: &Value) -> io::Result<()> {
    let body =
        serde_json::to_vec(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write!(out, "Content-Length: {}\r\n\r\n", body.len())?;
    out.write_all(&body)?;
    out.flush()?;
    Ok(())
}

/// Write a single outbound message (test helper surface).
#[cfg(test)]
pub fn write_outgoing(out: &mut impl Write, msg: &crate::session::Outgoing) -> io::Result<()> {
    write_message(out, &outgoing_to_json(msg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Outgoing;
    use serde_json::json;

    #[test]
    fn framing_roundtrip() {
        let msg = Outgoing::Response {
            id: json!(1),
            result: json!({"ok": true}),
        };
        let mut buf = Vec::new();
        write_outgoing(&mut buf, &msg).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.starts_with("Content-Length:"));
        assert!(s.contains("\"ok\":true") || s.contains("\"ok\": true"));
    }

    #[test]
    fn read_message_parses_body() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"shutdown"}"#;
        let mut raw = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        raw.extend_from_slice(body);
        let mut cursor = std::io::Cursor::new(raw);
        let v = read_message(&mut cursor).unwrap().unwrap();
        assert_eq!(v["method"], "shutdown");
        assert_eq!(v["id"], 1);
    }
}
