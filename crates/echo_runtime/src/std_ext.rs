//! Extra std primitives: math, random, os, time mono, json, dns, crypto, process capture, str helpers.

use std::cell::Cell;
use std::io::Read;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::{
    bytes_to_handle, echo_runtime_float_from_f64, echo_runtime_float_to_f64, echo_runtime_list_new,
    echo_runtime_list_push, echo_runtime_struct_new, string_as_str, string_data, string_to_handle,
    struct_set_str,
};

// --- Math (f64 heap floats) ---

macro_rules! math1 {
    ($name:ident, $op:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(x: i64) -> i64 {
            let v = echo_runtime_float_to_f64(x);
            echo_runtime_float_from_f64($op(v))
        }
    };
}

math1!(echo_runtime_math_sqrt, |v: f64| v.sqrt());
math1!(echo_runtime_math_sin, |v: f64| v.sin());
math1!(echo_runtime_math_cos, |v: f64| v.cos());
math1!(echo_runtime_math_tan, |v: f64| v.tan());
math1!(echo_runtime_math_floor, |v: f64| v.floor());
math1!(echo_runtime_math_ceil, |v: f64| v.ceil());
math1!(echo_runtime_math_abs_f, |v: f64| v.abs());

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_math_pow(a: i64, b: i64) -> i64 {
    let x = echo_runtime_float_to_f64(a);
    let y = echo_runtime_float_to_f64(b);
    echo_runtime_float_from_f64(x.powf(y))
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_math_abs_i(n: i64) -> i64 {
    n.saturating_abs()
}

// --- Random (non-crypto xorshift) ---

thread_local! {
    static RNG: Cell<u64> = Cell::new(0x4d595df4d0f33173);
}

fn rng_next() -> u64 {
    RNG.with(|c| {
        let mut x = c.get();
        if x == 0 {
            x = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1)
                | 1;
        }
        // xorshift64*
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        c.set(x);
        x.wrapping_mul(0x2545F4914F6CDD1D)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_random_seed(seed: i64) {
    let s = if seed == 0 { 1u64 } else { seed as u64 };
    RNG.with(|c| c.set(s));
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_random_u64() -> i64 {
    rng_next() as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_random_float() -> i64 {
    let u = rng_next() >> 11; // 53 bits
    let f = (u as f64) / ((1u64 << 53) as f64);
    echo_runtime_float_from_f64(f)
}

// --- CSPRNG ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_crypto_random_bytes(n: i64) -> i64 {
    let n = if n < 0 { 0 } else { n as usize };
    let mut buf = vec![0u8; n];
    if fill_csprng(&mut buf).is_err() {
        return 0;
    }
    bytes_to_handle(buf)
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_crypto_random_u64() -> i64 {
    let mut b = [0u8; 8];
    if fill_csprng(&mut b).is_err() {
        return 0;
    }
    i64::from_le_bytes(b)
}

fn fill_csprng(buf: &mut [u8]) -> Result<(), ()> {
    // Prefer OS CSPRNG via crate (not hand-rolled /dev/urandom).
    getrandom::getrandom(buf).map_err(|_| ())
}

// --- stdin (`std/io.read`) ---

fn stdin_read_from(reader: &mut impl Read) -> i64 {
    let mut buf = Vec::new();
    match reader.read_to_end(&mut buf) {
        Ok(_) => bytes_to_handle(buf),
        Err(_) => 0,
    }
}

/// Whole process stdin as a bytes handle.
///
/// Empty stdin is a live empty bytes value (not `0`). `0` means I/O error.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_stdin_read() -> i64 {
    stdin_read_from(&mut std::io::stdin())
}

// --- OS ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_os_pid() -> i64 {
    std::process::id() as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_os_cwd() -> i64 {
    match std::env::current_dir() {
        Ok(p) => string_to_handle(p.to_string_lossy().into_owned()),
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_os_chdir(path: i64) -> i64 {
    let Some(s) = string_data(path) else {
        return -1;
    };
    match std::env::set_current_dir(Path::new(&s)) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_os_hostname() -> i64 {
    #[cfg(unix)]
    {
        let from_file = std::fs::read_to_string("/etc/hostname")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let h = from_file
            .or_else(|| std::env::var("HOSTNAME").ok().filter(|s| !s.is_empty()))
            .unwrap_or_else(|| "unknown".into());
        return string_to_handle(h);
    }
    #[cfg(not(unix))]
    {
        string_to_handle(
            std::env::var("COMPUTERNAME")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| std::env::var("HOSTNAME").ok().filter(|s| !s.is_empty()))
                .unwrap_or_else(|| "unknown".into()),
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_os_platform() -> i64 {
    string_to_handle(std::env::consts::OS.to_string())
}

// --- Time monotonic ---

/// Process-wide origin (`docs/stdlib.md` `time.mono_ms`: since process start).
static MONO_START: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_now_mono_ms() -> i64 {
    MONO_START.elapsed().as_millis() as i64
}

// --- JSON ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_json_parse(s: i64) -> i64 {
    let Some(text) = string_data(s) else {
        return 0;
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => json_to_echo(&v),
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_json_stringify(v: i64) -> i64 {
    match echo_to_json(v) {
        Some(j) => string_to_handle(j.to_string()),
        None => 0,
    }
}

fn json_to_echo(v: &serde_json::Value) -> i64 {
    use serde_json::Value;
    match v {
        Value::Null => 0,
        Value::Bool(b) => {
            if *b {
                1
            } else {
                0
            }
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i
            } else if let Some(f) = n.as_f64() {
                echo_runtime_float_from_f64(f)
            } else {
                0
            }
        }
        Value::String(s) => string_to_handle(s.clone()),
        Value::Array(arr) => {
            let list = echo_runtime_list_new();
            for el in arr {
                unsafe {
                    echo_runtime_list_push(list, json_to_echo(el));
                }
            }
            list
        }
        Value::Object(map) => {
            let st = echo_runtime_struct_new();
            for (k, val) in map {
                let name = k.replace('-', "_");
                struct_set_str(st, &name, json_to_echo(val));
            }
            st
        }
    }
}

fn echo_to_json(v: i64) -> Option<serde_json::Value> {
    use crate::{
        KIND_FLOAT, KIND_LIST, KIND_STRING, KIND_STRUCT, header_at, is_live_heap, list_elems,
        struct_fields,
    };
    use serde_json::{Map, Value, json};
    if v == 0 {
        return Some(Value::Null);
    }
    if v == 1 {
        // ambiguous bool/int — prefer int in stringify of bare 1
        return Some(json!(1));
    }
    if !is_live_heap(v) {
        return Some(json!(v));
    }
    let h = unsafe { header_at(v)? };
    let kind = unsafe { (*h).kind };
    match kind {
        KIND_STRING => Some(Value::String(string_data(v)?)),
        KIND_FLOAT => Some(json!(echo_runtime_float_to_f64(v))),
        KIND_LIST => {
            let mut arr = Vec::new();
            for el in list_elems(v)? {
                arr.push(echo_to_json(el)?);
            }
            Some(Value::Array(arr))
        }
        KIND_STRUCT => {
            let mut map = Map::new();
            for (k, val) in struct_fields(v)? {
                map.insert(k, echo_to_json(val)?);
            }
            Some(Value::Object(map))
        }
        _ => Some(json!(v)),
    }
}

// --- DNS ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_dns_lookup(host: i64) -> i64 {
    let Some(h) = string_data(host) else {
        return 0;
    };
    let query = if h.contains(':') { h } else { format!("{h}:0") };
    let list = echo_runtime_list_new();
    let Ok(iter) = query.to_socket_addrs() else {
        return 0;
    };
    let mut seen = std::collections::HashSet::new();
    for addr in iter {
        let s = addr.ip().to_string();
        if seen.insert(s.clone()) {
            unsafe {
                echo_runtime_list_push(list, string_to_handle(s));
            }
        }
    }
    list
}

// --- SHA-256 (string or bytes handle) ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_sha256(data: i64) -> i64 {
    use sha2::{Digest, Sha256};
    // Borrow string/bytes once — never walk byte-by-byte through get (that was O(n²)).
    let hash = if let Some(s) = string_as_str(data) {
        Sha256::digest(s.as_bytes())
    } else if let Some(b) = crate::bytes_as_slice(data) {
        Sha256::digest(b)
    } else {
        return 0;
    };
    bytes_to_handle(hash.to_vec())
}

// --- Process run capture ---

/// Product `{ ok, code, stdout, stderr }` (`ok` internal; 0 = spawn fail).
/// Always a product so std can use `r.ok` (a 0 sentinel types as i64).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_run_capture(program: i64, args: i64) -> i64 {
    use std::process::Command;
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
    let mut cmd = Command::new(&prog);
    if let Some(list) = crate::list_elems(args) {
        for a in list {
            if let Some(s) = string_data(a) {
                cmd.arg(s);
            }
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

// --- FS polish ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_temp_dir() -> i64 {
    string_to_handle(std::env::temp_dir().to_string_lossy().into_owned())
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_create_temp(prefix: i64) -> i64 {
    let pre = string_data(prefix).unwrap_or_else(|| "echo".into());
    let dir = std::env::temp_dir();
    let name = format!(
        "{pre}_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let path = dir.join(name);
    match std::fs::write(&path, b"") {
        Ok(()) => string_to_handle(path.to_string_lossy().into_owned()),
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_symlink(original: i64, link: i64) -> i64 {
    let Some(o) = string_data(original) else {
        return -1;
    };
    let Some(l) = string_data(link) else {
        return -1;
    };
    #[cfg(unix)]
    {
        match std::os::unix::fs::symlink(&o, &l) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }
    #[cfg(not(unix))]
    {
        match std::os::windows::fs::symlink_file(&o, &l)
            .or_else(|_| std::os::windows::fs::symlink_dir(&o, &l))
        {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }
}

// --- Str helpers that are awkward pure ---

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_to_lower(s: i64) -> i64 {
    let Some(t) = string_data(s) else {
        return 0;
    };
    string_to_handle(t.to_lowercase())
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_to_upper(s: i64) -> i64 {
    let Some(t) = string_data(s) else {
        return 0;
    };
    string_to_handle(t.to_uppercase())
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_trim(s: i64) -> i64 {
    let Some(t) = string_data(s) else {
        return 0;
    };
    string_to_handle(t.trim().to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_split(s: i64, sep: i64) -> i64 {
    let Some(t) = string_data(s) else {
        return 0;
    };
    let Some(sp) = string_data(sep) else {
        return 0;
    };
    let list = echo_runtime_list_new();
    if sp.is_empty() {
        unsafe {
            echo_runtime_list_push(list, string_to_handle(t));
        }
        return list;
    }
    for part in t.split(&sp) {
        unsafe {
            echo_runtime_list_push(list, string_to_handle(part.to_string()));
        }
    }
    list
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_replace(s: i64, from: i64, to: i64) -> i64 {
    let Some(t) = string_data(s) else {
        return 0;
    };
    let Some(f) = string_data(from) else {
        return 0;
    };
    let Some(r) = string_data(to) else {
        return 0;
    };
    string_to_handle(t.replace(&f, &r))
}

// --- Hex / Base64 (crate-backed; do not hand-roll codecs here) ---

fn data_bytes(data: i64) -> Option<Vec<u8>> {
    if let Some(s) = string_as_str(data) {
        return Some(s.as_bytes().to_vec());
    }
    if let Some(b) = crate::bytes_as_slice(data) {
        return Some(b.to_vec());
    }
    None
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_hex_encode(data: i64) -> i64 {
    let Some(bytes) = data_bytes(data) else {
        return 0;
    };
    string_to_handle(hex::encode(bytes))
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_hex_decode(s: i64) -> i64 {
    let Some(t) = string_data(s) else {
        return 0;
    };
    match hex::decode(t.trim()) {
        Ok(b) => bytes_to_handle(b),
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_base64_encode(data: i64) -> i64 {
    use base64::Engine;
    let Some(bytes) = data_bytes(data) else {
        return 0;
    };
    string_to_handle(base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_utf8_valid(data: i64) -> i64 {
    let Some(bytes) = data_bytes(data) else {
        return 0;
    };
    i64::from(std::str::from_utf8(&bytes).is_ok())
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_utf8_decode(data: i64) -> i64 {
    let Some(bytes) = data_bytes(data) else {
        return 0;
    };
    match std::str::from_utf8(&bytes) {
        Ok(s) => string_to_handle(s.to_owned()),
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_base64_decode(s: i64) -> i64 {
    use base64::Engine;
    let Some(t) = string_data(s) else {
        return 0;
    };
    match base64::engine::general_purpose::STANDARD.decode(t.trim()) {
        Ok(b) => bytes_to_handle(b),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{echo_runtime_bytes_get, echo_runtime_bytes_len, echo_runtime_string_from_utf8};

    fn s(t: &str) -> i64 {
        unsafe { echo_runtime_string_from_utf8(t.as_ptr(), t.len()) }
    }

    #[cfg(unix)]
    #[test]
    fn process_run_capture_printf_hi() {
        let args = echo_runtime_list_new();
        unsafe {
            echo_runtime_list_push(args, s("hi"));
        }
        let r = echo_runtime_process_run_capture(s("printf"), args);
        assert_ne!(r, 0);
        let ok = unsafe { crate::echo_runtime_struct_get(r, b"ok".as_ptr(), 2) };
        assert_eq!(ok, 1);
        let code = unsafe { crate::echo_runtime_struct_get(r, b"code".as_ptr(), 4) };
        assert_eq!(code, 0);
        let out = unsafe { crate::echo_runtime_struct_get(r, b"stdout".as_ptr(), 6) };
        assert_eq!(string_data(out).as_deref(), Some("hi"));
    }

    #[cfg(unix)]
    #[test]
    fn process_run_capture_pwd() {
        let r = echo_runtime_process_run_capture(s("pwd"), echo_runtime_list_new());
        assert_ne!(r, 0);
        let ok = unsafe { crate::echo_runtime_struct_get(r, b"ok".as_ptr(), 2) };
        assert_eq!(ok, 1);
        let code = unsafe { crate::echo_runtime_struct_get(r, b"code".as_ptr(), 4) };
        assert_eq!(code, 0);
        let out = unsafe { crate::echo_runtime_struct_get(r, b"stdout".as_ptr(), 6) };
        let text = string_data(out).unwrap();
        assert!(text.contains('/'), "{text}");
    }

    #[test]
    fn process_run_capture_missing_fails() {
        let r = echo_runtime_process_run_capture(
            s("__echo_definitely_missing_binary_xyz__"),
            echo_runtime_list_new(),
        );
        assert_ne!(r, 0);
        let ok = unsafe { crate::echo_runtime_struct_get(r, b"ok".as_ptr(), 2) };
        assert_eq!(ok, 0);
    }

    #[test]
    fn now_mono_ms_nonneg_and_advances() {
        let a = echo_runtime_now_mono_ms();
        assert!(a >= 0);
        crate::echo_runtime_sleep_ms(30);
        let b = echo_runtime_now_mono_ms();
        assert!(b >= a);
        assert!(b - a >= 20, "mono {a} -> {b}");
    }

    #[test]
    fn now_mono_ms_shared_across_threads() {
        let _ = echo_runtime_now_mono_ms();
        crate::echo_runtime_sleep_ms(30);
        let a = echo_runtime_now_mono_ms();
        let b = std::thread::spawn(|| echo_runtime_now_mono_ms())
            .join()
            .unwrap();
        assert!(a >= 20, "main mono {a}");
        assert!(b >= 20, "worker mono reset? {b}");
    }

    #[test]
    fn math_sqrt_4() {
        let x = echo_runtime_float_from_f64(4.0);
        let y = echo_runtime_math_sqrt(x);
        assert!((echo_runtime_float_to_f64(y) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn math_floor_ceil_pow() {
        let f = echo_runtime_math_floor(echo_runtime_float_from_f64(3.7));
        assert!((echo_runtime_float_to_f64(f) - 3.0).abs() < 1e-9);
        let c = echo_runtime_math_ceil(echo_runtime_float_from_f64(3.2));
        assert!((echo_runtime_float_to_f64(c) - 4.0).abs() < 1e-9);
        let p = echo_runtime_math_pow(
            echo_runtime_float_from_f64(2.0),
            echo_runtime_float_from_f64(3.0),
        );
        assert!((echo_runtime_float_to_f64(p) - 8.0).abs() < 1e-9);
        let a = echo_runtime_math_abs_f(echo_runtime_float_from_f64(-2.5));
        assert!((echo_runtime_float_to_f64(a) - 2.5).abs() < 1e-9);
    }

    #[test]
    fn math_sin_cos_tan_zero() {
        let z = echo_runtime_float_from_f64(0.0);
        assert!((echo_runtime_float_to_f64(echo_runtime_math_sin(z))).abs() < 1e-12);
        assert!((echo_runtime_float_to_f64(echo_runtime_math_cos(z)) - 1.0).abs() < 1e-12);
        assert!((echo_runtime_float_to_f64(echo_runtime_math_tan(z))).abs() < 1e-12);
    }

    #[test]
    fn random_seed_deterministic() {
        echo_runtime_random_seed(42);
        let a = echo_runtime_random_u64();
        echo_runtime_random_seed(42);
        let b = echo_runtime_random_u64();
        assert_eq!(a, b);
    }

    #[test]
    fn json_roundtrip_object() {
        let raw = s(r#"{"a":1,"b":[2,3]}"#);
        let v = echo_runtime_json_parse(raw);
        assert_ne!(v, 0);
        let out = echo_runtime_json_stringify(v);
        let text = string_data(out).unwrap();
        assert!(text.contains("\"a\""));
        assert!(text.contains('1'));
    }

    /// `+` bodies run on the worker pool. Stringify must not treat a
    /// worker-allocated string as a bare integer.
    #[test]
    fn json_stringify_string_allocated_on_another_thread() {
        let h = std::thread::spawn(|| s("hello"))
            .join()
            .expect("alloc thread");
        let out = echo_runtime_json_stringify(h);
        assert_eq!(string_data(out).as_deref(), Some("\"hello\""));
    }

    #[test]
    fn hex_roundtrip_crate() {
        let raw = s("Hi");
        let enc = echo_runtime_hex_encode(raw);
        assert_eq!(string_data(enc).as_deref(), Some("4869"));
        let dec = echo_runtime_hex_decode(enc);
        assert_ne!(dec, 0);
        assert_eq!(unsafe { echo_runtime_bytes_len(dec) }, 2);
    }

    #[test]
    fn utf8_valid_decode() {
        let ok = echo_runtime_utf8_decode(s("hi"));
        assert_eq!(string_data(ok).as_deref(), Some("hi"));
        assert_eq!(echo_runtime_utf8_valid(s("hi")), 1);
        let bad = crate::bytes_to_handle(vec![0xff]);
        assert_eq!(echo_runtime_utf8_valid(bad), 0);
        assert_eq!(echo_runtime_utf8_decode(bad), 0);
    }

    #[test]
    fn base64_roundtrip_crate() {
        let raw = s("hi");
        let enc = echo_runtime_base64_encode(raw);
        assert_eq!(string_data(enc).as_deref(), Some("aGk="));
        let dec = echo_runtime_base64_decode(enc);
        assert_ne!(dec, 0);
        assert_eq!(unsafe { echo_runtime_bytes_len(dec) }, 2);
    }

    #[test]
    fn csprng_fill_nonzero_len() {
        let h = echo_runtime_crypto_random_bytes(16);
        assert_ne!(h, 0);
        assert_eq!(unsafe { echo_runtime_bytes_len(h) }, 16);
    }

    #[test]
    fn sha256_empty() {
        // e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let h = echo_runtime_sha256(s(""));
        assert_ne!(h, 0);
        assert_eq!(unsafe { echo_runtime_bytes_len(h) }, 32);
        assert_eq!(unsafe { echo_runtime_bytes_get(h, 0) }, 0xe3);
    }

    #[test]
    fn stdin_read_from_cursor_payload() {
        let mut cur = std::io::Cursor::new(b"hello");
        let h = super::stdin_read_from(&mut cur);
        assert_ne!(h, 0);
        assert_eq!(crate::bytes_data(h).as_deref(), Some(b"hello".as_slice()));

        let mut empty = std::io::Cursor::new(b"");
        let z = super::stdin_read_from(&mut empty);
        assert_ne!(z, 0, "empty stdin is a live empty bytes handle");
        assert_eq!(crate::bytes_data(z).as_deref(), Some(b"".as_slice()));

        let mut raw = std::io::Cursor::new([0xff, 0x00, b'A']);
        let b = super::stdin_read_from(&mut raw);
        assert_eq!(
            crate::bytes_data(b).as_deref(),
            Some([0xff, 0x00, b'A'].as_slice())
        );
    }

    #[test]
    fn os_pid_positive() {
        assert!(echo_runtime_os_pid() > 0);
    }

    #[test]
    fn os_platform_known() {
        let p = string_data(echo_runtime_os_platform()).unwrap();
        assert!(
            matches!(p.as_str(), "linux" | "macos" | "windows"),
            "unexpected os.platform {p}"
        );
    }

    #[test]
    fn os_hostname_nonempty() {
        let h = echo_runtime_os_hostname();
        assert_ne!(h, 0);
        let s = string_data(h).unwrap();
        assert!(!s.is_empty(), "hostname empty");
    }

    #[test]
    fn os_cwd_nonempty() {
        let h = echo_runtime_os_cwd();
        assert_ne!(h, 0);
        let p = string_data(h).unwrap();
        assert!(!p.is_empty(), "cwd empty");
    }

    #[test]
    fn os_chdir_missing_fails() {
        assert_eq!(
            echo_runtime_os_chdir(s("/tmp/echo_os_chdir_absent_nope")),
            -1
        );
    }

    #[cfg(unix)]
    #[test]
    fn fs_symlink_creates_link() {
        let dir = std::env::temp_dir();
        let tgt = dir.join("echo_symlink_crate_tgt.txt");
        let ln = dir.join("echo_symlink_crate.link");
        let _ = std::fs::remove_file(&ln);
        std::fs::write(&tgt, b"x").unwrap();
        assert_eq!(
            echo_runtime_fs_symlink(s(tgt.to_str().unwrap()), s(ln.to_str().unwrap())),
            0
        );
        assert!(
            std::fs::symlink_metadata(&ln)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let _ = std::fs::remove_file(&ln);
        let _ = std::fs::remove_file(&tgt);
    }

    #[test]
    fn fs_symlink_missing_parent_fails() {
        assert_eq!(
            echo_runtime_fs_symlink(
                s("/tmp/echo_symlink_tgt"),
                s("/tmp/echo_symlink_absent_dir/link")
            ),
            -1
        );
    }

    #[test]
    fn fs_create_temp_writes_empty_file() {
        let h = echo_runtime_fs_create_temp(s("echo_crate_tmp"));
        assert_ne!(h, 0);
        let p = string_data(h).unwrap();
        assert!(p.contains("echo_crate_tmp"), "{p}");
        let meta = std::fs::metadata(&p).unwrap();
        assert!(meta.is_file());
        assert_eq!(meta.len(), 0);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn dns_localhost() {
        let list = echo_runtime_dns_lookup(s("localhost"));
        assert_ne!(list, 0);
        let n = unsafe { crate::echo_runtime_list_len(list) };
        assert!(n >= 1);
    }

    #[test]
    fn str_split_join_parts() {
        let parts = echo_runtime_str_split(s("a,b,c"), s(","));
        assert_eq!(unsafe { crate::echo_runtime_list_len(parts) }, 3);
        assert_eq!(
            crate::string_data(unsafe { crate::echo_runtime_list_get(parts, 0) }).as_deref(),
            Some("a")
        );
        assert_eq!(
            crate::string_data(unsafe { crate::echo_runtime_list_get(parts, 2) }).as_deref(),
            Some("c")
        );
    }

    #[test]
    fn str_trim_replace_case() {
        assert_eq!(
            crate::string_data(echo_runtime_str_trim(s("  hi  "))).as_deref(),
            Some("hi")
        );
        assert_eq!(
            crate::string_data(echo_runtime_str_to_lower(s("AbC"))).as_deref(),
            Some("abc")
        );
        assert_eq!(
            crate::string_data(echo_runtime_str_to_upper(s("ab"))).as_deref(),
            Some("AB")
        );
        assert_eq!(
            crate::string_data(echo_runtime_str_replace(s("aa"), s("a"), s("b"))).as_deref(),
            Some("bb")
        );
    }
}
