//! S3-compatible object store handles (memory + optional remote).
//!
//! Userland never calls these; `std/store` wraps them.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::{
    bytes_as_slice, bytes_to_handle, echo_runtime_list_new, echo_runtime_struct_new,
    heap_to_handle, is_live_heap, list_push_value, note_heap_free, string_as_str,
    string_handle_from_utf8, struct_set_value,
};

struct Object {
    bytes: Vec<u8>,
    etag: String,
}

enum Backend {
    Memory {
        map: HashMap<String, Object>,
    },
    #[cfg(feature = "host-io")]
    S3(S3Cfg),
}

struct Store {
    backend: Backend,
}

#[cfg(feature = "host-io")]
#[derive(Clone)]
struct S3Cfg {
    endpoint: String,
    region: String,
    bucket: String,
    access: String,
    secret: String,
}

static STORES: OnceLock<Mutex<HashMap<i64, Store>>> = OnceLock::new();

fn stores() -> std::sync::MutexGuard<'static, HashMap<i64, Store>> {
    STORES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn key_text(key: i64) -> String {
    if let Some(s) = string_as_str(key) {
        return s.to_string();
    }
    if let Some(b) = bytes_as_slice(key) {
        return String::from_utf8_lossy(b).into_owned();
    }
    String::new()
}

fn payload(v: i64) -> Vec<u8> {
    if let Some(b) = bytes_as_slice(v) {
        return b.to_vec();
    }
    if let Some(s) = string_as_str(v) {
        return s.as_bytes().to_vec();
    }
    Vec::new()
}

fn etag_of(bytes: &[u8]) -> String {
    #[cfg(feature = "host-io")]
    {
        use sha2::{Digest, Sha256};
        let d = Sha256::digest(bytes);
        return hex_encode(&d);
    }
    #[cfg(not(feature = "host-io"))]
    {
        hex_encode(&(bytes.len() as u64).to_le_bytes())
    }
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

fn reply(status: i64, body: i64, etag: &str) -> i64 {
    let h = echo_runtime_struct_new();
    struct_set_value(h, "status", status);
    struct_set_value(h, "body", body);
    struct_set_value(h, "etag", string_handle_from_utf8(etag));
    h
}

fn empty_body() -> i64 {
    bytes_to_handle(Vec::new())
}

fn insert_store(store: Store) -> i64 {
    let handle = heap_to_handle(Box::new(1u8));
    stores().insert(handle, store);
    handle
}

/// `echo_runtime_store_memory_new() -> handle`
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_store_memory_new() -> i64 {
    insert_store(Store {
        backend: Backend::Memory {
            map: HashMap::new(),
        },
    })
}

/// `echo_runtime_store_s3_new() -> handle` — env: `ECHO_S3_ENDPOINT`,
/// `ECHO_S3_REGION`, `ECHO_S3_BUCKET`, `ECHO_S3_ACCESS_KEY`, `ECHO_S3_SECRET_KEY`.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_store_s3_new() -> i64 {
    #[cfg(feature = "host-io")]
    {
        let endpoint = std::env::var("ECHO_S3_ENDPOINT").unwrap_or_default();
        let bucket = std::env::var("ECHO_S3_BUCKET").unwrap_or_default();
        if endpoint.is_empty() || bucket.is_empty() {
            return 0;
        }
        return insert_store(Store {
            backend: Backend::S3(S3Cfg {
                endpoint,
                region: std::env::var("ECHO_S3_REGION").unwrap_or_else(|_| "auto".into()),
                bucket,
                access: std::env::var("ECHO_S3_ACCESS_KEY").unwrap_or_default(),
                secret: std::env::var("ECHO_S3_SECRET_KEY").unwrap_or_default(),
            }),
        });
    }
    #[cfg(not(feature = "host-io"))]
    {
        0
    }
}

/// Drop a store handle (optional; process exit also drops).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_store_close(handle: i64) -> i64 {
    if handle == 0 {
        return 0;
    }
    stores().remove(&handle);
    if is_live_heap(handle) {
        note_heap_free(handle);
        let _ = unsafe { Box::from_raw(handle as *mut u8) };
    }
    0
}

/// GET object. `{ status, body, etag }` — 200 or 404.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_store_get(handle: i64, key: i64) -> i64 {
    let k = key_text(key);
    match with_store(handle, |s| match &s.backend {
        Backend::Memory { map } => map.get(&k).map(|o| (o.bytes.clone(), o.etag.clone())),
        #[cfg(feature = "host-io")]
        Backend::S3(cfg) => s3_get(cfg, &k, None, None),
    }) {
        Some(Some((bytes, etag))) => reply(200, bytes_to_handle(bytes), &etag),
        Some(None) => reply(404, empty_body(), ""),
        None => reply(0, empty_body(), ""),
    }
}

/// Conditional GET (`If-None-Match`). 200 / 304 / 404.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_store_get_if_none_match(handle: i64, key: i64, etag: i64) -> i64 {
    let k = key_text(key);
    let want = string_as_str(etag).unwrap_or("").to_string();
    match with_store(handle, |s| match &s.backend {
        Backend::Memory { map } => map.get(&k).map(|o| (o.bytes.clone(), o.etag.clone())),
        #[cfg(feature = "host-io")]
        Backend::S3(cfg) => s3_get(cfg, &k, Some(want.as_str()), None),
    }) {
        Some(Some((_bytes, got))) if !want.is_empty() && got == want => {
            reply(304, empty_body(), &got)
        }
        Some(Some((bytes, got))) => reply(200, bytes_to_handle(bytes), &got),
        Some(None) => reply(404, empty_body(), ""),
        None => reply(0, empty_body(), ""),
    }
}

/// Range GET. `start` inclusive, `end` exclusive. 200 / 404.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_store_get_range(handle: i64, key: i64, start: i64, end: i64) -> i64 {
    let k = key_text(key);
    let start = start.max(0) as usize;
    let end = end.max(0) as usize;
    match with_store(handle, |s| match &s.backend {
        Backend::Memory { map } => map.get(&k).map(|o| {
            let e = end.min(o.bytes.len()).max(start);
            let s0 = start.min(o.bytes.len());
            (o.bytes[s0..e].to_vec(), o.etag.clone())
        }),
        #[cfg(feature = "host-io")]
        Backend::S3(cfg) => s3_get(cfg, &k, None, Some((start, end))),
    }) {
        Some(Some((bytes, etag))) => reply(200, bytes_to_handle(bytes), &etag),
        Some(None) => reply(404, empty_body(), ""),
        None => reply(0, empty_body(), ""),
    }
}

/// HEAD. `{ status, body, etag }` — 200 or 404 (empty body).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_store_head(handle: i64, key: i64) -> i64 {
    let k = key_text(key);
    match with_store(handle, |s| match &s.backend {
        Backend::Memory { map } => map.get(&k).map(|o| o.etag.clone()),
        #[cfg(feature = "host-io")]
        Backend::S3(cfg) => s3_head(cfg, &k),
    }) {
        Some(Some(etag)) => reply(200, empty_body(), &etag),
        Some(None) => reply(404, empty_body(), ""),
        None => reply(0, empty_body(), ""),
    }
}

/// Unconditional PUT. `{ status, etag }` (body empty).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_store_put(handle: i64, key: i64, data: i64) -> i64 {
    let k = key_text(key);
    let bytes = payload(data);
    let etag = etag_of(&bytes);
    let ok = with_store_mut(handle, |s| match &mut s.backend {
        Backend::Memory { map } => {
            map.insert(
                k.clone(),
                Object {
                    bytes: bytes.clone(),
                    etag: etag.clone(),
                },
            );
            true
        }
        #[cfg(feature = "host-io")]
        Backend::S3(cfg) => s3_put(cfg, &k, &bytes, None, None).is_ok(),
    });
    match ok {
        Some(true) => reply(200, empty_body(), &etag),
        Some(false) => reply(500, empty_body(), ""),
        None => reply(0, empty_body(), ""),
    }
}

/// CAS put (`If-Match: etag`). 200 or 412.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_store_cas(handle: i64, key: i64, etag: i64, data: i64) -> i64 {
    let k = key_text(key);
    let expect = string_as_str(etag).unwrap_or("").to_string();
    let bytes = payload(data);
    let new_etag = etag_of(&bytes);
    let status = with_store_mut(handle, |s| match &mut s.backend {
        Backend::Memory { map } => match map.get(&k) {
            Some(o) if o.etag == expect => {
                map.insert(
                    k.clone(),
                    Object {
                        bytes: bytes.clone(),
                        etag: new_etag.clone(),
                    },
                );
                200
            }
            Some(_) | None => 412,
        },
        #[cfg(feature = "host-io")]
        Backend::S3(cfg) => match s3_put(cfg, &k, &bytes, Some(expect.as_str()), None) {
            Ok(()) => 200,
            Err(412) => 412,
            Err(_) => 500,
        },
    });
    match status {
        Some(200) => reply(200, empty_body(), &new_etag),
        Some(code) => reply(code, empty_body(), ""),
        None => reply(0, empty_body(), ""),
    }
}

/// Create-only put (`If-None-Match: *`). 200 or 412.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_store_cas_create(handle: i64, key: i64, data: i64) -> i64 {
    let k = key_text(key);
    let bytes = payload(data);
    let new_etag = etag_of(&bytes);
    let status = with_store_mut(handle, |s| match &mut s.backend {
        Backend::Memory { map } => {
            if map.contains_key(&k) {
                412
            } else {
                map.insert(
                    k.clone(),
                    Object {
                        bytes: bytes.clone(),
                        etag: new_etag.clone(),
                    },
                );
                200
            }
        }
        #[cfg(feature = "host-io")]
        Backend::S3(cfg) => match s3_put(cfg, &k, &bytes, None, Some("*")) {
            Ok(()) => 200,
            Err(412) => 412,
            Err(_) => 500,
        },
    });
    match status {
        Some(200) => reply(200, empty_body(), &new_etag),
        Some(code) => reply(code, empty_body(), ""),
        None => reply(0, empty_body(), ""),
    }
}

fn with_store<T>(handle: i64, f: impl FnOnce(&Store) -> T) -> Option<T> {
    let g = stores();
    g.get(&handle).map(f)
}

fn with_store_mut<T>(handle: i64, f: impl FnOnce(&mut Store) -> T) -> Option<T> {
    let mut g = stores();
    g.get_mut(&handle).map(f)
}

#[cfg(feature = "host-io")]
fn s3_get(
    cfg: &S3Cfg,
    key: &str,
    if_none_match: Option<&str>,
    range: Option<(usize, usize)>,
) -> Option<(Vec<u8>, String)> {
    let mut extra = Vec::new();
    if let Some(tag) = if_none_match {
        extra.push(("If-None-Match".into(), format!("\"{tag}\"")));
    }
    if let Some((s, e)) = range {
        if e > s {
            extra.push((
                "Range".into(),
                format!("bytes={}-{}", s, e.saturating_sub(1)),
            ));
        }
    }
    match s3_request(cfg, "GET", key, &[], &extra) {
        Ok((200 | 206, headers, body)) => {
            let etag = header_etag(&headers);
            Some((body, etag))
        }
        Ok((304, headers, _)) => {
            let etag = header_etag(&headers);
            Some((Vec::new(), etag))
        }
        _ => None,
    }
}

#[cfg(feature = "host-io")]
fn s3_head(cfg: &S3Cfg, key: &str) -> Option<String> {
    match s3_request(cfg, "HEAD", key, &[], &[]) {
        Ok((200, headers, _)) => Some(header_etag(&headers)),
        _ => None,
    }
}

#[cfg(feature = "host-io")]
fn s3_put(
    cfg: &S3Cfg,
    key: &str,
    body: &[u8],
    if_match: Option<&str>,
    if_none_match: Option<&str>,
) -> Result<(), i64> {
    let mut extra = Vec::new();
    if let Some(tag) = if_match {
        extra.push(("If-Match".into(), format!("\"{tag}\"")));
    }
    if let Some(tag) = if_none_match {
        extra.push(("If-None-Match".into(), tag.to_string()));
    }
    match s3_request(cfg, "PUT", key, body, &extra) {
        Ok((200 | 201 | 204, _, _)) => Ok(()),
        Ok((412, _, _)) => Err(412),
        Ok((code, _, _)) => Err(code),
        Err(_) => Err(500),
    }
}

#[cfg(feature = "host-io")]
fn header_etag(headers: &str) -> String {
    for line in headers.lines() {
        let Some((n, v)) = line.split_once(':') else {
            continue;
        };
        if n.eq_ignore_ascii_case("etag") {
            return v.trim().trim_matches('"').to_string();
        }
    }
    String::new()
}

/// Minimal AWS SigV4 S3 GET/PUT/HEAD over HTTPS (R2 = custom endpoint).
#[cfg(feature = "host-io")]
fn s3_request(
    cfg: &S3Cfg,
    method: &str,
    key: &str,
    body: &[u8],
    extra: &[(String, String)],
) -> Result<(i64, String, Vec<u8>), String> {
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};

    type HmacSha256 = Hmac<Sha256>;

    let endpoint = cfg.endpoint.trim_end_matches('/');
    let host = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"))
        .unwrap_or(endpoint);
    let path = format!("/{}/{}", cfg.bucket, key.trim_start_matches('/'));
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let payload_hash = hex_encode(&Sha256::digest(body));
    let mut signed_headers = vec![
        ("host".to_string(), host.to_string()),
        ("x-amz-content-sha256".to_string(), payload_hash.clone()),
        ("x-amz-date".to_string(), amz_date.clone()),
    ];
    for (k, v) in extra {
        signed_headers.push((k.to_ascii_lowercase(), v.clone()));
    }
    signed_headers.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_headers: String = signed_headers
        .iter()
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect();
    let signed_names: String = signed_headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical =
        format!("{method}\n{path}\n\n{canonical_headers}\n{signed_names}\n{payload_hash}");
    let canonical_hash = hex_encode(&Sha256::digest(canonical.as_bytes()));
    let scope = format!("{date_stamp}/{}/s3/aws4_request", cfg.region);
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{canonical_hash}");
    let mut k_date = HmacSha256::new_from_slice(format!("AWS4{}", cfg.secret).as_bytes())
        .map_err(|e| e.to_string())?;
    k_date.update(date_stamp.as_bytes());
    let mut k_region =
        HmacSha256::new_from_slice(&k_date.finalize().into_bytes()).map_err(|e| e.to_string())?;
    k_region.update(cfg.region.as_bytes());
    let mut k_service =
        HmacSha256::new_from_slice(&k_region.finalize().into_bytes()).map_err(|e| e.to_string())?;
    k_service.update(b"s3");
    let mut k_signing = HmacSha256::new_from_slice(&k_service.finalize().into_bytes())
        .map_err(|e| e.to_string())?;
    k_signing.update(b"aws4_request");
    let mut sig = HmacSha256::new_from_slice(&k_signing.finalize().into_bytes())
        .map_err(|e| e.to_string())?;
    sig.update(string_to_sign.as_bytes());
    let signature = hex_encode(&sig.finalize().into_bytes());
    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_names}, Signature={signature}",
        cfg.access
    );

    let use_tls = endpoint.starts_with("https://") || !endpoint.starts_with("http://");
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: {auth}\r\nX-Amz-Date: {amz_date}\r\nX-Amz-Content-Sha256: {payload_hash}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in extra {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    let mut wire = req.into_bytes();
    wire.extend_from_slice(body);
    http1(host, use_tls, &wire)
}

#[cfg(feature = "host-io")]
fn http1(host: &str, use_tls: bool, req: &[u8]) -> Result<(i64, String, Vec<u8>), String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let addr = if host.contains(':') && !host.starts_with('[') {
        host.to_string()
    } else if use_tls {
        format!("{host}:443")
    } else {
        format!("{host}:80")
    };
    let stream = TcpStream::connect(&addr).map_err(|e| format!("connect {addr}: {e}"))?;
    let mut buf = Vec::new();
    if use_tls {
        static CRYPTO: std::sync::Once = std::sync::Once::new();
        CRYPTO.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let cfg = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from(host.split(':').next().unwrap_or(host))
            .map_err(|e| e.to_string())?
            .to_owned();
        let mut conn = rustls::ClientConnection::new(std::sync::Arc::new(cfg), name)
            .map_err(|e| e.to_string())?;
        let mut stream = stream;
        let mut tls = rustls::Stream::new(&mut conn, &mut stream);
        tls.write_all(req).map_err(|e| e.to_string())?;
        tls.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    } else {
        let mut s = stream;
        s.write_all(req).map_err(|e| e.to_string())?;
        s.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    }
    split_http_response(&buf)
}

#[cfg(feature = "host-io")]
fn split_http_response(buf: &[u8]) -> Result<(i64, String, Vec<u8>), String> {
    let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Err("no HTTP header terminator".into());
    };
    let headers = String::from_utf8_lossy(&buf[..pos]).into_owned();
    let status = headers
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok((status, headers, buf[pos + 4..].to_vec()))
}

/// Debug: list keys in a memory store (empty list for S3).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_store_keys(handle: i64) -> i64 {
    let list = echo_runtime_list_new();
    if let Some(keys) = with_store(handle, |s| match &s.backend {
        Backend::Memory { map } => map.keys().cloned().collect::<Vec<_>>(),
        #[cfg(feature = "host-io")]
        Backend::S3(_) => Vec::new(),
    }) {
        for k in keys {
            list_push_value(list, string_handle_from_utf8(&k));
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_get_put_cas() {
        let s = echo_runtime_store_memory_new();
        let key = string_handle_from_utf8("repos/a/b/manifest");
        let miss = echo_runtime_store_get(s, key);
        assert_eq!(crate::struct_get_value(miss, "status"), 404);

        let body = bytes_to_handle(b"v1".to_vec());
        let put = echo_runtime_store_put(s, key, body);
        assert_eq!(crate::struct_get_value(put, "status"), 200);
        let etag_h = crate::struct_get_value(put, "etag");
        let etag = string_as_str(etag_h).unwrap().to_string();
        assert!(!etag.is_empty());

        let got = echo_runtime_store_get(s, key);
        assert_eq!(crate::struct_get_value(got, "status"), 200);
        let raw = crate::struct_get_value(got, "body");
        assert_eq!(bytes_as_slice(raw).unwrap(), b"v1");

        let cond = echo_runtime_store_get_if_none_match(s, key, etag_h);
        assert_eq!(crate::struct_get_value(cond, "status"), 304);

        let create = echo_runtime_store_cas_create(s, key, body);
        assert_eq!(crate::struct_get_value(create, "status"), 412);

        let bad = string_handle_from_utf8("nope");
        let clash = echo_runtime_store_cas(s, key, bad, bytes_to_handle(b"v2".to_vec()));
        assert_eq!(crate::struct_get_value(clash, "status"), 412);

        let ok = echo_runtime_store_cas(s, key, etag_h, bytes_to_handle(b"v2".to_vec()));
        assert_eq!(crate::struct_get_value(ok, "status"), 200);
        echo_runtime_store_close(s);
    }

    #[test]
    fn memory_range() {
        let s = echo_runtime_store_memory_new();
        let key = string_handle_from_utf8("p");
        echo_runtime_store_put(s, key, bytes_to_handle(b"abcdef".to_vec()));
        let r = echo_runtime_store_get_range(s, key, 1, 4);
        assert_eq!(crate::struct_get_value(r, "status"), 200);
        let raw = crate::struct_get_value(r, "body");
        assert_eq!(bytes_as_slice(raw).unwrap(), b"bcd");
        echo_runtime_store_close(s);
    }
}
