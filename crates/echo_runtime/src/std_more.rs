//! P0/P1/P2 std natives: parse, time, compress, crypto, chmod, unix sockets, etc.

use std::io::{Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

use crate::{
    bytes_data, bytes_to_handle, echo_runtime_float_from_f64, echo_runtime_struct_new, header_at,
    string_data, string_to_handle, struct_set_str, HEAP_MAGIC,
};

// --- URL parse (owned product; pure-Echo products free slice strings) ---

/// Always returns a product `{ ok, scheme, host, port, path }` (ok=0 fail).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_url_parse(s: i64) -> i64 {
    let fail = || {
        let st = echo_runtime_struct_new();
        struct_set_str(st, "ok", 0);
        struct_set_str(st, "scheme", string_to_handle(String::new()));
        struct_set_str(st, "host", string_to_handle(String::new()));
        struct_set_str(st, "port", 0);
        struct_set_str(st, "path", string_to_handle(String::new()));
        st
    };
    let Some(raw) = string_data(s) else {
        return fail();
    };
    let t = raw.trim();
    let (scheme, rest) = if let Some(r) = t.strip_prefix("http://") {
        ("http", r)
    } else if let Some(r) = t.strip_prefix("https://") {
        ("https", r)
    } else {
        return fail();
    };
    if rest.is_empty() {
        return fail();
    }
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if hostport.is_empty() {
        return fail();
    }
    let (host, port) = match hostport.rfind(':') {
        Some(i) => {
            let h = &hostport[..i];
            let p = hostport[i + 1..].parse::<i64>().unwrap_or(-1);
            if h.is_empty() || p < 0 {
                return fail();
            }
            (h, p)
        }
        None => (hostport, 0i64),
    };
    let st = echo_runtime_struct_new();
    struct_set_str(st, "ok", 1);
    struct_set_str(st, "scheme", string_to_handle(scheme.to_string()));
    struct_set_str(st, "host", string_to_handle(host.to_string()));
    struct_set_str(st, "port", port);
    struct_set_str(st, "path", string_to_handle(path.to_string()));
    st
}

// --- parse int/float ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_parse_i64(s: i64) -> i64 {
    let Some(t) = string_data(s) else {
        return pack_parse_i64(0, 0);
    };
    match t.trim().parse::<i64>() {
        Ok(v) => pack_parse_i64(1, v),
        Err(_) => pack_parse_i64(0, 0),
    }
}

fn pack_parse_i64(ok: i64, val: i64) -> i64 {
    let st = echo_runtime_struct_new();
    struct_set_str(st, "ok", ok);
    struct_set_str(st, "val", val);
    st
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_parse_f64(s: i64) -> i64 {
    let Some(t) = string_data(s) else {
        return pack_parse_f64(0, 0.0);
    };
    match t.trim().parse::<f64>() {
        Ok(v) => pack_parse_f64(1, v),
        Err(_) => pack_parse_f64(0, 0.0),
    }
}

fn pack_parse_f64(ok: i64, val: f64) -> i64 {
    let st = echo_runtime_struct_new();
    struct_set_str(st, "ok", ok);
    struct_set_str(st, "val", echo_runtime_float_from_f64(val));
    st
}

// --- time format/parse (chrono) ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_time_format(ms: i64, fmt: i64) -> i64 {
    let Some(f) = string_data(fmt) else {
        return 0;
    };
    let secs = ms.div_euclid(1000);
    let nsec = (ms.rem_euclid(1000) * 1_000_000) as u32;
    let Some(dt) = chrono::DateTime::from_timestamp(secs, nsec) else {
        return 0;
    };
    string_to_handle(dt.format(&f).to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_time_parse(s: i64, fmt: i64) -> i64 {
    let Some(text) = string_data(s) else {
        return pack_parse_i64(0, 0);
    };
    let Some(f) = string_data(fmt) else {
        return pack_parse_i64(0, 0);
    };
    let text = text.trim();
    // Prefer datetime; fall back to date-only (midnight UTC).
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(text, &f) {
        return pack_parse_i64(1, ndt.and_utc().timestamp_millis());
    }
    if let Ok(nd) = chrono::NaiveDate::parse_from_str(text, &f) {
        let ndt = nd.and_hms_opt(0, 0, 0).unwrap_or_default();
        return pack_parse_i64(1, ndt.and_utc().timestamp_millis());
    }
    pack_parse_i64(0, 0)
}

// --- gzip ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_gzip_compress(data: i64) -> i64 {
    let Some(bytes) = data_as_bytes(data) else {
        return 0;
    };
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    if enc.write_all(&bytes).is_err() {
        return 0;
    }
    match enc.finish() {
        Ok(out) => bytes_to_handle(out),
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_gzip_decompress(data: i64) -> i64 {
    let Some(bytes) = data_as_bytes(data) else {
        return 0;
    };
    let mut dec = flate2::read::GzDecoder::new(std::io::Cursor::new(bytes));
    let mut out = Vec::new();
    if dec.read_to_end(&mut out).is_err() {
        return 0;
    }
    bytes_to_handle(out)
}

// --- zip (single entry) ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_zip_pack(name: i64, data: i64) -> i64 {
    let Some(n) = string_data(name) else {
        return 0;
    };
    let Some(bytes) = data_as_bytes(data) else {
        return 0;
    };
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    if zip.start_file(n, opts).is_err() {
        return 0;
    }
    if zip.write_all(&bytes).is_err() {
        return 0;
    }
    match zip.finish() {
        Ok(c) => bytes_to_handle(c.into_inner()),
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_zip_unpack_first(data: i64) -> i64 {
    let fail = || {
        let st = echo_runtime_struct_new();
        struct_set_str(st, "ok", 0);
        struct_set_str(st, "name", string_to_handle(String::new()));
        struct_set_str(st, "data", bytes_to_handle(Vec::new()));
        st
    };
    let Some(bytes) = data_as_bytes(data) else {
        return fail();
    };
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = match zip::ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(_) => return fail(),
    };
    if archive.is_empty() {
        return fail();
    }
    let mut file = match archive.by_index(0) {
        Ok(f) => f,
        Err(_) => return fail(),
    };
    let mut out = Vec::new();
    if file.read_to_end(&mut out).is_err() {
        return fail();
    }
    let name = file.name().to_string();
    let st = echo_runtime_struct_new();
    struct_set_str(st, "ok", 1);
    struct_set_str(st, "name", string_to_handle(name));
    struct_set_str(st, "data", bytes_to_handle(out));
    st
}

// --- HMAC-SHA256, SHA512 ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_hmac_sha256(key: i64, data: i64) -> i64 {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let Some(k) = data_as_bytes(key) else {
        return 0;
    };
    let Some(d) = data_as_bytes(data) else {
        return 0;
    };
    let mut mac = match HmacSha256::new_from_slice(&k) {
        Ok(m) => m,
        Err(_) => return 0,
    };
    mac.update(&d);
    bytes_to_handle(mac.finalize().into_bytes().to_vec())
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_sha512(data: i64) -> i64 {
    use sha2::{Digest, Sha512};
    let Some(d) = data_as_bytes(data) else {
        return 0;
    };
    let mut h = Sha512::new();
    h.update(&d);
    bytes_to_handle(h.finalize().to_vec())
}

// --- AES-256-GCM ---
// key: 32 bytes, nonce: 12 bytes, returns ciphertext||tag as bytes

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_aes_gcm_encrypt(key: i64, nonce: i64, plaintext: i64) -> i64 {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    let Some(k) = data_as_bytes(key) else {
        return 0;
    };
    let Some(n) = data_as_bytes(nonce) else {
        return 0;
    };
    let Some(pt) = data_as_bytes(plaintext) else {
        return 0;
    };
    if k.len() != 32 || n.len() != 12 {
        return 0;
    }
    let cipher = match Aes256Gcm::new_from_slice(&k) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let nonce = Nonce::from_slice(&n);
    match cipher.encrypt(nonce, pt.as_ref()) {
        Ok(ct) => bytes_to_handle(ct),
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_aes_gcm_decrypt(key: i64, nonce: i64, ciphertext: i64) -> i64 {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    let Some(k) = data_as_bytes(key) else {
        return 0;
    };
    let Some(n) = data_as_bytes(nonce) else {
        return 0;
    };
    let Some(ct) = data_as_bytes(ciphertext) else {
        return 0;
    };
    if k.len() != 32 || n.len() != 12 {
        return 0;
    }
    let cipher = match Aes256Gcm::new_from_slice(&k) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let nonce = Nonce::from_slice(&n);
    match cipher.decrypt(nonce, ct.as_ref()) {
        Ok(pt) => bytes_to_handle(pt),
        Err(_) => 0,
    }
}

// --- fs chmod ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_chmod(path: i64, mode: i64) -> i64 {
    let Some(p) = string_data(path) else {
        return -1;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(mode as u32);
        match std::fs::set_permissions(p, perms) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (p, mode);
        -1
    }
}

// --- path clean (rust Path normalize-ish) ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_path_clean(path: i64) -> i64 {
    let Some(p) = string_data(path) else {
        return 0;
    };
    let cleaned = clean_path(&p);
    string_to_handle(cleaned)
}

fn clean_path(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let absolute = p.starts_with('/');
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|s| *s != "..") {
                    out.pop();
                } else if !absolute {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    let mut s = out.join("/");
    if absolute {
        s = format!("/{s}");
    }
    if s.is_empty() {
        if absolute {
            "/".into()
        } else {
            ".".into()
        }
    } else {
        s
    }
}

/// Relative path from `base` to `target` (POSIX-ish, both cleaned). Empty string on failure.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_path_rel(base: i64, target: i64) -> i64 {
    let Some(b) = string_data(base) else {
        return 0;
    };
    let Some(t) = string_data(target) else {
        return 0;
    };
    let bc = clean_path(&b);
    let tc = clean_path(&t);
    match path_rel_str(&bc, &tc) {
        Some(r) => string_to_handle(r),
        None => 0,
    }
}

fn path_rel_str(base: &str, target: &str) -> Option<String> {
    let base_abs = base.starts_with('/');
    let target_abs = target.starts_with('/');
    if base_abs != target_abs {
        return None;
    }
    let base_parts: Vec<&str> = if base == "/" || base == "." {
        Vec::new()
    } else {
        base.trim_start_matches('/').split('/').filter(|s| !s.is_empty()).collect()
    };
    let target_parts: Vec<&str> = if target == "/" || target == "." {
        Vec::new()
    } else {
        target.trim_start_matches('/').split('/').filter(|s| !s.is_empty()).collect()
    };
    let mut i = 0;
    while i < base_parts.len() && i < target_parts.len() && base_parts[i] == target_parts[i] {
        i += 1;
    }
    let mut out: Vec<&str> = Vec::new();
    for _ in i..base_parts.len() {
        out.push("..");
    }
    for p in &target_parts[i..] {
        out.push(p);
    }
    if out.is_empty() {
        Some(".".into())
    } else {
        Some(out.join("/"))
    }
}

// --- process run with cwd ---

/// Product `{ ok, code, stdout, stderr }` (`ok` internal; 0 = spawn fail).
/// Always a product so std can use `r.ok` (a 0 sentinel types as i64).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_run_cwd(program: i64, args: i64, cwd: i64) -> i64 {
    let fail = || {
        let st = echo_runtime_struct_new();
        struct_set_str(st, "ok", 0);
        struct_set_str(st, "code", -1);
        struct_set_str(st, "stdout", string_to_handle(String::new()));
        struct_set_str(st, "stderr", string_to_handle(String::new()));
        st
    };
    let Some(prog) = string_data(program) else {
        return fail();
    };
    if prog.is_empty() {
        return fail();
    }
    let mut cmd = Command::new(prog);
    if let Some(c) = string_data(cwd) {
        if !c.is_empty() {
            cmd.current_dir(c);
        }
    }
    if let Some(list) = list_strings(args) {
        for a in list {
            cmd.arg(a);
        }
    }
    match cmd.output() {
        Ok(out) => {
            let st = echo_runtime_struct_new();
            struct_set_str(st, "ok", 1);
            struct_set_str(st, "code", out.status.code().unwrap_or(-1) as i64);
            struct_set_str(
                st,
                "stdout",
                string_to_handle(String::from_utf8_lossy(&out.stdout).into_owned()),
            );
            struct_set_str(
                st,
                "stderr",
                string_to_handle(String::from_utf8_lossy(&out.stderr).into_owned()),
            );
            st
        }
        Err(_) => fail(),
    }
}

// --- process pipes (spawn + stdin/stdout/stderr handles) ---

const KIND_PROC_CHILD: u32 = 32;
const KIND_PROC_PIPE: u32 = 33;

#[repr(C)]
struct EchoProcChild {
    header: crate::HeapHeader,
    child: Option<Child>,
}

enum ProcPipeInner {
    Stdin(ChildStdin),
    Stdout(ChildStdout),
    Stderr(ChildStderr),
}

#[repr(C)]
struct EchoProcPipe {
    header: crate::HeapHeader,
    inner: Option<ProcPipeInner>,
}

fn proc_child_header() -> crate::HeapHeader {
    crate::HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_PROC_CHILD,
        promotion_epoch: 0,
    }
}

fn proc_pipe_header() -> crate::HeapHeader {
    crate::HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_PROC_PIPE,
        promotion_epoch: 0,
    }
}

fn pipe_handle(inner: ProcPipeInner) -> i64 {
    // Like TCP/TLS: do not note_heap_alloc — scope free would reclaim handles
    // held only in returned products.
    Box::into_raw(Box::new(EchoProcPipe {
        header: proc_pipe_header(),
        inner: Some(inner),
    })) as i64
}

/// Spawn with piped stdin/stdout/stderr.
/// Returns product `{ ok, child, stdin, stdout, stderr }` (ok=0 on failure).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_spawn_pipes(program: i64, args: i64) -> i64 {
    let fail = || {
        let st = echo_runtime_struct_new();
        struct_set_str(st, "ok", 0);
        struct_set_str(st, "child", 0);
        struct_set_str(st, "stdin", 0);
        struct_set_str(st, "stdout", 0);
        struct_set_str(st, "stderr", 0);
        st
    };
    let Some(prog) = string_data(program) else {
        return fail();
    };
    if prog.is_empty() {
        return fail();
    }
    let mut cmd = Command::new(&prog);
    if let Some(list) = list_strings(args) {
        for a in list {
            cmd.arg(a);
        }
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return fail(),
    };
    let Some(stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return fail();
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return fail();
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return fail();
    };
    let child_h = Box::into_raw(Box::new(EchoProcChild {
        header: proc_child_header(),
        child: Some(child),
    })) as i64;
    let st = echo_runtime_struct_new();
    struct_set_str(st, "ok", 1);
    struct_set_str(st, "child", child_h);
    struct_set_str(st, "stdin", pipe_handle(ProcPipeInner::Stdin(stdin)));
    struct_set_str(st, "stdout", pipe_handle(ProcPipeInner::Stdout(stdout)));
    struct_set_str(st, "stderr", pipe_handle(ProcPipeInner::Stderr(stderr)));
    st
}

/// Write all bytes/string to a process stdin pipe. 0 ok, -1 fail.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_pipe_write(pipe: i64, data: i64) -> i64 {
    if pipe == 0 {
        return -1;
    }
    let Some(bytes) = data_as_bytes(data) else {
        return -1;
    };
    unsafe {
        let Some(h) = header_at(pipe) else {
            return -1;
        };
        if (*h).kind != KIND_PROC_PIPE {
            return -1;
        }
        let p = &mut *(pipe as *mut EchoProcPipe);
        match p.inner.as_mut() {
            Some(ProcPipeInner::Stdin(w)) => match w.write_all(&bytes) {
                Ok(()) => 0,
                Err(_) => -1,
            },
            _ => -1,
        }
    }
}

/// Read up to `limit` bytes from stdout/stderr pipe. 0 = fail; empty bytes = EOF.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_pipe_read(pipe: i64, limit: i64) -> i64 {
    if pipe == 0 {
        return 0;
    }
    let limit = if limit <= 0 { 0 } else { limit as usize };
    if limit == 0 {
        return bytes_to_handle(Vec::new());
    }
    unsafe {
        let Some(h) = header_at(pipe) else {
            return 0;
        };
        if (*h).kind != KIND_PROC_PIPE {
            return 0;
        }
        let p = &mut *(pipe as *mut EchoProcPipe);
        let mut buf = vec![0u8; limit];
        let n = match p.inner.as_mut() {
            Some(ProcPipeInner::Stdout(r)) => r.read(&mut buf),
            Some(ProcPipeInner::Stderr(r)) => r.read(&mut buf),
            _ => return 0,
        };
        match n {
            Ok(0) => bytes_to_handle(Vec::new()),
            Ok(n) => {
                buf.truncate(n);
                bytes_to_handle(buf)
            }
            Err(_) => 0,
        }
    }
}

/// Close a process pipe end (drop half).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_pipe_close(pipe: i64) {
    if pipe == 0 {
        return;
    }
    unsafe {
        let Some(h) = header_at(pipe) else {
            return;
        };
        if (*h).kind != KIND_PROC_PIPE {
            return;
        }
        let p = &mut *(pipe as *mut EchoProcPipe);
        p.inner = None;
    }
}

/// Wait for child; return exit code, or -1 on failure.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_wait(child: i64) -> i64 {
    if child == 0 {
        return -1;
    }
    unsafe {
        let Some(h) = header_at(child) else {
            return -1;
        };
        if (*h).kind != KIND_PROC_CHILD {
            return -1;
        }
        let c = &mut *(child as *mut EchoProcChild);
        let Some(ch) = c.child.as_mut() else {
            return -1;
        };
        match ch.wait() {
            Ok(st) => st.code().unwrap_or(-1) as i64,
            Err(_) => -1,
        }
    }
}

// --- unix domain sockets ---
//
// Natives exist on every host (codegen always declares the symbols). On
// non-Unix they return handle 0 / -1. The listener/stream types are Unix-only.

#[cfg(unix)]
const KIND_UNIX_LISTENER: u32 = 30;
#[cfg(unix)]
const KIND_UNIX_STREAM: u32 = 31;

#[cfg(unix)]
#[repr(C)]
struct EchoUnixListener {
    header: crate::HeapHeader,
    path: String,
    inner: Option<std::os::unix::net::UnixListener>,
}

#[cfg(unix)]
#[repr(C)]
struct EchoUnixStream {
    header: crate::HeapHeader,
    inner: Option<std::os::unix::net::UnixStream>,
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_unix_listen(path: i64) -> i64 {
    let Some(p) = string_data(path) else {
        return 0;
    };
    let _ = std::fs::remove_file(&p);
    match std::os::unix::net::UnixListener::bind(&p) {
        Ok(lis) => {
            let boxed = Box::new(EchoUnixListener {
                header: crate::HeapHeader {
                    magic: HEAP_MAGIC,
                    kind: KIND_UNIX_LISTENER,
                    promotion_epoch: 0,
                },
                path: p,
                inner: Some(lis),
            });
            Box::into_raw(boxed) as i64
        }
        Err(_) => 0,
    }
}

#[cfg(not(unix))]
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_unix_listen(_path: i64) -> i64 {
    0
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_unix_accept(listener: i64) -> i64 {
    if listener == 0 {
        return 0;
    }
    unsafe {
        let Some(h) = crate::header_at(listener) else {
            return 0;
        };
        if (*h).kind != KIND_UNIX_LISTENER {
            return 0;
        }
        let lis = &mut *(listener as *mut EchoUnixListener);
        let Some(inner) = lis.inner.as_ref() else {
            return 0;
        };
        match inner.accept() {
            Ok((stream, _)) => {
                let boxed = Box::new(EchoUnixStream {
                    header: crate::HeapHeader {
                        magic: HEAP_MAGIC,
                        kind: KIND_UNIX_STREAM,
                        promotion_epoch: 0,
                    },
                    inner: Some(stream),
                });
                Box::into_raw(boxed) as i64
            }
            Err(_) => 0,
        }
    }
}

#[cfg(not(unix))]
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_unix_accept(_listener: i64) -> i64 {
    0
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_unix_connect(path: i64) -> i64 {
    let Some(p) = string_data(path) else {
        return 0;
    };
    match std::os::unix::net::UnixStream::connect(p) {
        Ok(stream) => {
            let boxed = Box::new(EchoUnixStream {
                header: crate::HeapHeader {
                    magic: HEAP_MAGIC,
                    kind: KIND_UNIX_STREAM,
                    promotion_epoch: 0,
                },
                inner: Some(stream),
            });
            Box::into_raw(boxed) as i64
        }
        Err(_) => 0,
    }
}

#[cfg(not(unix))]
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_unix_connect(_path: i64) -> i64 {
    0
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_unix_read(stream: i64, limit: i64) -> i64 {
    if stream == 0 {
        return 0;
    }
    let limit = if limit <= 0 { 0 } else { limit as usize };
    unsafe {
        let Some(h) = crate::header_at(stream) else {
            return 0;
        };
        if (*h).kind != KIND_UNIX_STREAM {
            return 0;
        }
        let st = &mut *(stream as *mut EchoUnixStream);
        let Some(inner) = st.inner.as_mut() else {
            return 0;
        };
        let mut buf = vec![0u8; limit];
        match inner.read(&mut buf) {
            Ok(0) => bytes_to_handle(Vec::new()),
            Ok(n) => {
                buf.truncate(n);
                bytes_to_handle(buf)
            }
            Err(_) => 0,
        }
    }
}

#[cfg(not(unix))]
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_unix_read(_stream: i64, _limit: i64) -> i64 {
    0
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_unix_write(stream: i64, data: i64) -> i64 {
    if stream == 0 {
        return -1;
    }
    let Some(bytes) = data_as_bytes(data) else {
        return -1;
    };
    unsafe {
        let Some(h) = crate::header_at(stream) else {
            return -1;
        };
        if (*h).kind != KIND_UNIX_STREAM {
            return -1;
        }
        let st = &mut *(stream as *mut EchoUnixStream);
        let Some(inner) = st.inner.as_mut() else {
            return -1;
        };
        match inner.write_all(&bytes) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }
}

#[cfg(not(unix))]
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_unix_write(_stream: i64, _data: i64) -> i64 {
    -1
}

#[cfg(unix)]
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_unix_close(stream: i64) {
    if stream == 0 {
        return;
    }
    unsafe {
        let Some(h) = crate::header_at(stream) else {
            return;
        };
        if (*h).kind == KIND_UNIX_STREAM {
            let st = &mut *(stream as *mut EchoUnixStream);
            st.inner = None;
        } else if (*h).kind == KIND_UNIX_LISTENER {
            let lis = &mut *(stream as *mut EchoUnixListener);
            if let Some(path) = Some(lis.path.clone()) {
                lis.inner = None;
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

#[cfg(not(unix))]
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_unix_close(_stream: i64) {}

// --- helpers ---

fn data_as_bytes(v: i64) -> Option<Vec<u8>> {
    if let Some(s) = string_data(v) {
        return Some(s.into_bytes());
    }
    bytes_data(v)
}

fn list_strings(list: i64) -> Option<Vec<String>> {
    if list == 0 || !crate::is_live_heap(list) {
        return Some(Vec::new());
    }
    let n = unsafe { crate::echo_runtime_list_len(list) };
    if n < 0 {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..n {
        let h = unsafe { crate::echo_runtime_list_get(list, i) };
        if let Some(s) = string_data(h) {
            out.push(s);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::echo_runtime_string_from_utf8;

    fn s(t: &str) -> i64 {
        unsafe { echo_runtime_string_from_utf8(t.as_ptr(), t.len()) }
    }

    #[cfg(unix)]
    #[test]
    fn process_run_cwd_pwd_tmp() {
        let r = echo_runtime_process_run_cwd(s("pwd"), crate::echo_runtime_list_new(), s("/tmp"));
        assert_ne!(r, 0);
        let ok = unsafe { crate::echo_runtime_struct_get(r, b"ok".as_ptr(), 2) };
        assert_eq!(ok, 1);
        let code = unsafe { crate::echo_runtime_struct_get(r, b"code".as_ptr(), 4) };
        assert_eq!(code, 0);
        let out = unsafe { crate::echo_runtime_struct_get(r, b"stdout".as_ptr(), 6) };
        let text = string_data(out).unwrap();
        assert!(text.contains("tmp"), "{text}");
    }

    #[test]
    fn process_run_cwd_missing_fails() {
        let r = echo_runtime_process_run_cwd(
            s("__echo_definitely_missing_binary_xyz__"),
            crate::echo_runtime_list_new(),
            s("/tmp"),
        );
        assert_ne!(r, 0);
        let ok = unsafe { crate::echo_runtime_struct_get(r, b"ok".as_ptr(), 2) };
        assert_eq!(ok, 0);
    }

    #[cfg(unix)]
    #[test]
    fn fs_chmod_sets_mode() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join("echo_chmod_crate.txt");
        std::fs::write(&path, b"x").unwrap();
        let p = s(path.to_str().unwrap());
        assert_eq!(echo_runtime_fs_chmod(p, 0o640), 0);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fs_chmod_missing_fails() {
        assert_eq!(
            echo_runtime_fs_chmod(s("/tmp/echo_chmod_absent_nope"), 0o644),
            -1
        );
    }

    #[test]
    fn parse_i64_ok() {
        let r = echo_runtime_parse_i64(s("42"));
        assert_ne!(r, 0);
    }

    #[test]
    fn parse_f64_ok() {
        let r = echo_runtime_parse_f64(s("3.5"));
        assert_ne!(r, 0);
        let ok = unsafe { crate::echo_runtime_struct_get(r, b"ok".as_ptr(), 2) };
        assert_eq!(ok, 1);
        let val = unsafe { crate::echo_runtime_struct_get(r, b"val".as_ptr(), 3) };
        assert!((crate::echo_runtime_float_to_f64(val) - 3.5).abs() < 1e-9);
    }

    #[test]
    fn parse_f64_fail() {
        let r = echo_runtime_parse_f64(s("nope"));
        assert_ne!(r, 0);
        let ok = unsafe { crate::echo_runtime_struct_get(r, b"ok".as_ptr(), 2) };
        assert_eq!(ok, 0);
    }

    #[test]
    fn gzip_roundtrip() {
        let raw = s("hello-gzip");
        let c = echo_runtime_gzip_compress(raw);
        assert_ne!(c, 0);
        let d = echo_runtime_gzip_decompress(c);
        assert_ne!(d, 0);
        let text = String::from_utf8(bytes_data(d).unwrap()).unwrap();
        assert_eq!(text, "hello-gzip");
    }

    #[test]
    fn zip_roundtrip_first_entry() {
        let z = echo_runtime_zip_pack(s("a.txt"), s("hi"));
        assert_ne!(z, 0);
        let e = echo_runtime_zip_unpack_first(z);
        assert_ne!(e, 0);
        let ok = unsafe { crate::echo_runtime_struct_get(e, b"ok".as_ptr(), 2) };
        let name = unsafe { crate::echo_runtime_struct_get(e, b"name".as_ptr(), 4) };
        let data = unsafe { crate::echo_runtime_struct_get(e, b"data".as_ptr(), 4) };
        assert_eq!(ok, 1);
        assert_eq!(string_data(name).as_deref(), Some("a.txt"));
        assert_eq!(bytes_data(data).as_deref(), Some(b"hi".as_slice()));
    }

    #[test]
    fn zip_unpack_invalid_ok_zero() {
        let e = echo_runtime_zip_unpack_first(s("not-a-zip"));
        assert_ne!(e, 0);
        let ok = unsafe { crate::echo_runtime_struct_get(e, b"ok".as_ptr(), 2) };
        assert_eq!(ok, 0);
    }

    #[test]
    fn path_clean_dots() {
        let c = echo_runtime_path_clean(s("/a/b/../c/./d"));
        let t = string_data(c).unwrap();
        assert_eq!(t, "/a/c/d");
    }

    #[test]
    fn path_rel_child_and_mismatch() {
        let r = echo_runtime_path_rel(s("/a/b"), s("/a/b/c"));
        assert_eq!(string_data(r).as_deref(), Some("c"));
        let same = echo_runtime_path_rel(s("/a/b"), s("/a/b"));
        assert_eq!(string_data(same).as_deref(), Some("."));
        let up = echo_runtime_path_rel(s("/a/b"), s("/a"));
        assert_eq!(string_data(up).as_deref(), Some(".."));
        assert_eq!(echo_runtime_path_rel(s("/a"), s("b")), 0);
    }

    #[test]
    fn hmac_sha256_known() {
        // RFC 4231 test case 2 (ASCII key/data).
        let mac = echo_runtime_hmac_sha256(s("Jefe"), s("what do ya want for nothing?"));
        assert_ne!(mac, 0);
        let d = bytes_data(mac).unwrap();
        assert_eq!(d.len(), 32);
        assert_eq!(
            hex::encode(&d),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn sha512_empty() {
        // cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce
        // 47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e
        let h = echo_runtime_sha512(s(""));
        assert_ne!(h, 0);
        let d = bytes_data(h).unwrap();
        assert_eq!(d.len(), 64);
        assert_eq!(d[0], 0xcf);
    }

    #[test]
    fn aes_gcm_roundtrip() {
        let key = vec![7u8; 32];
        let nonce = vec![1u8; 12];
        let key_h = bytes_to_handle(key);
        let nonce_h = bytes_to_handle(nonce);
        let pt = s("secret");
        let ct = echo_runtime_aes_gcm_encrypt(key_h, nonce_h, pt);
        assert_ne!(ct, 0);
        let out = echo_runtime_aes_gcm_decrypt(key_h, nonce_h, ct);
        assert_ne!(out, 0);
        assert_eq!(String::from_utf8(bytes_data(out).unwrap()).unwrap(), "secret");
    }

    #[cfg(unix)]
    #[test]
    fn unix_socket_echo() {
        let path = format!("/tmp/echo_unix_{}.sock", std::process::id());
        let lis = echo_runtime_unix_listen(s(&path));
        assert_ne!(lis, 0);
        let path2 = path.clone();
        let t = std::thread::spawn(move || {
            let st = echo_runtime_unix_accept(lis);
            assert_ne!(st, 0);
            let b = echo_runtime_unix_read(st, 16);
            assert_eq!(String::from_utf8(bytes_data(b).unwrap()).unwrap(), "hi");
            assert_eq!(echo_runtime_unix_write(st, s("yo")), 0);
            echo_runtime_unix_close(st);
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        let c = echo_runtime_unix_connect(s(&path2));
        assert_ne!(c, 0);
        assert_eq!(echo_runtime_unix_write(c, s("hi")), 0);
        let b = echo_runtime_unix_read(c, 16);
        assert_eq!(String::from_utf8(bytes_data(b).unwrap()).unwrap(), "yo");
        echo_runtime_unix_close(c);
        t.join().unwrap();
        echo_runtime_unix_close(lis);
    }

    #[cfg(not(unix))]
    #[test]
    fn unix_socket_stubs_unavailable() {
        assert_eq!(echo_runtime_unix_listen(s("/tmp/x.sock")), 0);
        assert_eq!(echo_runtime_unix_accept(1), 0);
        assert_eq!(echo_runtime_unix_connect(s("/tmp/x.sock")), 0);
        assert_eq!(echo_runtime_unix_read(1, 16), 0);
        assert_eq!(echo_runtime_unix_write(1, s("hi")), -1);
        echo_runtime_unix_close(1);
    }

    #[test]
    fn time_format_parse_roundtrip() {
        let ms = 1_700_000_000_000i64;
        let fmt = s("%Y-%m-%d");
        let s_h = echo_runtime_time_format(ms, fmt);
        assert_eq!(string_data(s_h).as_deref(), Some("2023-11-14"));
        let p = echo_runtime_time_parse(s_h, fmt);
        let ok = unsafe { crate::echo_runtime_struct_get(p, b"ok".as_ptr(), 2) };
        let val = unsafe { crate::echo_runtime_struct_get(p, b"val".as_ptr(), 3) };
        assert_eq!(ok, 1);
        assert!(val > 0);
        assert_eq!(
            string_data(echo_runtime_time_format(val, fmt)).as_deref(),
            Some("2023-11-14")
        );
        let bad = echo_runtime_time_parse(s("nope"), fmt);
        let bad_ok = unsafe { crate::echo_runtime_struct_get(bad, b"ok".as_ptr(), 2) };
        assert_eq!(bad_ok, 0);
    }

    #[cfg(unix)]
    #[test]
    fn process_pipes_cat_echo() {
        let empty = crate::echo_runtime_list_new();
        let r = echo_runtime_process_spawn_pipes(s("cat"), empty);
        assert_ne!(r, 0);
        // ok field
        let ok_name = b"ok";
        let ok = unsafe {
            crate::echo_runtime_struct_get(r, ok_name.as_ptr(), ok_name.len())
        };
        assert_eq!(ok, 1);
        let stdin = unsafe {
            crate::echo_runtime_struct_get(r, b"stdin".as_ptr(), 5)
        };
        let stdout = unsafe {
            crate::echo_runtime_struct_get(r, b"stdout".as_ptr(), 6)
        };
        let child = unsafe {
            crate::echo_runtime_struct_get(r, b"child".as_ptr(), 5)
        };
        assert_ne!(stdin, 0);
        assert_ne!(stdout, 0);
        assert_ne!(child, 0);
        assert_eq!(echo_runtime_process_pipe_write(stdin, s("ping")), 0);
        echo_runtime_process_pipe_close(stdin);
        let b = echo_runtime_process_pipe_read(stdout, 64);
        assert_ne!(b, 0);
        assert_eq!(String::from_utf8(bytes_data(b).unwrap()).unwrap(), "ping");
        echo_runtime_process_pipe_close(stdout);
        let code = echo_runtime_process_wait(child);
        assert_eq!(code, 0);
    }
}
