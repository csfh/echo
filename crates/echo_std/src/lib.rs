//! Standard-library and runtime-primitive package facts.
//!
//! Authority: `docs/stdlib.md`. The `/ runtime` package is virtual (no `.echo`
//! tree); exports map to `echo_runtime_*` via [`echo_codegen_abi`].

#![forbid(unsafe_code)]

use echo_codegen_abi::{
    RT_AES_GCM_DECRYPT, RT_AES_GCM_ENCRYPT, RT_BASE64_DECODE, RT_BASE64_ENCODE,
    RT_CRYPTO_RANDOM_BYTES, RT_CRYPTO_RANDOM_U64, RT_DNS_LOOKUP, RT_FS_CHMOD, RT_FS_CREATE_TEMP,
    RT_FS_SYMLINK, RT_FS_TEMP_DIR, RT_GIT_INDEX_PACK, RT_GIT_PACK_COUNT, RT_GIT_PACK_VALID,
    RT_GIT_RECEIVE_PACK_SPLIT, RT_GIT_UPLOAD_PACK_WANTS, RT_GZIP_COMPRESS, RT_GZIP_DECOMPRESS,
    RT_HEX_DECODE, RT_HEX_ENCODE, RT_HMAC_SHA256, RT_JSON_PARSE, RT_JSON_STRINGIFY,
    RT_LOCATOR_CLASS, RT_MATH_ABS_F, RT_MATH_ABS_I, RT_MATH_CEIL, RT_MATH_COS, RT_MATH_FLOOR,
    RT_MATH_POW, RT_MATH_SIN, RT_MATH_SQRT, RT_MATH_TAN, RT_NOW_MONO_MS, RT_OS_CHDIR, RT_OS_CWD,
    RT_OS_HOSTNAME, RT_OS_PID, RT_OS_PLATFORM, RT_PARSE_F64, RT_PARSE_I64, RT_PATH_CLEAN,
    RT_PATH_REL, RT_PROCESS_PIPE_CLOSE, RT_PROCESS_PIPE_READ, RT_PROCESS_PIPE_WRITE,
    RT_PROCESS_RUN_CAPTURE, RT_PROCESS_RUN_CWD, RT_PROCESS_SPAWN_PIPES, RT_PROCESS_WAIT,
    RT_RANDOM_FLOAT, RT_RANDOM_SEED, RT_RANDOM_U64, RT_SHA256, RT_SHA512, RT_STORE_CAS,
    RT_STORE_CAS_CREATE, RT_STORE_CLOSE, RT_STORE_GET, RT_STORE_GET_IF_NONE_MATCH,
    RT_STORE_GET_RANGE, RT_STORE_HEAD, RT_STORE_KEYS, RT_STORE_MEMORY_NEW, RT_STORE_PUT,
    RT_STORE_S3_NEW, RT_STR_REPLACE, RT_STR_SPLIT, RT_STR_TO_LOWER, RT_STR_TO_UPPER, RT_STR_TRIM,
    RT_TIME_FORMAT, RT_TIME_PARSE, RT_TLS_ACCEPT, RT_TLS_CLOSE, RT_TLS_CLOSE_LISTENER,
    RT_TLS_CONNECT, RT_TLS_LISTEN, RT_TLS_READ, RT_TLS_WRITE, RT_UNIX_ACCEPT, RT_UNIX_CLOSE,
    RT_UNIX_CONNECT, RT_UNIX_LISTEN, RT_UNIX_READ, RT_UNIX_WRITE, RT_URL_PARSE, RT_UTF8_DECODE,
    RT_UTF8_VALID, RT_ZIP_PACK, RT_ZIP_UNPACK_FIRST,
};

use std::path::{Path, PathBuf};

use echo_codegen_abi::{
    RT_BYTES_CAT, RT_BYTES_FROM_I64, RT_BYTES_FROM_STR, RT_BYTES_GET, RT_BYTES_LEN, RT_BYTES_SLICE,
    RT_EPRINT, RT_FLOAT_FROM_F64, RT_FLOAT_TO_F64, RT_FS_COPY, RT_FS_CREATE_DIR,
    RT_FS_CREATE_DIR_ALL, RT_FS_EXISTS, RT_FS_FILE_CLOSE, RT_FS_FILE_READ, RT_FS_FILE_SEEK,
    RT_FS_FILE_WRITE, RT_FS_IS_DIR, RT_FS_IS_FILE, RT_FS_JOIN, RT_FS_METADATA, RT_FS_OPEN_APPEND,
    RT_FS_OPEN_READ, RT_FS_OPEN_WRITE, RT_FS_READ, RT_FS_READ_DIR, RT_FS_REMOVE, RT_FS_REMOVE_DIR,
    RT_FS_RENAME, RT_FS_WRITE, RT_HTTP_HEADERS_COMPLETE, RT_HTTP_PARSE_REQUEST,
    RT_HTTP_REQUEST_COMPLETE, RT_LIST_GET, RT_LIST_LEN, RT_LIST_NEW_EMPTY_LISTS, RT_LIST_RESERVE,
    RT_NOW_MS, RT_PRINT_I64, RT_PROCESS_ARGS, RT_PROCESS_ENV_GET, RT_PROCESS_ENV_HAS,
    RT_PROCESS_ENV_SET, RT_PROCESS_ENV_UNSET, RT_PROCESS_EXIT, RT_PROCESS_RUN,
    RT_REFLECT_KEY_BYTES, RT_REFLECT_KIND, RT_REFLECT_KIND_NAME, RT_SLEEP_MS, RT_STDIN_READ,
    RT_STR_CAT, RT_STR_CONTAINS, RT_STR_ENDS_WITH, RT_STR_FROM_BYTES, RT_STR_FROM_DEBUG,
    RT_STR_FROM_DURATION, RT_STR_FROM_FLOAT, RT_STR_FROM_INT, RT_STR_FROM_LOCATOR, RT_STR_GET,
    RT_STR_LEN, RT_STR_REPEAT, RT_STR_SLICE, RT_STR_STARTS_WITH, RT_TCP_ACCEPT, RT_TCP_CLOSE,
    RT_TCP_CONNECT, RT_TCP_LISTEN, RT_TCP_READ, RT_TCP_WRITE, RT_TEST_BENCH_REGISTER, RT_TEST_FAIL,
    RT_TEST_FINISH, RT_TEST_REGISTER, RT_UDP_BIND, RT_UDP_CLOSE, RT_UDP_RECV_FROM, RT_UDP_SEND_TO,
};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// Sentinel path for the privileged runtime-primitive package (not a real file).
pub const RUNTIME_MODULE_PATH: &str = "<echo:runtime>";

/// One export on the virtual `runtime` module.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeExport {
    pub name: &'static str,
    /// Native symbol (`echo_runtime_*`).
    pub native: &'static str,
}

/// Known return kind label for a runtime export (`string`, `i64`, …).
/// `None` = unconstrained (same as a fresh checker hole).
#[must_use]
pub fn runtime_ret_kind(name: &str) -> Option<&'static str> {
    match name {
        "str_from_int" | "str_from_float" | "str_from_bytes" | "str_from_duration"
        | "str_from_locator" | "str_from_debug" | "str_cat" | "str_trim" | "str_to_lower"
        | "str_to_upper" | "str_slice" | "str_repeat" => Some("string"),
        "str_len" | "bytes_len" | "str_get" | "bytes_get" | "locator_class" => Some("i64"),
        _ => None,
    }
}

/// Runtime package exports (extend as the ABI grows).
pub const RUNTIME_EXPORTS: &[RuntimeExport] = &[
    RuntimeExport {
        name: "print",
        native: RT_PRINT_I64,
    },
    RuntimeExport {
        name: "eprint",
        native: RT_EPRINT,
    },
    RuntimeExport {
        name: "stdin_read",
        native: RT_STDIN_READ,
    },
    RuntimeExport {
        name: "str_from_int",
        native: RT_STR_FROM_INT,
    },
    RuntimeExport {
        name: "str_from_float",
        native: RT_STR_FROM_FLOAT,
    },
    RuntimeExport {
        name: "str_from_bytes",
        native: RT_STR_FROM_BYTES,
    },
    RuntimeExport {
        name: "str_from_duration",
        native: RT_STR_FROM_DURATION,
    },
    RuntimeExport {
        name: "str_from_locator",
        native: RT_STR_FROM_LOCATOR,
    },
    RuntimeExport {
        name: "str_from_debug",
        native: RT_STR_FROM_DEBUG,
    },
    RuntimeExport {
        name: "str_len",
        native: RT_STR_LEN,
    },
    RuntimeExport {
        name: "bytes_len",
        native: RT_BYTES_LEN,
    },
    RuntimeExport {
        name: "list_len",
        native: RT_LIST_LEN,
    },
    RuntimeExport {
        name: "list_get",
        native: RT_LIST_GET,
    },
    RuntimeExport {
        name: "bytes_get",
        native: RT_BYTES_GET,
    },
    RuntimeExport {
        name: "bytes_from_i64",
        native: RT_BYTES_FROM_I64,
    },
    RuntimeExport {
        name: "bytes_slice",
        native: RT_BYTES_SLICE,
    },
    RuntimeExport {
        name: "bytes_cat",
        native: RT_BYTES_CAT,
    },
    RuntimeExport {
        name: "bytes_from_str",
        native: RT_BYTES_FROM_STR,
    },
    RuntimeExport {
        name: "str_get",
        native: RT_STR_GET,
    },
    RuntimeExport {
        name: "reflect_kind",
        native: RT_REFLECT_KIND,
    },
    RuntimeExport {
        name: "reflect_kind_name",
        native: RT_REFLECT_KIND_NAME,
    },
    RuntimeExport {
        name: "reflect_key_bytes",
        native: RT_REFLECT_KEY_BYTES,
    },
    RuntimeExport {
        name: "str_cat",
        native: RT_STR_CAT,
    },
    RuntimeExport {
        name: "str_slice",
        native: RT_STR_SLICE,
    },
    RuntimeExport {
        name: "str_contains",
        native: RT_STR_CONTAINS,
    },
    RuntimeExport {
        name: "str_starts_with",
        native: RT_STR_STARTS_WITH,
    },
    RuntimeExport {
        name: "str_ends_with",
        native: RT_STR_ENDS_WITH,
    },
    RuntimeExport {
        name: "str_repeat",
        native: RT_STR_REPEAT,
    },
    RuntimeExport {
        name: "list_reserve",
        native: RT_LIST_RESERVE,
    },
    RuntimeExport {
        name: "list_new_empty_lists",
        native: RT_LIST_NEW_EMPTY_LISTS,
    },
    RuntimeExport {
        name: "float_from_f64",
        native: RT_FLOAT_FROM_F64,
    },
    RuntimeExport {
        name: "float_to_f64",
        native: RT_FLOAT_TO_F64,
    },
    RuntimeExport {
        name: "http_parse_request",
        native: RT_HTTP_PARSE_REQUEST,
    },
    RuntimeExport {
        name: "http_headers_complete",
        native: RT_HTTP_HEADERS_COMPLETE,
    },
    RuntimeExport {
        name: "http_request_complete",
        native: RT_HTTP_REQUEST_COMPLETE,
    },
    RuntimeExport {
        name: "tcp_listen",
        native: RT_TCP_LISTEN,
    },
    RuntimeExport {
        name: "tcp_accept",
        native: RT_TCP_ACCEPT,
    },
    RuntimeExport {
        name: "tcp_connect",
        native: RT_TCP_CONNECT,
    },
    RuntimeExport {
        name: "tcp_read",
        native: RT_TCP_READ,
    },
    RuntimeExport {
        name: "tcp_write",
        native: RT_TCP_WRITE,
    },
    RuntimeExport {
        name: "tcp_close",
        native: RT_TCP_CLOSE,
    },
    RuntimeExport {
        name: "udp_bind",
        native: RT_UDP_BIND,
    },
    RuntimeExport {
        name: "udp_send_to",
        native: RT_UDP_SEND_TO,
    },
    RuntimeExport {
        name: "udp_recv_from",
        native: RT_UDP_RECV_FROM,
    },
    RuntimeExport {
        name: "udp_close",
        native: RT_UDP_CLOSE,
    },
    RuntimeExport {
        name: "test_register",
        native: RT_TEST_REGISTER,
    },
    RuntimeExport {
        name: "test_bench_register",
        native: RT_TEST_BENCH_REGISTER,
    },
    RuntimeExport {
        name: "test_fail",
        native: RT_TEST_FAIL,
    },
    RuntimeExport {
        name: "test_finish",
        native: RT_TEST_FINISH,
    },
    RuntimeExport {
        name: "now_ms",
        native: RT_NOW_MS,
    },
    RuntimeExport {
        name: "sleep_ms",
        native: RT_SLEEP_MS,
    },
    RuntimeExport {
        name: "process_args",
        native: RT_PROCESS_ARGS,
    },
    RuntimeExport {
        name: "process_env_has",
        native: RT_PROCESS_ENV_HAS,
    },
    RuntimeExport {
        name: "process_env_get",
        native: RT_PROCESS_ENV_GET,
    },
    RuntimeExport {
        name: "process_env_set",
        native: RT_PROCESS_ENV_SET,
    },
    RuntimeExport {
        name: "process_env_unset",
        native: RT_PROCESS_ENV_UNSET,
    },
    RuntimeExport {
        name: "process_exit",
        native: RT_PROCESS_EXIT,
    },
    RuntimeExport {
        name: "process_run",
        native: RT_PROCESS_RUN,
    },
    RuntimeExport {
        name: "fs_exists",
        native: RT_FS_EXISTS,
    },
    RuntimeExport {
        name: "fs_is_file",
        native: RT_FS_IS_FILE,
    },
    RuntimeExport {
        name: "fs_is_dir",
        native: RT_FS_IS_DIR,
    },
    RuntimeExport {
        name: "fs_join",
        native: RT_FS_JOIN,
    },
    RuntimeExport {
        name: "fs_read",
        native: RT_FS_READ,
    },
    RuntimeExport {
        name: "fs_write",
        native: RT_FS_WRITE,
    },
    RuntimeExport {
        name: "fs_remove",
        native: RT_FS_REMOVE,
    },
    RuntimeExport {
        name: "fs_create_dir",
        native: RT_FS_CREATE_DIR,
    },
    RuntimeExport {
        name: "fs_create_dir_all",
        native: RT_FS_CREATE_DIR_ALL,
    },
    RuntimeExport {
        name: "fs_read_dir",
        native: RT_FS_READ_DIR,
    },
    RuntimeExport {
        name: "fs_remove_dir",
        native: RT_FS_REMOVE_DIR,
    },
    RuntimeExport {
        name: "fs_copy",
        native: RT_FS_COPY,
    },
    RuntimeExport {
        name: "fs_rename",
        native: RT_FS_RENAME,
    },
    RuntimeExport {
        name: "fs_metadata",
        native: RT_FS_METADATA,
    },
    RuntimeExport {
        name: "fs_open_read",
        native: RT_FS_OPEN_READ,
    },
    RuntimeExport {
        name: "fs_open_write",
        native: RT_FS_OPEN_WRITE,
    },
    RuntimeExport {
        name: "fs_open_append",
        native: RT_FS_OPEN_APPEND,
    },
    RuntimeExport {
        name: "fs_file_read",
        native: RT_FS_FILE_READ,
    },
    RuntimeExport {
        name: "fs_file_write",
        native: RT_FS_FILE_WRITE,
    },
    RuntimeExport {
        name: "fs_file_seek",
        native: RT_FS_FILE_SEEK,
    },
    RuntimeExport {
        name: "fs_file_close",
        native: RT_FS_FILE_CLOSE,
    },
    RuntimeExport {
        name: "math_sqrt",
        native: RT_MATH_SQRT,
    },
    RuntimeExport {
        name: "math_sin",
        native: RT_MATH_SIN,
    },
    RuntimeExport {
        name: "math_cos",
        native: RT_MATH_COS,
    },
    RuntimeExport {
        name: "math_tan",
        native: RT_MATH_TAN,
    },
    RuntimeExport {
        name: "math_floor",
        native: RT_MATH_FLOOR,
    },
    RuntimeExport {
        name: "math_ceil",
        native: RT_MATH_CEIL,
    },
    RuntimeExport {
        name: "math_abs_f",
        native: RT_MATH_ABS_F,
    },
    RuntimeExport {
        name: "math_pow",
        native: RT_MATH_POW,
    },
    RuntimeExport {
        name: "math_abs_i",
        native: RT_MATH_ABS_I,
    },
    RuntimeExport {
        name: "random_seed",
        native: RT_RANDOM_SEED,
    },
    RuntimeExport {
        name: "random_u64",
        native: RT_RANDOM_U64,
    },
    RuntimeExport {
        name: "random_float",
        native: RT_RANDOM_FLOAT,
    },
    RuntimeExport {
        name: "crypto_random_bytes",
        native: RT_CRYPTO_RANDOM_BYTES,
    },
    RuntimeExport {
        name: "crypto_random_u64",
        native: RT_CRYPTO_RANDOM_U64,
    },
    RuntimeExport {
        name: "os_pid",
        native: RT_OS_PID,
    },
    RuntimeExport {
        name: "os_cwd",
        native: RT_OS_CWD,
    },
    RuntimeExport {
        name: "os_chdir",
        native: RT_OS_CHDIR,
    },
    RuntimeExport {
        name: "os_hostname",
        native: RT_OS_HOSTNAME,
    },
    RuntimeExport {
        name: "os_platform",
        native: RT_OS_PLATFORM,
    },
    RuntimeExport {
        name: "now_mono_ms",
        native: RT_NOW_MONO_MS,
    },
    RuntimeExport {
        name: "json_parse",
        native: RT_JSON_PARSE,
    },
    RuntimeExport {
        name: "json_stringify",
        native: RT_JSON_STRINGIFY,
    },
    RuntimeExport {
        name: "dns_lookup",
        native: RT_DNS_LOOKUP,
    },
    RuntimeExport {
        name: "sha256",
        native: RT_SHA256,
    },
    RuntimeExport {
        name: "process_run_capture",
        native: RT_PROCESS_RUN_CAPTURE,
    },
    RuntimeExport {
        name: "fs_temp_dir",
        native: RT_FS_TEMP_DIR,
    },
    RuntimeExport {
        name: "fs_create_temp",
        native: RT_FS_CREATE_TEMP,
    },
    RuntimeExport {
        name: "fs_symlink",
        native: RT_FS_SYMLINK,
    },
    RuntimeExport {
        name: "str_to_lower",
        native: RT_STR_TO_LOWER,
    },
    RuntimeExport {
        name: "str_to_upper",
        native: RT_STR_TO_UPPER,
    },
    RuntimeExport {
        name: "str_trim",
        native: RT_STR_TRIM,
    },
    RuntimeExport {
        name: "str_split",
        native: RT_STR_SPLIT,
    },
    RuntimeExport {
        name: "str_replace",
        native: RT_STR_REPLACE,
    },
    RuntimeExport {
        name: "hex_encode",
        native: RT_HEX_ENCODE,
    },
    RuntimeExport {
        name: "hex_decode",
        native: RT_HEX_DECODE,
    },
    RuntimeExport {
        name: "base64_encode",
        native: RT_BASE64_ENCODE,
    },
    RuntimeExport {
        name: "base64_decode",
        native: RT_BASE64_DECODE,
    },
    RuntimeExport {
        name: "utf8_valid",
        native: RT_UTF8_VALID,
    },
    RuntimeExport {
        name: "utf8_decode",
        native: RT_UTF8_DECODE,
    },
    RuntimeExport {
        name: "tls_listen",
        native: RT_TLS_LISTEN,
    },
    RuntimeExport {
        name: "tls_accept",
        native: RT_TLS_ACCEPT,
    },
    RuntimeExport {
        name: "tls_connect",
        native: RT_TLS_CONNECT,
    },
    RuntimeExport {
        name: "tls_read",
        native: RT_TLS_READ,
    },
    RuntimeExport {
        name: "tls_write",
        native: RT_TLS_WRITE,
    },
    RuntimeExport {
        name: "tls_close",
        native: RT_TLS_CLOSE,
    },
    RuntimeExport {
        name: "tls_close_listener",
        native: RT_TLS_CLOSE_LISTENER,
    },
    RuntimeExport {
        name: "parse_i64",
        native: RT_PARSE_I64,
    },
    RuntimeExport {
        name: "parse_f64",
        native: RT_PARSE_F64,
    },
    RuntimeExport {
        name: "url_parse",
        native: RT_URL_PARSE,
    },
    RuntimeExport {
        name: "time_format",
        native: RT_TIME_FORMAT,
    },
    RuntimeExport {
        name: "time_parse",
        native: RT_TIME_PARSE,
    },
    RuntimeExport {
        name: "gzip_compress",
        native: RT_GZIP_COMPRESS,
    },
    RuntimeExport {
        name: "gzip_decompress",
        native: RT_GZIP_DECOMPRESS,
    },
    RuntimeExport {
        name: "zip_pack",
        native: RT_ZIP_PACK,
    },
    RuntimeExport {
        name: "zip_unpack_first",
        native: RT_ZIP_UNPACK_FIRST,
    },
    RuntimeExport {
        name: "hmac_sha256",
        native: RT_HMAC_SHA256,
    },
    RuntimeExport {
        name: "sha512",
        native: RT_SHA512,
    },
    RuntimeExport {
        name: "aes_gcm_encrypt",
        native: RT_AES_GCM_ENCRYPT,
    },
    RuntimeExport {
        name: "aes_gcm_decrypt",
        native: RT_AES_GCM_DECRYPT,
    },
    RuntimeExport {
        name: "fs_chmod",
        native: RT_FS_CHMOD,
    },
    RuntimeExport {
        name: "locator_class",
        native: RT_LOCATOR_CLASS,
    },
    RuntimeExport {
        name: "path_clean",
        native: RT_PATH_CLEAN,
    },
    RuntimeExport {
        name: "path_rel",
        native: RT_PATH_REL,
    },
    RuntimeExport {
        name: "process_run_cwd",
        native: RT_PROCESS_RUN_CWD,
    },
    RuntimeExport {
        name: "process_spawn_pipes",
        native: RT_PROCESS_SPAWN_PIPES,
    },
    RuntimeExport {
        name: "process_pipe_write",
        native: RT_PROCESS_PIPE_WRITE,
    },
    RuntimeExport {
        name: "process_pipe_read",
        native: RT_PROCESS_PIPE_READ,
    },
    RuntimeExport {
        name: "process_pipe_close",
        native: RT_PROCESS_PIPE_CLOSE,
    },
    RuntimeExport {
        name: "process_wait",
        native: RT_PROCESS_WAIT,
    },
    RuntimeExport {
        name: "unix_listen",
        native: RT_UNIX_LISTEN,
    },
    RuntimeExport {
        name: "unix_accept",
        native: RT_UNIX_ACCEPT,
    },
    RuntimeExport {
        name: "unix_connect",
        native: RT_UNIX_CONNECT,
    },
    RuntimeExport {
        name: "unix_read",
        native: RT_UNIX_READ,
    },
    RuntimeExport {
        name: "unix_write",
        native: RT_UNIX_WRITE,
    },
    RuntimeExport {
        name: "unix_close",
        native: RT_UNIX_CLOSE,
    },
    RuntimeExport {
        name: "store_memory_new",
        native: RT_STORE_MEMORY_NEW,
    },
    RuntimeExport {
        name: "store_s3_new",
        native: RT_STORE_S3_NEW,
    },
    RuntimeExport {
        name: "store_close",
        native: RT_STORE_CLOSE,
    },
    RuntimeExport {
        name: "store_get",
        native: RT_STORE_GET,
    },
    RuntimeExport {
        name: "store_get_if_none_match",
        native: RT_STORE_GET_IF_NONE_MATCH,
    },
    RuntimeExport {
        name: "store_get_range",
        native: RT_STORE_GET_RANGE,
    },
    RuntimeExport {
        name: "store_head",
        native: RT_STORE_HEAD,
    },
    RuntimeExport {
        name: "store_put",
        native: RT_STORE_PUT,
    },
    RuntimeExport {
        name: "store_cas",
        native: RT_STORE_CAS,
    },
    RuntimeExport {
        name: "store_cas_create",
        native: RT_STORE_CAS_CREATE,
    },
    RuntimeExport {
        name: "store_keys",
        native: RT_STORE_KEYS,
    },
    RuntimeExport {
        name: "git_pack_valid",
        native: RT_GIT_PACK_VALID,
    },
    RuntimeExport {
        name: "git_pack_count",
        native: RT_GIT_PACK_COUNT,
    },
    RuntimeExport {
        name: "git_index_pack",
        native: RT_GIT_INDEX_PACK,
    },
    RuntimeExport {
        name: "git_receive_pack_split",
        native: RT_GIT_RECEIVE_PACK_SPLIT,
    },
    RuntimeExport {
        name: "git_upload_pack_wants",
        native: RT_GIT_UPLOAD_PACK_WANTS,
    },
];

#[must_use]
pub fn is_runtime_module_path(path: &Path) -> bool {
    path.to_str() == Some(RUNTIME_MODULE_PATH)
}

#[must_use]
pub fn runtime_module_path() -> PathBuf {
    PathBuf::from(RUNTIME_MODULE_PATH)
}

#[must_use]
pub fn runtime_native_symbol(export: &str) -> Option<&'static str> {
    RUNTIME_EXPORTS
        .iter()
        .find(|e| e.name == export)
        .map(|e| e.native)
}

/// True if `file` is under a privileged `std/` directory for the given package roots.
///
/// Package roots are parents of `std/` (same as resolver `SearchPaths`).
/// Disk paths use canonicalize when the files exist. Logical paths (virtual
/// playground sources) match by path prefix so privilege does not depend on
/// a real filesystem.
#[must_use]
pub fn is_under_privileged_std(file: &Path, package_roots: &[PathBuf]) -> bool {
    if let Ok(file) = file.canonicalize() {
        for root in package_roots {
            let std_dir = root.join("std");
            if let Ok(std_canon) = std_dir.canonicalize() {
                if file.starts_with(&std_canon) {
                    return true;
                }
            }
        }
    }
    for root in package_roots {
        if file.starts_with(root.join("std")) {
            return true;
        }
    }
    false
}

/// Import path is exactly `/ runtime` (single name segment `runtime`).
#[must_use]
pub fn is_runtime_import_segments(segments: &[impl AsRef<str>]) -> bool {
    matches!(segments, [s] if s.as_ref() == "runtime")
}

#[cfg(test)]
mod tests {
    use super::*;
    use echo_codegen_abi::{
        RT_EPRINT, RT_LIST_LEN, RT_PRINT_I64, RT_STR_FROM_BYTES, RT_STR_FROM_DURATION,
        RT_STR_FROM_FLOAT, RT_STR_FROM_INT, RT_STR_FROM_LOCATOR, RT_STR_LEN,
    };

    #[test]
    fn runtime_print_and_str_maps() {
        assert_eq!(runtime_native_symbol("print"), Some(RT_PRINT_I64));
        assert_eq!(runtime_native_symbol("eprint"), Some(RT_EPRINT));
        assert_eq!(
            runtime_native_symbol("stdin_read"),
            Some(echo_codegen_abi::RT_STDIN_READ)
        );
        assert_eq!(runtime_native_symbol("str_from_int"), Some(RT_STR_FROM_INT));
        assert_eq!(
            runtime_native_symbol("str_from_float"),
            Some(RT_STR_FROM_FLOAT)
        );
        assert_eq!(
            runtime_native_symbol("str_from_bytes"),
            Some(RT_STR_FROM_BYTES)
        );
        assert_eq!(
            runtime_native_symbol("str_from_duration"),
            Some(RT_STR_FROM_DURATION)
        );
        assert_eq!(
            runtime_native_symbol("str_from_locator"),
            Some(RT_STR_FROM_LOCATOR)
        );
        assert_eq!(runtime_native_symbol("str_len"), Some(RT_STR_LEN));
        assert_eq!(runtime_ret_kind("str_from_int"), Some("string"));
        assert_eq!(runtime_ret_kind("print"), None);
        assert_eq!(runtime_native_symbol("list_len"), Some(RT_LIST_LEN));
        assert_eq!(runtime_native_symbol("nope"), None);
    }

    #[test]
    fn runtime_path_sentinel() {
        assert!(is_runtime_module_path(&runtime_module_path()));
        assert!(!is_runtime_module_path(Path::new("/tmp/runtime.echo")));
    }

    #[test]
    fn privileged_std_logical_path_without_canonicalize() {
        let roots = [PathBuf::from("/echo")];
        assert!(is_under_privileged_std(
            Path::new("/echo/std/io.echo"),
            &roots
        ));
        assert!(is_under_privileged_std(
            Path::new("/echo/std/net/http.echo"),
            &roots
        ));
        assert!(!is_under_privileged_std(
            Path::new("/echo/playground.echo"),
            &roots
        ));
        assert!(!is_under_privileged_std(
            Path::new("/echo/stdio.echo"),
            &roots
        ));
    }
}
