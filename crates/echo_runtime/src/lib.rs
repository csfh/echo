//! Shared AOT and JIT executable runtime (ADR 0004).
//!
//! Language-facing behavior lives here as `extern "C"` symbols that LLVM IR
//! calls. Host clang is link plumbing only.
//!
//! Heap values (lists, strings) carry a magic header so `echo_runtime_print_i64`
//! can print either integers or heap objects (same `runtime.print` entry).

use std::cell::RefCell;
use std::collections::HashSet;
use std::ptr;
use std::sync::{LazyLock, Mutex, MutexGuard};

/// Handles produced by `Box::into_raw` in this runtime (exact set — no pointer probe).
///
/// Process-global: `+` task bodies run on the worker pool, and natives such as
/// `json_stringify` consult this set on the joining thread.
static LIVE_HEAP: LazyLock<Mutex<HashSet<i64>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn live_heap() -> MutexGuard<'static, HashSet<i64>> {
    LIVE_HEAP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Record a new heap handle (every `Box::into_raw` path must call this).
#[inline]
pub(crate) fn note_heap_alloc(handle: i64) {
    if handle != 0 {
        live_heap().insert(handle);
    }
}

/// Drop a heap handle from the live set (after logical free / physical free).
#[inline]
pub(crate) fn note_heap_free(handle: i64) {
    if handle != 0 {
        live_heap().remove(&handle);
    }
}

/// True if `handle` was allocated by this runtime and not yet freed.
#[inline]
pub(crate) fn is_live_heap(handle: i64) -> bool {
    if handle == 0 {
        return false;
    }
    live_heap().contains(&handle)
}

/// `Box::into_raw` + live-set registration.
#[inline]
pub(crate) fn heap_to_handle<T>(b: Box<T>) -> i64 {
    let h = Box::into_raw(b) as i64;
    note_heap_alloc(h);
    h
}

#[cfg(feature = "host-io")]
mod fs;
mod git;
#[cfg(feature = "host-io")]
mod net;
#[cfg(feature = "host-io")]
mod poll;
#[cfg(feature = "host-io")]
mod process;
#[cfg(feature = "host-io")]
mod sched;
mod scope;
#[cfg(feature = "host-io")]
mod std_ext;
#[cfg(feature = "host-io")]
mod std_more;
mod store;
#[cfg(feature = "host-io")]
mod task;
#[cfg(feature = "host-io")]
mod test_suite;
#[cfg(feature = "host-io")]
pub(crate) mod tls;

#[cfg(feature = "host-io")]
pub use std_ext::*;
#[cfg(feature = "host-io")]
pub use std_more::{
    echo_runtime_aes_gcm_decrypt, echo_runtime_aes_gcm_encrypt, echo_runtime_fs_chmod,
    echo_runtime_gzip_compress, echo_runtime_gzip_decompress, echo_runtime_hmac_sha256,
    echo_runtime_parse_f64, echo_runtime_parse_i64, echo_runtime_path_clean, echo_runtime_path_rel,
    echo_runtime_process_pipe_close, echo_runtime_process_pipe_read,
    echo_runtime_process_pipe_write, echo_runtime_process_run_cwd,
    echo_runtime_process_spawn_pipes, echo_runtime_process_wait, echo_runtime_sha512,
    echo_runtime_time_format, echo_runtime_time_parse, echo_runtime_unix_accept,
    echo_runtime_unix_close, echo_runtime_unix_connect, echo_runtime_unix_listen,
    echo_runtime_unix_read, echo_runtime_unix_write, echo_runtime_url_parse, echo_runtime_zip_pack,
    echo_runtime_zip_unpack_first,
};
#[cfg(feature = "host-io")]
pub use tls::{
    echo_runtime_tls_accept, echo_runtime_tls_close, echo_runtime_tls_close_listener,
    echo_runtime_tls_connect, echo_runtime_tls_listen, echo_runtime_tls_read,
    echo_runtime_tls_write,
};

pub use git::{
    echo_runtime_git_index_pack, echo_runtime_git_pack_count, echo_runtime_git_pack_valid,
    echo_runtime_git_receive_pack_split, echo_runtime_git_upload_pack_wants,
};
pub use store::{
    echo_runtime_store_cas, echo_runtime_store_cas_create, echo_runtime_store_close,
    echo_runtime_store_get, echo_runtime_store_get_if_none_match, echo_runtime_store_get_range,
    echo_runtime_store_head, echo_runtime_store_keys, echo_runtime_store_memory_new,
    echo_runtime_store_put, echo_runtime_store_s3_new,
};

pub use scope::{
    echo_runtime_scope_disown, echo_runtime_scope_drain_deferred,
    echo_runtime_scope_enqueue_release, echo_runtime_scope_enter, echo_runtime_scope_exit,
    echo_runtime_scope_promote, echo_runtime_scope_promote_graph, echo_runtime_scope_register,
    echo_runtime_scope_release,
};

// TCP/UDP — re-export for JIT symbol mapping (`echo_codegen`).
#[cfg(feature = "host-io")]
pub use net::{
    echo_runtime_tcp_accept, echo_runtime_tcp_close, echo_runtime_tcp_connect,
    echo_runtime_tcp_listen, echo_runtime_tcp_read, echo_runtime_tcp_write, echo_runtime_udp_bind,
    echo_runtime_udp_close, echo_runtime_udp_recv_from, echo_runtime_udp_send_to,
};

// Tasks / event loop (ADR 0013) — JIT mapping.
#[cfg(feature = "host-io")]
pub use task::{
    echo_runtime_task_after_run, echo_runtime_task_block, echo_runtime_task_block_wide,
    echo_runtime_task_check_joined, echo_runtime_task_join, echo_runtime_task_join_wide,
    echo_runtime_task_new, echo_runtime_task_new_args, echo_runtime_task_shape,
    echo_runtime_task_spawn, echo_runtime_task_spawn_args, echo_runtime_task_spawn_entry,
};

// Suite runner (Model A) — JIT mapping.
#[cfg(feature = "host-io")]
pub use test_suite::{
    echo_runtime_bench_configure, echo_runtime_test_bench_register, echo_runtime_test_enable,
    echo_runtime_test_enable_bench, echo_runtime_test_fail, echo_runtime_test_finish,
    echo_runtime_test_register,
};

// Process / env / spawn — JIT mapping.
#[cfg(feature = "host-io")]
pub use process::{
    ProcessArgsOverride, echo_runtime_process_args, echo_runtime_process_clear_args,
    echo_runtime_process_env_get, echo_runtime_process_env_has, echo_runtime_process_env_set,
    echo_runtime_process_env_unset, echo_runtime_process_exit, echo_runtime_process_run,
    echo_runtime_process_set_args,
};

// Filesystem — JIT mapping.
#[cfg(feature = "host-io")]
pub use fs::{
    echo_runtime_fs_copy, echo_runtime_fs_create_dir, echo_runtime_fs_create_dir_all,
    echo_runtime_fs_exists, echo_runtime_fs_file_close, echo_runtime_fs_file_read,
    echo_runtime_fs_file_seek, echo_runtime_fs_file_write, echo_runtime_fs_is_dir,
    echo_runtime_fs_is_file, echo_runtime_fs_join, echo_runtime_fs_metadata,
    echo_runtime_fs_open_append, echo_runtime_fs_open_read, echo_runtime_fs_open_write,
    echo_runtime_fs_read, echo_runtime_fs_read_dir, echo_runtime_fs_remove,
    echo_runtime_fs_remove_dir, echo_runtime_fs_rename, echo_runtime_fs_write,
};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// Heap object magic (little-endian bytes spell a distinct pattern).
pub(crate) const HEAP_MAGIC: u64 = 0x004F_4843_4545_4845; // "EHECHO\0\0" style unique

pub(crate) const KIND_LIST: u32 = 1;
pub(crate) const KIND_STRING: u32 = 2;
pub(crate) const KIND_STRUCT: u32 = 4;
pub(crate) const KIND_FLOAT: u32 = 5;
pub(crate) const KIND_BYTES: u32 = 6;
pub(crate) const KIND_LOCATOR: u32 = 7;
pub(crate) const KIND_RANGE: u32 = 8;
pub(crate) const KIND_FN: u32 = 9;

/// Ret shape codes stored on first-class function values.
pub const FN_SHAPE_PLAIN: i64 = 0;
pub const FN_SHAPE_RESULT: i64 = 1;
pub const FN_SHAPE_OPTION: i64 = 2;

#[repr(C)]
pub(crate) struct HeapHeader {
    pub magic: u64,
    pub kind: u32,
    /// Graph-promote visit epoch (ADR 0016 region evacuation). 0 = never visited.
    pub promotion_epoch: u32,
}

/// Heap list of i64 elements.
#[repr(C)]
pub(crate) struct EchoList {
    header: HeapHeader,
    elems: Vec<i64>,
}

/// Heap UTF-8 string.
#[repr(C)]
pub(crate) struct EchoString {
    header: HeapHeader,
    data: String,
}

/// Heap named struct (field name → i64 value; order preserved for print).
///
/// `type_name` is the `% Shape` name when constructed as a tagged lit
/// (`circle { … }`); empty for anonymous `{ … }` and runtime-built products.
#[repr(C)]
pub(crate) struct EchoStruct {
    header: HeapHeader,
    type_name: String,
    fields: Vec<(String, i64)>,
}

/// Heap-boxed IEEE f64 (universal ABI handle).
#[repr(C)]
pub(crate) struct EchoFloat {
    header: HeapHeader,
    value: f64,
}

/// Heap bytes blob (not necessarily UTF-8).
#[repr(C)]
pub(crate) struct EchoBytes {
    header: HeapHeader,
    data: Vec<u8>,
}

/// Heap locator (path / URI text).
#[repr(C)]
pub(crate) struct EchoLocator {
    header: HeapHeader,
    data: String,
}

/// Inclusive integer range `lo..hi` (empty when lo > hi).
#[repr(C)]
pub(crate) struct EchoRange {
    header: HeapHeader,
    lo: i64,
    hi: i64,
}

/// First-class function value: code pointer + return shape.
#[repr(C)]
pub(crate) struct EchoFn {
    header: HeapHeader,
    /// Function pointer bits (same as LLVM `ptrtoint`).
    code: i64,
    /// [`FN_SHAPE_PLAIN`], [`FN_SHAPE_RESULT`], or [`FN_SHAPE_OPTION`].
    shape: i64,
}

fn list_header() -> HeapHeader {
    HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_LIST,
        promotion_epoch: 0,
    }
}

fn string_header() -> HeapHeader {
    HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_STRING,
        promotion_epoch: 0,
    }
}

fn struct_header() -> HeapHeader {
    HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_STRUCT,
        promotion_epoch: 0,
    }
}

fn float_header() -> HeapHeader {
    HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_FLOAT,
        promotion_epoch: 0,
    }
}

fn bytes_header() -> HeapHeader {
    HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_BYTES,
        promotion_epoch: 0,
    }
}

fn locator_header() -> HeapHeader {
    HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_LOCATOR,
        promotion_epoch: 0,
    }
}

fn range_header() -> HeapHeader {
    HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_RANGE,
        promotion_epoch: 0,
    }
}

/// Allocate inclusive range `lo..hi` (empty when lo > hi).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_range_new(lo: i64, hi: i64) -> i64 {
    let r = Box::new(EchoRange {
        header: range_header(),
        lo,
        hi,
    });
    heap_to_handle(r)
}

fn fn_header() -> HeapHeader {
    HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_FN,
        promotion_epoch: 0,
    }
}

/// Box a code pointer + ret shape as a first-class function value.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fn_new(code: i64, shape: i64) -> i64 {
    let f = Box::new(EchoFn {
        header: fn_header(),
        code,
        shape,
    });
    heap_to_handle(f)
}

/// Code pointer bits from a function value handle (0 if invalid).
///
/// # Safety
/// `handle` must be 0 or a valid `echo_runtime_fn_new` handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_fn_code(handle: i64) -> i64 {
    if handle == 0 {
        return 0;
    }
    let Some(h) = (unsafe { header_at(handle) }) else {
        return 0;
    };
    if unsafe { (*h).kind } != KIND_FN {
        return 0;
    }
    let f = unsafe { &*(handle as *const EchoFn) };
    f.code
}

/// Ret shape code from a function value handle (defaults to plain if invalid).
///
/// # Safety
/// `handle` must be 0 or a valid `echo_runtime_fn_new` handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_fn_shape(handle: i64) -> i64 {
    if handle == 0 {
        return FN_SHAPE_PLAIN;
    }
    let Some(h) = (unsafe { header_at(handle) }) else {
        return FN_SHAPE_PLAIN;
    };
    if unsafe { (*h).kind } != KIND_FN {
        return FN_SHAPE_PLAIN;
    }
    let f = unsafe { &*(handle as *const EchoFn) };
    f.shape
}

pub(crate) unsafe fn header_at(v: i64) -> Option<*const HeapHeader> {
    if v == 0 {
        return None;
    }
    // Reject obvious small integers (not valid heap pointers for our purposes).
    if (v as u64) < 4096 {
        return None;
    }
    let p = v as *const HeapHeader;
    if !(p as usize).is_multiple_of(std::mem::align_of::<HeapHeader>()) {
        return None;
    }
    let h = unsafe { &*p };
    if h.magic != HEAP_MAGIC {
        return None;
    }
    Some(p)
}

/// Abort the process with a UTF-8 message. Called from generated IR.
///
/// # Safety
/// `ptr` must be valid for `len` bytes of UTF-8 (or any bytes; lossy printed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_abort(ptr: *const u8, len: usize) -> ! {
    let bytes = if ptr.is_null() || len == 0 {
        b"echo abort"
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }
    };
    let msg = String::from_utf8_lossy(bytes);
    eprintln!("echo: {msg}");
    std::process::exit(1);
}

// Optional capture sink for `echo_runtime_print_i64` (REPL eager-eval hints).
// When `Some`, prints append here (with trailing newline) instead of stdout.
thread_local! {
    static PRINT_CAPTURE: RefCell<Option<String>> = const { RefCell::new(None) };
}

// Separate capture for `echo_runtime_eprint` (crate tests; not mixed with stdout).
thread_local! {
    static EPRINT_CAPTURE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Run `f` while routing `runtime.print` into a string buffer (not stdout).
///
/// Nested calls are not supported; the outer capture is replaced.
pub fn with_print_capture<F, R>(f: F) -> (R, String)
where
    F: FnOnce() -> R,
{
    PRINT_CAPTURE.with(|cell| {
        *cell.borrow_mut() = Some(String::new());
    });
    let result = f();
    let captured = PRINT_CAPTURE.with(|cell| cell.borrow_mut().take().unwrap_or_default());
    (result, captured)
}

/// Run `f` while routing `runtime.eprint` into a string buffer (not stderr).
///
/// Nested calls are not supported; the outer capture is replaced.
pub fn with_eprint_capture<F, R>(f: F) -> (R, String)
where
    F: FnOnce() -> R,
{
    EPRINT_CAPTURE.with(|cell| {
        *cell.borrow_mut() = Some(String::new());
    });
    let result = f();
    let captured = EPRINT_CAPTURE.with(|cell| cell.borrow_mut().take().unwrap_or_default());
    (result, captured)
}

/// UTF-8 text → string handle (safe wrapper around [`echo_runtime_string_from_utf8`]).
#[must_use]
pub fn string_handle_from_utf8(text: &str) -> i64 {
    unsafe { echo_runtime_string_from_utf8(text.as_ptr(), text.len()) }
}

/// Payload bytes → bytes handle (safe wrapper around [`echo_runtime_bytes_from_ptr`]).
#[must_use]
pub fn bytes_handle_from_slice(bytes: &[u8]) -> i64 {
    unsafe { echo_runtime_bytes_from_ptr(bytes.as_ptr(), bytes.len()) }
}

/// Push `value` onto a list handle.
pub fn list_push_value(list: i64, value: i64) {
    unsafe { echo_runtime_list_push(list, value) }
}

/// Length of a list or range handle.
#[must_use]
pub fn list_len_value(list: i64) -> i64 {
    unsafe { echo_runtime_list_len(list) }
}

/// Element at `index`, or 0 if out of range.
#[must_use]
pub fn list_get_value(list: i64, index: i64) -> i64 {
    unsafe { echo_runtime_list_get(list, index) }
}

/// Named or anonymous struct handle.
#[must_use]
pub fn struct_new_value(type_name: &str) -> i64 {
    if type_name.is_empty() {
        echo_runtime_struct_new()
    } else {
        unsafe { echo_runtime_struct_new_named(type_name.as_ptr(), type_name.len()) }
    }
}

/// Set a struct field by name.
pub fn struct_set_value(handle: i64, field: &str, value: i64) {
    unsafe { echo_runtime_struct_set(handle, field.as_ptr(), field.len(), value) }
}

/// Get a struct field by name (0 if missing).
#[must_use]
pub fn struct_get_value(handle: i64, field: &str) -> i64 {
    unsafe { echo_runtime_struct_get(handle, field.as_ptr(), field.len()) }
}

/// Store `value` at `index` in a list handle.
pub fn list_set_value(list: i64, index: i64, value: i64) {
    unsafe { echo_runtime_list_set(list, index, value) }
}

/// 1 if `handle` is a struct tagged `type_name`.
#[must_use]
pub fn struct_type_is_value(handle: i64, type_name: &str) -> i64 {
    unsafe { echo_runtime_struct_type_is(handle, type_name.as_ptr(), type_name.len()) }
}

/// Append a UTF-8 literal to a string builder.
pub fn string_builder_push_utf8(builder: i64, text: &str) {
    unsafe { echo_runtime_string_builder_push_str(builder, text.as_ptr(), text.len()) }
}

/// Print a **string** handle followed by newline.
///
/// Non-strings (ints, floats, lists, structs, …) produce no output. Convert
/// explicitly via `str_from_int` / `str_from_float` (and later helpers) first.
///
/// Name keeps `_i64` for ABI stability; it is the general `runtime.print` target.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_print_i64(v: i64) {
    if let Some(s) = string_data(v) {
        let routed = PRINT_CAPTURE.with(|cell| {
            if let Some(buf) = cell.borrow_mut().as_mut() {
                buf.push_str(&s);
                buf.push('\n');
                true
            } else {
                false
            }
        });
        if !routed {
            println!("{s}");
        }
    }
}

/// Print a **string** handle to stderr followed by newline (`std/io.eprint`).
///
/// Non-strings produce no output. Independent of [`echo_runtime_print_i64`]
/// capture — REPL/playground stdout capture must not swallow stderr.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_eprint(v: i64) {
    if let Some(s) = string_data(v) {
        let routed = EPRINT_CAPTURE.with(|cell| {
            if let Some(buf) = cell.borrow_mut().as_mut() {
                buf.push_str(&s);
                buf.push('\n');
                true
            } else {
                false
            }
        });
        if !routed {
            eprintln!("{s}");
        }
    }
}

/// Format a signed integer (universal i64 bits) as a heap string handle.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_from_int(n: i64) -> i64 {
    string_to_handle(n.to_string())
}

/// Shallow debug string for any ABI value (REPL bare-expr display, diagnostics).
///
/// Recognizes heap kinds (string, float, list, struct, bytes, locator, range, fn);
/// everything else is printed as a signed integer (includes bools as `0`/`1` —
/// hosts that know the kind should prefer type-directed display for bools).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_from_debug(v: i64) -> i64 {
    string_to_handle(format_debug_value(v, 0))
}

fn format_debug_value(v: i64, depth: u32) -> String {
    const MAX_DEPTH: u32 = 3;
    if depth > MAX_DEPTH {
        return "…".into();
    }
    if let Some(s) = string_as_str(v) {
        return format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    }
    if let Some(f) = float_data(v) {
        return format!("{f}");
    }
    if let Some(elems) = list_as_slice(v) {
        let inner: Vec<String> = elems
            .iter()
            .map(|e| format_debug_value(*e, depth + 1))
            .collect();
        return format!("[{}]", inner.join(", "));
    }
    if let Some(st) = struct_debug(v) {
        let fields: Vec<String> = st
            .fields
            .iter()
            .map(|(n, val)| format!("{n}: {}", format_debug_value(*val, depth + 1)))
            .collect();
        let body = fields.join(", ");
        if st.type_name.is_empty() {
            return format!("{{ {body} }}");
        }
        return format!("{} {{ {body} }}", st.type_name);
    }
    if let Some(b) = bytes_as_slice(v) {
        return format!("b\"{}\"", String::from_utf8_lossy(b));
    }
    if let Some(loc) = locator_as_str(v) {
        return format!("p\"{loc}\"");
    }
    if let Some((lo, hi)) = range_data(v) {
        return format!("{lo}..{hi}");
    }
    if is_fn_handle(v) {
        return "<fn>".into();
    }
    // Bare integer / bool bits / unknown.
    format!("{v}")
}

struct StructDebug {
    type_name: String,
    fields: Vec<(String, i64)>,
}

fn struct_debug(v: i64) -> Option<StructDebug> {
    let h = unsafe { header_at(v)? };
    if unsafe { (*h).kind } != KIND_STRUCT {
        return None;
    }
    let st = unsafe { &*(v as *const EchoStruct) };
    Some(StructDebug {
        type_name: st.type_name.clone(),
        fields: st.fields.clone(),
    })
}

fn range_data(v: i64) -> Option<(i64, i64)> {
    let h = unsafe { header_at(v)? };
    if unsafe { (*h).kind } != KIND_RANGE {
        return None;
    }
    let r = unsafe { &*(v as *const EchoRange) };
    Some((r.lo, r.hi))
}

fn is_fn_handle(v: i64) -> bool {
    let Some(h) = (unsafe { header_at(v) }) else {
        return false;
    };
    unsafe { (*h).kind == KIND_FN }
}

/// Format a float value as a heap string handle.
///
/// `handle` is a heap float or raw f64 bit pattern (from native float boxing).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_from_float(handle: i64) -> i64 {
    let v = echo_runtime_float_to_f64(handle);
    string_to_handle(v.to_string())
}

/// Format a bytes handle as a heap string (UTF-8 lossy) for `print` / display.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_from_bytes(handle: i64) -> i64 {
    match bytes_data(handle) {
        Some(data) => string_to_handle(String::from_utf8_lossy(&data).into_owned()),
        None => string_to_handle(String::new()),
    }
}

/// Format a duration value (i64 nanoseconds) as a heap string for print.
///
/// Picks the largest unit among `h`/`m`/`s`/`ms`/`us` that divides evenly;
/// otherwise formats as `…ns`.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_from_duration(nanos: i64) -> i64 {
    string_to_handle(format_duration_nanos(nanos))
}

fn format_duration_nanos(nanos: i64) -> String {
    if nanos == 0 {
        return "0s".into();
    }
    let neg = nanos < 0;
    let n = nanos.unsigned_abs();
    const US: u64 = 1_000;
    const MS: u64 = 1_000_000;
    const S: u64 = 1_000_000_000;
    const M: u64 = 60 * S;
    const H: u64 = 3600 * S;
    let (mag, unit) = if n % H == 0 {
        (n / H, "h")
    } else if n % M == 0 {
        (n / M, "m")
    } else if n % S == 0 {
        (n / S, "s")
    } else if n % MS == 0 {
        (n / MS, "ms")
    } else if n % US == 0 {
        (n / US, "us")
    } else {
        (n, "ns")
    };
    if neg {
        format!("-{mag}{unit}")
    } else {
        format!("{mag}{unit}")
    }
}

/// Build a bytes handle from a pointer+length (copied).
///
/// # Safety
/// `ptr` must be valid for `len` bytes (may be null if `len == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_bytes_from_ptr(ptr: *const u8, len: usize) -> i64 {
    let data = if ptr.is_null() || len == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
    };
    bytes_to_handle(data)
}

/// Format a locator handle as a heap string (path/URI text) for print.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_from_locator(handle: i64) -> i64 {
    match locator_data(handle) {
        Some(s) => string_to_handle(s),
        None => string_to_handle(String::new()),
    }
}

/// Byte length of a string or bytes handle (0 if invalid).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_len(handle: i64) -> i64 {
    if let Some(s) = string_as_str(handle) {
        return s.len() as i64;
    }
    if let Some(b) = bytes_as_slice(handle) {
        return b.len() as i64;
    }
    0
}

/// Length of a **bytes** handle only (0 if not a bytes value).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_bytes_len(handle: i64) -> i64 {
    bytes_as_slice(handle).map(|b| b.len() as i64).unwrap_or(0)
}

/// Pack a signed `i64` as **8 little-endian bytes** (bit pattern of the integer).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_bytes_from_i64(n: i64) -> i64 {
    bytes_to_handle(n.to_le_bytes().to_vec())
}

// --- Value reflection (`std/reflect` → runtime) --------------------------------
//
// Public kind codes align with heap `KIND_*` for heap values; bare integers
// (non-heap ABI slots, including bool 0/1) are kind `0` (`"int"`).

/// Bare integer / non-heap ABI value (includes bool as 0/1).
pub const REFLECT_KIND_INT: i64 = 0;

fn reflect_kind_code(v: i64) -> i64 {
    let Some(h) = (unsafe { header_at(v) }) else {
        return REFLECT_KIND_INT;
    };
    i64::from(unsafe { (*h).kind })
}

fn reflect_kind_name_str(kind: i64) -> &'static str {
    match kind {
        REFLECT_KIND_INT => "int",
        k if k == i64::from(KIND_LIST) => "list",
        k if k == i64::from(KIND_STRING) => "string",
        k if k == i64::from(KIND_STRUCT) => "struct",
        k if k == i64::from(KIND_FLOAT) => "float",
        k if k == i64::from(KIND_BYTES) => "bytes",
        k if k == i64::from(KIND_LOCATOR) => "locator",
        k if k == i64::from(KIND_RANGE) => "range",
        k if k == i64::from(KIND_FN) => "fn",
        _ => "unknown",
    }
}

/// Runtime kind code of a universal `i64` ABI value.
///
/// `0` = bare int (non-heap). Heap kinds match internal `KIND_*` tags.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_reflect_kind(v: i64) -> i64 {
    reflect_kind_code(v)
}

/// Stable kind name string handle (`"int"`, `"string"`, `"bytes"`, …).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_reflect_kind_name(v: i64) -> i64 {
    string_to_handle(reflect_kind_name_str(reflect_kind_code(v)).to_string())
}

/// Kind-tagged byte material for content hashing (map/set keys).
///
/// Layout:
/// - **int** (non-heap): `[0] || le8(bits)`
/// - **string**: `[2] || utf-8`
/// - **bytes**: `[6] || payload`
/// - **other heap**: `[kind] || le8(handle bits)` (identity-stable, not deep content)
///
/// Distinct kinds never share a key-bytes prefix, so `1` and `"1"` hash apart.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_reflect_key_bytes(v: i64) -> i64 {
    if let Some(s) = string_data(v) {
        let mut out = Vec::with_capacity(1 + s.len());
        out.push(KIND_STRING as u8);
        out.extend_from_slice(s.as_bytes());
        return bytes_to_handle(out);
    }
    if let Some(b) = bytes_data(v) {
        let mut out = Vec::with_capacity(1 + b.len());
        out.push(KIND_BYTES as u8);
        out.extend_from_slice(&b);
        return bytes_to_handle(out);
    }
    let kind = reflect_kind_code(v);
    let mut out = Vec::with_capacity(9);
    out.push(kind as u8);
    out.extend_from_slice(&v.to_le_bytes());
    bytes_to_handle(out)
}

/// Byte at `index` (0-based) as `i64` in `0..255`.
///
/// Returns `-1` if the handle is not bytes or `index` is out of range.
/// Prefer bounds checks in `std/bytes` before calling.
///
/// **Hot path:** borrows the heap blob (no full-buffer clone).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_bytes_get(handle: i64, index: i64) -> i64 {
    let Some(b) = bytes_as_slice(handle) else {
        return -1;
    };
    if index < 0 {
        return -1;
    }
    match b.get(index as usize) {
        Some(&byte) => i64::from(byte),
        None => -1,
    }
}

/// Sub-blob by **byte** indices `[start, end)` (half-open).
///
/// Empty bytes handle if not bytes or range invalid. Prefer checks in `std/bytes`.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_bytes_slice(handle: i64, start: i64, end: i64) -> i64 {
    let Some(b) = bytes_as_slice(handle) else {
        return bytes_to_handle(Vec::new());
    };
    let len = b.len() as i64;
    if start < 0 || end < start || end > len {
        return bytes_to_handle(Vec::new());
    }
    bytes_to_handle(b[start as usize..end as usize].to_vec())
}

/// Concatenate two **bytes** handles → new bytes handle.
/// Non-bytes arguments contribute empty payloads.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_bytes_cat(a: i64, b: i64) -> i64 {
    let left = bytes_as_slice(a).unwrap_or_default();
    let right = bytes_as_slice(b).unwrap_or_default();
    let mut out = Vec::with_capacity(left.len() + right.len());
    out.extend_from_slice(left);
    out.extend_from_slice(right);
    bytes_to_handle(out)
}

/// UTF-8 payload of a string handle as a **bytes** handle (copy).
/// Empty if not a string.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_bytes_from_str(handle: i64) -> i64 {
    match string_as_str(handle) {
        Some(s) => bytes_to_handle(s.as_bytes().to_vec()),
        None => bytes_to_handle(Vec::new()),
    }
}

/// Bytes payload of a universal value for rich `b"…{name}…"` interpolation.
///
/// Bytes copy as raw. String and locator copy UTF-8 text. Floats use Display.
/// Bare integers use decimal. Other heap kinds yield empty.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_bytes_from_value(v: i64) -> i64 {
    if let Some(b) = bytes_as_slice(v) {
        return bytes_to_handle(b.to_vec());
    }
    if let Some(s) = string_as_str(v) {
        return bytes_to_handle(s.as_bytes().to_vec());
    }
    if let Some(s) = locator_as_str(v) {
        return bytes_to_handle(s.as_bytes().to_vec());
    }
    if let Some(h) = unsafe { header_at(v) } {
        if unsafe { (*h).kind } == KIND_FLOAT {
            let f = unsafe { &*(v as *const EchoFloat) };
            return bytes_to_handle(f.value.to_string().into_bytes());
        }
        return bytes_to_handle(Vec::new());
    }
    bytes_to_handle(v.to_string().into_bytes())
}

/// Byte at `index` of a **string** (UTF-8 bytes) as `i64` in `0..255`.
///
/// Returns `-1` if not a string or OOB. Prefer bounds checks in `std/str`.
///
/// **Hot path:** borrows the heap string (no full clone).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_get(handle: i64, index: i64) -> i64 {
    let Some(s) = string_as_str(handle) else {
        return -1;
    };
    if index < 0 {
        return -1;
    }
    match s.as_bytes().get(index as usize) {
        Some(&byte) => i64::from(byte),
        None => -1,
    }
}

/// Repeat string `s` exactly `n` times (O(n·|s|) with one allocation).
///
/// `n <= 0` or non-string `s` → empty string. Used by `std/str.repeat` so bulk
/// benches are not dominated by quadratic `cat` in a loop.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_repeat(s: i64, n: i64) -> i64 {
    if n <= 0 {
        return string_to_handle(String::new());
    }
    let Some(base) = string_as_str(s) else {
        return string_to_handle(String::new());
    };
    if base.is_empty() {
        return string_to_handle(String::new());
    }
    let times = n as usize;
    let mut out = String::with_capacity(base.len().saturating_mul(times));
    for _ in 0..times {
        out.push_str(base);
    }
    string_to_handle(out)
}

/// Concatenate two string/bytes handles → new string handle.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_cat(a: i64, b: i64) -> i64 {
    let left = string_as_str(a)
        .map(str::to_owned)
        .or_else(|| bytes_as_slice(a).map(|b| String::from_utf8_lossy(b).into_owned()))
        .unwrap_or_default();
    let right = string_as_str(b)
        .map(str::to_owned)
        .or_else(|| bytes_as_slice(b).map(|b| String::from_utf8_lossy(b).into_owned()))
        .unwrap_or_default();
    string_to_handle(format!("{left}{right}"))
}

/// Borrow string/bytes payload (no full clone). Indices for slice are these bytes.
fn str_payload_bytes(handle: i64) -> Option<&'static [u8]> {
    if let Some(s) = string_as_str(handle) {
        return Some(s.as_bytes());
    }
    bytes_as_slice(handle)
}

/// UTF-8 view: borrow a string; lossy-decode bytes only when needed.
fn str_as_utf8(handle: i64) -> Option<std::borrow::Cow<'static, str>> {
    if let Some(s) = string_as_str(handle) {
        return Some(std::borrow::Cow::Borrowed(s));
    }
    bytes_as_slice(handle).map(String::from_utf8_lossy)
}

/// Substring by **UTF-8 byte** indices `[start, end)` (half-open).
///
/// Returns empty string handle if the range is invalid (`start < 0`, `end < start`,
/// `end > len`, or not a string/bytes). Prefer bounds checks in `std/str`.
/// Indices apply to the original payload; the slice is then lossy-decoded.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_slice(handle: i64, start: i64, end: i64) -> i64 {
    let Some(bytes) = str_payload_bytes(handle) else {
        return string_to_handle(String::new());
    };
    let len = bytes.len() as i64;
    if start < 0 || end < start || end > len {
        return string_to_handle(String::new());
    }
    let a = start as usize;
    let b = end as usize;
    // Slice may land mid-codepoint; still return those bytes as a new string.
    string_to_handle(String::from_utf8_lossy(&bytes[a..b]).into_owned())
}

/// 1 if `hay` contains `needle` as a substring (UTF-8 content); else 0.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_contains(hay: i64, needle: i64) -> i64 {
    let Some(h) = str_as_utf8(hay) else {
        return 0;
    };
    let Some(n) = str_as_utf8(needle) else {
        return 0;
    };
    i64::from(h.contains(n.as_ref()))
}

/// 1 if `s` starts with `prefix`; else 0.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_starts_with(s: i64, prefix: i64) -> i64 {
    let Some(h) = str_as_utf8(s) else {
        return 0;
    };
    let Some(p) = str_as_utf8(prefix) else {
        return 0;
    };
    i64::from(h.starts_with(p.as_ref()))
}

/// 1 if `s` ends with `suffix`; else 0.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_str_ends_with(s: i64, suffix: i64) -> i64 {
    let Some(h) = str_as_utf8(s) else {
        return 0;
    };
    let Some(p) = str_as_utf8(suffix) else {
        return 0;
    };
    i64::from(h.ends_with(p.as_ref()))
}

/// Build a locator handle from UTF-8 path/URI bytes (copied).
///
/// # Safety
/// `ptr` must be valid for `len` bytes (may be null if `len == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_locator_from_utf8(ptr: *const u8, len: usize) -> i64 {
    let data = if ptr.is_null() || len == 0 {
        String::new()
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    locator_to_handle(data)
}

/// Locator handle from a string handle's UTF-8 text (copy). Empty if not a string.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_locator_from_string(handle: i64) -> i64 {
    match string_as_str(handle) {
        Some(s) => locator_to_handle(s.to_owned()),
        None => locator_to_handle(String::new()),
    }
}

/// Locator class codes (`docs/semantics.md`): relative / absolute / URI.
pub const LOCATOR_CLASS_REL: i64 = 0;
pub const LOCATOR_CLASS_ABS: i64 = 1;
pub const LOCATOR_CLASS_URI: i64 = 2;

/// Classify stored locator or string text. Non-text handles are relative (0).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_locator_class(handle: i64) -> i64 {
    let Some(text) = locator_or_string_text(handle) else {
        return LOCATOR_CLASS_REL;
    };
    classify_locator_text(&text)
}

fn locator_or_string_text(handle: i64) -> Option<String> {
    if let Some(s) = locator_data(handle) {
        return Some(s);
    }
    string_data(handle)
}

/// Path vs URI class of UTF-8 text as written (no normalize).
#[must_use]
pub fn classify_locator_text(text: &str) -> i64 {
    if locator_text_is_uri(text) {
        LOCATOR_CLASS_URI
    } else if text.starts_with('/') {
        LOCATOR_CLASS_ABS
    } else {
        LOCATOR_CLASS_REL
    }
}

/// URI when a RFC 3986 scheme is followed by `://` (spec example `http://…`).
/// `mailto:x` and `C:` stay relative text.
#[must_use]
pub fn locator_text_is_uri(text: &str) -> bool {
    let b = text.as_bytes();
    if b.is_empty() || !b[0].is_ascii_alphabetic() {
        return false;
    }
    let mut i = 1;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.' {
            i += 1;
            continue;
        }
        break;
    }
    b.get(i..i + 3) == Some(b"://")
}

/// Box an `f64` as a heap float handle (universal ABI).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_float_from_f64(v: f64) -> i64 {
    let f = Box::new(EchoFloat {
        header: float_header(),
        value: v,
    });
    heap_to_handle(f)
}

/// Unbox a float handle to `f64`. Non-float handles fall back to bitcast of the
/// raw bits (legacy / non-heap) so callers must only pass float-shaped values.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_float_to_f64(handle: i64) -> f64 {
    if let Some(h) = unsafe { header_at(handle) } {
        if unsafe { (*h).kind } == KIND_FLOAT {
            let f = unsafe { &*(handle as *const EchoFloat) };
            return f.value;
        }
    }
    f64::from_bits(handle as u64)
}

/// Build a string handle from UTF-8 bytes (copied).
///
/// # Safety
/// `ptr` must be valid for `len` bytes (may be null if `len == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_string_from_utf8(ptr: *const u8, len: usize) -> i64 {
    let data = if ptr.is_null() || len == 0 {
        String::new()
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    string_to_handle(data)
}

pub(crate) fn string_to_handle(data: String) -> i64 {
    let s = Box::new(EchoString {
        header: string_header(),
        data,
    });
    heap_to_handle(s)
}

/// Borrow UTF-8 payload of a string handle (no clone).
///
/// The returned slice is valid for as long as the heap object is live and not
/// mutated. Callers must not free/mutate the handle while using the slice.
pub(crate) fn string_as_str(v: i64) -> Option<&'static str> {
    let h = unsafe { header_at(v)? };
    if unsafe { (*h).kind } != KIND_STRING {
        return None;
    }
    let s = unsafe { &*(v as *const EchoString) };
    // Heap string is valid while the handle lives; extend lifetime for FFI-style access.
    Some(unsafe { &*(&s.data as *const String) }.as_str())
}

/// Owned copy of string payload (for APIs that need ownership).
pub(crate) fn string_data(v: i64) -> Option<String> {
    string_as_str(v).map(str::to_owned)
}

fn float_data(v: i64) -> Option<f64> {
    let h = unsafe { header_at(v)? };
    if unsafe { (*h).kind } != KIND_FLOAT {
        return None;
    }
    let f = unsafe { &*(v as *const EchoFloat) };
    Some(f.value)
}

pub(crate) fn bytes_to_handle(data: Vec<u8>) -> i64 {
    let b = Box::new(EchoBytes {
        header: bytes_header(),
        data,
    });
    heap_to_handle(b)
}

/// Borrow bytes payload (no full-buffer clone). Hot path for `bytes_get` / scan.
///
/// The returned slice is valid for as long as the heap object is live and not
/// mutated. Callers must not free/mutate the handle while using the slice.
pub(crate) fn bytes_as_slice(v: i64) -> Option<&'static [u8]> {
    let h = unsafe { header_at(v)? };
    if unsafe { (*h).kind } != KIND_BYTES {
        return None;
    }
    let b = unsafe { &*(v as *const EchoBytes) };
    Some(unsafe { &*(&b.data as *const Vec<u8>) }.as_slice())
}

/// Owned copy of bytes payload (for APIs that need ownership / return ownership).
pub(crate) fn bytes_data(v: i64) -> Option<Vec<u8>> {
    bytes_as_slice(v).map(|s| s.to_vec())
}

fn locator_to_handle(data: String) -> i64 {
    let loc = Box::new(EchoLocator {
        header: locator_header(),
        data,
    });
    heap_to_handle(loc)
}

pub(crate) fn locator_as_str(v: i64) -> Option<&'static str> {
    let h = unsafe { header_at(v)? };
    if unsafe { (*h).kind } != KIND_LOCATOR {
        return None;
    }
    let loc = unsafe { &*(v as *const EchoLocator) };
    Some(unsafe { &*(&loc.data as *const String) }.as_str())
}

pub(crate) fn locator_data(v: i64) -> Option<String> {
    locator_as_str(v).map(str::to_owned)
}

/// Borrow list element slice (no clone). Prefer over [`list_elems`] on hot paths.
pub(crate) fn list_as_slice(v: i64) -> Option<&'static [i64]> {
    let h = unsafe { header_at(v)? };
    if unsafe { (*h).kind } != KIND_LIST {
        return None;
    }
    let list = unsafe { &*(v as *const EchoList) };
    Some(unsafe { &*(&list.elems as *const Vec<i64>) }.as_slice())
}

pub(crate) fn list_elems(v: i64) -> Option<Vec<i64>> {
    list_as_slice(v).map(|s| s.to_vec())
}

pub(crate) fn struct_fields(v: i64) -> Option<Vec<(String, i64)>> {
    let h = unsafe { header_at(v)? };
    if unsafe { (*h).kind } != KIND_STRUCT {
        return None;
    }
    let st = unsafe { &*(v as *const EchoStruct) };
    Some(st.fields.clone())
}

fn deep_eq(a: i64, b: i64) -> bool {
    // Same handle is always deep-equal (and avoids re-entrancy cost).
    if a == b {
        return true;
    }
    match (string_as_str(a), string_as_str(b)) {
        (Some(sa), Some(sb)) => return sa == sb,
        (Some(_), None) | (None, Some(_)) => return false,
        (None, None) => {}
    }
    match (bytes_as_slice(a), bytes_as_slice(b)) {
        (Some(ba), Some(bb)) => return ba == bb,
        (Some(_), None) | (None, Some(_)) => return false,
        (None, None) => {}
    }
    match (locator_as_str(a), locator_as_str(b)) {
        (Some(la), Some(lb)) => return la == lb,
        (Some(_), None) | (None, Some(_)) => return false,
        (None, None) => {}
    }
    match (float_data(a), float_data(b)) {
        (Some(fa), Some(fb)) => return fa == fb,
        (Some(_), None) | (None, Some(_)) => return false,
        (None, None) => {}
    }
    match (list_as_slice(a), list_as_slice(b)) {
        (Some(la), Some(lb)) => {
            if la.len() != lb.len() {
                return false;
            }
            return la.iter().zip(lb.iter()).all(|(x, y)| deep_eq(*x, *y));
        }
        (Some(_), None) | (None, Some(_)) => return false,
        (None, None) => {}
    }
    match (struct_fields(a), struct_fields(b)) {
        (Some(fa), Some(fb)) => {
            if fa.len() != fb.len() {
                return false;
            }
            for (name, va) in &fa {
                match fb.iter().find(|(n, _)| n == name) {
                    Some((_, vb)) if deep_eq(*va, *vb) => {}
                    _ => return false,
                }
            }
            return true;
        }
        (Some(_), None) | (None, Some(_)) => return false,
        (None, None) => {}
    }
    // Inclusive ranges: same lo/hi. Function values: same code + shape.
    if let (Some(ha), Some(hb)) = (unsafe { header_at(a) }, unsafe { header_at(b) }) {
        let ka = unsafe { (*ha).kind };
        let kb = unsafe { (*hb).kind };
        if ka == KIND_RANGE && kb == KIND_RANGE {
            let ra = unsafe { &*(a as *const EchoRange) };
            let rb = unsafe { &*(b as *const EchoRange) };
            return ra.lo == rb.lo && ra.hi == rb.hi;
        }
        if ka == KIND_FN && kb == KIND_FN {
            let fa = unsafe { &*(a as *const EchoFn) };
            let fb = unsafe { &*(b as *const EchoFn) };
            return fa.code == fb.code && fa.shape == fb.shape;
        }
    }
    // Unboxed scalars (int/bool bits): numeric equality.
    if unsafe { header_at(a) }.is_none() && unsafe { header_at(b) }.is_none() {
        return a == b;
    }
    // Other heap kinds (builder, …) or mixed: not deep-equal.
    false
}

/// Deep equality for values (`==`). Returns 1 or 0.
///
/// Lists and structs compare structurally; strings/bytes/locators by content;
/// floats by value; unboxed ints/bools by bits. Different kinds → 0.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_eq(a: i64, b: i64) -> i64 {
    i64::from(deep_eq(a, b))
}

/// Deep inequality (`!=`).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_ne(a: i64, b: i64) -> i64 {
    i64::from(!deep_eq(a, b))
}

/// Identity equality (`===`): same handle/bit pattern.
///
/// Heap values are equal only if they are the same object; content copies are not.
/// Unboxed ints/bools use the same bit comparison as deep eq.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_eq_id(a: i64, b: i64) -> i64 {
    i64::from(a == b)
}

/// Identity inequality (`!==`).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_ne_id(a: i64, b: i64) -> i64 {
    i64::from(a != b)
}

/// String builder handle (for rich-string interpolation).
#[repr(C)]
struct EchoStringBuilder {
    header: HeapHeader,
    buf: String,
}

const KIND_BUILDER: u32 = 3;

#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_string_builder_new() -> i64 {
    let b = Box::new(EchoStringBuilder {
        header: HeapHeader {
            magic: HEAP_MAGIC,
            kind: KIND_BUILDER,
            promotion_epoch: 0,
        },
        buf: String::new(),
    });
    heap_to_handle(b)
}

/// # Safety
/// `b` must be a builder handle; `ptr` valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_string_builder_push_str(b: i64, ptr: *const u8, len: usize) {
    if b == 0 {
        return;
    }
    let builder = unsafe { &mut *(b as *mut EchoStringBuilder) };
    if builder.header.magic != HEAP_MAGIC || builder.header.kind != KIND_BUILDER {
        return;
    }
    if ptr.is_null() || len == 0 {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    builder.buf.push_str(&String::from_utf8_lossy(bytes));
}

/// Append a value: string or locator text, float Display, or decimal integer.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_string_builder_push_value(b: i64, v: i64) {
    if b == 0 {
        return;
    }
    let builder = unsafe { &mut *(b as *mut EchoStringBuilder) };
    if builder.header.magic != HEAP_MAGIC || builder.header.kind != KIND_BUILDER {
        return;
    }
    if let Some(s) = string_as_str(v) {
        builder.buf.push_str(s);
        return;
    }
    if let Some(s) = locator_as_str(v) {
        builder.buf.push_str(s);
        return;
    }
    if let Some(h) = unsafe { header_at(v) } {
        if unsafe { (*h).kind } == KIND_FLOAT {
            use std::fmt::Write;
            let f = unsafe { &*(v as *const EchoFloat) };
            let _ = write!(builder.buf, "{}", f.value);
            return;
        }
        return;
    }
    use std::fmt::Write;
    let _ = write!(builder.buf, "{v}");
}

/// Finish builder → string handle (consumes builder).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_string_builder_finish(b: i64) -> i64 {
    if b == 0 {
        return string_to_handle(String::new());
    }
    let builder = unsafe { Box::from_raw(b as *mut EchoStringBuilder) };
    if builder.header.magic != HEAP_MAGIC || builder.header.kind != KIND_BUILDER {
        return string_to_handle(String::new());
    }
    string_to_handle(builder.buf)
}

/// Allocate an empty list. Returns an opaque handle as `i64` (pointer bits).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_list_new() -> i64 {
    let list = Box::new(EchoList {
        header: list_header(),
        elems: Vec::new(),
    });
    heap_to_handle(list)
}

/// Push one i64 element onto a list handle.
///
/// # Safety
/// `list` must be a handle from `echo_runtime_list_new` (or null no-op).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_list_push(list: i64, value: i64) {
    if list == 0 {
        return;
    }
    let list = unsafe { &mut *(list as *mut EchoList) };
    if list.header.magic != HEAP_MAGIC || list.header.kind != KIND_LIST {
        return;
    }
    list.elems.push(value);
}

/// Reserve capacity for at least `additional` more pushes (no-op if invalid).
///
/// # Safety
/// `list` must be a valid list handle or 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_list_reserve(list: i64, additional: i64) {
    if list == 0 || additional <= 0 {
        return;
    }
    let list = unsafe { &mut *(list as *mut EchoList) };
    if list.header.magic != HEAP_MAGIC || list.header.kind != KIND_LIST {
        return;
    }
    list.elems.reserve(additional as usize);
}

/// Outer list of `n` distinct empty list handles (hash-table bucket array).
///
/// Faster than an Echo loop of `n` × `[]` + push for map/set rehash paths.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_list_new_empty_lists(n: i64) -> i64 {
    if n <= 0 {
        return echo_runtime_list_new();
    }
    let n = n as usize;
    let mut elems = Vec::with_capacity(n);
    for _ in 0..n {
        elems.push(echo_runtime_list_new());
    }
    let list = Box::new(EchoList {
        header: list_header(),
        elems,
    });
    heap_to_handle(list)
}

/// Length of a list **or** inclusive range handle.
///
/// # Safety
/// `list` must be a valid handle or 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_list_len(list: i64) -> i64 {
    if list == 0 {
        return 0;
    }
    let Some(h) = (unsafe { header_at(list) }) else {
        return 0;
    };
    let kind = unsafe { (*h).kind };
    match kind {
        KIND_LIST => {
            let list = unsafe { &*(list as *const EchoList) };
            list.elems.len() as i64
        }
        KIND_RANGE => {
            let r = unsafe { &*(list as *const EchoRange) };
            if r.lo > r.hi {
                0
            } else {
                // Inclusive: lo..=hi
                r.hi.saturating_sub(r.lo).saturating_add(1)
            }
        }
        _ => 0,
    }
}

/// Wall-clock milliseconds since Unix epoch (UTC). Non-negative; 0 on error.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => {
            let ms = d.as_millis();
            if ms > i64::MAX as u128 {
                i64::MAX
            } else {
                ms as i64
            }
        }
        Err(_) => 0,
    }
}

/// Sleep at least `ms` milliseconds. Negative or zero is a no-op.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_sleep_ms(ms: i64) {
    if ms <= 0 {
        return;
    }
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
}

/// Element at index for list **or** inclusive range, or 0 if OOB / null.
///
/// # Safety
/// `list` must be a valid handle or 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_list_get(list: i64, index: i64) -> i64 {
    if list == 0 || index < 0 {
        return 0;
    }
    let Some(h) = (unsafe { header_at(list) }) else {
        return 0;
    };
    let kind = unsafe { (*h).kind };
    match kind {
        KIND_LIST => {
            let list = unsafe { &*(list as *const EchoList) };
            list.elems.get(index as usize).copied().unwrap_or(0)
        }
        KIND_RANGE => {
            let r = unsafe { &*(list as *const EchoRange) };
            if r.lo > r.hi {
                return 0;
            }
            let len = r.hi.saturating_sub(r.lo).saturating_add(1);
            if index >= len {
                return 0;
            }
            r.lo.saturating_add(index)
        }
        _ => 0,
    }
}

/// Store `value` at `index`, or no-op if out of range / null / invalid.
///
/// # Safety
/// `list` must be a valid handle or 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_list_set(list: i64, index: i64, value: i64) {
    if list == 0 || index < 0 {
        return;
    }
    let list = unsafe { &mut *(list as *mut EchoList) };
    if list.header.magic != HEAP_MAGIC || list.header.kind != KIND_LIST {
        return;
    }
    let i = index as usize;
    if i < list.elems.len() {
        list.elems[i] = value;
    }
}

/// Allocate an empty anonymous struct. Returns opaque handle as `i64`.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_struct_new() -> i64 {
    let st = Box::new(EchoStruct {
        header: struct_header(),
        type_name: String::new(),
        fields: Vec::new(),
    });
    heap_to_handle(st)
}

/// Allocate an empty struct tagged with a `% Shape` type name.
///
/// # Safety
/// `name_ptr` valid for `name_len` (or null when `name_len == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_struct_new_named(
    name_ptr: *const u8,
    name_len: usize,
) -> i64 {
    let type_name = if name_ptr.is_null() || name_len == 0 {
        String::new()
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    let st = Box::new(EchoStruct {
        header: struct_header(),
        type_name,
        fields: Vec::new(),
    });
    heap_to_handle(st)
}

/// Return 1 if `handle` is a struct whose type tag equals `name`, else 0.
///
/// Used by `|` match `% TypeName` arms. Anonymous structs never match.
///
/// # Safety
/// `handle` valid or 0; `name_ptr` valid for `name_len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_struct_type_is(
    handle: i64,
    name_ptr: *const u8,
    name_len: usize,
) -> i64 {
    if handle == 0 || name_ptr.is_null() || name_len == 0 {
        return 0;
    }
    let st = unsafe { &*(handle as *const EchoStruct) };
    if st.header.magic != HEAP_MAGIC || st.header.kind != KIND_STRUCT {
        return 0;
    }
    if st.type_name.is_empty() {
        return 0;
    }
    let name = {
        let bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
        String::from_utf8_lossy(bytes)
    };
    if st.type_name.as_str() == name { 1 } else { 0 }
}

/// Set field `name` on a struct handle (insert or replace).
///
/// # Safety
/// `handle` from `echo_runtime_struct_new` (or 0 no-op); `name_ptr` valid for `name_len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_struct_set(
    handle: i64,
    name_ptr: *const u8,
    name_len: usize,
    value: i64,
) {
    if handle == 0 {
        return;
    }
    let st = unsafe { &mut *(handle as *mut EchoStruct) };
    if st.header.magic != HEAP_MAGIC || st.header.kind != KIND_STRUCT {
        return;
    }
    let name = if name_ptr.is_null() || name_len == 0 {
        String::new()
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    if let Some((_, slot)) = st.fields.iter_mut().find(|(n, _)| n == &name) {
        *slot = value;
    } else {
        st.fields.push((name, value));
    }
}

/// Get field `name` from a struct handle, or 0 if missing / invalid.
///
/// # Safety
/// `handle` valid or 0; `name_ptr` valid for `name_len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_struct_get(
    handle: i64,
    name_ptr: *const u8,
    name_len: usize,
) -> i64 {
    if handle == 0 {
        return 0;
    }
    let st = unsafe { &*(handle as *const EchoStruct) };
    if st.header.magic != HEAP_MAGIC || st.header.kind != KIND_STRUCT {
        return 0;
    }
    let name = if name_ptr.is_null() || name_len == 0 {
        return 0;
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
        String::from_utf8_lossy(bytes)
    };
    st.fields
        .iter()
        .find(|(n, _)| n.as_str() == name)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

#[allow(dead_code)]
fn _null() -> *const u8 {
    ptr::null()
}

pub(crate) fn struct_set_str(handle: i64, name: &str, value: i64) {
    unsafe {
        echo_runtime_struct_set(handle, name.as_ptr(), name.len(), value);
    }
}

/// Normalize HTTP header name for Echo field keys: lowercase, `-` → `_`.
///
/// So `Content-Type` → `content_type` (usable as `.content_type` on the headers
/// product when names are simple identifiers).
fn normalize_header_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        match b {
            b'A'..=b'Z' => out.push((b + 32) as char),
            b'-' => out.push('_'),
            _ => out.push(b as char),
        }
    }
    out
}

/// Borrow HTTP raw payload (bytes handle, else UTF-8 of a string). No clone.
fn http_raw_bytes(raw: i64) -> &'static [u8] {
    bytes_as_slice(raw)
        .or_else(|| string_as_str(raw).map(str::as_bytes))
        .unwrap_or(&[])
}

/// httparse scratch; 64 silently failed open as `GET /` (TooManyHeaders).
const HTTP_MAX_HEADERS: usize = 128;

/// Return 1 if `raw` (string or bytes) contains a complete HTTP header block
/// (`\r\n\r\n`), else 0. Used by `http.handle_connection` to accumulate reads.
///
/// # Safety
/// `raw` is 0 or a valid string/bytes handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_http_headers_complete(raw: i64) -> i64 {
    let bytes = http_raw_bytes(raw);
    if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
        1
    } else {
        0
    }
}

/// Return 1 when `raw` has complete headers **and** body bytes for
/// `Content-Length` (if present). No `Content-Length` → complete once headers end.
/// Incomplete headers or short body → 0.
///
/// # Safety
/// `raw` is 0 or a valid string/bytes handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_http_request_complete(raw: i64) -> i64 {
    let bytes = http_raw_bytes(raw);
    if !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
        return 0;
    }
    let mut headers_buf = [httparse::EMPTY_HEADER; HTTP_MAX_HEADERS];
    let mut req = httparse::Request::new(&mut headers_buf);
    match req.parse(&bytes) {
        Ok(httparse::Status::Complete(header_end)) => {
            let mut content_len: Option<usize> = None;
            for h in req.headers.iter() {
                if h.name.eq_ignore_ascii_case("content-length") {
                    let v = String::from_utf8_lossy(h.value);
                    content_len = v.trim().parse().ok();
                    break;
                }
            }
            let body_have = bytes.len().saturating_sub(header_end);
            match content_len {
                None => 1,
                Some(need) if body_have >= need => 1,
                Some(_) => 0,
            }
        }
        Ok(httparse::Status::Partial) | Err(_) => 0,
    }
}

/// Parse an HTTP/1.x request from a string or bytes handle.
///
/// Returns an anonymous struct handle with fields:
/// - `method` (string)
/// - `path` (string)
/// - `body` (string; bytes after the header block, UTF-8 lossy)
/// - `headers` (struct product: normalized name → string value; last wins)
///
/// Incomplete / invalid input → defaults (`GET`, `/`, body = raw text, empty headers).
///
/// # Safety
/// `raw` is 0 or a valid string/bytes handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_http_parse_request(raw: i64) -> i64 {
    let bytes = http_raw_bytes(raw);

    let mut headers_buf = [httparse::EMPTY_HEADER; HTTP_MAX_HEADERS];
    let mut req = httparse::Request::new(&mut headers_buf);

    let headers = echo_runtime_struct_new();
    let (method, path, body) = match req.parse(&bytes) {
        Ok(httparse::Status::Complete(n)) => {
            let method = req.method.unwrap_or("GET").to_string();
            let path = req.path.unwrap_or("/").to_string();
            let body = if n < bytes.len() {
                String::from_utf8_lossy(&bytes[n..]).into_owned()
            } else {
                String::new()
            };
            for h in req.headers.iter() {
                let name = normalize_header_name(h.name);
                let value = String::from_utf8_lossy(h.value).into_owned();
                struct_set_str(headers, &name, string_to_handle(value));
            }
            (method, path, body)
        }
        Ok(httparse::Status::Partial) | Err(_) => (
            "GET".into(),
            "/".into(),
            String::from_utf8_lossy(&bytes).into_owned(),
        ),
    };

    let out = echo_runtime_struct_new();
    struct_set_str(out, "method", string_to_handle(method));
    struct_set_str(out, "path", string_to_handle(path));
    struct_set_str(out, "body", string_to_handle(body));
    struct_set_str(out, "headers", headers);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_from_utf8_and_print_dispatch() {
        let s = "hello";
        let h = unsafe { echo_runtime_string_from_utf8(s.as_ptr(), s.len()) };
        assert_ne!(h, 0);
        // Magic header recognizes string (not a small int).
        assert!(unsafe { header_at(h) }.is_some());
        // Integers are not heap objects.
        assert!(unsafe { header_at(42) }.is_none());
    }

    /// Task workers allocate on other threads; `is_live_heap` must still see the handle
    /// (`json_stringify` and list-arg natives consult the live set, not a pointer probe).
    #[test]
    fn live_heap_is_visible_on_other_threads() {
        let h =
            std::thread::spawn(|| unsafe { echo_runtime_string_from_utf8(b"hello".as_ptr(), 5) })
                .join()
                .expect("alloc thread");
        assert!(
            is_live_heap(h),
            "handle allocated on a worker thread must stay live on the joiner"
        );
        assert_eq!(string_as_str(h), Some("hello"));
    }

    #[test]
    fn str_slice_and_contains() {
        let s = "abcdef";
        let h = unsafe { echo_runtime_string_from_utf8(s.as_ptr(), s.len()) };
        assert_eq!(
            string_data(echo_runtime_str_slice(h, 1, 4)).as_deref(),
            Some("bcd")
        );
        let needle = unsafe { echo_runtime_string_from_utf8(b"cd".as_ptr(), 2) };
        assert_eq!(echo_runtime_str_contains(h, needle), 1);
        let pref = unsafe { echo_runtime_string_from_utf8(b"ab".as_ptr(), 2) };
        assert_eq!(echo_runtime_str_starts_with(h, pref), 1);
        let suf = unsafe { echo_runtime_string_from_utf8(b"ef".as_ptr(), 2) };
        assert_eq!(echo_runtime_str_ends_with(h, suf), 1);
        assert_eq!(
            string_data(echo_runtime_str_slice(h, 0, 99)).as_deref(),
            Some("")
        );
        assert_eq!(echo_runtime_str_get(h, 0), i64::from(b'a'));
        assert_eq!(echo_runtime_str_get(h, 99), -1);
    }

    #[test]
    fn str_contains_starts_ends_empty_and_miss() {
        let h = unsafe { echo_runtime_string_from_utf8(b"hello".as_ptr(), 5) };
        let hit = unsafe { echo_runtime_string_from_utf8(b"ell".as_ptr(), 3) };
        let miss = unsafe { echo_runtime_string_from_utf8(b"xyz".as_ptr(), 3) };
        let empty = unsafe { echo_runtime_string_from_utf8(b"".as_ptr(), 0) };
        assert_eq!(echo_runtime_str_contains(h, hit), 1);
        assert_eq!(echo_runtime_str_contains(h, miss), 0);
        assert_eq!(echo_runtime_str_contains(h, empty), 1);
        let foo = unsafe { echo_runtime_string_from_utf8(b"foobar".as_ptr(), 6) };
        let pref = unsafe { echo_runtime_string_from_utf8(b"foo".as_ptr(), 3) };
        let suf = unsafe { echo_runtime_string_from_utf8(b"bar".as_ptr(), 3) };
        assert_eq!(echo_runtime_str_starts_with(foo, pref), 1);
        assert_eq!(echo_runtime_str_starts_with(foo, suf), 0);
        assert_eq!(echo_runtime_str_starts_with(foo, empty), 1);
        assert_eq!(echo_runtime_str_ends_with(foo, suf), 1);
        assert_eq!(echo_runtime_str_ends_with(foo, pref), 0);
        assert_eq!(echo_runtime_str_ends_with(foo, empty), 1);
        assert_eq!(echo_runtime_str_contains(0, hit), 0);
        assert_eq!(echo_runtime_str_starts_with(42, empty), 0);
        assert_eq!(echo_runtime_str_ends_with(42, empty), 0);
    }

    /// Byte indices are against the original payload. Lossy-decoding the whole
    /// buffer first expands `0xff` to U+FFFD (3 bytes) and shifts later indices.
    #[test]
    fn str_slice_indexes_original_bytes_not_lossy_expansion() {
        let raw = [0xff, b'A', b'B'];
        let h = unsafe { echo_runtime_bytes_from_ptr(raw.as_ptr(), raw.len()) };
        assert_eq!(echo_runtime_str_len(h), 3);
        let s = echo_runtime_str_slice(h, 1, 3);
        assert_eq!(
            string_as_str(s),
            Some("AB"),
            "slice [1,3) of [0xff, A, B] must be AB, not a window into U+FFFD"
        );
        let one = echo_runtime_str_slice(h, 1, 2);
        assert_eq!(string_as_str(one), Some("A"));
    }

    /// Prefix slice of a large string must be cheaper than cloning the haystack
    /// each time (`str_utf8` + `to_owned` used to copy all bytes).
    #[test]
    fn str_slice_prefix_cheaper_than_full_clone() {
        use std::time::Instant;
        let n = 1024 * 1024usize;
        let s = "x".repeat(n);
        let h = unsafe { echo_runtime_string_from_utf8(s.as_ptr(), s.len()) };
        let reps = 400u32;
        let t0 = Instant::now();
        for _ in 0..reps {
            assert_eq!(string_data(h).as_deref().map(str::len), Some(n));
        }
        let clone_dt = t0.elapsed();
        let t1 = Instant::now();
        for _ in 0..reps {
            let p = echo_runtime_str_slice(h, 0, 1);
            assert_eq!(string_as_str(p), Some("x"));
        }
        let slice_dt = t1.elapsed();
        assert!(
            slice_dt.as_nanos() < clone_dt.as_nanos() / 2 + 8_000_000,
            "str_slice {slice_dt:?} must beat full-payload clone {clone_dt:?}"
        );
    }

    #[test]
    fn bytes_slice_cat_from_str() {
        let raw = b"abcdef";
        let h = unsafe { echo_runtime_bytes_from_ptr(raw.as_ptr(), raw.len()) };
        let mid = echo_runtime_bytes_slice(h, 1, 4);
        assert_eq!(bytes_data(mid).as_deref(), Some(&b"bcd"[..]));
        let ab = echo_runtime_bytes_cat(
            unsafe { echo_runtime_bytes_from_ptr(b"A".as_ptr(), 1) },
            unsafe { echo_runtime_bytes_from_ptr(b"B".as_ptr(), 1) },
        );
        assert_eq!(bytes_data(ab).as_deref(), Some(&b"AB"[..]));
        let s = unsafe { echo_runtime_string_from_utf8(b"Hi".as_ptr(), 2) };
        assert_eq!(
            bytes_data(echo_runtime_bytes_from_str(s)).as_deref(),
            Some(&b"Hi"[..])
        );
    }

    #[test]
    fn bytes_slice_empty_range_and_invalid() {
        let h = unsafe { echo_runtime_bytes_from_ptr(b"abcdef".as_ptr(), 6) };
        assert_eq!(
            bytes_data(echo_runtime_bytes_slice(h, 6, 6)).as_deref(),
            Some(&b""[..])
        );
        assert_eq!(
            bytes_data(echo_runtime_bytes_slice(h, 0, 7)).as_deref(),
            Some(&b""[..])
        );
        assert_eq!(
            bytes_data(echo_runtime_bytes_slice(h, 4, 2)).as_deref(),
            Some(&b""[..])
        );
        assert_eq!(
            bytes_data(echo_runtime_bytes_slice(h, -1, 2)).as_deref(),
            Some(&b""[..])
        );
        assert_eq!(
            bytes_data(echo_runtime_bytes_slice(0, 0, 0)).as_deref(),
            Some(&b""[..])
        );
    }

    #[test]
    fn bytes_from_value_copies_bytes_string_and_int() {
        let raw = unsafe { echo_runtime_bytes_from_ptr(b"ab\xff".as_ptr(), 3) };
        assert_eq!(
            bytes_data(echo_runtime_bytes_from_value(raw)).as_deref(),
            Some(&b"ab\xff"[..])
        );
        let s = unsafe { echo_runtime_string_from_utf8(b"Hi".as_ptr(), 2) };
        assert_eq!(
            bytes_data(echo_runtime_bytes_from_value(s)).as_deref(),
            Some(&b"Hi"[..])
        );
        assert_eq!(
            bytes_data(echo_runtime_bytes_from_value(3)).as_deref(),
            Some(&b"3"[..])
        );
        let loc = unsafe { echo_runtime_locator_from_utf8(b"/tmp".as_ptr(), 4) };
        assert_eq!(
            bytes_data(echo_runtime_bytes_from_value(loc)).as_deref(),
            Some(&b"/tmp"[..])
        );
    }

    #[test]
    fn str_repeat_linear() {
        let s = unsafe { echo_runtime_string_from_utf8(b"ab".as_ptr(), 2) };
        let r = echo_runtime_str_repeat(s, 4);
        assert_eq!(string_data(r).as_deref(), Some("abababab"));
        assert_eq!(
            string_data(echo_runtime_str_repeat(s, 0)).as_deref(),
            Some("")
        );
        assert_eq!(
            string_data(echo_runtime_str_repeat(s, -1)).as_deref(),
            Some("")
        );
    }

    #[test]
    fn list_reserve_allows_push() {
        let h = echo_runtime_list_new();
        unsafe {
            echo_runtime_list_reserve(h, 100);
            for i in 0..100 {
                echo_runtime_list_push(h, i);
            }
            assert_eq!(echo_runtime_list_len(h), 100);
            assert_eq!(echo_runtime_list_get(h, 50), 50);
        }
    }

    #[test]
    fn list_new_empty_lists_n() {
        let h = echo_runtime_list_new_empty_lists(8);
        unsafe {
            assert_eq!(echo_runtime_list_len(h), 8);
            for i in 0..8 {
                let chain = echo_runtime_list_get(h, i);
                assert_ne!(chain, 0);
                assert_eq!(echo_runtime_list_len(chain), 0);
                // Distinct handles.
                if i > 0 {
                    assert_ne!(chain, echo_runtime_list_get(h, i - 1));
                }
            }
        }
    }

    /// Indexing must not clone the whole buffer per access (was O(n²) for scans).
    #[test]
    fn bytes_get_scans_without_full_clone() {
        let n = 4096usize;
        let raw = vec![0xABu8; n];
        let h = unsafe { echo_runtime_bytes_from_ptr(raw.as_ptr(), raw.len()) };
        assert_eq!(echo_runtime_bytes_len(h), n as i64);
        // Full linear scan — correctness; perf is covered by benches.
        let mut sum = 0i64;
        for i in 0..n as i64 {
            let b = echo_runtime_bytes_get(h, i);
            assert_eq!(b, 0xAB);
            sum += b;
        }
        assert_eq!(sum, (n as i64) * 0xAB);
        assert_eq!(echo_runtime_bytes_get(h, n as i64), -1);
        assert_eq!(echo_runtime_bytes_get(h, -1), -1);
        // Borrowed view must match owned copy API.
        assert_eq!(bytes_as_slice(h).map(|s| s.len()), Some(n));
        assert_eq!(bytes_data(h).as_deref(), Some(raw.as_slice()));
    }

    #[test]
    fn string_get_scans_without_full_clone() {
        let s = "x".repeat(2048);
        let h = unsafe { echo_runtime_string_from_utf8(s.as_ptr(), s.len()) };
        assert_eq!(echo_runtime_str_len(h), 2048);
        for i in 0..2048i64 {
            assert_eq!(echo_runtime_str_get(h, i), i64::from(b'x'));
        }
        assert_eq!(echo_runtime_str_get(h, 2048), -1);
    }

    #[test]
    fn reflect_kind_and_key_bytes() {
        assert_eq!(echo_runtime_reflect_kind(42), REFLECT_KIND_INT);
        assert_eq!(
            string_data(echo_runtime_reflect_kind_name(42)).as_deref(),
            Some("int")
        );
        let kb = echo_runtime_reflect_key_bytes(0x0102_i64);
        let raw = bytes_data(kb).expect("int key bytes");
        assert_eq!(raw[0], 0);
        assert_eq!(&raw[1..], &0x0102_i64.to_le_bytes());

        let s = "hi";
        let sh = unsafe { echo_runtime_string_from_utf8(s.as_ptr(), s.len()) };
        assert_eq!(echo_runtime_reflect_kind(sh), i64::from(KIND_STRING));
        assert_eq!(
            string_data(echo_runtime_reflect_kind_name(sh)).as_deref(),
            Some("string")
        );
        let sk = bytes_data(echo_runtime_reflect_key_bytes(sh)).expect("str key");
        assert_eq!(sk[0], KIND_STRING as u8);
        assert_eq!(&sk[1..], b"hi");

        let bh = echo_runtime_bytes_from_i64(7);
        assert_eq!(echo_runtime_reflect_kind(bh), i64::from(KIND_BYTES));
        let bk = bytes_data(echo_runtime_reflect_key_bytes(bh)).expect("bytes key");
        assert_eq!(bk[0], KIND_BYTES as u8);
        assert_eq!(&bk[1..], &7_i64.to_le_bytes());

        let list = echo_runtime_list_new();
        assert_eq!(echo_runtime_reflect_kind(list), i64::from(KIND_LIST));
        assert_eq!(
            string_data(echo_runtime_reflect_kind_name(list)).as_deref(),
            Some("list")
        );
    }

    #[test]
    fn bytes_from_i64_is_little_endian() {
        let packed = echo_runtime_bytes_from_i64(0x0102);
        assert_eq!(echo_runtime_bytes_len(packed), 8);
        assert_eq!(echo_runtime_bytes_get(packed, 0), 2);
        assert_eq!(echo_runtime_bytes_get(packed, 1), 1);
        assert_eq!(
            bytes_data(packed).as_deref(),
            Some(0x0102_i64.to_le_bytes().as_slice())
        );
    }

    #[test]
    fn bytes_from_str_copies_utf8_payload() {
        let hi = unsafe { echo_runtime_string_from_utf8(b"Hi".as_ptr(), 2) };
        assert_eq!(
            bytes_data(echo_runtime_bytes_from_str(hi)).as_deref(),
            Some(&b"Hi"[..])
        );
        let empty = unsafe { echo_runtime_string_from_utf8(b"".as_ptr(), 0) };
        assert_eq!(
            bytes_data(echo_runtime_bytes_from_str(empty)).as_deref(),
            Some(&b""[..])
        );
        let cafe = "café";
        let s = unsafe { echo_runtime_string_from_utf8(cafe.as_ptr(), cafe.len()) };
        assert_eq!(
            bytes_data(echo_runtime_bytes_from_str(s)).as_deref(),
            Some(cafe.as_bytes())
        );
        // Non-strings (incl. bytes handles) yield empty — not `bytes_from_value`.
        let raw = unsafe { echo_runtime_bytes_from_ptr(b"AB".as_ptr(), 2) };
        assert_eq!(
            bytes_data(echo_runtime_bytes_from_str(raw)).as_deref(),
            Some(&b""[..])
        );
        assert_eq!(
            bytes_data(echo_runtime_bytes_from_str(0)).as_deref(),
            Some(&b""[..])
        );
        assert_eq!(
            bytes_data(echo_runtime_bytes_from_str(42)).as_deref(),
            Some(&b""[..])
        );
    }

    #[test]
    fn str_from_int_and_float_roundtrip_handles() {
        let si = echo_runtime_str_from_int(42);
        assert!(string_data(si).as_deref() == Some("42"));
        let fh = echo_runtime_float_from_f64(3.5);
        assert_eq!(echo_runtime_float_to_f64(fh), 3.5);
        let sf = echo_runtime_str_from_float(fh);
        assert_eq!(string_data(sf).as_deref(), Some("3.5"));
    }

    #[test]
    fn str_from_duration_formats_units() {
        assert_eq!(
            string_data(echo_runtime_str_from_duration(5_000_000_000)).as_deref(),
            Some("5s")
        );
        assert_eq!(
            string_data(echo_runtime_str_from_duration(10_000_000)).as_deref(),
            Some("10ms")
        );
        assert_eq!(
            string_data(echo_runtime_str_from_duration(5_000_000_000 + 10_000_000)).as_deref(),
            Some("5010ms")
        );
        assert_eq!(
            string_data(echo_runtime_str_from_duration(0)).as_deref(),
            Some("0s")
        );
    }

    #[test]
    fn deep_eq_list_and_identity() {
        let a = echo_runtime_list_new();
        let b = echo_runtime_list_new();
        unsafe {
            echo_runtime_list_push(a, 1);
            echo_runtime_list_push(a, 2);
            echo_runtime_list_push(b, 1);
            echo_runtime_list_push(b, 2);
        }
        assert_eq!(echo_runtime_eq(a, b), 1, "deep list eq");
        assert_eq!(echo_runtime_eq_id(a, b), 0, "distinct list objects");
        assert_eq!(echo_runtime_eq_id(a, a), 1);
        let c = a;
        assert_eq!(echo_runtime_eq_id(a, c), 1);
    }

    #[test]
    fn deep_eq_struct_fields() {
        let a = echo_runtime_struct_new();
        let b = echo_runtime_struct_new();
        let x = b"x";
        unsafe {
            echo_runtime_struct_set(a, x.as_ptr(), x.len(), 3);
            echo_runtime_struct_set(b, x.as_ptr(), x.len(), 3);
        }
        assert_eq!(echo_runtime_eq(a, b), 1);
        assert_eq!(echo_runtime_eq_id(a, b), 0);
    }

    #[test]
    fn http_headers_complete_detects_end() {
        let partial = b"GET / HTTP/1.1\r\nHost: x";
        let full = b"GET / HTTP/1.1\r\nHost: x\r\n\r\n";
        unsafe {
            let p = echo_runtime_bytes_from_ptr(partial.as_ptr(), partial.len());
            let f = echo_runtime_bytes_from_ptr(full.as_ptr(), full.len());
            assert_eq!(echo_runtime_http_headers_complete(p), 0);
            assert_eq!(echo_runtime_http_headers_complete(f), 1);
            assert_eq!(echo_runtime_http_headers_complete(0), 0);
        }
    }

    /// Completeness checks run on every `handle_connection` read. Cloning the
    /// growing buffer each time is Θ(n) extra memcpy (same class as old `str_slice`).
    #[test]
    fn http_request_complete_cheaper_than_full_clone() {
        use std::time::Instant;
        let mut raw = b"GET / HTTP/1.1\r\nHost: x\r\n\r\n".to_vec();
        raw.resize(1024 * 1024, b'x');
        let h = unsafe { echo_runtime_bytes_from_ptr(raw.as_ptr(), raw.len()) };
        let reps = 1500u32;
        let t0 = Instant::now();
        for _ in 0..reps {
            assert_eq!(bytes_data(h).as_deref().map(<[u8]>::len), Some(raw.len()));
        }
        let clone_dt = t0.elapsed();
        let t1 = Instant::now();
        for _ in 0..reps {
            assert_eq!(unsafe { echo_runtime_http_request_complete(h) }, 1);
        }
        let complete_dt = t1.elapsed();
        assert!(
            complete_dt.as_nanos() < clone_dt.as_nanos() / 2 + 8_000_000,
            "http_request_complete {complete_dt:?} must beat full-payload clone {clone_dt:?}"
        );
    }

    #[test]
    fn http_parse_utf8_body_after_byte_concat() {
        let mut raw = b"POST /e HTTP/1.1\r\nContent-Length: 2\r\n\r\n".to_vec();
        raw.push(0xc3);
        raw.push(0xa9);
        unsafe {
            let h = echo_runtime_bytes_from_ptr(raw.as_ptr(), raw.len());
            assert_eq!(echo_runtime_http_request_complete(h), 1);
            let p = echo_runtime_http_parse_request(h);
            let body = b"body";
            let b = echo_runtime_struct_get(p, body.as_ptr(), body.len());
            assert_eq!(string_as_str(b), Some("é"));
        }
    }

    #[test]
    fn http_parse_more_than_64_headers() {
        let mut raw = b"GET /health HTTP/1.1\r\nHost: x\r\n".to_vec();
        for i in 0..70 {
            raw.extend(format!("X-H{i}: v\r\n").as_bytes());
        }
        raw.extend(b"\r\n");
        unsafe {
            let h = echo_runtime_bytes_from_ptr(raw.as_ptr(), raw.len());
            let p = echo_runtime_http_parse_request(h);
            let path = b"path";
            let pth = echo_runtime_struct_get(p, path.as_ptr(), path.len());
            assert_eq!(
                string_as_str(pth),
                Some("/health"),
                "httparse EMPTY_HEADER cap must not drop a well-formed request"
            );
            assert_eq!(echo_runtime_http_request_complete(h), 1);
        }
    }

    #[test]
    fn http_request_complete_respects_content_length() {
        let headers_only = b"POST /a HTTP/1.1\r\nContent-Length: 5\r\n\r\n";
        let short_body = b"POST /a HTTP/1.1\r\nContent-Length: 5\r\n\r\nhel";
        let full = b"POST /a HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let get = b"GET / HTTP/1.1\r\nHost: x\r\n\r\n";
        unsafe {
            let h = echo_runtime_bytes_from_ptr(headers_only.as_ptr(), headers_only.len());
            let s = echo_runtime_bytes_from_ptr(short_body.as_ptr(), short_body.len());
            let f = echo_runtime_bytes_from_ptr(full.as_ptr(), full.len());
            let g = echo_runtime_bytes_from_ptr(get.as_ptr(), get.len());
            assert_eq!(echo_runtime_http_request_complete(h), 0);
            assert_eq!(echo_runtime_http_request_complete(s), 0);
            assert_eq!(echo_runtime_http_request_complete(f), 1);
            assert_eq!(echo_runtime_http_request_complete(g), 1);
            assert_eq!(echo_runtime_http_headers_complete(h), 1);
        }
    }

    #[test]
    fn http_parse_request_get() {
        let raw = b"GET /health HTTP/1.1\r\nHost: xo\r\nContent-Type: text/plain\r\n\r\n";
        let h = unsafe { echo_runtime_string_from_utf8(raw.as_ptr(), raw.len()) };
        let p = unsafe { echo_runtime_http_parse_request(h) };
        let method = b"method";
        let path = b"path";
        let headers = b"headers";
        let host = b"host";
        let ctype = b"content_type";
        unsafe {
            let m = echo_runtime_struct_get(p, method.as_ptr(), method.len());
            let pth = echo_runtime_struct_get(p, path.as_ptr(), path.len());
            assert_eq!(string_data(m).as_deref(), Some("GET"));
            assert_eq!(string_data(pth).as_deref(), Some("/health"));
            let hdrs = echo_runtime_struct_get(p, headers.as_ptr(), headers.len());
            let hv = echo_runtime_struct_get(hdrs, host.as_ptr(), host.len());
            let ct = echo_runtime_struct_get(hdrs, ctype.as_ptr(), ctype.len());
            assert_eq!(string_data(hv).as_deref(), Some("xo"));
            assert_eq!(string_data(ct).as_deref(), Some("text/plain"));
        }
    }

    #[test]
    fn http_parse_request_post_body() {
        let raw = b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhello";
        let h = unsafe { echo_runtime_string_from_utf8(raw.as_ptr(), raw.len()) };
        let p = unsafe { echo_runtime_http_parse_request(h) };
        let method = b"method";
        let path = b"path";
        let body = b"body";
        unsafe {
            let m = echo_runtime_struct_get(p, method.as_ptr(), method.len());
            let pth = echo_runtime_struct_get(p, path.as_ptr(), path.len());
            let b = echo_runtime_struct_get(p, body.as_ptr(), body.len());
            assert_eq!(string_data(m).as_deref(), Some("POST"));
            assert_eq!(string_data(pth).as_deref(), Some("/echo"));
            assert_eq!(string_data(b).as_deref(), Some("hello"));
            assert_eq!(echo_runtime_http_request_complete(h), 1);
            let short = b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhel";
            let s = echo_runtime_string_from_utf8(short.as_ptr(), short.len());
            assert_eq!(echo_runtime_http_request_complete(s), 0);
        }
    }

    #[test]
    fn locator_from_utf8_and_str_roundtrip() {
        let path = "/home/user";
        let h = unsafe { echo_runtime_locator_from_utf8(path.as_ptr(), path.len()) };
        assert!(unsafe { header_at(h) }.is_some());
        assert_eq!(locator_data(h).as_deref(), Some(path));
        let s = echo_runtime_str_from_locator(h);
        assert_eq!(string_data(s).as_deref(), Some(path));
        assert_eq!(
            echo_runtime_eq(h, unsafe {
                echo_runtime_locator_from_utf8(path.as_ptr(), path.len())
            }),
            1
        );
    }

    #[test]
    fn locator_from_string_copies_text() {
        let path = "/tmp/echo";
        let s = unsafe { echo_runtime_string_from_utf8(path.as_ptr(), path.len()) };
        let loc = echo_runtime_locator_from_string(s);
        assert_eq!(locator_data(loc).as_deref(), Some(path));
        assert_eq!(echo_runtime_locator_class(loc), LOCATOR_CLASS_ABS);
        let empty = echo_runtime_locator_from_string(0);
        assert_eq!(locator_data(empty).as_deref(), Some(""));
        assert_eq!(echo_runtime_locator_class(empty), LOCATOR_CLASS_REL);
    }

    #[test]
    fn string_builder_push_value_appends_locator_text() {
        let loc = unsafe { echo_runtime_locator_from_utf8(b"/tmp".as_ptr(), 4) };
        let b = echo_runtime_string_builder_new();
        echo_runtime_string_builder_push_value(b, loc);
        unsafe { echo_runtime_string_builder_push_str(b, b"/x".as_ptr(), 2) };
        let s = echo_runtime_string_builder_finish(b);
        assert_eq!(string_data(s).as_deref(), Some("/tmp/x"));
    }

    #[test]
    fn locator_class_path_vs_uri() {
        assert_eq!(classify_locator_text(""), LOCATOR_CLASS_REL);
        assert_eq!(classify_locator_text("home/user"), LOCATOR_CLASS_REL);
        assert_eq!(classify_locator_text("mailto:x"), LOCATOR_CLASS_REL);
        assert_eq!(classify_locator_text("C:/Windows"), LOCATOR_CLASS_REL);
        assert_eq!(classify_locator_text("/home/user"), LOCATOR_CLASS_ABS);
        assert_eq!(classify_locator_text("/"), LOCATOR_CLASS_ABS);
        assert_eq!(classify_locator_text("http://xo.run"), LOCATOR_CLASS_URI);
        assert_eq!(classify_locator_text("HTTPS://X"), LOCATOR_CLASS_URI);
        assert_eq!(classify_locator_text("file:///tmp"), LOCATOR_CLASS_URI);
        assert_eq!(classify_locator_text("://x"), LOCATOR_CLASS_REL);
        assert_eq!(classify_locator_text("file:///tmp"), LOCATOR_CLASS_URI);
        assert_eq!(classify_locator_text("ftp://h"), LOCATOR_CLASS_URI);
        assert_eq!(classify_locator_text("."), LOCATOR_CLASS_REL);
        let loc = unsafe { echo_runtime_locator_from_utf8(b"http://a".as_ptr(), 8) };
        assert_eq!(echo_runtime_locator_class(loc), LOCATOR_CLASS_URI);
        let s = unsafe { echo_runtime_string_from_utf8(b"/tmp".as_ptr(), 4) };
        assert_eq!(echo_runtime_locator_class(s), LOCATOR_CLASS_ABS);
        assert_eq!(echo_runtime_locator_class(0), LOCATOR_CLASS_REL);
    }

    #[test]
    fn bytes_from_ptr_and_str_from_bytes() {
        let raw = b"raw\xff";
        let h = unsafe { echo_runtime_bytes_from_ptr(raw.as_ptr(), raw.len()) };
        assert!(unsafe { header_at(h) }.is_some());
        assert_eq!(bytes_data(h).as_deref(), Some(raw.as_slice()));
        let s = echo_runtime_str_from_bytes(h);
        // lossy UTF-8 for the invalid trail byte
        assert!(string_data(s).is_some());
        assert!(
            echo_runtime_eq(h, unsafe {
                echo_runtime_bytes_from_ptr(raw.as_ptr(), raw.len())
            }) == 1
        );
    }

    #[test]
    fn str_from_bytes_is_utf8_lossy() {
        let hi = unsafe { echo_runtime_bytes_from_ptr(b"hi".as_ptr(), 2) };
        assert_eq!(
            string_data(echo_runtime_str_from_bytes(hi)).as_deref(),
            Some("hi")
        );
        let empty = unsafe { echo_runtime_bytes_from_ptr(b"".as_ptr(), 0) };
        assert_eq!(
            string_data(echo_runtime_str_from_bytes(empty)).as_deref(),
            Some("")
        );
        let bad = unsafe { echo_runtime_bytes_from_ptr(b"\xff".as_ptr(), 1) };
        assert_eq!(
            string_data(echo_runtime_str_from_bytes(bad)).as_deref(),
            Some("\u{FFFD}")
        );
        let cafe = "café";
        let cb = unsafe { echo_runtime_bytes_from_ptr(cafe.as_ptr(), cafe.len()) };
        assert_eq!(
            string_data(echo_runtime_str_from_bytes(cb)).as_deref(),
            Some(cafe)
        );
        // Non-bytes yield empty — not a string copy.
        let s = unsafe { echo_runtime_string_from_utf8(b"Hi".as_ptr(), 2) };
        assert_eq!(
            string_data(echo_runtime_str_from_bytes(s)).as_deref(),
            Some("")
        );
        assert_eq!(
            string_data(echo_runtime_str_from_bytes(0)).as_deref(),
            Some("")
        );
        assert_eq!(
            string_data(echo_runtime_str_from_bytes(42)).as_deref(),
            Some("")
        );
    }

    #[test]
    fn list_has_magic_header() {
        let h = echo_runtime_list_new();
        assert!(unsafe { header_at(h) }.is_some());
        unsafe {
            echo_runtime_list_push(h, 1);
            echo_runtime_list_push(h, 2);
            assert_eq!(echo_runtime_list_len(h), 2);
            assert_eq!(echo_runtime_list_get(h, 0), 1);
            assert_eq!(echo_runtime_list_get(h, 1), 2);
        }
    }

    #[test]
    fn list_set_updates_element() {
        let h = echo_runtime_list_new();
        unsafe {
            echo_runtime_list_push(h, 1);
            echo_runtime_list_push(h, 2);
            echo_runtime_list_set(h, 0, 9);
            assert_eq!(echo_runtime_list_get(h, 0), 9);
            assert_eq!(echo_runtime_list_get(h, 1), 2);
            // OOB soft no-op
            echo_runtime_list_set(h, 99, 7);
            assert_eq!(echo_runtime_list_len(h), 2);
        }
    }

    #[test]
    fn struct_field_set_get() {
        let h = echo_runtime_struct_new();
        // type tag empty on anonymous
        assert!(unsafe { header_at(h) }.is_some());
        let x = b"x";
        let y = b"y";
        unsafe {
            echo_runtime_struct_set(h, x.as_ptr(), x.len(), 3);
            echo_runtime_struct_set(h, y.as_ptr(), y.len(), 4);
            assert_eq!(echo_runtime_struct_get(h, x.as_ptr(), x.len()), 3);
            assert_eq!(echo_runtime_struct_get(h, y.as_ptr(), y.len()), 4);
            echo_runtime_struct_set(h, x.as_ptr(), x.len(), 9);
            assert_eq!(echo_runtime_struct_get(h, x.as_ptr(), x.len()), 9);
        }
    }

    #[test]
    fn struct_type_tag_match() {
        let circle = b"circle";
        let rect = b"rect";
        unsafe {
            let named = echo_runtime_struct_new_named(circle.as_ptr(), circle.len());
            let anon = echo_runtime_struct_new();
            assert_eq!(
                echo_runtime_struct_type_is(named, circle.as_ptr(), circle.len()),
                1
            );
            assert_eq!(
                echo_runtime_struct_type_is(named, rect.as_ptr(), rect.len()),
                0
            );
            assert_eq!(
                echo_runtime_struct_type_is(anon, circle.as_ptr(), circle.len()),
                0
            );
            assert_eq!(
                echo_runtime_struct_type_is(0, circle.as_ptr(), circle.len()),
                0
            );
        }
    }

    #[test]
    fn now_ms_after_2020() {
        let t = echo_runtime_now_ms();
        assert!(t > 1_500_000_000_000, "{t}");
    }

    #[test]
    fn sleep_ms_nonpositive_is_noop() {
        let start = std::time::Instant::now();
        echo_runtime_sleep_ms(0);
        echo_runtime_sleep_ms(-1);
        assert!(
            start.elapsed().as_millis() < 50,
            "sleep_ms ≤ 0 should not block"
        );
    }

    #[test]
    fn print_capture_routes_away_from_stdout() {
        let s = echo_runtime_str_from_int(42);
        let ((), out) = with_print_capture(|| {
            echo_runtime_print_i64(s);
        });
        assert_eq!(out.trim(), "42");
    }

    #[test]
    fn eprint_writes_strings_and_ignores_non_strings() {
        let s = echo_runtime_str_from_int(7);
        let (((), err), out) = with_print_capture(|| {
            with_eprint_capture(|| {
                echo_runtime_eprint(s);
                echo_runtime_eprint(123);
                echo_runtime_print_i64(s);
            })
        });
        assert_eq!(err, "7\n");
        assert_eq!(out, "7\n");
    }

    #[test]
    fn str_from_debug_formats_struct_and_string() {
        let h = echo_runtime_struct_new();
        let x = b"x";
        unsafe {
            echo_runtime_struct_set(h, x.as_ptr(), x.len(), 3);
        }
        let dbg = echo_runtime_str_from_debug(h);
        let text = string_data(dbg).expect("string");
        assert!(text.contains("x") && text.contains('3'), "{text}");

        let s = echo_runtime_str_from_int(0); // wrong - use utf8
        let pure = unsafe { echo_runtime_string_from_utf8(b"hi".as_ptr(), 2) };
        let ds = string_data(echo_runtime_str_from_debug(pure)).expect("s");
        assert_eq!(ds, "\"hi\"");
        let _ = s;

        let list = echo_runtime_list_new();
        unsafe {
            echo_runtime_list_push(list, 1);
            echo_runtime_list_push(list, 2);
        }
        let dl = string_data(echo_runtime_str_from_debug(list)).expect("list");
        assert_eq!(dl, "[1, 2]");
    }
}
