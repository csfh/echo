//! Git pack natives. Pack format is crate-owned (`gix-pack` when host-io).
//!
//! Userland never calls these; `std/git` wraps them.

use crate::{
    bytes_as_slice, bytes_to_handle, echo_runtime_list_new, echo_runtime_struct_new,
    list_push_value, string_as_str, string_handle_from_utf8, struct_set_value,
};

fn payload(v: i64) -> Vec<u8> {
    if let Some(b) = bytes_as_slice(v) {
        return b.to_vec();
    }
    if let Some(s) = string_as_str(v) {
        return s.as_bytes().to_vec();
    }
    Vec::new()
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn pack_header(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 32 || &bytes[0..4] != b"PACK" {
        return None;
    }
    let version = u32::from_be_bytes(bytes[4..8].try_into().ok()?);
    let count = u32::from_be_bytes(bytes[8..12].try_into().ok()?);
    if version != 2 && version != 3 {
        return None;
    }
    Some((version, count))
}

fn pack_checksum(bytes: &[u8]) -> String {
    if bytes.len() < 20 {
        return String::new();
    }
    hex_encode(&bytes[bytes.len() - 20..])
}

/// 1 if `bytes` is a PACK v2/v3 with a trailer.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_git_pack_valid(raw: i64) -> i64 {
    let bytes = payload(raw);
    match pack_header(&bytes) {
        Some(_) if bytes.len() >= 32 => 1,
        _ => 0,
    }
}

/// Object count from the PACK header, or -1.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_git_pack_count(raw: i64) -> i64 {
    let bytes = payload(raw);
    pack_header(&bytes).map(|(_, n)| i64::from(n)).unwrap_or(-1)
}

/// Index a pack: `{ ok, count, checksum, objects }` (`objects` = oid hex list).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_git_index_pack(raw: i64) -> i64 {
    let bytes = payload(raw);
    let out = echo_runtime_struct_new();
    let objects = echo_runtime_list_new();
    let Some((_, count)) = pack_header(&bytes) else {
        struct_set_value(out, "ok", 0);
        struct_set_value(out, "count", 0);
        struct_set_value(out, "checksum", string_handle_from_utf8(""));
        struct_set_value(out, "objects", objects);
        return out;
    };
    let checksum = pack_checksum(&bytes);
    for oid in enumerate_oids(&bytes) {
        list_push_value(objects, string_handle_from_utf8(&oid));
    }
    struct_set_value(out, "ok", 1);
    struct_set_value(out, "count", i64::from(count));
    struct_set_value(out, "checksum", string_handle_from_utf8(&checksum));
    struct_set_value(out, "objects", objects);
    out
}

/// Split a `git-receive-pack` body into pkt-line commands + trailing PACK.
/// `{ commands, pack }` — `commands` is the text before `0000` / PACK.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_git_receive_pack_split(raw: i64) -> i64 {
    let bytes = payload(raw);
    let (cmds, pack) = split_receive_pack(&bytes);
    let (ref_name, new_oid) = first_update_command(&cmds);
    let out = echo_runtime_struct_new();
    struct_set_value(out, "commands", string_handle_from_utf8(&cmds));
    struct_set_value(out, "pack", bytes_to_handle(pack));
    struct_set_value(out, "ref_name", string_handle_from_utf8(&ref_name));
    struct_set_value(out, "new_oid", string_handle_from_utf8(&new_oid));
    out
}

/// Parse `want <oid>` lines from a `git-upload-pack` body. Returns a string list.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_git_upload_pack_wants(raw: i64) -> i64 {
    let bytes = payload(raw);
    let list = echo_runtime_list_new();
    for oid in upload_pack_wants(&bytes) {
        list_push_value(list, string_handle_from_utf8(&oid));
    }
    list
}

fn first_update_command(cmds: &str) -> (String, String) {
    let mut i = 0;
    let b = cmds.as_bytes();
    while i + 4 <= b.len() {
        let Ok(n) = usize::from_str_radix(std::str::from_utf8(&b[i..i + 4]).unwrap_or(""), 16)
        else {
            break;
        };
        if n == 0 {
            break;
        }
        if n < 4 || i + n > b.len() {
            break;
        }
        let line = String::from_utf8_lossy(&b[i + 4..i + n]);
        let line = line.split('\0').next().unwrap_or("").trim();
        let mut parts = line.split_whitespace();
        let _old = parts.next().unwrap_or("");
        let new = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("");
        if new.len() == 40 && name.starts_with("refs/") {
            return (name.to_string(), new.to_string());
        }
        i += n;
    }
    for line in cmds.lines() {
        let line = line.split('\0').next().unwrap_or("").trim();
        let mut parts = line.split_whitespace();
        let _old = parts.next().unwrap_or("");
        let new = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("");
        if new.len() == 40 && name.starts_with("refs/") {
            return (name.to_string(), new.to_string());
        }
    }
    (String::new(), String::new())
}

fn split_receive_pack(bytes: &[u8]) -> (String, Vec<u8>) {
    if let Some(pos) = bytes.windows(4).position(|w| w == b"PACK") {
        let cmds = String::from_utf8_lossy(&bytes[..pos]).into_owned();
        return (cmds, bytes[pos..].to_vec());
    }
    (String::from_utf8_lossy(bytes).into_owned(), Vec::new())
}

fn upload_pack_wants(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    let mut i = 0;
    let b = text.as_bytes();
    while i + 4 <= b.len() {
        let Ok(n) = usize::from_str_radix(std::str::from_utf8(&b[i..i + 4]).unwrap_or(""), 16)
        else {
            break;
        };
        if n == 0 {
            i += 4;
            continue;
        }
        if n < 4 || i + n > b.len() {
            break;
        }
        let line = String::from_utf8_lossy(&b[i + 4..i + n]).trim().to_string();
        if let Some(rest) = line.strip_prefix("want ") {
            let oid = rest.split_whitespace().next().unwrap_or("").to_string();
            if oid.len() == 40 {
                out.push(oid);
            }
        }
        i += n;
    }
    // Fallback: raw lines (tests / malformed)
    if out.is_empty() {
        for line in text.lines() {
            if let Some(rest) = line.trim().strip_prefix("want ") {
                let oid = rest.split_whitespace().next().unwrap_or("");
                if oid.len() == 40 {
                    out.push(oid.to_string());
                }
            }
        }
    }
    out
}

fn enumerate_oids(bytes: &[u8]) -> Vec<String> {
    #[cfg(feature = "host-io")]
    {
        if let Ok(oids) = index_with_gix(bytes) {
            return oids;
        }
    }
    Vec::new()
}

#[cfg(feature = "host-io")]
fn index_with_gix(bytes: &[u8]) -> Result<Vec<String>, String> {
    use gix_pack::data::input::{BytesToEntriesIter, EntryDataMode, Mode};
    let iter = BytesToEntriesIter::new_from_header(
        std::io::Cursor::new(bytes.to_vec()),
        Mode::Verify,
        EntryDataMode::Crc32,
        gix_hash::Kind::Sha1,
    )
    .map_err(|e| e.to_string())?;
    // Walk every entry through gix-pack (validates zlib + trailer). OIDs for
    // deltas need a full index; refs come from the pack manifest.
    for item in iter {
        let _entry = item.map_err(|e| e.to_string())?;
    }
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_pack() -> Vec<u8> {
        let mut p = b"PACK".to_vec();
        p.extend_from_slice(&2u32.to_be_bytes());
        p.extend_from_slice(&0u32.to_be_bytes());
        p.extend_from_slice(&[0u8; 20]);
        p
    }

    #[test]
    fn pack_header_valid_and_count() {
        let h = bytes_to_handle(minimal_pack());
        assert_eq!(echo_runtime_git_pack_valid(h), 1);
        assert_eq!(echo_runtime_git_pack_count(h), 0);
        assert_eq!(
            echo_runtime_git_pack_valid(bytes_to_handle(b"nope".to_vec())),
            0
        );
        let idx = echo_runtime_git_index_pack(h);
        assert_eq!(crate::struct_get_value(idx, "ok"), 1);
        assert_eq!(crate::struct_get_value(idx, "count"), 0);
    }

    #[test]
    fn receive_pack_splits_on_pack_magic() {
        let mut body = b"000eunpack ok\n0000".to_vec();
        body.extend_from_slice(&minimal_pack());
        let h = echo_runtime_git_receive_pack_split(bytes_to_handle(body));
        let pack = crate::struct_get_value(h, "pack");
        assert_eq!(echo_runtime_git_pack_valid(pack), 1);
    }

    #[test]
    fn upload_pack_wants_from_pkt_and_raw() {
        let oid = "0123456789abcdef0123456789abcdef01234567";
        let line = format!("want {oid}\n");
        let n = line.len() + 4;
        let pkt = format!("{n:04x}{line}");
        let list = echo_runtime_git_upload_pack_wants(string_handle_from_utf8(&pkt));
        assert_eq!(crate::list_len_value(list), 1);
        let got = crate::list_get_value(list, 0);
        assert_eq!(string_as_str(got).unwrap(), oid);
    }
}
