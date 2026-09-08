//! Stable ABI symbols between codegen and `echo_runtime`.
//!
//! Both AOT (clang link) and JIT must call the same `echo_runtime_*` names.
//! See `docs/runtime-abi.md` and ADR 0004.

#![forbid(unsafe_code)]

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// Program entry emitted by codegen (returns process status as i64).
pub const ECHO_ENTRY: &str = "echo_entry";

/// C `main` wrapper that truncates `echo_entry` to i32.
pub const C_MAIN: &str = "main";

/// Abort the process with a UTF-8 message (`ptr`, `len`).
pub const RT_ABORT: &str = "echo_runtime_abort";

/// Print a heap value (string / list / struct) followed by newline.
/// Bare integers and floats are not printed — convert with `str_from_*` first.
pub const RT_PRINT_I64: &str = "echo_runtime_print_i64";

/// Print a string handle to stderr followed by newline (`std/io.eprint`).
/// Non-strings are ignored, same as [`RT_PRINT_I64`].
pub const RT_EPRINT: &str = "echo_runtime_eprint";

/// `echo_runtime_stdin_read() -> bytes handle` — whole process stdin until EOF.
/// Empty stdin is a live empty bytes value; `0` only on I/O error.
pub const RT_STDIN_READ: &str = "echo_runtime_stdin_read";

/// `echo_runtime_string_from_utf8(ptr, len) -> handle`
pub const RT_STRING_FROM_UTF8: &str = "echo_runtime_string_from_utf8";

/// `echo_runtime_str_from_int(i64) -> string handle`
pub const RT_STR_FROM_INT: &str = "echo_runtime_str_from_int";

/// `echo_runtime_str_from_float(f64) -> string handle`
pub const RT_STR_FROM_FLOAT: &str = "echo_runtime_str_from_float";

/// `echo_runtime_bytes_from_ptr(ptr, len) -> bytes handle`
pub const RT_BYTES_FROM_PTR: &str = "echo_runtime_bytes_from_ptr";

/// `echo_runtime_str_from_bytes(bytes_handle) -> string handle` (UTF-8 lossy)
pub const RT_STR_FROM_BYTES: &str = "echo_runtime_str_from_bytes";

/// `echo_runtime_str_from_duration(i64 nanos) -> string handle`
pub const RT_STR_FROM_DURATION: &str = "echo_runtime_str_from_duration";

/// `echo_runtime_locator_from_utf8(ptr, len) -> locator handle`
pub const RT_LOCATOR_FROM_UTF8: &str = "echo_runtime_locator_from_utf8";

/// `echo_runtime_locator_from_string(string_handle) -> locator handle`
pub const RT_LOCATOR_FROM_STRING: &str = "echo_runtime_locator_from_string";

/// `echo_runtime_str_from_locator(locator_handle) -> string handle`
pub const RT_STR_FROM_LOCATOR: &str = "echo_runtime_str_from_locator";

/// `echo_runtime_locator_class(string_or_locator) -> i64` 0 rel / 1 abs / 2 uri
pub const RT_LOCATOR_CLASS: &str = "echo_runtime_locator_class";

/// `echo_runtime_str_from_debug(any_i64) -> string handle` — shallow debug text
/// for REPL / diagnostics (structs, lists, strings, floats, bare ints).
pub const RT_STR_FROM_DEBUG: &str = "echo_runtime_str_from_debug";

/// `echo_runtime_str_len(string_or_bytes) -> i64` UTF-8/byte length.
pub const RT_STR_LEN: &str = "echo_runtime_str_len";

/// `echo_runtime_bytes_len(bytes) -> i64` — length of a bytes handle only.
pub const RT_BYTES_LEN: &str = "echo_runtime_bytes_len";

/// `echo_runtime_bytes_get(bytes, index) -> i64` — byte 0..255, or -1 if OOB/invalid.
pub const RT_BYTES_GET: &str = "echo_runtime_bytes_get";

/// `echo_runtime_bytes_from_i64(n) -> bytes` — 8 little-endian bytes of `n`.
pub const RT_BYTES_FROM_I64: &str = "echo_runtime_bytes_from_i64";

/// `echo_runtime_bytes_slice(b, start, end) -> bytes` — byte range `[start, end)`.
pub const RT_BYTES_SLICE: &str = "echo_runtime_bytes_slice";

/// `echo_runtime_bytes_cat(a, b) -> bytes` — concatenate two bytes values.
pub const RT_BYTES_CAT: &str = "echo_runtime_bytes_cat";

/// `echo_runtime_bytes_from_str(string) -> bytes` — UTF-8 payload copy.
pub const RT_BYTES_FROM_STR: &str = "echo_runtime_bytes_from_str";

/// `echo_runtime_bytes_from_value(any) -> bytes` — interp a `{name}` into bytes.
pub const RT_BYTES_FROM_VALUE: &str = "echo_runtime_bytes_from_value";

/// `echo_runtime_str_get(s, index) -> i64` — UTF-8 byte 0..255, or -1 if OOB.
pub const RT_STR_GET: &str = "echo_runtime_str_get";

/// `echo_runtime_reflect_kind(v) -> i64` — runtime kind code (0=int, heap kinds match header).
pub const RT_REFLECT_KIND: &str = "echo_runtime_reflect_kind";

/// `echo_runtime_reflect_kind_name(v) -> string` — stable kind name (`"int"`, `"string"`, …).
pub const RT_REFLECT_KIND_NAME: &str = "echo_runtime_reflect_kind_name";

/// `echo_runtime_reflect_key_bytes(v) -> bytes` — kind-tagged key material for hashing.
pub const RT_REFLECT_KEY_BYTES: &str = "echo_runtime_reflect_key_bytes";

/// `echo_runtime_str_cat(a, b) -> string handle` — concatenate two strings (or string+bytes).
pub const RT_STR_CAT: &str = "echo_runtime_str_cat";

/// `echo_runtime_str_slice(s, start, end) -> string` — UTF-8 byte range `[start, end)`.
pub const RT_STR_SLICE: &str = "echo_runtime_str_slice";

/// `echo_runtime_str_contains(hay, needle) -> i64` — 1/0 substring match.
pub const RT_STR_CONTAINS: &str = "echo_runtime_str_contains";

/// `echo_runtime_str_starts_with(s, prefix) -> i64` — 1/0.
pub const RT_STR_STARTS_WITH: &str = "echo_runtime_str_starts_with";

/// `echo_runtime_str_ends_with(s, suffix) -> i64` — 1/0.
pub const RT_STR_ENDS_WITH: &str = "echo_runtime_str_ends_with";

/// `echo_runtime_str_repeat(s, n) -> string` — `s` concatenated `n` times (O(n·|s|)).
pub const RT_STR_REPEAT: &str = "echo_runtime_str_repeat";

/// `echo_runtime_list_reserve(list, additional) -> void` — reserve capacity for push.
pub const RT_LIST_RESERVE: &str = "echo_runtime_list_reserve";

/// `echo_runtime_list_new_empty_lists(n) -> list` — outer list of `n` fresh empty lists.
pub const RT_LIST_NEW_EMPTY_LISTS: &str = "echo_runtime_list_new_empty_lists";

/// Box an `f64` as a heap float handle (`i64` bits).
pub const RT_FLOAT_FROM_F64: &str = "echo_runtime_float_from_f64";

/// Unbox a heap float handle (or raw bitcast fallback) to `f64`.
pub const RT_FLOAT_TO_F64: &str = "echo_runtime_float_to_f64";

/// `echo_runtime_eq(a, b) -> i64` (1/0); **deep** content equality.
pub const RT_EQ: &str = "echo_runtime_eq";

/// `echo_runtime_ne(a, b) -> i64` (1/0); deep inequality.
pub const RT_NE: &str = "echo_runtime_ne";

/// `echo_runtime_eq_id(a, b) -> i64` (1/0); **identity** (handle/bit equality).
pub const RT_EQ_ID: &str = "echo_runtime_eq_id";

/// `echo_runtime_ne_id(a, b) -> i64` (1/0); identity inequality.
pub const RT_NE_ID: &str = "echo_runtime_ne_id";

/// Rich-string interpolation builder.
pub const RT_STR_BUILDER_NEW: &str = "echo_runtime_string_builder_new";
pub const RT_STR_BUILDER_PUSH_STR: &str = "echo_runtime_string_builder_push_str";
pub const RT_STR_BUILDER_PUSH_VALUE: &str = "echo_runtime_string_builder_push_value";
pub const RT_STR_BUILDER_FINISH: &str = "echo_runtime_string_builder_finish";

/// Allocate empty list; returns handle as i64 (pointer bits).
pub const RT_LIST_NEW: &str = "echo_runtime_list_new";

/// `echo_runtime_list_push(list, value)`
pub const RT_LIST_PUSH: &str = "echo_runtime_list_push";

/// `echo_runtime_list_len(list) -> i64`
pub const RT_LIST_LEN: &str = "echo_runtime_list_len";

/// `echo_runtime_list_get(list, index) -> i64` (checked soft OOB → 0)
pub const RT_LIST_GET: &str = "echo_runtime_list_get";

/// `echo_runtime_list_set(list, index, value)` (soft OOB → no-op)
pub const RT_LIST_SET: &str = "echo_runtime_list_set";
/// Inclusive range `lo..hi` handle.
pub const RT_RANGE_NEW: &str = "echo_runtime_range_new";
/// First-class function value: code pointer + ret shape.
pub const RT_FN_NEW: &str = "echo_runtime_fn_new";
pub const RT_FN_CODE: &str = "echo_runtime_fn_code";
pub const RT_FN_SHAPE: &str = "echo_runtime_fn_shape";

/// `echo_runtime_http_parse_request(raw_string_or_bytes) -> struct handle`
pub const RT_HTTP_PARSE_REQUEST: &str = "echo_runtime_http_parse_request";

/// `echo_runtime_http_headers_complete(raw) -> i64` — 1 if `\r\n\r\n` present.
pub const RT_HTTP_HEADERS_COMPLETE: &str = "echo_runtime_http_headers_complete";
/// `echo_runtime_http_request_complete(raw) -> i64` — 1 if headers + body (Content-Length) ready.
pub const RT_HTTP_REQUEST_COMPLETE: &str = "echo_runtime_http_request_complete";

/// TCP/UDP (`std/net`) — OS sockets via `echo_runtime` net module.
pub const RT_TCP_LISTEN: &str = "echo_runtime_tcp_listen";
pub const RT_TCP_ACCEPT: &str = "echo_runtime_tcp_accept";
pub const RT_TCP_CONNECT: &str = "echo_runtime_tcp_connect";
pub const RT_TCP_READ: &str = "echo_runtime_tcp_read";
pub const RT_TCP_WRITE: &str = "echo_runtime_tcp_write";
pub const RT_TCP_CLOSE: &str = "echo_runtime_tcp_close";
pub const RT_UDP_BIND: &str = "echo_runtime_udp_bind";
pub const RT_UDP_SEND_TO: &str = "echo_runtime_udp_send_to";
pub const RT_UDP_RECV_FROM: &str = "echo_runtime_udp_recv_from";
pub const RT_UDP_CLOSE: &str = "echo_runtime_udp_close";

/// Tasks / mio event loop (ADR 0013).
/// `task_spawn_entry(code_ptr, shape) -> handle` — shape 0 plain / 1 result / 2 option.
pub const RT_TASK_SPAWN_ENTRY: &str = "echo_runtime_task_spawn_entry";
/// `task_spawn_args(code, shape, argc, a0..a7) -> handle`
pub const RT_TASK_SPAWN_ARGS: &str = "echo_runtime_task_spawn_args";
/// Fail process end if unjoined tasks remain.
pub const RT_TASK_CHECK_JOINED: &str = "echo_runtime_task_check_joined";
/// `task_join(handle) -> i64` — low 64 bits of packed result.
pub const RT_TASK_JOIN: &str = "echo_runtime_task_join";
/// `task_join_wide(handle) -> i128` — full pack for result/option.
pub const RT_TASK_JOIN_WIDE: &str = "echo_runtime_task_join_wide";
/// `task_block(code_ptr, shape) -> i64` — spawn + join (plain / low bits).
pub const RT_TASK_BLOCK: &str = "echo_runtime_task_block";
/// `task_block_wide(code_ptr, shape) -> i128`.
pub const RT_TASK_BLOCK_WIDE: &str = "echo_runtime_task_block_wide";
/// `task_shape(handle) -> i64` shape code.
pub const RT_TASK_SHAPE: &str = "echo_runtime_task_shape";

/// Allocate empty anonymous struct; returns handle as i64 (pointer bits).
pub const RT_STRUCT_NEW: &str = "echo_runtime_struct_new";

/// `echo_runtime_struct_new_named(name_ptr, name_len) -> i64` — tagged `% Shape` lit.
pub const RT_STRUCT_NEW_NAMED: &str = "echo_runtime_struct_new_named";

/// `echo_runtime_struct_type_is(handle, name_ptr, name_len) -> i64` — 1 if type tag matches.
pub const RT_STRUCT_TYPE_IS: &str = "echo_runtime_struct_type_is";

/// `echo_runtime_struct_set(handle, name_ptr, name_len, value)`
pub const RT_STRUCT_SET: &str = "echo_runtime_struct_set";

/// `echo_runtime_struct_get(handle, name_ptr, name_len) -> i64`
pub const RT_STRUCT_GET: &str = "echo_runtime_struct_get";

// --- Scope-owned memory (ADR 0016) ---
/// `void echo_runtime_scope_enter(int64_t scope_id)`
pub const RT_SCOPE_ENTER: &str = "echo_runtime_scope_enter";
/// `void echo_runtime_scope_exit(int64_t scope_id)`
pub const RT_SCOPE_EXIT: &str = "echo_runtime_scope_exit";
/// `void echo_runtime_scope_register(int64_t handle)`
pub const RT_SCOPE_REGISTER: &str = "echo_runtime_scope_register";
/// `void echo_runtime_scope_promote(int64_t handle, int64_t target_scope_id)`
/// Graph promotion: rehomes root and every reachable alloc still owned by root's source frame.
pub const RT_SCOPE_PROMOTE: &str = "echo_runtime_scope_promote";
/// Same as [`RT_SCOPE_PROMOTE`] (explicit graph name for docs / optional dual bind).
pub const RT_SCOPE_PROMOTE_GRAPH: &str = "echo_runtime_scope_promote_graph";
/// `void echo_runtime_scope_disown(int64_t handle)`
pub const RT_SCOPE_DISOWN: &str = "echo_runtime_scope_disown";
/// `void echo_runtime_scope_release(int64_t handle)`
pub const RT_SCOPE_RELEASE: &str = "echo_runtime_scope_release";
/// `void echo_runtime_scope_enqueue_release(int64_t handle)`
pub const RT_SCOPE_ENQUEUE_RELEASE: &str = "echo_runtime_scope_enqueue_release";
/// `void echo_runtime_scope_drain_deferred(void)`
pub const RT_SCOPE_DRAIN_DEFERRED: &str = "echo_runtime_scope_drain_deferred";

/// `void echo_runtime_test_register(int64_t name_str, int64_t fn_value)`
pub const RT_TEST_REGISTER: &str = "echo_runtime_test_register";
/// `void echo_runtime_test_bench_register(int64_t name_str, int64_t fn_value)`
pub const RT_TEST_BENCH_REGISTER: &str = "echo_runtime_test_bench_register";
/// `void echo_runtime_test_fail(int64_t msg_str)`
pub const RT_TEST_FAIL: &str = "echo_runtime_test_fail";
/// `int64_t echo_runtime_test_finish(void)` — fail count, or -1 if suite mode off
pub const RT_TEST_FINISH: &str = "echo_runtime_test_finish";

/// `echo_runtime_now_ms() -> i64` — wall clock ms since Unix epoch.
pub const RT_NOW_MS: &str = "echo_runtime_now_ms";
/// `echo_runtime_sleep_ms(i64)` — sleep at least `ms` milliseconds (void).
pub const RT_SLEEP_MS: &str = "echo_runtime_sleep_ms";

// --- Process / env / spawn (`std/process`) ---
/// `echo_runtime_process_args() -> list` — argv as list of strings.
pub const RT_PROCESS_ARGS: &str = "echo_runtime_process_args";
/// `echo_runtime_process_env_has(name_str) -> i64` — 1/0.
pub const RT_PROCESS_ENV_HAS: &str = "echo_runtime_process_env_has";
/// `echo_runtime_process_env_get(name_str) -> string` — empty if unset.
pub const RT_PROCESS_ENV_GET: &str = "echo_runtime_process_env_get";
/// `echo_runtime_process_env_set(name_str, value_str)` void.
pub const RT_PROCESS_ENV_SET: &str = "echo_runtime_process_env_set";
/// `echo_runtime_process_env_unset(name_str)` void.
pub const RT_PROCESS_ENV_UNSET: &str = "echo_runtime_process_env_unset";
/// `echo_runtime_process_exit(code)` void — terminates process.
pub const RT_PROCESS_EXIT: &str = "echo_runtime_process_exit";
/// `echo_runtime_process_run(program_str, args_list) -> i64` — exit code, or -1 spawn fail.
pub const RT_PROCESS_RUN: &str = "echo_runtime_process_run";

// --- Filesystem (`std/fs`) ---
/// `fs_exists(path) -> i64` 1/0 — path is string or locator.
pub const RT_FS_EXISTS: &str = "echo_runtime_fs_exists";
pub const RT_FS_IS_FILE: &str = "echo_runtime_fs_is_file";
pub const RT_FS_IS_DIR: &str = "echo_runtime_fs_is_dir";
/// `fs_join(base, rel) -> string`
pub const RT_FS_JOIN: &str = "echo_runtime_fs_join";
/// `fs_read(path) -> bytes` — 0 on failure.
pub const RT_FS_READ: &str = "echo_runtime_fs_read";
/// `fs_write(path, data) -> i64` — 0 ok, -1 fail; data is bytes or string.
pub const RT_FS_WRITE: &str = "echo_runtime_fs_write";
/// `fs_remove(path) -> i64` — remove file; 0 ok, -1 fail.
pub const RT_FS_REMOVE: &str = "echo_runtime_fs_remove";
pub const RT_FS_CREATE_DIR: &str = "echo_runtime_fs_create_dir";
pub const RT_FS_CREATE_DIR_ALL: &str = "echo_runtime_fs_create_dir_all";
/// `fs_read_dir(path) -> list` of name strings; 0 on failure.
pub const RT_FS_READ_DIR: &str = "echo_runtime_fs_read_dir";
/// `fs_remove_dir(path) -> i64` — empty dir only; 0 ok, -1 fail.
pub const RT_FS_REMOVE_DIR: &str = "echo_runtime_fs_remove_dir";
/// `fs_copy(from, to) -> i64` — 0 ok, -1 fail.
pub const RT_FS_COPY: &str = "echo_runtime_fs_copy";
/// `fs_rename(from, to) -> i64` — 0 ok, -1 fail.
pub const RT_FS_RENAME: &str = "echo_runtime_fs_rename";
/// `fs_metadata(path) -> struct meta` — 0 on fail (`len`, `is_file`, …).
pub const RT_FS_METADATA: &str = "echo_runtime_fs_metadata";
/// Open file handles (streaming).
pub const RT_FS_OPEN_READ: &str = "echo_runtime_fs_open_read";
pub const RT_FS_OPEN_WRITE: &str = "echo_runtime_fs_open_write";
pub const RT_FS_OPEN_APPEND: &str = "echo_runtime_fs_open_append";
/// `fs_file_read(handle, limit) -> bytes` — 0 error; empty bytes = EOF.
pub const RT_FS_FILE_READ: &str = "echo_runtime_fs_file_read";
/// `fs_file_write(handle, data) -> i64` — 0 ok, -1 fail.
pub const RT_FS_FILE_WRITE: &str = "echo_runtime_fs_file_write";
/// `fs_file_seek(handle, pos) -> i64` — new pos, or -1 fail.
pub const RT_FS_FILE_SEEK: &str = "echo_runtime_fs_file_seek";
/// `fs_file_close(handle)` void.
pub const RT_FS_FILE_CLOSE: &str = "echo_runtime_fs_file_close";

// --- Math / random / os / json / dns / crypto / str helpers (expansive std) ---
pub const RT_MATH_SQRT: &str = "echo_runtime_math_sqrt";
pub const RT_MATH_SIN: &str = "echo_runtime_math_sin";
pub const RT_MATH_COS: &str = "echo_runtime_math_cos";
pub const RT_MATH_TAN: &str = "echo_runtime_math_tan";
pub const RT_MATH_FLOOR: &str = "echo_runtime_math_floor";
pub const RT_MATH_CEIL: &str = "echo_runtime_math_ceil";
pub const RT_MATH_ABS_F: &str = "echo_runtime_math_abs_f";
pub const RT_MATH_POW: &str = "echo_runtime_math_pow";
pub const RT_MATH_ABS_I: &str = "echo_runtime_math_abs_i";
pub const RT_RANDOM_SEED: &str = "echo_runtime_random_seed";
pub const RT_RANDOM_U64: &str = "echo_runtime_random_u64";
pub const RT_RANDOM_FLOAT: &str = "echo_runtime_random_float";
pub const RT_CRYPTO_RANDOM_BYTES: &str = "echo_runtime_crypto_random_bytes";
pub const RT_CRYPTO_RANDOM_U64: &str = "echo_runtime_crypto_random_u64";
pub const RT_OS_PID: &str = "echo_runtime_os_pid";
pub const RT_OS_CWD: &str = "echo_runtime_os_cwd";
pub const RT_OS_CHDIR: &str = "echo_runtime_os_chdir";
pub const RT_OS_HOSTNAME: &str = "echo_runtime_os_hostname";
pub const RT_OS_PLATFORM: &str = "echo_runtime_os_platform";
pub const RT_NOW_MONO_MS: &str = "echo_runtime_now_mono_ms";
pub const RT_JSON_PARSE: &str = "echo_runtime_json_parse";
pub const RT_JSON_STRINGIFY: &str = "echo_runtime_json_stringify";
pub const RT_DNS_LOOKUP: &str = "echo_runtime_dns_lookup";
pub const RT_SHA256: &str = "echo_runtime_sha256";
pub const RT_PROCESS_RUN_CAPTURE: &str = "echo_runtime_process_run_capture";
pub const RT_FS_TEMP_DIR: &str = "echo_runtime_fs_temp_dir";
pub const RT_FS_CREATE_TEMP: &str = "echo_runtime_fs_create_temp";
pub const RT_FS_SYMLINK: &str = "echo_runtime_fs_symlink";
pub const RT_STR_TO_LOWER: &str = "echo_runtime_str_to_lower";
pub const RT_STR_TO_UPPER: &str = "echo_runtime_str_to_upper";
pub const RT_STR_TRIM: &str = "echo_runtime_str_trim";
pub const RT_STR_SPLIT: &str = "echo_runtime_str_split";
pub const RT_STR_REPLACE: &str = "echo_runtime_str_replace";

pub const RT_HEX_ENCODE: &str = "echo_runtime_hex_encode";
pub const RT_HEX_DECODE: &str = "echo_runtime_hex_decode";
pub const RT_BASE64_ENCODE: &str = "echo_runtime_base64_encode";
pub const RT_BASE64_DECODE: &str = "echo_runtime_base64_decode";
pub const RT_UTF8_VALID: &str = "echo_runtime_utf8_valid";
pub const RT_UTF8_DECODE: &str = "echo_runtime_utf8_decode";

pub const RT_TLS_LISTEN: &str = "echo_runtime_tls_listen";
pub const RT_TLS_ACCEPT: &str = "echo_runtime_tls_accept";
pub const RT_TLS_CONNECT: &str = "echo_runtime_tls_connect";
pub const RT_TLS_READ: &str = "echo_runtime_tls_read";
pub const RT_TLS_WRITE: &str = "echo_runtime_tls_write";
pub const RT_TLS_CLOSE: &str = "echo_runtime_tls_close";
pub const RT_TLS_CLOSE_LISTENER: &str = "echo_runtime_tls_close_listener";

// --- P0/P1/P2 expansive std (parse, time, compress, crypto, path, unix) ---
pub const RT_PARSE_I64: &str = "echo_runtime_parse_i64";
pub const RT_PARSE_F64: &str = "echo_runtime_parse_f64";
pub const RT_URL_PARSE: &str = "echo_runtime_url_parse";
pub const RT_TIME_FORMAT: &str = "echo_runtime_time_format";
pub const RT_TIME_PARSE: &str = "echo_runtime_time_parse";
pub const RT_GZIP_COMPRESS: &str = "echo_runtime_gzip_compress";
pub const RT_GZIP_DECOMPRESS: &str = "echo_runtime_gzip_decompress";
pub const RT_ZIP_PACK: &str = "echo_runtime_zip_pack";
pub const RT_ZIP_UNPACK_FIRST: &str = "echo_runtime_zip_unpack_first";
pub const RT_HMAC_SHA256: &str = "echo_runtime_hmac_sha256";
pub const RT_SHA512: &str = "echo_runtime_sha512";
pub const RT_AES_GCM_ENCRYPT: &str = "echo_runtime_aes_gcm_encrypt";
pub const RT_AES_GCM_DECRYPT: &str = "echo_runtime_aes_gcm_decrypt";
pub const RT_FS_CHMOD: &str = "echo_runtime_fs_chmod";
pub const RT_PATH_CLEAN: &str = "echo_runtime_path_clean";
pub const RT_PATH_REL: &str = "echo_runtime_path_rel";
pub const RT_PROCESS_RUN_CWD: &str = "echo_runtime_process_run_cwd";
pub const RT_PROCESS_SPAWN_PIPES: &str = "echo_runtime_process_spawn_pipes";
pub const RT_PROCESS_PIPE_WRITE: &str = "echo_runtime_process_pipe_write";
pub const RT_PROCESS_PIPE_READ: &str = "echo_runtime_process_pipe_read";
pub const RT_PROCESS_PIPE_CLOSE: &str = "echo_runtime_process_pipe_close";
pub const RT_PROCESS_WAIT: &str = "echo_runtime_process_wait";
pub const RT_UNIX_LISTEN: &str = "echo_runtime_unix_listen";
pub const RT_UNIX_ACCEPT: &str = "echo_runtime_unix_accept";
pub const RT_UNIX_CONNECT: &str = "echo_runtime_unix_connect";
pub const RT_UNIX_READ: &str = "echo_runtime_unix_read";
pub const RT_UNIX_WRITE: &str = "echo_runtime_unix_write";
pub const RT_UNIX_CLOSE: &str = "echo_runtime_unix_close";

/// Object store (memory / S3-compatible). Returns `{ status, body, etag }`.
pub const RT_STORE_MEMORY_NEW: &str = "echo_runtime_store_memory_new";
pub const RT_STORE_S3_NEW: &str = "echo_runtime_store_s3_new";
pub const RT_STORE_CLOSE: &str = "echo_runtime_store_close";
pub const RT_STORE_GET: &str = "echo_runtime_store_get";
pub const RT_STORE_GET_IF_NONE_MATCH: &str = "echo_runtime_store_get_if_none_match";
pub const RT_STORE_GET_RANGE: &str = "echo_runtime_store_get_range";
pub const RT_STORE_HEAD: &str = "echo_runtime_store_head";
pub const RT_STORE_PUT: &str = "echo_runtime_store_put";
pub const RT_STORE_CAS: &str = "echo_runtime_store_cas";
pub const RT_STORE_CAS_CREATE: &str = "echo_runtime_store_cas_create";
pub const RT_STORE_KEYS: &str = "echo_runtime_store_keys";

/// Git pack / smart-HTTP body helpers (`gix-pack` on host-io).
pub const RT_GIT_PACK_VALID: &str = "echo_runtime_git_pack_valid";
pub const RT_GIT_PACK_COUNT: &str = "echo_runtime_git_pack_count";
pub const RT_GIT_INDEX_PACK: &str = "echo_runtime_git_index_pack";
pub const RT_GIT_RECEIVE_PACK_SPLIT: &str = "echo_runtime_git_receive_pack_split";
pub const RT_GIT_UPLOAD_PACK_WANTS: &str = "echo_runtime_git_upload_pack_wants";
