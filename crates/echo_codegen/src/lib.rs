//! MIR → LLVM IR (inkwell / LLVM 22) and AOT link via host clang.
//!
//! See `docs/llvm.md`, ADR 0002, `docs/runtime-abi.md`.
//!
//! Kinds are shapes from syntax; only width tags like `<i32>` are explicit.

mod debug;
mod elf;
mod metrics;
mod opt;
mod runtime_lib;

pub use elf::{elf_needs_interpreter, elf_pt_interp};
pub use metrics::{IrMetrics, measure_ir};
pub use opt::OptLevel;
pub use runtime_lib::find_runtime_staticlib;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use echo_ast::{BinaryOp, UnaryOp};
use echo_codegen_abi::{
    C_MAIN, ECHO_ENTRY, RT_ABORT, RT_AES_GCM_DECRYPT, RT_AES_GCM_ENCRYPT, RT_BASE64_DECODE,
    RT_BASE64_ENCODE, RT_BYTES_CAT, RT_BYTES_FROM_I64, RT_BYTES_FROM_PTR, RT_BYTES_FROM_STR,
    RT_BYTES_FROM_VALUE, RT_BYTES_GET, RT_BYTES_LEN, RT_BYTES_SLICE, RT_CRYPTO_RANDOM_BYTES,
    RT_CRYPTO_RANDOM_U64, RT_DNS_LOOKUP, RT_EPRINT, RT_EQ, RT_EQ_ID, RT_FLOAT_FROM_F64,
    RT_FLOAT_TO_F64, RT_FN_CODE, RT_FN_NEW, RT_FN_SHAPE, RT_FS_CHMOD, RT_FS_COPY, RT_FS_CREATE_DIR,
    RT_FS_CREATE_DIR_ALL, RT_FS_CREATE_TEMP, RT_FS_EXISTS, RT_FS_FILE_CLOSE, RT_FS_FILE_READ,
    RT_FS_FILE_SEEK, RT_FS_FILE_WRITE, RT_FS_IS_DIR, RT_FS_IS_FILE, RT_FS_JOIN, RT_FS_METADATA,
    RT_FS_OPEN_APPEND, RT_FS_OPEN_READ, RT_FS_OPEN_WRITE, RT_FS_READ, RT_FS_READ_DIR, RT_FS_REMOVE,
    RT_FS_REMOVE_DIR, RT_FS_RENAME, RT_FS_SYMLINK, RT_FS_TEMP_DIR, RT_FS_WRITE, RT_GIT_INDEX_PACK,
    RT_GIT_PACK_COUNT, RT_GIT_PACK_VALID, RT_GIT_RECEIVE_PACK_SPLIT, RT_GIT_UPLOAD_PACK_WANTS,
    RT_GZIP_COMPRESS, RT_GZIP_DECOMPRESS, RT_HEX_DECODE, RT_HEX_ENCODE, RT_HMAC_SHA256,
    RT_HTTP_HEADERS_COMPLETE, RT_HTTP_PARSE_REQUEST, RT_HTTP_REQUEST_COMPLETE, RT_JSON_PARSE,
    RT_JSON_STRINGIFY, RT_LIST_GET, RT_LIST_LEN, RT_LIST_NEW, RT_LIST_NEW_EMPTY_LISTS,
    RT_LIST_PUSH, RT_LIST_RESERVE, RT_LIST_SET, RT_LOCATOR_CLASS, RT_LOCATOR_FROM_STRING,
    RT_LOCATOR_FROM_UTF8, RT_MATH_ABS_F, RT_MATH_ABS_I, RT_MATH_CEIL, RT_MATH_COS, RT_MATH_FLOOR,
    RT_MATH_POW, RT_MATH_SIN, RT_MATH_SQRT, RT_MATH_TAN, RT_NE, RT_NE_ID, RT_NOW_MONO_MS,
    RT_NOW_MS, RT_OS_CHDIR, RT_OS_CWD, RT_OS_HOSTNAME, RT_OS_PID, RT_OS_PLATFORM, RT_PARSE_F64,
    RT_PARSE_I64, RT_PATH_CLEAN, RT_PATH_REL, RT_PRINT_I64, RT_PROCESS_ARGS, RT_PROCESS_ENV_GET,
    RT_PROCESS_ENV_HAS, RT_PROCESS_ENV_SET, RT_PROCESS_ENV_UNSET, RT_PROCESS_EXIT,
    RT_PROCESS_PIPE_CLOSE, RT_PROCESS_PIPE_READ, RT_PROCESS_PIPE_WRITE, RT_PROCESS_RUN,
    RT_PROCESS_RUN_CAPTURE, RT_PROCESS_RUN_CWD, RT_PROCESS_SPAWN_PIPES, RT_PROCESS_WAIT,
    RT_RANDOM_FLOAT, RT_RANDOM_SEED, RT_RANDOM_U64, RT_RANGE_NEW, RT_REFLECT_KEY_BYTES,
    RT_REFLECT_KIND, RT_REFLECT_KIND_NAME, RT_SCOPE_DISOWN, RT_SCOPE_ENTER, RT_SCOPE_EXIT,
    RT_SCOPE_PROMOTE, RT_SCOPE_REGISTER, RT_SCOPE_RELEASE, RT_SHA256, RT_SHA512, RT_SLEEP_MS,
    RT_STDIN_READ, RT_STORE_CAS, RT_STORE_CAS_CREATE, RT_STORE_CLOSE, RT_STORE_GET,
    RT_STORE_GET_IF_NONE_MATCH, RT_STORE_GET_RANGE, RT_STORE_HEAD, RT_STORE_KEYS,
    RT_STORE_MEMORY_NEW, RT_STORE_PUT, RT_STORE_S3_NEW, RT_STR_BUILDER_FINISH, RT_STR_BUILDER_NEW,
    RT_STR_BUILDER_PUSH_STR, RT_STR_BUILDER_PUSH_VALUE, RT_STR_CAT, RT_STR_CONTAINS,
    RT_STR_ENDS_WITH, RT_STR_FROM_BYTES, RT_STR_FROM_DEBUG, RT_STR_FROM_DURATION,
    RT_STR_FROM_FLOAT, RT_STR_FROM_INT, RT_STR_FROM_LOCATOR, RT_STR_GET, RT_STR_LEN, RT_STR_REPEAT,
    RT_STR_REPLACE, RT_STR_SLICE, RT_STR_SPLIT, RT_STR_STARTS_WITH, RT_STR_TO_LOWER,
    RT_STR_TO_UPPER, RT_STR_TRIM, RT_STRING_FROM_UTF8, RT_STRUCT_GET, RT_STRUCT_NEW,
    RT_STRUCT_NEW_NAMED, RT_STRUCT_SET, RT_STRUCT_TYPE_IS, RT_TASK_BLOCK, RT_TASK_BLOCK_WIDE,
    RT_TASK_CHECK_JOINED, RT_TASK_JOIN, RT_TASK_JOIN_WIDE, RT_TASK_SHAPE, RT_TASK_SPAWN_ARGS,
    RT_TASK_SPAWN_ENTRY, RT_TCP_ACCEPT, RT_TCP_CLOSE, RT_TCP_CONNECT, RT_TCP_LISTEN, RT_TCP_READ,
    RT_TCP_WRITE, RT_TEST_BENCH_REGISTER, RT_TEST_FAIL, RT_TEST_FINISH, RT_TEST_REGISTER,
    RT_TIME_FORMAT, RT_TIME_PARSE, RT_TLS_ACCEPT, RT_TLS_CLOSE, RT_TLS_CLOSE_LISTENER,
    RT_TLS_CONNECT, RT_TLS_LISTEN, RT_TLS_READ, RT_TLS_WRITE, RT_UDP_BIND, RT_UDP_CLOSE,
    RT_UDP_RECV_FROM, RT_UDP_SEND_TO, RT_UNIX_ACCEPT, RT_UNIX_CLOSE, RT_UNIX_CONNECT,
    RT_UNIX_LISTEN, RT_UNIX_READ, RT_UNIX_WRITE, RT_URL_PARSE, RT_UTF8_DECODE, RT_UTF8_VALID,
    RT_ZIP_PACK, RT_ZIP_UNPACK_FIRST,
};
use echo_diagnostics::{Diagnostic, Diagnostics};
use echo_mir::{
    BlockId, CallTarget, MirExpr, MirFn, MirOp, MirPrim, MirProgram, MirRepr, MirRetShape, MirStmt,
    StrPart, TAG_ERR, TAG_NONE, TAG_OK, TAG_SOME, Terminator, mangle_fn,
};
use echo_source::LineMap;
use echo_std::runtime_native_symbol;
use inkwell::AddressSpace;
use inkwell::FloatPredicate;
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::memory_buffer::MemoryBuffer;
use inkwell::module::Module;
use inkwell::targets::{InitializationConfig, Target};
use inkwell::types::{BasicMetadataTypeEnum, BasicTypeEnum, FloatType, IntType, PointerType};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValue, BasicValueEnum, FunctionValue, IntValue, PointerValue,
};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// Result of MIR → LLVM emission (and optional opt).
#[derive(Debug)]
pub struct EmitResult {
    /// Final IR (post-opt when `opt != O0`).
    pub ir: String,
    /// IR immediately after emit, before `run_passes` (same as `ir` at O0).
    pub ir_pre_opt: String,
    pub diagnostics: Diagnostics,
    pub opt: OptLevel,
}

/// Emit LLVM IR at [`OptLevel::O0`] (no mid-end passes).
#[must_use]
pub fn emit_llvm(prog: &MirProgram) -> EmitResult {
    emit_llvm_with(prog, OptLevel::O0)
}

/// Emit LLVM IR, verify, optionally run `default<On>`, verify again.
#[must_use]
pub fn emit_llvm_with(prog: &MirProgram, opt: OptLevel) -> EmitResult {
    let mut diagnostics = Diagnostics::new();
    let context = Context::create();
    let module = context.create_module("echo");
    let builder = context.create_builder();
    let i64t = context.i64_type();
    let i32t = context.i32_type();
    let i128t = context.i128_type();

    let ptr_ty = context.ptr_type(AddressSpace::default());
    let abort_ty = context
        .void_type()
        .fn_type(&[ptr_ty.into(), i64t.into()], false);
    module.add_function(RT_ABORT, abort_ty, None);

    let print_ty = context.void_type().fn_type(&[i64t.into()], false);
    module.add_function(RT_PRINT_I64, print_ty, None);
    module.add_function(RT_EPRINT, print_ty, None);

    let string_from_ty = i64t.fn_type(&[ptr_ty.into(), i64t.into()], false);
    module.add_function(RT_STRING_FROM_UTF8, string_from_ty, None);
    module.add_function(RT_BYTES_FROM_PTR, string_from_ty, None);
    module.add_function(RT_LOCATOR_FROM_UTF8, string_from_ty, None);

    let str_from_int_ty = i64t.fn_type(&[i64t.into()], false);
    module.add_function(RT_STR_FROM_INT, str_from_int_ty, None);
    module.add_function(RT_STR_FROM_FLOAT, str_from_int_ty, None);
    module.add_function(RT_STR_FROM_BYTES, str_from_int_ty, None);
    module.add_function(RT_STR_FROM_DURATION, str_from_int_ty, None);
    module.add_function(RT_STR_FROM_LOCATOR, str_from_int_ty, None);
    module.add_function(RT_LOCATOR_FROM_STRING, str_from_int_ty, None);
    module.add_function(RT_STR_FROM_DEBUG, str_from_int_ty, None);
    module.add_function(RT_STR_LEN, str_from_int_ty, None);
    module.add_function(RT_BYTES_LEN, str_from_int_ty, None);
    module.add_function(RT_BYTES_FROM_I64, str_from_int_ty, None);
    module.add_function(RT_BYTES_FROM_STR, str_from_int_ty, None);
    module.add_function(RT_BYTES_FROM_VALUE, str_from_int_ty, None);
    let bytes_get_ty = i64t.fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_BYTES_GET, bytes_get_ty, None);
    module.add_function(RT_STR_GET, bytes_get_ty, None);
    let str_cat_ty = i64t.fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_STR_CAT, str_cat_ty, None);
    module.add_function(RT_BYTES_CAT, str_cat_ty, None);
    module.add_function(RT_STR_CONTAINS, str_cat_ty, None);
    module.add_function(RT_STR_STARTS_WITH, str_cat_ty, None);
    module.add_function(RT_STR_ENDS_WITH, str_cat_ty, None);
    module.add_function(RT_STR_REPEAT, str_cat_ty, None);
    let str_slice_ty = i64t.fn_type(&[i64t.into(), i64t.into(), i64t.into()], false);
    module.add_function(RT_STR_SLICE, str_slice_ty, None);
    module.add_function(RT_BYTES_SLICE, str_slice_ty, None);
    module.add_function(RT_REFLECT_KIND, str_from_int_ty, None);
    module.add_function(RT_REFLECT_KIND_NAME, str_from_int_ty, None);
    module.add_function(RT_REFLECT_KEY_BYTES, str_from_int_ty, None);

    let f64t = context.f64_type();
    let float_from_ty = i64t.fn_type(&[f64t.into()], false);
    module.add_function(RT_FLOAT_FROM_F64, float_from_ty, None);
    let float_to_ty = f64t.fn_type(&[i64t.into()], false);
    module.add_function(RT_FLOAT_TO_F64, float_to_ty, None);

    let eq_ty = i64t.fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_EQ, eq_ty, None);
    module.add_function(RT_NE, eq_ty, None);
    module.add_function(RT_EQ_ID, eq_ty, None);
    module.add_function(RT_NE_ID, eq_ty, None);

    let builder_new_ty = i64t.fn_type(&[], false);
    module.add_function(RT_STR_BUILDER_NEW, builder_new_ty, None);
    let push_str_ty = context
        .void_type()
        .fn_type(&[i64t.into(), ptr_ty.into(), i64t.into()], false);
    module.add_function(RT_STR_BUILDER_PUSH_STR, push_str_ty, None);
    let push_val_ty = context
        .void_type()
        .fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_STR_BUILDER_PUSH_VALUE, push_val_ty, None);
    let finish_ty = i64t.fn_type(&[i64t.into()], false);
    module.add_function(RT_STR_BUILDER_FINISH, finish_ty, None);

    let list_new_ty = i64t.fn_type(&[], false);
    module.add_function(RT_LIST_NEW, list_new_ty, None);
    let list_new_n_ty = i64t.fn_type(&[i64t.into()], false);
    module.add_function(RT_LIST_NEW_EMPTY_LISTS, list_new_n_ty, None);
    let list_push_ty = context
        .void_type()
        .fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_LIST_PUSH, list_push_ty, None);
    module.add_function(RT_LIST_RESERVE, list_push_ty, None);
    let list_len_ty = i64t.fn_type(&[i64t.into()], false);
    module.add_function(RT_LIST_LEN, list_len_ty, None);
    let list_get_ty = i64t.fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_LIST_GET, list_get_ty, None);
    let list_set_ty = context
        .void_type()
        .fn_type(&[i64t.into(), i64t.into(), i64t.into()], false);
    module.add_function(RT_LIST_SET, list_set_ty, None);
    let range_new_ty = i64t.fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_RANGE_NEW, range_new_ty, None);
    let fn_new_ty = i64t.fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_FN_NEW, fn_new_ty, None);
    let fn_code_ty = i64t.fn_type(&[i64t.into()], false);
    module.add_function(RT_FN_CODE, fn_code_ty, None);
    module.add_function(RT_FN_SHAPE, fn_code_ty, None);
    let test_reg_ty = context
        .void_type()
        .fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_TEST_REGISTER, test_reg_ty, None);
    module.add_function(RT_TEST_BENCH_REGISTER, test_reg_ty, None);
    let test_fail_ty = context.void_type().fn_type(&[i64t.into()], false);
    module.add_function(RT_TEST_FAIL, test_fail_ty, None);
    let test_finish_ty = i64t.fn_type(&[], false);
    module.add_function(RT_TEST_FINISH, test_finish_ty, None);
    module.add_function(RT_NOW_MS, test_finish_ty, None);
    let sleep_ms_ty = context.void_type().fn_type(&[i64t.into()], false);
    module.add_function(RT_SLEEP_MS, sleep_ms_ty, None);

    // Expansive std primitives
    let rt1 = i64t.fn_type(&[i64t.into()], false);
    let rt0 = i64t.fn_type(&[], false);
    let rt2 = i64t.fn_type(&[i64t.into(), i64t.into()], false);
    let rt3 = i64t.fn_type(&[i64t.into(), i64t.into(), i64t.into()], false);
    let rt4 = i64t.fn_type(&[i64t.into(), i64t.into(), i64t.into(), i64t.into()], false);
    let void1 = context.void_type().fn_type(&[i64t.into()], false);
    for name in [
        RT_MATH_SQRT,
        RT_MATH_SIN,
        RT_MATH_COS,
        RT_MATH_TAN,
        RT_MATH_FLOOR,
        RT_MATH_CEIL,
        RT_MATH_ABS_F,
        RT_MATH_ABS_I,
        RT_JSON_PARSE,
        RT_JSON_STRINGIFY,
        RT_DNS_LOOKUP,
        RT_SHA256,
        RT_CRYPTO_RANDOM_BYTES,
        RT_OS_CHDIR,
        RT_FS_CREATE_TEMP,
        RT_STR_TO_LOWER,
        RT_STR_TO_UPPER,
        RT_STR_TRIM,
        RT_HEX_ENCODE,
        RT_HEX_DECODE,
        RT_BASE64_ENCODE,
        RT_BASE64_DECODE,
        RT_UTF8_VALID,
        RT_UTF8_DECODE,
        RT_TLS_LISTEN,
        RT_PARSE_I64,
        RT_PARSE_F64,
        RT_URL_PARSE,
        RT_GZIP_COMPRESS,
        RT_GZIP_DECOMPRESS,
        RT_ZIP_UNPACK_FIRST,
        RT_SHA512,
        RT_PATH_CLEAN,
        RT_LOCATOR_CLASS,
        RT_UNIX_LISTEN,
        RT_UNIX_ACCEPT,
        RT_UNIX_CONNECT,
        RT_PROCESS_WAIT,
    ] {
        module.add_function(name, rt1, None);
    }
    for name in [
        RT_MATH_POW,
        RT_STR_SPLIT,
        RT_FS_SYMLINK,
        RT_PROCESS_RUN_CAPTURE,
        RT_TLS_READ,
        RT_TLS_WRITE,
        RT_TIME_FORMAT,
        RT_TIME_PARSE,
        RT_ZIP_PACK,
        RT_HMAC_SHA256,
        RT_FS_CHMOD,
        RT_PATH_REL,
        RT_UNIX_READ,
        RT_UNIX_WRITE,
        RT_PROCESS_SPAWN_PIPES,
        RT_PROCESS_PIPE_WRITE,
        RT_PROCESS_PIPE_READ,
    ] {
        module.add_function(name, rt2, None);
    }
    module.add_function(RT_TLS_ACCEPT, rt3, None);
    module.add_function(RT_TLS_CONNECT, rt4, None);
    module.add_function(RT_STR_REPLACE, rt3, None);
    module.add_function(RT_AES_GCM_ENCRYPT, rt3, None);
    module.add_function(RT_AES_GCM_DECRYPT, rt3, None);
    module.add_function(RT_PROCESS_RUN_CWD, rt3, None);
    for name in [
        RT_RANDOM_U64,
        RT_RANDOM_FLOAT,
        RT_CRYPTO_RANDOM_U64,
        RT_OS_PID,
        RT_OS_CWD,
        RT_OS_HOSTNAME,
        RT_OS_PLATFORM,
        RT_NOW_MONO_MS,
        RT_FS_TEMP_DIR,
        RT_STDIN_READ,
        RT_STORE_MEMORY_NEW,
        RT_STORE_S3_NEW,
    ] {
        module.add_function(name, rt0, None);
    }
    module.add_function(RT_RANDOM_SEED, void1, None);
    for name in [
        RT_TLS_CLOSE,
        RT_TLS_CLOSE_LISTENER,
        RT_UNIX_CLOSE,
        RT_PROCESS_PIPE_CLOSE,
    ] {
        module.add_function(name, void1, None);
    }

    // process / env / spawn
    module.add_function(RT_PROCESS_ARGS, test_finish_ty, None);
    module.add_function(RT_PROCESS_ENV_HAS, list_len_ty, None);
    module.add_function(RT_PROCESS_ENV_GET, list_len_ty, None);
    let process_env_set_ty = context
        .void_type()
        .fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_PROCESS_ENV_SET, process_env_set_ty, None);
    module.add_function(RT_PROCESS_ENV_UNSET, sleep_ms_ty, None);
    module.add_function(RT_PROCESS_EXIT, sleep_ms_ty, None);
    module.add_function(RT_PROCESS_RUN, list_get_ty, None);
    // filesystem
    module.add_function(RT_FS_EXISTS, list_len_ty, None);
    module.add_function(RT_FS_IS_FILE, list_len_ty, None);
    module.add_function(RT_FS_IS_DIR, list_len_ty, None);
    module.add_function(RT_FS_JOIN, list_get_ty, None);
    module.add_function(RT_FS_READ, list_len_ty, None);
    module.add_function(RT_FS_WRITE, list_get_ty, None);
    module.add_function(RT_FS_REMOVE, list_len_ty, None);
    module.add_function(RT_FS_CREATE_DIR, list_len_ty, None);
    module.add_function(RT_FS_CREATE_DIR_ALL, list_len_ty, None);
    module.add_function(RT_FS_READ_DIR, list_len_ty, None);
    module.add_function(RT_FS_REMOVE_DIR, list_len_ty, None);
    module.add_function(RT_FS_COPY, list_get_ty, None);
    module.add_function(RT_FS_RENAME, list_get_ty, None);
    module.add_function(RT_FS_METADATA, list_len_ty, None);
    module.add_function(RT_FS_OPEN_READ, list_len_ty, None);
    module.add_function(RT_FS_OPEN_WRITE, list_len_ty, None);
    module.add_function(RT_FS_OPEN_APPEND, list_len_ty, None);
    module.add_function(RT_FS_FILE_READ, list_get_ty, None);
    module.add_function(RT_FS_FILE_WRITE, list_get_ty, None);
    module.add_function(RT_FS_FILE_SEEK, list_get_ty, None);
    module.add_function(RT_FS_FILE_CLOSE, sleep_ms_ty, None);
    let http_parse_ty = i64t.fn_type(&[i64t.into()], false);
    module.add_function(RT_HTTP_PARSE_REQUEST, http_parse_ty, None);
    module.add_function(RT_HTTP_HEADERS_COMPLETE, http_parse_ty, None);
    module.add_function(RT_HTTP_REQUEST_COMPLETE, http_parse_ty, None);
    module.add_function(RT_STORE_CLOSE, http_parse_ty, None);
    module.add_function(RT_STORE_KEYS, http_parse_ty, None);
    module.add_function(RT_STORE_GET, rt2, None);
    module.add_function(RT_STORE_HEAD, rt2, None);
    module.add_function(RT_STORE_GET_IF_NONE_MATCH, rt3, None);
    module.add_function(RT_STORE_PUT, rt3, None);
    module.add_function(RT_STORE_CAS_CREATE, rt3, None);
    module.add_function(RT_STORE_GET_RANGE, rt4, None);
    module.add_function(RT_STORE_CAS, rt4, None);
    module.add_function(RT_GIT_PACK_VALID, http_parse_ty, None);
    module.add_function(RT_GIT_PACK_COUNT, http_parse_ty, None);
    module.add_function(RT_GIT_INDEX_PACK, http_parse_ty, None);
    module.add_function(RT_GIT_RECEIVE_PACK_SPLIT, http_parse_ty, None);
    module.add_function(RT_GIT_UPLOAD_PACK_WANTS, http_parse_ty, None);

    // TCP/UDP: i64 args → i64 / void
    let rt1 = i64t.fn_type(&[i64t.into()], false);
    module.add_function(RT_TCP_LISTEN, rt1, None);
    module.add_function(RT_TCP_ACCEPT, rt1, None);
    module.add_function(RT_TCP_CONNECT, rt1, None);
    module.add_function(RT_UDP_BIND, rt1, None);
    let rt1_void = context.void_type().fn_type(&[i64t.into()], false);
    module.add_function(RT_TCP_CLOSE, rt1_void, None);
    module.add_function(RT_UDP_CLOSE, rt1_void, None);
    let rt2 = i64t.fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_TCP_READ, rt2, None);
    module.add_function(RT_TCP_WRITE, rt2, None);
    module.add_function(RT_UDP_RECV_FROM, rt2, None);
    let rt3 = i64t.fn_type(&[i64t.into(), i64t.into(), i64t.into()], false);
    module.add_function(RT_UDP_SEND_TO, rt3, None);

    // Tasks (mio event loop): code + shape → handle; join plain/wide
    let rt_spawn = i64t.fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_TASK_SPAWN_ENTRY, rt_spawn, None);
    // code, shape, argc, a0..a7
    let mut spawn_args_params = vec![i64t.into(), i64t.into(), i64t.into()];
    for _ in 0..8 {
        spawn_args_params.push(i64t.into());
    }
    let rt_spawn_args = i64t.fn_type(&spawn_args_params, false);
    module.add_function(RT_TASK_SPAWN_ARGS, rt_spawn_args, None);
    let rt_check = i64t.fn_type(&[], false);
    module.add_function(RT_TASK_CHECK_JOINED, rt_check, None);
    module.add_function(RT_TASK_JOIN, rt1, None);
    module.add_function(RT_TASK_BLOCK, rt_spawn, None);
    let rt_join_wide = i128t.fn_type(&[i64t.into()], false);
    module.add_function(RT_TASK_JOIN_WIDE, rt_join_wide, None);
    let rt_block_wide = i128t.fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_TASK_BLOCK_WIDE, rt_block_wide, None);
    module.add_function(RT_TASK_SHAPE, rt1, None);

    // Scope-owned memory (ADR 0016)
    let scope_id_ty = context.void_type().fn_type(&[i64t.into()], false);
    module.add_function(RT_SCOPE_ENTER, scope_id_ty, None);
    module.add_function(RT_SCOPE_EXIT, scope_id_ty, None);
    module.add_function(RT_SCOPE_REGISTER, scope_id_ty, None);
    module.add_function(RT_SCOPE_DISOWN, scope_id_ty, None);
    module.add_function(RT_SCOPE_RELEASE, scope_id_ty, None);
    let scope_promote_ty = context
        .void_type()
        .fn_type(&[i64t.into(), i64t.into()], false);
    module.add_function(RT_SCOPE_PROMOTE, scope_promote_ty, None);

    let struct_new_ty = i64t.fn_type(&[], false);
    module.add_function(RT_STRUCT_NEW, struct_new_ty, None);
    let struct_new_named_ty = i64t.fn_type(&[ptr_ty.into(), i64t.into()], false);
    module.add_function(RT_STRUCT_NEW_NAMED, struct_new_named_ty, None);
    let struct_type_is_ty = i64t.fn_type(&[i64t.into(), ptr_ty.into(), i64t.into()], false);
    module.add_function(RT_STRUCT_TYPE_IS, struct_type_is_ty, None);
    let struct_set_ty = context.void_type().fn_type(
        &[i64t.into(), ptr_ty.into(), i64t.into(), i64t.into()],
        false,
    );
    module.add_function(RT_STRUCT_SET, struct_set_ty, None);
    let struct_get_ty = i64t.fn_type(&[i64t.into(), ptr_ty.into(), i64t.into()], false);
    module.add_function(RT_STRUCT_GET, struct_get_ty, None);

    // Mangled name → (LLVM function, return shape)
    let mut fn_map: HashMap<String, (FunctionValue<'_>, MirRetShape)> = HashMap::new();
    for f in &prog.functions {
        let params: Vec<BasicMetadataTypeEnum> = f.params.iter().map(|_| i64t.into()).collect();
        let ret_ty = match f.ret {
            MirRetShape::Plain => i64t.fn_type(&params, false),
            MirRetShape::Result | MirRetShape::Option => i128t.fn_type(&params, false),
        };
        let llvm_name = f.mangled_name();
        let fv = module.add_function(&llvm_name, ret_ty, None);
        fn_map.insert(llvm_name, (fv, f.ret));
    }

    let (dibuilder, di_cu, di_sub_ty) = debug::begin_module(&context, &module, &prog.entry_path);
    for f in &prog.functions {
        let key = f.mangled_name();
        let (fv, _) = fn_map[&key];
        debug::attach_subprogram(
            &dibuilder,
            &di_cu,
            di_sub_ty,
            fv,
            &f.name,
            &f.module_path,
            true,
        );
    }

    for f in &prog.functions {
        let key = f.mangled_name();
        let (fv, _) = fn_map[&key];
        emit_function(
            &context,
            &builder,
            &module,
            i64t,
            i128t,
            fv,
            f,
            &fn_map,
            &dibuilder,
            &mut diagnostics,
        );
    }

    // echo_entry: run the entry module's top-level body (`__toplevel`).
    // C `main` is only the process wrapper — Echo has no entry keyword.
    let entry_ty = i64t.fn_type(&[], false);
    let entry_fn = module.add_function(ECHO_ENTRY, entry_ty, None);
    debug::attach_subprogram(
        &dibuilder,
        &di_cu,
        di_sub_ty,
        entry_fn,
        ECHO_ENTRY,
        &prog.entry_path,
        false,
    );
    let entry_bb = context.append_basic_block(entry_fn, "entry");
    builder.position_at_end(entry_bb);
    debug::set_function_line(&context, &builder, &dibuilder, entry_fn);

    let toplevel = prog.functions.iter().find(|f| {
        f.module_path == prog.entry_path
            && f.name == "__toplevel"
            && f.params.is_empty()
            && f.ret == MirRetShape::Plain
    });
    // After toplevel: fail if any `+` task was left unjoined (`-`).
    // Then, if suite mode registered cases (`XO_TEST`), prefer suite status.
    let check_f = module
        .get_function(RT_TASK_CHECK_JOINED)
        .expect("task_check_joined");
    let test_finish_f = module.get_function(RT_TEST_FINISH).expect("test_finish");
    let zero = i64t.const_int(0, false);
    let neg_one = i64t.const_int((-1i64) as u64, true);
    if let Some(top_f) = toplevel {
        let key = top_f.mangled_name();
        let (fv, _) = fn_map[&key];
        let call = builder
            .build_call(fv, &[], "toplevel_status")
            .expect("call toplevel");
        let ret64 = call.try_as_basic_value().unwrap_basic().into_int_value();
        let check = builder
            .build_call(check_f, &[], "tasks_joined")
            .expect("check joined")
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value();
        // Prefer check failure over program status.
        let bad = builder
            .build_int_compare(inkwell::IntPredicate::NE, check, zero, "unjoined")
            .expect("cmp");
        let prog_status = builder
            .build_select(bad, check, ret64, "prog_status")
            .expect("select")
            .into_int_value();
        let suite = builder
            .build_call(test_finish_f, &[], "suite_status")
            .expect("test_finish")
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value();
        // suite == -1 → suite mode off; else use suite fail count as exit.
        let has_suite = builder
            .build_int_compare(inkwell::IntPredicate::NE, suite, neg_one, "has_suite")
            .expect("cmp suite");
        let status = builder
            .build_select(has_suite, suite, prog_status, "exit_status")
            .expect("select suite")
            .into_int_value();
        let _ = builder.build_return(Some(&status));
    } else {
        let check = builder
            .build_call(check_f, &[], "tasks_joined")
            .expect("check joined")
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value();
        let suite = builder
            .build_call(test_finish_f, &[], "suite_status")
            .expect("test_finish")
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value();
        let has_suite = builder
            .build_int_compare(inkwell::IntPredicate::NE, suite, neg_one, "has_suite")
            .expect("cmp suite");
        let status = builder
            .build_select(has_suite, suite, check, "exit_status")
            .expect("select suite")
            .into_int_value();
        let _ = builder.build_return(Some(&status));
    }

    let c_main_ty = i32t.fn_type(&[], false);
    let c_main_fn = module.add_function(C_MAIN, c_main_ty, None);
    debug::attach_subprogram(
        &dibuilder,
        &di_cu,
        di_sub_ty,
        c_main_fn,
        C_MAIN,
        &prog.entry_path,
        false,
    );
    let bb = context.append_basic_block(c_main_fn, "entry");
    builder.position_at_end(bb);
    debug::set_function_line(&context, &builder, &dibuilder, c_main_fn);
    let call = builder
        .build_call(entry_fn, &[], "status")
        .expect("call entry");
    let ret64 = call.try_as_basic_value().unwrap_basic().into_int_value();
    let ret32 = builder
        .build_int_truncate(ret64, i32t, "code")
        .expect("trunc");
    builder.build_return(Some(&ret32)).expect("ret c main");

    dibuilder.finalize();

    // Verify after emit (before any opt).
    if let Err(e) = module.verify() {
        diagnostics.push(
            Diagnostic::error(format!("LLVM verify after emit failed: {e}"))
                .with_code("llvm-verify-emit"),
        );
        let ir = module.print_to_string().to_string();
        return EmitResult {
            ir: ir.clone(),
            ir_pre_opt: ir,
            diagnostics,
            opt,
        };
    }

    let ir_pre_opt = module.print_to_string().to_string();

    if let Err(e) = opt::optimize_module(&module, opt) {
        diagnostics.push(Diagnostic::error(e).with_code("llvm-opt"));
        return EmitResult {
            ir: ir_pre_opt.clone(),
            ir_pre_opt,
            diagnostics,
            opt,
        };
    }

    // Verify again (post-opt when passes ran; same IR as post-emit at O0).
    if let Err(e) = module.verify() {
        diagnostics.push(
            Diagnostic::error(format!("LLVM verify after opt failed: {e}"))
                .with_code("llvm-verify-opt"),
        );
        let ir = module.print_to_string().to_string();
        return EmitResult {
            ir,
            ir_pre_opt,
            diagnostics,
            opt,
        };
    }

    let ir = module.print_to_string().to_string();
    EmitResult {
        ir,
        ir_pre_opt,
        diagnostics,
        opt,
    }
}

/// Run a program in-process via LLVM MCJIT (same IR + `echo_runtime_*` as AOT).
///
/// Returns the `echo_entry` status as `i64` (process exit semantics).
pub fn run_jit(prog: &MirProgram) -> Result<i64, String> {
    run_jit_with(prog, OptLevel::O0)
}

/// JIT at the given opt level (IR optimized in-process; MCJIT uses `None` so the
/// mid-end is not re-run).
pub fn run_jit_with(prog: &MirProgram, opt: OptLevel) -> Result<i64, String> {
    let emitted = emit_llvm_with(prog, opt);
    if emitted.diagnostics.error_count() > 0 {
        let msgs: Vec<_> = emitted
            .diagnostics
            .items()
            .iter()
            .map(|d| d.message.clone())
            .collect();
        return Err(format!("codegen errors: {}", msgs.join("; ")));
    }
    run_jit_ir(&emitted.ir)
}

/// Serializes in-process JIT runs so process-global task state
/// (`UNJOINED` / live task list) is not raced across concurrent tests or hosts.
static JIT_TASK_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// JIT-execute LLVM IR text that declares `echo_runtime_*` and defines `echo_entry`.
///
/// IR is assumed already at the desired opt level; the execution engine uses
/// [`OptimizationLevel::None`] so MCJIT does not re-optimize.
pub fn run_jit_ir(ir: &str) -> Result<i64, String> {
    let _task_gate = JIT_TASK_GATE.lock().unwrap_or_else(|e| e.into_inner());
    Target::initialize_native(&InitializationConfig::default())
        .map_err(|e| format!("init native target: {e}"))?;

    let context = Context::create();
    // inkwell requires a trailing NUL for parse_ir memory buffers.
    let mut ir_bytes = ir.as_bytes().to_vec();
    if ir_bytes.last().copied() != Some(0) {
        ir_bytes.push(0);
    }
    let buffer = MemoryBuffer::create_from_memory_range_copy(&ir_bytes, "echo.ll");
    let module = context
        .create_module_from_ir(buffer)
        .map_err(|e| format!("parse IR: {e}"))?;

    if let Err(e) = module.verify() {
        return Err(format!("LLVM verify before JIT failed: {e}"));
    }

    let ee = module
        .create_jit_execution_engine(OptimizationLevel::None)
        .map_err(|e| format!("create JIT: {e}"))?;

    // Map declared externs to the linked `echo_runtime` crate (same ABI as AOT).
    map_runtime_symbol(
        &module,
        &ee,
        RT_ABORT,
        echo_runtime_abort as unsafe extern "C" fn(*const u8, usize) -> ! as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PRINT_I64,
        echo_runtime_print_i64 as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_EPRINT,
        echo_runtime_eprint as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STDIN_READ,
        echo_runtime_stdin_read as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STRING_FROM_UTF8,
        echo_runtime_string_from_utf8 as unsafe extern "C" fn(*const u8, usize) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_BYTES_FROM_PTR,
        echo_runtime_bytes_from_ptr as unsafe extern "C" fn(*const u8, usize) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_LOCATOR_FROM_UTF8,
        echo_runtime_locator_from_utf8 as unsafe extern "C" fn(*const u8, usize) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_LOCATOR_FROM_STRING,
        echo_runtime_locator_from_string as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_FROM_INT,
        echo_runtime_str_from_int as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_LEN,
        echo_runtime_str_len as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_BYTES_LEN,
        echo_runtime_bytes_len as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_BYTES_FROM_I64,
        echo_runtime_bytes_from_i64 as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_BYTES_GET,
        echo_runtime_bytes_get as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_BYTES_SLICE,
        echo_runtime_bytes_slice as extern "C" fn(i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_BYTES_CAT,
        echo_runtime_bytes_cat as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_BYTES_FROM_STR,
        echo_runtime_bytes_from_str as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_BYTES_FROM_VALUE,
        echo_runtime_bytes_from_value as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_GET,
        echo_runtime_str_get as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_REFLECT_KIND,
        echo_runtime_reflect_kind as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_REFLECT_KIND_NAME,
        echo_runtime_reflect_kind_name as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_REFLECT_KEY_BYTES,
        echo_runtime_reflect_key_bytes as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_CAT,
        echo_runtime_str_cat as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_SLICE,
        echo_runtime_str_slice as extern "C" fn(i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_CONTAINS,
        echo_runtime_str_contains as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_STARTS_WITH,
        echo_runtime_str_starts_with as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_ENDS_WITH,
        echo_runtime_str_ends_with as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_REPEAT,
        echo_runtime_str_repeat as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_FROM_FLOAT,
        echo_runtime_str_from_float as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_FROM_BYTES,
        echo_runtime_str_from_bytes as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_FROM_DURATION,
        echo_runtime_str_from_duration as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_FROM_LOCATOR,
        echo_runtime_str_from_locator as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_FROM_DEBUG,
        echo_runtime_str_from_debug as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FLOAT_FROM_F64,
        echo_runtime_float_from_f64 as extern "C" fn(f64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FLOAT_TO_F64,
        echo_runtime_float_to_f64 as extern "C" fn(i64) -> f64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_EQ,
        echo_runtime_eq as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_NE,
        echo_runtime_ne as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_EQ_ID,
        echo_runtime_eq_id as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_NE_ID,
        echo_runtime_ne_id as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_BUILDER_NEW,
        echo_runtime_string_builder_new as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_BUILDER_PUSH_STR,
        echo_runtime_string_builder_push_str as unsafe extern "C" fn(i64, *const u8, usize)
            as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_BUILDER_PUSH_VALUE,
        echo_runtime_string_builder_push_value as extern "C" fn(i64, i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_BUILDER_FINISH,
        echo_runtime_string_builder_finish as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_LIST_NEW,
        echo_runtime_list_new as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_LIST_NEW_EMPTY_LISTS,
        echo_runtime_list_new_empty_lists as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_LIST_PUSH,
        echo_runtime_list_push as unsafe extern "C" fn(i64, i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_LIST_RESERVE,
        echo_runtime_list_reserve as unsafe extern "C" fn(i64, i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_LIST_LEN,
        echo_runtime_list_len as unsafe extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_LIST_GET,
        echo_runtime_list_get as unsafe extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_LIST_SET,
        echo_runtime_list_set as unsafe extern "C" fn(i64, i64, i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_RANGE_NEW,
        echo_runtime_range_new as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FN_NEW,
        echo_runtime_fn_new as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FN_CODE,
        echo_runtime_fn_code as unsafe extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FN_SHAPE,
        echo_runtime_fn_shape as unsafe extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TEST_REGISTER,
        echo_runtime_test_register as unsafe extern "C" fn(i64, i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TEST_BENCH_REGISTER,
        echo_runtime_test_bench_register as unsafe extern "C" fn(i64, i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TEST_FAIL,
        echo_runtime_test_fail as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TEST_FINISH,
        echo_runtime_test_finish as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_NOW_MS,
        echo_runtime_now_ms as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_SLEEP_MS,
        echo_runtime_sleep_ms as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_MATH_SQRT,
        echo_runtime_math_sqrt as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_MATH_SIN,
        echo_runtime_math_sin as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_MATH_COS,
        echo_runtime_math_cos as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_MATH_TAN,
        echo_runtime_math_tan as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_MATH_FLOOR,
        echo_runtime_math_floor as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_MATH_CEIL,
        echo_runtime_math_ceil as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_MATH_ABS_F,
        echo_runtime_math_abs_f as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_MATH_POW,
        echo_runtime_math_pow as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_MATH_ABS_I,
        echo_runtime_math_abs_i as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_RANDOM_SEED,
        echo_runtime_random_seed as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_RANDOM_U64,
        echo_runtime_random_u64 as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_RANDOM_FLOAT,
        echo_runtime_random_float as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_CRYPTO_RANDOM_BYTES,
        echo_runtime_crypto_random_bytes as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_CRYPTO_RANDOM_U64,
        echo_runtime_crypto_random_u64 as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_OS_PID,
        echo_runtime_os_pid as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_OS_CWD,
        echo_runtime_os_cwd as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_OS_CHDIR,
        echo_runtime_os_chdir as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_OS_HOSTNAME,
        echo_runtime_os_hostname as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_OS_PLATFORM,
        echo_runtime_os_platform as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_NOW_MONO_MS,
        echo_runtime_now_mono_ms as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_JSON_PARSE,
        echo_runtime_json_parse as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_JSON_STRINGIFY,
        echo_runtime_json_stringify as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_DNS_LOOKUP,
        echo_runtime_dns_lookup as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_SHA256,
        echo_runtime_sha256 as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_RUN_CAPTURE,
        echo_runtime_process_run_capture as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_TEMP_DIR,
        echo_runtime_fs_temp_dir as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_CREATE_TEMP,
        echo_runtime_fs_create_temp as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_SYMLINK,
        echo_runtime_fs_symlink as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_TO_LOWER,
        echo_runtime_str_to_lower as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_TO_UPPER,
        echo_runtime_str_to_upper as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_TRIM,
        echo_runtime_str_trim as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_SPLIT,
        echo_runtime_str_split as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STR_REPLACE,
        echo_runtime_str_replace as extern "C" fn(i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_HEX_ENCODE,
        echo_runtime_hex_encode as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_HEX_DECODE,
        echo_runtime_hex_decode as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_BASE64_ENCODE,
        echo_runtime_base64_encode as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_BASE64_DECODE,
        echo_runtime_base64_decode as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_UTF8_VALID,
        echo_runtime_utf8_valid as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_UTF8_DECODE,
        echo_runtime_utf8_decode as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TLS_LISTEN,
        echo_runtime_tls_listen as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TLS_ACCEPT,
        echo_runtime_tls_accept as extern "C" fn(i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TLS_CONNECT,
        echo_runtime_tls_connect as extern "C" fn(i64, i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TLS_READ,
        echo_runtime_tls_read as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TLS_WRITE,
        echo_runtime_tls_write as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TLS_CLOSE,
        echo_runtime_tls_close as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TLS_CLOSE_LISTENER,
        echo_runtime_tls_close_listener as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PARSE_I64,
        echo_runtime_parse_i64 as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PARSE_F64,
        echo_runtime_parse_f64 as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_URL_PARSE,
        echo_runtime_url_parse as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TIME_FORMAT,
        echo_runtime_time_format as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TIME_PARSE,
        echo_runtime_time_parse as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_GZIP_COMPRESS,
        echo_runtime_gzip_compress as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_GZIP_DECOMPRESS,
        echo_runtime_gzip_decompress as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_ZIP_PACK,
        echo_runtime_zip_pack as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_ZIP_UNPACK_FIRST,
        echo_runtime_zip_unpack_first as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_HMAC_SHA256,
        echo_runtime_hmac_sha256 as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_SHA512,
        echo_runtime_sha512 as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_AES_GCM_ENCRYPT,
        echo_runtime_aes_gcm_encrypt as extern "C" fn(i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_AES_GCM_DECRYPT,
        echo_runtime_aes_gcm_decrypt as extern "C" fn(i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_CHMOD,
        echo_runtime_fs_chmod as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PATH_CLEAN,
        echo_runtime_path_clean as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_LOCATOR_CLASS,
        echo_runtime_locator_class as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PATH_REL,
        echo_runtime_path_rel as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_RUN_CWD,
        echo_runtime_process_run_cwd as extern "C" fn(i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_SPAWN_PIPES,
        echo_runtime_process_spawn_pipes as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_PIPE_WRITE,
        echo_runtime_process_pipe_write as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_PIPE_READ,
        echo_runtime_process_pipe_read as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_PIPE_CLOSE,
        echo_runtime_process_pipe_close as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_WAIT,
        echo_runtime_process_wait as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_UNIX_LISTEN,
        echo_runtime_unix_listen as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_UNIX_ACCEPT,
        echo_runtime_unix_accept as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_UNIX_CONNECT,
        echo_runtime_unix_connect as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_UNIX_READ,
        echo_runtime_unix_read as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_UNIX_WRITE,
        echo_runtime_unix_write as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_UNIX_CLOSE,
        echo_runtime_unix_close as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_ARGS,
        echo_runtime_process_args as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_ENV_HAS,
        echo_runtime_process_env_has as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_ENV_GET,
        echo_runtime_process_env_get as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_ENV_SET,
        echo_runtime_process_env_set as extern "C" fn(i64, i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_ENV_UNSET,
        echo_runtime_process_env_unset as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_EXIT,
        echo_runtime_process_exit as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_PROCESS_RUN,
        echo_runtime_process_run as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_EXISTS,
        echo_runtime_fs_exists as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_IS_FILE,
        echo_runtime_fs_is_file as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_IS_DIR,
        echo_runtime_fs_is_dir as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_JOIN,
        echo_runtime_fs_join as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_READ,
        echo_runtime_fs_read as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_WRITE,
        echo_runtime_fs_write as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_REMOVE,
        echo_runtime_fs_remove as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_CREATE_DIR,
        echo_runtime_fs_create_dir as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_CREATE_DIR_ALL,
        echo_runtime_fs_create_dir_all as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_READ_DIR,
        echo_runtime_fs_read_dir as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_REMOVE_DIR,
        echo_runtime_fs_remove_dir as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_COPY,
        echo_runtime_fs_copy as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_RENAME,
        echo_runtime_fs_rename as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_METADATA,
        echo_runtime_fs_metadata as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_OPEN_READ,
        echo_runtime_fs_open_read as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_OPEN_WRITE,
        echo_runtime_fs_open_write as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_OPEN_APPEND,
        echo_runtime_fs_open_append as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_FILE_READ,
        echo_runtime_fs_file_read as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_FILE_WRITE,
        echo_runtime_fs_file_write as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_FILE_SEEK,
        echo_runtime_fs_file_seek as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_FS_FILE_CLOSE,
        echo_runtime_fs_file_close as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_HTTP_PARSE_REQUEST,
        echo_runtime_http_parse_request as unsafe extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_HTTP_HEADERS_COMPLETE,
        echo_runtime_http_headers_complete as unsafe extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_HTTP_REQUEST_COMPLETE,
        echo_runtime_http_request_complete as unsafe extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STORE_MEMORY_NEW,
        echo_runtime_store_memory_new as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STORE_S3_NEW,
        echo_runtime_store_s3_new as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STORE_CLOSE,
        echo_runtime_store_close as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STORE_GET,
        echo_runtime_store_get as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STORE_GET_IF_NONE_MATCH,
        echo_runtime_store_get_if_none_match as extern "C" fn(i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STORE_GET_RANGE,
        echo_runtime_store_get_range as extern "C" fn(i64, i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STORE_HEAD,
        echo_runtime_store_head as extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STORE_PUT,
        echo_runtime_store_put as extern "C" fn(i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STORE_CAS,
        echo_runtime_store_cas as extern "C" fn(i64, i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STORE_CAS_CREATE,
        echo_runtime_store_cas_create as extern "C" fn(i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STORE_KEYS,
        echo_runtime_store_keys as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_GIT_PACK_VALID,
        echo_runtime_git_pack_valid as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_GIT_PACK_COUNT,
        echo_runtime_git_pack_count as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_GIT_INDEX_PACK,
        echo_runtime_git_index_pack as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_GIT_RECEIVE_PACK_SPLIT,
        echo_runtime_git_receive_pack_split as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_GIT_UPLOAD_PACK_WANTS,
        echo_runtime_git_upload_pack_wants as extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TCP_LISTEN,
        echo_runtime_tcp_listen as unsafe extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TCP_ACCEPT,
        echo_runtime_tcp_accept as unsafe extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TCP_CONNECT,
        echo_runtime_tcp_connect as unsafe extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TCP_READ,
        echo_runtime_tcp_read as unsafe extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TCP_WRITE,
        echo_runtime_tcp_write as unsafe extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TCP_CLOSE,
        echo_runtime_tcp_close as unsafe extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_UDP_BIND,
        echo_runtime_udp_bind as unsafe extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_UDP_SEND_TO,
        echo_runtime_udp_send_to as unsafe extern "C" fn(i64, i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_UDP_RECV_FROM,
        echo_runtime_udp_recv_from as unsafe extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_UDP_CLOSE,
        echo_runtime_udp_close as unsafe extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TASK_SPAWN_ENTRY,
        echo_runtime_task_spawn_entry as unsafe extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TASK_SPAWN_ARGS,
        echo_runtime_task_spawn_args
            as unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64) -> i64
            as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TASK_CHECK_JOINED,
        echo_runtime_task_check_joined as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TASK_JOIN,
        echo_runtime_task_join as unsafe extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TASK_JOIN_WIDE,
        echo_runtime_task_join_wide as unsafe extern "C" fn(i64) -> i128 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TASK_BLOCK,
        echo_runtime_task_block as unsafe extern "C" fn(i64, i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TASK_BLOCK_WIDE,
        echo_runtime_task_block_wide as unsafe extern "C" fn(i64, i64) -> i128 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_TASK_SHAPE,
        echo_runtime_task_shape as unsafe extern "C" fn(i64) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STRUCT_NEW,
        echo_runtime_struct_new as extern "C" fn() -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STRUCT_NEW_NAMED,
        echo_runtime_struct_new_named as unsafe extern "C" fn(*const u8, usize) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STRUCT_TYPE_IS,
        echo_runtime_struct_type_is as unsafe extern "C" fn(i64, *const u8, usize) -> i64 as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STRUCT_SET,
        echo_runtime_struct_set as unsafe extern "C" fn(i64, *const u8, usize, i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_STRUCT_GET,
        echo_runtime_struct_get as unsafe extern "C" fn(i64, *const u8, usize) -> i64 as usize,
    )?;
    // Scope-owned memory (ADR 0016) — IR emits these on every function entry/exit.
    map_runtime_symbol(
        &module,
        &ee,
        RT_SCOPE_ENTER,
        echo_runtime_scope_enter as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_SCOPE_EXIT,
        echo_runtime_scope_exit as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_SCOPE_REGISTER,
        echo_runtime_scope_register as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_SCOPE_DISOWN,
        echo_runtime_scope_disown as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_SCOPE_RELEASE,
        echo_runtime_scope_release as extern "C" fn(i64) as usize,
    )?;
    map_runtime_symbol(
        &module,
        &ee,
        RT_SCOPE_PROMOTE,
        echo_runtime_scope_promote as extern "C" fn(i64, i64) as usize,
    )?;

    if module.get_function(ECHO_ENTRY).is_none() {
        return Err(format!("JIT module missing `{ECHO_ENTRY}`"));
    }

    // SAFETY: echo_entry is `i64 ()` and we mapped all externs to matching C ABIs.
    let status = unsafe {
        let f = ee
            .get_function::<unsafe extern "C" fn() -> i64>(ECHO_ENTRY)
            .map_err(|e| format!("lookup {ECHO_ENTRY}: {e:?}"))?;
        f.call()
    };
    // Drain worker threads before dropping the EE (JIT code must not run after).
    // Also resets process-global unjoined state for the next in-process JIT run.
    echo_runtime_task_after_run();
    Ok(status)
}

fn map_runtime_symbol<'ctx>(
    module: &Module<'ctx>,
    ee: &inkwell::execution_engine::ExecutionEngine<'ctx>,
    name: &str,
    addr: usize,
) -> Result<(), String> {
    let Some(f) = module.get_function(name) else {
        // Not every program references every runtime symbol.
        return Ok(());
    };
    ee.add_global_mapping(&f, addr);
    Ok(())
}

// Re-export runtime symbols for mapping (must match `echo_codegen_abi` names).
use echo_runtime::{
    echo_runtime_abort, echo_runtime_aes_gcm_decrypt, echo_runtime_aes_gcm_encrypt,
    echo_runtime_base64_decode, echo_runtime_base64_encode, echo_runtime_bytes_cat,
    echo_runtime_bytes_from_i64, echo_runtime_bytes_from_ptr, echo_runtime_bytes_from_str,
    echo_runtime_bytes_from_value, echo_runtime_bytes_get, echo_runtime_bytes_len,
    echo_runtime_bytes_slice, echo_runtime_crypto_random_bytes, echo_runtime_crypto_random_u64,
    echo_runtime_dns_lookup, echo_runtime_eprint, echo_runtime_eq, echo_runtime_eq_id,
    echo_runtime_float_from_f64, echo_runtime_float_to_f64, echo_runtime_fn_code,
    echo_runtime_fn_new, echo_runtime_fn_shape, echo_runtime_fs_chmod, echo_runtime_fs_copy,
    echo_runtime_fs_create_dir, echo_runtime_fs_create_dir_all, echo_runtime_fs_create_temp,
    echo_runtime_fs_exists, echo_runtime_fs_file_close, echo_runtime_fs_file_read,
    echo_runtime_fs_file_seek, echo_runtime_fs_file_write, echo_runtime_fs_is_dir,
    echo_runtime_fs_is_file, echo_runtime_fs_join, echo_runtime_fs_metadata,
    echo_runtime_fs_open_append, echo_runtime_fs_open_read, echo_runtime_fs_open_write,
    echo_runtime_fs_read, echo_runtime_fs_read_dir, echo_runtime_fs_remove,
    echo_runtime_fs_remove_dir, echo_runtime_fs_rename, echo_runtime_fs_symlink,
    echo_runtime_fs_temp_dir, echo_runtime_fs_write, echo_runtime_git_index_pack,
    echo_runtime_git_pack_count, echo_runtime_git_pack_valid, echo_runtime_git_receive_pack_split,
    echo_runtime_git_upload_pack_wants, echo_runtime_gzip_compress, echo_runtime_gzip_decompress,
    echo_runtime_hex_decode, echo_runtime_hex_encode, echo_runtime_hmac_sha256,
    echo_runtime_http_headers_complete, echo_runtime_http_parse_request,
    echo_runtime_http_request_complete, echo_runtime_json_parse, echo_runtime_json_stringify,
    echo_runtime_list_get, echo_runtime_list_len, echo_runtime_list_new,
    echo_runtime_list_new_empty_lists, echo_runtime_list_push, echo_runtime_list_reserve,
    echo_runtime_list_set, echo_runtime_locator_class, echo_runtime_locator_from_string,
    echo_runtime_locator_from_utf8, echo_runtime_math_abs_f, echo_runtime_math_abs_i,
    echo_runtime_math_ceil, echo_runtime_math_cos, echo_runtime_math_floor, echo_runtime_math_pow,
    echo_runtime_math_sin, echo_runtime_math_sqrt, echo_runtime_math_tan, echo_runtime_ne,
    echo_runtime_ne_id, echo_runtime_now_mono_ms, echo_runtime_now_ms, echo_runtime_os_chdir,
    echo_runtime_os_cwd, echo_runtime_os_hostname, echo_runtime_os_pid, echo_runtime_os_platform,
    echo_runtime_parse_f64, echo_runtime_parse_i64, echo_runtime_path_clean, echo_runtime_path_rel,
    echo_runtime_print_i64, echo_runtime_process_args, echo_runtime_process_env_get,
    echo_runtime_process_env_has, echo_runtime_process_env_set, echo_runtime_process_env_unset,
    echo_runtime_process_exit, echo_runtime_process_pipe_close, echo_runtime_process_pipe_read,
    echo_runtime_process_pipe_write, echo_runtime_process_run, echo_runtime_process_run_capture,
    echo_runtime_process_run_cwd, echo_runtime_process_spawn_pipes, echo_runtime_process_wait,
    echo_runtime_random_float, echo_runtime_random_seed, echo_runtime_random_u64,
    echo_runtime_range_new, echo_runtime_reflect_key_bytes, echo_runtime_reflect_kind,
    echo_runtime_reflect_kind_name, echo_runtime_scope_disown, echo_runtime_scope_enter,
    echo_runtime_scope_exit, echo_runtime_scope_promote, echo_runtime_scope_register,
    echo_runtime_scope_release, echo_runtime_sha256, echo_runtime_sha512, echo_runtime_sleep_ms,
    echo_runtime_stdin_read, echo_runtime_store_cas, echo_runtime_store_cas_create,
    echo_runtime_store_close, echo_runtime_store_get, echo_runtime_store_get_if_none_match,
    echo_runtime_store_get_range, echo_runtime_store_head, echo_runtime_store_keys,
    echo_runtime_store_memory_new, echo_runtime_store_put, echo_runtime_store_s3_new,
    echo_runtime_str_cat, echo_runtime_str_contains, echo_runtime_str_ends_with,
    echo_runtime_str_from_bytes, echo_runtime_str_from_debug, echo_runtime_str_from_duration,
    echo_runtime_str_from_float, echo_runtime_str_from_int, echo_runtime_str_from_locator,
    echo_runtime_str_get, echo_runtime_str_len, echo_runtime_str_repeat, echo_runtime_str_replace,
    echo_runtime_str_slice, echo_runtime_str_split, echo_runtime_str_starts_with,
    echo_runtime_str_to_lower, echo_runtime_str_to_upper, echo_runtime_str_trim,
    echo_runtime_string_builder_finish, echo_runtime_string_builder_new,
    echo_runtime_string_builder_push_str, echo_runtime_string_builder_push_value,
    echo_runtime_string_from_utf8, echo_runtime_struct_get, echo_runtime_struct_new,
    echo_runtime_struct_new_named, echo_runtime_struct_set, echo_runtime_struct_type_is,
    echo_runtime_task_after_run, echo_runtime_task_block, echo_runtime_task_block_wide,
    echo_runtime_task_check_joined, echo_runtime_task_join, echo_runtime_task_join_wide,
    echo_runtime_task_shape, echo_runtime_task_spawn_args, echo_runtime_task_spawn_entry,
    echo_runtime_tcp_accept, echo_runtime_tcp_close, echo_runtime_tcp_connect,
    echo_runtime_tcp_listen, echo_runtime_tcp_read, echo_runtime_tcp_write,
    echo_runtime_test_bench_register, echo_runtime_test_fail, echo_runtime_test_finish,
    echo_runtime_test_register, echo_runtime_time_format, echo_runtime_time_parse,
    echo_runtime_tls_accept, echo_runtime_tls_close, echo_runtime_tls_close_listener,
    echo_runtime_tls_connect, echo_runtime_tls_listen, echo_runtime_tls_read,
    echo_runtime_tls_write, echo_runtime_udp_bind, echo_runtime_udp_close,
    echo_runtime_udp_recv_from, echo_runtime_udp_send_to, echo_runtime_unix_accept,
    echo_runtime_unix_close, echo_runtime_unix_connect, echo_runtime_unix_listen,
    echo_runtime_unix_read, echo_runtime_unix_write, echo_runtime_url_parse,
    echo_runtime_utf8_decode, echo_runtime_utf8_valid, echo_runtime_zip_pack,
    echo_runtime_zip_unpack_first,
};

#[allow(clippy::too_many_arguments)]
fn emit_function<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    module: &Module<'ctx>,
    i64t: IntType<'ctx>,
    i128t: IntType<'ctx>,
    function: FunctionValue<'ctx>,
    mir_fn: &MirFn,
    fn_map: &HashMap<String, (FunctionValue<'ctx>, MirRetShape)>,
    dibuilder: &inkwell::debug_info::DebugInfoBuilder<'ctx>,
    diags: &mut Diagnostics,
) {
    emit_function_cfg(
        context, builder, module, i64t, i128t, function, mir_fn, fn_map, dibuilder, diags,
    );
}

/// Emit from SSA CFG (`mir_fn.cfg`): LLVM blocks + φ-nodes + value map.
#[allow(clippy::too_many_arguments)]
fn emit_function_cfg<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    module: &Module<'ctx>,
    i64t: IntType<'ctx>,
    i128t: IntType<'ctx>,
    function: FunctionValue<'ctx>,
    mir_fn: &MirFn,
    fn_map: &HashMap<String, (FunctionValue<'ctx>, MirRetShape)>,
    dibuilder: &inkwell::debug_info::DebugInfoBuilder<'ctx>,
    diags: &mut Diagnostics,
) {
    debug::set_function_line(context, builder, dibuilder, function);
    let line_map = std::fs::read_to_string(&mir_fn.module_path)
        .ok()
        .map(|t| LineMap::from_text(&t));
    let cfg = &mir_fn.cfg;
    // LLVM entry must be the MIR entry block (first append = entry).
    let mut llvm_bbs: HashMap<u32, BasicBlock<'ctx>> = HashMap::new();
    let entry_bb = context.append_basic_block(function, "entry");
    llvm_bbs.insert(cfg.entry.0, entry_bb);
    for b in &cfg.blocks {
        if b.id == cfg.entry {
            continue;
        }
        llvm_bbs.insert(
            b.id.0,
            context.append_basic_block(function, &format!("bb{}", b.id.0)),
        );
    }

    let f64t = context.f64_type();
    let f32t = context.f32_type();
    let i32t = context.i32_type();
    let i16t = context.i16_type();
    let i8t = context.i8_type();
    let i1t = context.bool_type();
    let ptr_ty = context.ptr_type(AddressSpace::default());

    let mut values: HashMap<String, BasicValueEnum<'ctx>> = HashMap::new();
    // Params are SSA `name@0` after construct_ssa — ABI is boxed i64.
    for (i, name) in mir_fn.params.iter().enumerate() {
        let pv = function
            .get_nth_param(i as u32)
            .expect("param")
            .into_int_value();
        values.insert(format!("{name}@0"), pv.as_basic_value_enum());
        values.insert(name.clone(), pv.as_basic_value_enum());
    }

    builder.position_at_end(entry_bb);
    debug::emit_variables(
        context,
        builder,
        dibuilder,
        function,
        &mir_fn.module_path,
        &mir_fn.debug_vars,
        &mir_fn.params,
        |n| values.get(n).copied(),
    );

    // Create empty φ nodes first (typed by representation facts).
    let mut phi_nodes: HashMap<String, inkwell::values::PhiValue<'ctx>> = HashMap::new();
    for b in &cfg.blocks {
        let bb = llvm_bbs[&b.id.0];
        builder.position_at_end(bb);
        for op in &b.ops {
            if let MirOp::Phi { name, .. } = op {
                let rep = mir_fn.reprs.get(name).copied().unwrap_or(MirRepr::Boxed);
                let ty = llvm_type_for_repr(
                    context, i64t, i32t, i16t, i8t, f64t, f32t, i1t, ptr_ty, rep,
                );
                let phi = builder.build_phi(ty, name).expect("phi");
                values.insert(name.clone(), phi.as_basic_value());
                phi_nodes.insert(name.clone(), phi);
            } else {
                break;
            }
        }
    }

    let mut cx = EmitCx {
        context,
        builder,
        module,
        i64t,
        i32t,
        i16t,
        i8t,
        i128t,
        f64t,
        f32t,
        i1t,
        ptr_ty,
        function,
        fn_ret: mir_fn.ret,
        locals: HashMap::new(),
        values,
        reprs: &mir_fn.reprs,
        match_payloads: HashMap::new(),
        fn_map,
        diags,
        loops: Vec::new(),
        tagged_locals: HashSet::new(),
    };

    // Emit in reverse postorder so defs in predecessors exist before uses
    // (needed for MatchPayload / straight SSA values across edges).
    let order = reverse_postorder(cfg);
    for &bid in &order {
        let b = cfg.block(bid);
        let bb = llvm_bbs[&b.id.0];
        cx.builder.position_at_end(bb);
        // Skip phi ops (already created)
        for op in &b.ops {
            if let (Some(map), Some(span)) = (line_map.as_ref(), op.span()) {
                let (line, col) = map.line_col_1based(span.start);
                debug::set_line_col(context, builder, dibuilder, function, line, col);
            }
            match op {
                MirOp::Phi { .. } => {}
                MirOp::MatchPayload { name } => {
                    if !cx.values.contains_key(name) {
                        if let Some(payload) = cx.match_payloads.get(name).copied() {
                            cx.values
                                .insert(name.clone(), payload.as_basic_value_enum());
                        }
                    }
                }
                MirOp::Set { name, value, .. } => {
                    let want = cx.reprs.get(name).copied().unwrap_or(MirRepr::Unknown);
                    if let Some(v) = emit_expr_as(&mut cx, value, want) {
                        cx.values.insert(name.clone(), v);
                        if std::env::var_os("ECHO_DEBUG_CG").is_some() {
                            eprintln!("cg Set OK name={name:?} want={want:?}");
                        }
                    } else if std::env::var_os("ECHO_DEBUG_CG").is_some() {
                        eprintln!("cg Set FAIL name={name:?} want={want:?} val={value:?}");
                    }
                }
                MirOp::Eval(e) => {
                    let _ = emit_expr_as(&mut cx, e, MirRepr::Unknown);
                }
                MirOp::FieldSet { base, field, value } => {
                    let Some(handle) = emit_expr_i64(&mut cx, base) else {
                        continue;
                    };
                    let Some(v) = emit_expr_i64(&mut cx, value) else {
                        continue;
                    };
                    let (ptr, len) = emit_const_bytes(&mut cx, field.as_bytes());
                    let set_f = cx.module.get_function(RT_STRUCT_SET).expect("struct_set");
                    let _ = cx
                        .builder
                        .build_call(
                            set_f,
                            &[handle.into(), ptr.into(), len.into(), v.into()],
                            "",
                        )
                        .expect("struct_set");
                }
                MirOp::TaskSpawn {
                    module_path,
                    body_symbol,
                    bind,
                } => {
                    emit_stmt(
                        &mut cx,
                        &MirStmt::TaskSpawn {
                            module_path: module_path.clone(),
                            body_symbol: body_symbol.clone(),
                            bind: bind.clone(),
                        },
                    );
                }
                MirOp::TaskSpawnFn {
                    module_path,
                    fn_symbol,
                    args,
                    bind,
                } => {
                    emit_stmt(
                        &mut cx,
                        &MirStmt::TaskSpawnFn {
                            module_path: module_path.clone(),
                            fn_symbol: fn_symbol.clone(),
                            args: args.clone(),
                            bind: bind.clone(),
                        },
                    );
                }
                MirOp::TaskJoin {
                    module_path,
                    body_symbol,
                    handle,
                    bind,
                } => {
                    emit_stmt(
                        &mut cx,
                        &MirStmt::TaskJoin {
                            module_path: module_path.clone(),
                            body_symbol: body_symbol.clone(),
                            handle: handle.clone(),
                            bind: bind.clone(),
                        },
                    );
                }
                MirOp::IndexSet { base, index, value } => {
                    let Some(handle) = emit_expr_i64(&mut cx, base) else {
                        continue;
                    };
                    let Some(idx) = emit_expr_i64(&mut cx, index) else {
                        continue;
                    };
                    let Some(v) = emit_expr_i64(&mut cx, value) else {
                        continue;
                    };
                    let set_f = cx.module.get_function(RT_LIST_SET).expect("list_set");
                    let _ = cx
                        .builder
                        .build_call(set_f, &[handle.into(), idx.into(), v.into()], "")
                        .expect("list_set");
                }
                MirOp::ListPush { base, value } => {
                    let Some(handle) = emit_expr_i64(&mut cx, base) else {
                        continue;
                    };
                    let Some(v) = emit_expr_i64(&mut cx, value) else {
                        continue;
                    };
                    let push_f = cx.module.get_function(RT_LIST_PUSH).expect("list_push");
                    let _ = cx
                        .builder
                        .build_call(push_f, &[handle.into(), v.into()], "")
                        .expect("list_push");
                }
                MirOp::ScopeEnter { id } => {
                    let f = cx.module.get_function(RT_SCOPE_ENTER).expect("scope_enter");
                    let sid = cx.i64t.const_int(*id as u64, false);
                    let _ = cx
                        .builder
                        .build_call(f, &[sid.into()], "")
                        .expect("scope_enter");
                }
                MirOp::ScopeExit { id } => {
                    let f = cx.module.get_function(RT_SCOPE_EXIT).expect("scope_exit");
                    let sid = cx.i64t.const_int(*id as u64, false);
                    let _ = cx
                        .builder
                        .build_call(f, &[sid.into()], "")
                        .expect("scope_exit");
                }
                MirOp::ScopeRegister { value } => {
                    let Some(h) = emit_expr_i64(&mut cx, value) else {
                        continue;
                    };
                    let f = cx
                        .module
                        .get_function(RT_SCOPE_REGISTER)
                        .expect("scope_register");
                    let _ = cx
                        .builder
                        .build_call(f, &[h.into()], "")
                        .expect("scope_register");
                }
                MirOp::ScopePromote { value, target } => {
                    let Some(h) = emit_expr_i64(&mut cx, value) else {
                        continue;
                    };
                    let f = cx
                        .module
                        .get_function(RT_SCOPE_PROMOTE)
                        .expect("scope_promote");
                    let tid = cx.i64t.const_int(*target as u64, false);
                    let _ = cx
                        .builder
                        .build_call(f, &[h.into(), tid.into()], "")
                        .expect("scope_promote");
                }
                MirOp::ScopeDisown { value } => {
                    let Some(h) = emit_expr_i64(&mut cx, value) else {
                        continue;
                    };
                    let f = cx
                        .module
                        .get_function(RT_SCOPE_DISOWN)
                        .expect("scope_disown");
                    let _ = cx
                        .builder
                        .build_call(f, &[h.into()], "")
                        .expect("scope_disown");
                }
                MirOp::ScopeRelease { value } => {
                    let Some(h) = emit_expr_i64(&mut cx, value) else {
                        continue;
                    };
                    let f = cx
                        .module
                        .get_function(RT_SCOPE_RELEASE)
                        .expect("scope_release");
                    let _ = cx
                        .builder
                        .build_call(f, &[h.into()], "")
                        .expect("scope_release");
                }
            }
        }

        if cx
            .builder
            .get_insert_block()
            .and_then(|bb| bb.get_terminator())
            .is_some()
        {
            continue;
        }

        if let (Some(map), Some(span)) = (line_map.as_ref(), b.term.span()) {
            let (line, col) = map.line_col_1based(span.start);
            debug::set_line_col(context, builder, dibuilder, function, line, col);
        }
        emit_terminator_cfg(&mut cx, cfg, &b.term, &llvm_bbs, mir_fn.ret);
    }

    // Fill φ incomings
    for b in &cfg.blocks {
        for op in &b.ops {
            match op {
                MirOp::Phi { name, incomings } => {
                    let Some(phi) = phi_nodes.get(name) else {
                        continue;
                    };
                    let dest_rep = mir_fn.reprs.get(name).copied().unwrap_or(MirRepr::Boxed);
                    for (pred, in_name) in incomings {
                        let pred_bb = llvm_bbs[&pred.0];
                        let val =
                            cx.values.get(in_name).copied().unwrap_or_else(|| {
                                default_value(&cx, dest_rep).as_basic_value_enum()
                            });
                        // Phi requires matching types; values should already match via repr pass.
                        match val {
                            BasicValueEnum::IntValue(iv) => phi.add_incoming(&[(&iv, pred_bb)]),
                            BasicValueEnum::FloatValue(fv) => phi.add_incoming(&[(&fv, pred_bb)]),
                            BasicValueEnum::PointerValue(pv) => phi.add_incoming(&[(&pv, pred_bb)]),
                            other => {
                                let _ = other;
                                let z = i64t.const_int(0, false);
                                phi.add_incoming(&[(&z, pred_bb)]);
                            }
                        }
                    }
                }
                _ => break,
            }
        }
    }
}

fn llvm_type_for_repr<'ctx>(
    _context: &'ctx Context,
    i64t: IntType<'ctx>,
    i32t: IntType<'ctx>,
    i16t: IntType<'ctx>,
    i8t: IntType<'ctx>,
    f64t: FloatType<'ctx>,
    f32t: FloatType<'ctx>,
    i1t: IntType<'ctx>,
    ptr_ty: PointerType<'ctx>,
    rep: MirRepr,
) -> BasicTypeEnum<'ctx> {
    match rep {
        MirRepr::Int64
        | MirRepr::UInt64
        | MirRepr::Duration
        | MirRepr::Boxed
        | MirRepr::Unknown => i64t.into(),
        MirRepr::Int32 | MirRepr::UInt32 => i32t.into(),
        MirRepr::Int16 | MirRepr::UInt16 => i16t.into(),
        MirRepr::Int8 | MirRepr::UInt8 => i8t.into(),
        MirRepr::Bool => i1t.into(),
        MirRepr::Float64 => f64t.into(),
        MirRepr::Float32 => f32t.into(),
        // Runtime heap handles are pointer-width integers today; use `ptr` when
        // we inttoptr at production sites. Prefer i64 handle bits for ABI parity
        // with `echo_runtime_*` until full ptr SSA is universal.
        MirRepr::StringRef
        | MirRepr::BytesRef
        | MirRepr::LocatorRef
        | MirRepr::ObjectRef
        | MirRepr::ListRef => {
            let _ = ptr_ty;
            i64t.into()
        }
    }
}

fn default_value<'ctx>(cx: &EmitCx<'_, 'ctx>, rep: MirRepr) -> BasicValueEnum<'ctx> {
    match rep {
        MirRepr::Bool => cx.i1t.const_int(0, false).as_basic_value_enum(),
        MirRepr::Int32 | MirRepr::UInt32 => cx.i32t.const_int(0, false).as_basic_value_enum(),
        MirRepr::Int16 | MirRepr::UInt16 => cx.i16t.const_int(0, false).as_basic_value_enum(),
        MirRepr::Int8 | MirRepr::UInt8 => cx.i8t.const_int(0, false).as_basic_value_enum(),
        MirRepr::Float64 => cx.f64t.const_float(0.0).as_basic_value_enum(),
        MirRepr::Float32 => cx.f32t.const_float(0.0).as_basic_value_enum(),
        MirRepr::StringRef | MirRepr::ObjectRef | MirRepr::ListRef => {
            cx.i64t.const_int(0, false).as_basic_value_enum()
        }
        _ => cx.i64t.const_int(0, false).as_basic_value_enum(),
    }
}

fn int_ty_for_repr<'ctx>(cx: &EmitCx<'_, 'ctx>, rep: MirRepr) -> Option<IntType<'ctx>> {
    match rep {
        MirRepr::Int64 | MirRepr::UInt64 => Some(cx.i64t),
        MirRepr::Int32 | MirRepr::UInt32 => Some(cx.i32t),
        MirRepr::Int16 | MirRepr::UInt16 => Some(cx.i16t),
        MirRepr::Int8 | MirRepr::UInt8 => Some(cx.i8t),
        _ => None,
    }
}

fn width_to_repr(w: echo_ast::Width) -> MirRepr {
    match w {
        echo_ast::Width::I8 => MirRepr::Int8,
        echo_ast::Width::I16 => MirRepr::Int16,
        echo_ast::Width::I32 => MirRepr::Int32,
        echo_ast::Width::I64 => MirRepr::Int64,
        echo_ast::Width::Ui8 => MirRepr::UInt8,
        echo_ast::Width::Ui16 => MirRepr::UInt16,
        echo_ast::Width::Ui32 => MirRepr::UInt32,
        echo_ast::Width::Ui64 => MirRepr::UInt64,
        echo_ast::Width::F32 => MirRepr::Float32,
        echo_ast::Width::F64 => MirRepr::Float64,
    }
}

fn emit_int_cast<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    v: BasicValueEnum<'ctx>,
    from: MirRepr,
    to: echo_ast::Width,
) -> Option<(BasicValueEnum<'ctx>, MirRepr)> {
    emit_width_cast(cx, v, from, to)
}

fn float_ty_for_width<'ctx>(
    cx: &EmitCx<'_, 'ctx>,
    w: echo_ast::Width,
) -> Option<inkwell::types::FloatType<'ctx>> {
    match w {
        echo_ast::Width::F32 => Some(cx.f32t),
        echo_ast::Width::F64 => Some(cx.f64t),
        _ => None,
    }
}

fn as_float_value<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    v: BasicValueEnum<'ctx>,
    from: MirRepr,
) -> Option<inkwell::values::FloatValue<'ctx>> {
    if v.is_float_value() {
        return Some(v.into_float_value());
    }
    if matches!(from, MirRepr::Float32 | MirRepr::Float64) {
        let uv = unbox_value(cx, v, from)?;
        if uv.is_float_value() {
            return Some(uv.into_float_value());
        }
    }
    None
}

fn emit_width_cast<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    v: BasicValueEnum<'ctx>,
    from: MirRepr,
    to: echo_ast::Width,
) -> Option<(BasicValueEnum<'ctx>, MirRepr)> {
    let to_rep = width_to_repr(to);
    let from_float = matches!(from, MirRepr::Float32 | MirRepr::Float64) || v.is_float_value();

    if to.is_float() {
        let ft = float_ty_for_width(cx, to)?;
        if from_float {
            let fv = as_float_value(cx, v, from)?;
            let src_ty = fv.get_type();
            let out = if src_ty == ft {
                fv
            } else if to == echo_ast::Width::F64 {
                cx.builder
                    .build_float_ext(fv, ft, "cast.fpext")
                    .expect("fpext")
            } else {
                cx.builder
                    .build_float_trunc(fv, ft, "cast.fptrunc")
                    .expect("fptrunc")
            };
            return Some((out.as_basic_value_enum(), to_rep));
        }
        let iv = if v.is_int_value() {
            v.into_int_value()
        } else {
            box_value(cx, v, from)?
        };
        let signed = !from.is_unsigned_int();
        let out = if signed {
            cx.builder
                .build_signed_int_to_float(iv, ft, "cast.sitofp")
                .expect("sitofp")
        } else {
            cx.builder
                .build_unsigned_int_to_float(iv, ft, "cast.uitofp")
                .expect("uitofp")
        };
        return Some((out.as_basic_value_enum(), to_rep));
    }

    if !to.is_int() {
        return None;
    }
    let to_ty = int_ty_for_repr(cx, to_rep)?;

    if from_float {
        let fv = as_float_value(cx, v, from)?;
        let out = if to.is_unsigned_int() {
            cx.builder
                .build_float_to_unsigned_int(fv, to_ty, "cast.fptoui")
                .expect("fptoui")
        } else {
            cx.builder
                .build_float_to_signed_int(fv, to_ty, "cast.fptosi")
                .expect("fptosi")
        };
        return Some((out.as_basic_value_enum(), to_rep));
    }

    // Universal/boxed i64 → truncate/zext into target int width (e.g. `<ui8> call(...)`).
    if from.is_universal() || from == MirRepr::Int64 || from == MirRepr::UInt64 {
        let iv = v.into_int_value();
        let out = if to_ty.get_bit_width() == 64 {
            iv
        } else {
            cx.builder
                .build_int_truncate(iv, to_ty, "cast.trunc")
                .expect("trunc")
        };
        return Some((out.as_basic_value_enum(), to_rep));
    }
    if !from.is_native_int() {
        return None;
    }
    let from_ty = int_ty_for_repr(cx, from)?;
    let iv = v.into_int_value();
    let from_bits = from_ty.get_bit_width();
    let to_bits = to_ty.get_bit_width();
    let out = if to_bits == from_bits {
        iv
    } else if to_bits > from_bits {
        if from.is_unsigned_int() {
            cx.builder
                .build_int_z_extend(iv, to_ty, "zext")
                .expect("zext")
        } else {
            cx.builder
                .build_int_s_extend(iv, to_ty, "sext")
                .expect("sext")
        }
    } else {
        cx.builder
            .build_int_truncate(iv, to_ty, "trunc")
            .expect("trunc")
    };
    Some((out.as_basic_value_enum(), to_rep))
}

fn reverse_postorder(cfg: &echo_mir::MirCfg) -> Vec<BlockId> {
    let n = cfg.blocks.len();
    let mut seen = vec![false; n];
    let mut post = Vec::new();
    fn dfs(cfg: &echo_mir::MirCfg, id: BlockId, seen: &mut [bool], post: &mut Vec<BlockId>) {
        if seen[id.0 as usize] {
            return;
        }
        seen[id.0 as usize] = true;
        for s in cfg.successors(id) {
            dfs(cfg, s, seen, post);
        }
        post.push(id);
    }
    dfs(cfg, cfg.entry, &mut seen, &mut post);
    // unreachable blocks (if any)
    for b in &cfg.blocks {
        dfs(cfg, b.id, &mut seen, &mut post);
    }
    post.reverse();
    post
}

fn emit_terminator_cfg<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    cfg: &echo_mir::MirCfg,
    term: &Terminator,
    llvm_bbs: &HashMap<u32, BasicBlock<'ctx>>,
    fn_ret: MirRetShape,
) {
    match term {
        Terminator::Goto(t) => {
            let _ = cx.builder.build_unconditional_branch(llvm_bbs[&t.0]);
        }
        Terminator::Branch {
            cond,
            then_bb,
            else_bb,
        } => {
            let Some(is_true) = emit_condition(cx, cond) else {
                let _ = cx.builder.build_unconditional_branch(llvm_bbs[&else_bb.0]);
                return;
            };
            let _ = cx.builder.build_conditional_branch(
                is_true,
                llvm_bbs[&then_bb.0],
                llvm_bbs[&else_bb.0],
            );
        }
        Terminator::MatchTagged {
            scrutinee,
            ok_bb,
            err_bb,
        } => {
            let Some(packed) = emit_expr_tagged(cx, scrutinee) else {
                let _ = cx.builder.build_unconditional_branch(llvm_bbs[&err_bb.0]);
                return;
            };
            let (tag, payload) = unpack_tag_payload(cx, packed);
            // Bind every payload name in the successor blocks (not a single
            // slot — nested `|` / `&` must not clobber an outer arm).
            for target in [*ok_bb, *err_bb] {
                for name in echo_mir::match_payload_names(&cfg.block(target).ops) {
                    cx.match_payloads.insert(name.clone(), payload);
                    cx.values.insert(name, payload.as_basic_value_enum());
                }
            }
            let zero = cx.i64t.const_int(0, false);
            let is_ok = cx
                .builder
                .build_int_compare(IntPredicate::EQ, tag, zero, "is_ok")
                .expect("cmp tag");
            let _ =
                cx.builder
                    .build_conditional_branch(is_ok, llvm_bbs[&ok_bb.0], llvm_bbs[&err_bb.0]);
        }
        Terminator::ReturnOk(e, _) => {
            if let Some(payload) = emit_expr_i64(cx, e) {
                match fn_ret {
                    MirRetShape::Plain => {
                        let _ = cx.builder.build_return(Some(&payload));
                    }
                    MirRetShape::Result => {
                        let p = pack_tagged(cx, TAG_OK, payload);
                        let _ = cx.builder.build_return(Some(&p));
                    }
                    MirRetShape::Option => {
                        let p = pack_tagged(cx, TAG_SOME, payload);
                        let _ = cx.builder.build_return(Some(&p));
                    }
                }
            }
        }
        Terminator::ReturnErr(e) => {
            if let Some(payload) = emit_expr_i64(cx, e) {
                let p = pack_tagged(cx, TAG_ERR, payload);
                let _ = cx.builder.build_return(Some(&p));
            }
        }
        Terminator::ReturnNone => {
            let zero = cx.i64t.const_int(0, false);
            let p = pack_tagged(cx, TAG_NONE, zero);
            let _ = cx.builder.build_return(Some(&p));
        }
        Terminator::Unreachable => {
            let z = cx.i64t.const_int(0, false);
            let _ = cx.builder.build_return(Some(&z));
        }
    }
}

struct EmitCx<'a, 'ctx> {
    context: &'ctx Context,
    builder: &'a Builder<'ctx>,
    module: &'a Module<'ctx>,
    i64t: IntType<'ctx>,
    i32t: IntType<'ctx>,
    i16t: IntType<'ctx>,
    i8t: IntType<'ctx>,
    i128t: IntType<'ctx>,
    f64t: FloatType<'ctx>,
    f32t: FloatType<'ctx>,
    i1t: IntType<'ctx>,
    #[allow(dead_code)] // reserved for full ptr-SSA ref lowering
    ptr_ty: PointerType<'ctx>,
    function: FunctionValue<'ctx>,
    fn_ret: MirRetShape,
    /// Legacy alloca locals (structured emit helpers still used for nested paths).
    locals: HashMap<String, PointerValue<'ctx>>,
    /// SSA name → native or boxed LLVM value.
    values: HashMap<String, BasicValueEnum<'ctx>>,
    /// Representation facts for SSA names.
    reprs: &'a HashMap<String, MirRepr>,
    /// Payload per MatchPayload SSA name (nested matches keep distinct bindings).
    match_payloads: HashMap<String, IntValue<'ctx>>,
    fn_map: &'a HashMap<String, (FunctionValue<'ctx>, MirRetShape)>,
    diags: &'a mut Diagnostics,
    /// Nested loops: (continue_target, break_target) — structured fallback only.
    loops: Vec<(BasicBlock<'ctx>, BasicBlock<'ctx>)>,
    /// Locals that hold packed result/option from task join (`- name =`).
    tagged_locals: HashSet<String>,
}

// --- Legacy structured MIR emit (kept for reference; codegen walks SSA CFG) ---
#[allow(dead_code)]
fn emit_block<'ctx>(cx: &mut EmitCx<'_, 'ctx>, body: &[MirStmt]) {
    for stmt in body {
        if cx
            .builder
            .get_insert_block()
            .and_then(|bb| bb.get_terminator())
            .is_some()
        {
            break;
        }
        emit_stmt(cx, stmt);
    }
}

#[allow(dead_code)]
/// Code pointer bits + ret shape code (0 plain / 1 result / 2 option).
fn emit_body_code_and_shape<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    module_path: &std::path::Path,
    body_symbol: &str,
) -> Option<(inkwell::values::IntValue<'ctx>, i64)> {
    let key = mangle_fn(module_path, body_symbol);
    let (fv, ret) = match cx.fn_map.get(&key) {
        Some(x) => *x,
        None => {
            cx.diags.push(
                Diagnostic::error(format!("unknown task body `{body_symbol}` ({key})"))
                    .with_code("cg-task"),
            );
            return None;
        }
    };
    let ptr = fv.as_global_value().as_pointer_value();
    let code = cx
        .builder
        .build_ptr_to_int(ptr, cx.i64t, "task.code")
        .expect("ptrtoint");
    let shape: i64 = match ret {
        MirRetShape::Plain => 0,
        MirRetShape::Result => 1,
        MirRetShape::Option => 2,
    };
    Some((code, shape))
}

fn ensure_local_i128<'ctx>(cx: &mut EmitCx<'_, 'ctx>, name: &str) -> PointerValue<'ctx> {
    let key = format!("__i128_{name}");
    if let Some(p) = cx.locals.get(&key) {
        return *p;
    }
    let slot = cx
        .builder
        .build_alloca(cx.i128t, &key)
        .expect("alloca i128 local");
    let _ = cx.builder.build_store(slot, cx.i128t.const_int(0, false));
    cx.locals.insert(key, slot);
    slot
}

/// Bind a task handle/result into SSA `values` and legacy alloca (CFG emit).
fn bind_task_local<'ctx>(cx: &mut EmitCx<'_, 'ctx>, name: &str, v: IntValue<'ctx>) {
    cx.values.insert(name.to_string(), v.as_basic_value_enum());
    let slot = ensure_local(cx, name);
    let _ = cx.builder.build_store(slot, v);
}

fn bind_task_local_wide<'ctx>(cx: &mut EmitCx<'_, 'ctx>, name: &str, packed: IntValue<'ctx>) {
    let wslot = ensure_local_i128(cx, name);
    let _ = cx.builder.build_store(wslot, packed);
    let low = cx
        .builder
        .build_int_truncate(packed, cx.i64t, "task.low")
        .expect("trunc");
    bind_task_local(cx, name, low);
    cx.tagged_locals.insert(name.to_string());
}

fn emit_stmt<'ctx>(cx: &mut EmitCx<'_, 'ctx>, stmt: &MirStmt) {
    match stmt {
        MirStmt::Set { name, value, .. } => {
            // Locals are plain i64 payloads (tags only on function returns / match temps).
            if let Some(v) = emit_expr_i64(cx, value) {
                let slot = ensure_local(cx, name);
                let _ = cx.builder.build_store(slot, v);
            }
        }
        MirStmt::ReturnOk(e, _) => {
            if let Some(payload) = emit_expr_i64(cx, e) {
                match cx.fn_ret {
                    MirRetShape::Plain => {
                        let _ = cx.builder.build_return(Some(&payload));
                    }
                    MirRetShape::Result => {
                        let p = pack_tagged(cx, TAG_OK, payload);
                        let _ = cx.builder.build_return(Some(&p));
                    }
                    MirRetShape::Option => {
                        let p = pack_tagged(cx, TAG_SOME, payload);
                        let _ = cx.builder.build_return(Some(&p));
                    }
                }
            }
        }
        MirStmt::ReturnErr(e) => {
            // `! expr` — result err path only (not panic / process abort).
            if let Some(payload) = emit_expr_i64(cx, e) {
                let p = pack_tagged(cx, TAG_ERR, payload);
                let _ = cx.builder.build_return(Some(&p));
            }
        }
        MirStmt::ReturnNone => {
            let zero = cx.i64t.const_int(0, false);
            let p = pack_tagged(cx, TAG_NONE, zero);
            let _ = cx.builder.build_return(Some(&p));
        }
        MirStmt::Eval(e) => {
            let _ = emit_expr_i64(cx, e);
        }
        MirStmt::If { arms, else_body } => {
            emit_if(cx, arms, else_body.as_deref());
        }
        MirStmt::MatchTagged {
            scrutinee,
            ok_name,
            ok_body,
            err_name,
            err_body,
        } => {
            emit_match_tagged(
                cx,
                scrutinee,
                ok_name.as_deref(),
                ok_body,
                err_name.as_deref(),
                err_body,
            );
        }
        MirStmt::Loop { cond, body } => emit_loop(cx, cond.as_ref(), body),
        MirStmt::ForIn { item, iter, body } => emit_for_in(cx, item, iter, body),
        MirStmt::Break => {
            let Some((_, brk)) = cx.loops.last().copied() else {
                cx.diags
                    .push(Diagnostic::error("break outside loop in codegen").with_code("cg-break"));
                return;
            };
            let _ = cx.builder.build_unconditional_branch(brk);
        }
        MirStmt::Continue => {
            let Some((cont, _)) = cx.loops.last().copied() else {
                cx.diags.push(
                    Diagnostic::error("continue outside loop in codegen").with_code("cg-continue"),
                );
                return;
            };
            let _ = cx.builder.build_unconditional_branch(cont);
        }
        MirStmt::FieldSet { base, field, value } => {
            let Some(handle) = emit_expr_i64(cx, base) else {
                return;
            };
            let Some(v) = emit_expr_i64(cx, value) else {
                return;
            };
            let (ptr, len) = emit_const_bytes(cx, field.as_bytes());
            let set_f = cx.module.get_function(RT_STRUCT_SET).expect("struct_set");
            let _ = cx
                .builder
                .build_call(
                    set_f,
                    &[handle.into(), ptr.into(), len.into(), v.into()],
                    "",
                )
                .expect("struct_set");
        }
        MirStmt::TaskSpawn {
            module_path,
            body_symbol,
            bind,
        } => {
            let Some((code, shape)) = emit_body_code_and_shape(cx, module_path, body_symbol) else {
                return;
            };
            let f = cx
                .module
                .get_function(RT_TASK_SPAWN_ENTRY)
                .expect("task_spawn_entry");
            let shape_v = cx.i64t.const_int(shape as u64, false);
            let call = cx
                .builder
                .build_call(f, &[code.into(), shape_v.into()], "task_spawn")
                .expect("task_spawn");
            let handle = call.try_as_basic_value().unwrap_basic().into_int_value();
            if let Some(name) = bind {
                bind_task_local(cx, name, handle);
            }
        }
        MirStmt::TaskSpawnFn {
            module_path,
            fn_symbol,
            args,
            bind,
        } => {
            let Some((code, shape)) = emit_body_code_and_shape(cx, module_path, fn_symbol) else {
                return;
            };
            if args.len() > 8 {
                cx.diags.push(
                    Diagnostic::error("task spawn supports at most 8 arguments")
                        .with_code("cg-task"),
                );
                return;
            }
            let mut argv = Vec::new();
            for a in args {
                let Some(v) = emit_expr_i64(cx, a) else {
                    return;
                };
                argv.push(v);
            }
            while argv.len() < 8 {
                argv.push(cx.i64t.const_int(0, false));
            }
            let f = cx
                .module
                .get_function(RT_TASK_SPAWN_ARGS)
                .expect("task_spawn_args");
            let shape_v = cx.i64t.const_int(shape as u64, false);
            let argc = cx.i64t.const_int(args.len() as u64, false);
            let call = cx
                .builder
                .build_call(
                    f,
                    &[
                        code.into(),
                        shape_v.into(),
                        argc.into(),
                        argv[0].into(),
                        argv[1].into(),
                        argv[2].into(),
                        argv[3].into(),
                        argv[4].into(),
                        argv[5].into(),
                        argv[6].into(),
                        argv[7].into(),
                    ],
                    "task_spawn_fn",
                )
                .expect("task_spawn_args");
            let handle = call.try_as_basic_value().unwrap_basic().into_int_value();
            if let Some(name) = bind {
                bind_task_local(cx, name, handle);
            }
        }
        MirStmt::TaskJoin {
            module_path,
            body_symbol,
            handle,
            bind,
        } => {
            // Immediate block: know shape from body. Handle join: use wide + shape on handle.
            if let Some(sym) = body_symbol {
                let Some((code, shape)) = emit_body_code_and_shape(cx, module_path, sym) else {
                    return;
                };
                let shape_v = cx.i64t.const_int(shape as u64, false);
                if shape == 0 {
                    let f = cx.module.get_function(RT_TASK_BLOCK).expect("task_block");
                    let call = cx
                        .builder
                        .build_call(f, &[code.into(), shape_v.into()], "task_block")
                        .expect("task_block");
                    let result = call.try_as_basic_value().unwrap_basic().into_int_value();
                    if let Some(name) = bind {
                        bind_task_local(cx, name, result);
                    }
                } else {
                    let f = cx
                        .module
                        .get_function(RT_TASK_BLOCK_WIDE)
                        .expect("task_block_wide");
                    let call = cx
                        .builder
                        .build_call(f, &[code.into(), shape_v.into()], "task_block_w")
                        .expect("task_block_wide");
                    let packed = call.try_as_basic_value().unwrap_basic().into_int_value();
                    if let Some(name) = bind {
                        bind_task_local_wide(cx, name, packed);
                    }
                }
            } else if let Some(h) = handle {
                let Some(hv) = emit_expr_i64(cx, h) else {
                    return;
                };
                // One wide join (marks joined once); low bits are the plain payload.
                let fw = cx
                    .module
                    .get_function(RT_TASK_JOIN_WIDE)
                    .expect("task_join_wide");
                let packed = cx
                    .builder
                    .build_call(fw, &[hv.into()], "task_join_w")
                    .expect("task_join_wide")
                    .try_as_basic_value()
                    .unwrap_basic()
                    .into_int_value();
                if let Some(name) = bind {
                    bind_task_local_wide(cx, name, packed);
                }
            } else {
                cx.diags.push(
                    Diagnostic::error("task join without body or handle").with_code("cg-task"),
                );
            }
        }
        MirStmt::IndexSet { base, index, value } => {
            let Some(handle) = emit_expr_i64(cx, base) else {
                return;
            };
            let Some(idx) = emit_expr_i64(cx, index) else {
                return;
            };
            let Some(v) = emit_expr_i64(cx, value) else {
                return;
            };
            let set_f = cx.module.get_function(RT_LIST_SET).expect("list_set");
            let _ = cx
                .builder
                .build_call(set_f, &[handle.into(), idx.into(), v.into()], "")
                .expect("list_set");
        }
        MirStmt::ListPush { base, value } => {
            let Some(handle) = emit_expr_i64(cx, base) else {
                return;
            };
            let Some(v) = emit_expr_i64(cx, value) else {
                return;
            };
            let push_f = cx.module.get_function(RT_LIST_PUSH).expect("list_push");
            let _ = cx
                .builder
                .build_call(push_f, &[handle.into(), v.into()], "")
                .expect("list_push");
        }
        MirStmt::ScopeEnter { id } => {
            let f = cx.module.get_function(RT_SCOPE_ENTER).expect("scope_enter");
            let sid = cx.i64t.const_int(*id as u64, false);
            let _ = cx
                .builder
                .build_call(f, &[sid.into()], "")
                .expect("scope_enter");
        }
        MirStmt::ScopeExit { id } => {
            let f = cx.module.get_function(RT_SCOPE_EXIT).expect("scope_exit");
            let sid = cx.i64t.const_int(*id as u64, false);
            let _ = cx
                .builder
                .build_call(f, &[sid.into()], "")
                .expect("scope_exit");
        }
        MirStmt::ScopeRegister { value } => {
            let Some(h) = emit_expr_i64(cx, value) else {
                return;
            };
            let f = cx
                .module
                .get_function(RT_SCOPE_REGISTER)
                .expect("scope_register");
            let _ = cx
                .builder
                .build_call(f, &[h.into()], "")
                .expect("scope_register");
        }
        MirStmt::ScopePromote { value, target } => {
            let Some(h) = emit_expr_i64(cx, value) else {
                return;
            };
            let f = cx
                .module
                .get_function(RT_SCOPE_PROMOTE)
                .expect("scope_promote");
            let tid = cx.i64t.const_int(*target as u64, false);
            let _ = cx
                .builder
                .build_call(f, &[h.into(), tid.into()], "")
                .expect("scope_promote");
        }
        MirStmt::ScopeDisown { value } => {
            let Some(h) = emit_expr_i64(cx, value) else {
                return;
            };
            let f = cx
                .module
                .get_function(RT_SCOPE_DISOWN)
                .expect("scope_disown");
            let _ = cx
                .builder
                .build_call(f, &[h.into()], "")
                .expect("scope_disown");
        }
        MirStmt::ScopeRelease { value } => {
            let Some(h) = emit_expr_i64(cx, value) else {
                return;
            };
            let f = cx
                .module
                .get_function(RT_SCOPE_RELEASE)
                .expect("scope_release");
            let _ = cx
                .builder
                .build_call(f, &[h.into()], "")
                .expect("scope_release");
        }
    }
}

#[allow(dead_code)]
fn emit_loop<'ctx>(cx: &mut EmitCx<'_, 'ctx>, cond: Option<&MirExpr>, body: &[MirStmt]) {
    let header = cx.context.append_basic_block(cx.function, "loop.header");
    let body_bb = cx.context.append_basic_block(cx.function, "loop.body");
    let after = cx.context.append_basic_block(cx.function, "loop.after");

    let _ = cx.builder.build_unconditional_branch(header);
    cx.builder.position_at_end(header);

    match cond {
        None => {
            let _ = cx.builder.build_unconditional_branch(body_bb);
        }
        Some(c) => {
            let cond_v = match emit_expr_i64(cx, c) {
                Some(v) => v,
                None => {
                    let _ = cx.builder.build_unconditional_branch(after);
                    cx.builder.position_at_end(after);
                    return;
                }
            };
            let zero = cx.i64t.const_int(0, false);
            let is_true = cx
                .builder
                .build_int_compare(IntPredicate::NE, cond_v, zero, "loop.cond")
                .expect("cmp");
            let _ = cx.builder.build_conditional_branch(is_true, body_bb, after);
        }
    }

    cx.builder.position_at_end(body_bb);
    // continue → header; break → after
    cx.loops.push((header, after));
    emit_block(cx, body);
    cx.loops.pop();
    if cx
        .builder
        .get_insert_block()
        .and_then(|bb| bb.get_terminator())
        .is_none()
    {
        let _ = cx.builder.build_unconditional_branch(header);
    }

    cx.builder.position_at_end(after);
}

#[allow(dead_code)]
fn emit_for_in<'ctx>(cx: &mut EmitCx<'_, 'ctx>, item: &str, iter: &MirExpr, body: &[MirStmt]) {
    // Runtime for-in over a list handle:
    //   i = 0
    //   header: i < len(list) ? body : after
    //   body: item = get(list, i); …; cont: i++; br header
    let list = match emit_expr_i64(cx, iter) {
        Some(v) => v,
        None => return,
    };

    let idx_slot = cx.builder.build_alloca(cx.i64t, "for.i").expect("alloca i");
    let _ = cx
        .builder
        .build_store(idx_slot, cx.i64t.const_int(0, false));

    let header = cx.context.append_basic_block(cx.function, "for.header");
    let body_bb = cx.context.append_basic_block(cx.function, "for.body");
    let cont_bb = cx.context.append_basic_block(cx.function, "for.cont");
    let after = cx.context.append_basic_block(cx.function, "for.after");

    let _ = cx.builder.build_unconditional_branch(header);
    cx.builder.position_at_end(header);

    let i_val = cx
        .builder
        .build_load(cx.i64t, idx_slot, "i")
        .expect("load i")
        .into_int_value();
    let len_f = cx.module.get_function(RT_LIST_LEN).expect("list_len");
    let len = cx
        .builder
        .build_call(len_f, &[list.into()], "len")
        .expect("len")
        .try_as_basic_value()
        .unwrap_basic()
        .into_int_value();
    let in_range = cx
        .builder
        .build_int_compare(IntPredicate::SLT, i_val, len, "in_range")
        .expect("cmp");
    let _ = cx
        .builder
        .build_conditional_branch(in_range, body_bb, after);

    cx.builder.position_at_end(body_bb);
    let get_f = cx.module.get_function(RT_LIST_GET).expect("list_get");
    let i_val = cx
        .builder
        .build_load(cx.i64t, idx_slot, "i")
        .expect("load i")
        .into_int_value();
    let elem = cx
        .builder
        .build_call(get_f, &[list.into(), i_val.into()], "elem")
        .expect("get")
        .try_as_basic_value()
        .unwrap_basic()
        .into_int_value();
    let item_slot = ensure_local(cx, item);
    let _ = cx.builder.build_store(item_slot, elem);

    // continue → cont (i++); break → after
    cx.loops.push((cont_bb, after));
    emit_block(cx, body);
    cx.loops.pop();
    if cx
        .builder
        .get_insert_block()
        .and_then(|bb| bb.get_terminator())
        .is_none()
    {
        let _ = cx.builder.build_unconditional_branch(cont_bb);
    }

    cx.builder.position_at_end(cont_bb);
    let i_val = cx
        .builder
        .build_load(cx.i64t, idx_slot, "i")
        .expect("load i")
        .into_int_value();
    let one = cx.i64t.const_int(1, false);
    let next = cx.builder.build_int_add(i_val, one, "i.next").expect("add");
    let _ = cx.builder.build_store(idx_slot, next);
    let _ = cx.builder.build_unconditional_branch(header);

    cx.builder.position_at_end(after);
}

fn pack_tagged<'ctx>(cx: &EmitCx<'_, 'ctx>, tag: i64, payload: IntValue<'ctx>) -> IntValue<'ctx> {
    let pay = cx
        .builder
        .build_int_z_extend(payload, cx.i128t, "pay")
        .expect("zext payload");
    if tag == 0 {
        return pay;
    }
    let tag_shift = cx.i128t.const_int(64, false);
    let one = cx.i128t.const_int(1, false);
    let tag_bit = cx
        .builder
        .build_left_shift(one, tag_shift, "tagbit")
        .expect("shl");
    // For tag values > 1 would multiply; v1 tags are 0/1 only.
    cx.builder.build_or(pay, tag_bit, "packed").expect("or")
}

fn unpack_tag_payload<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    packed: IntValue<'ctx>,
) -> (IntValue<'ctx>, IntValue<'ctx>) {
    let payload = cx
        .builder
        .build_int_truncate(packed, cx.i64t, "payload")
        .expect("trunc payload");
    let shift = cx.i128t.const_int(64, false);
    let tag_wide = cx
        .builder
        .build_right_shift(packed, shift, false, "tagw")
        .expect("lshr");
    let tag = cx
        .builder
        .build_int_truncate(tag_wide, cx.i64t, "tag")
        .expect("trunc tag");
    (tag, payload)
}

#[allow(dead_code)]
fn emit_match_tagged<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    scrutinee: &MirExpr,
    ok_name: Option<&str>,
    ok_body: &[MirStmt],
    err_name: Option<&str>,
    err_body: &[MirStmt],
) {
    let packed = match emit_expr_tagged(cx, scrutinee) {
        Some(p) => p,
        None => return,
    };
    let (tag, payload) = unpack_tag_payload(cx, packed);

    let ok_bb = cx.context.append_basic_block(cx.function, "match.ok");
    let err_bb = cx.context.append_basic_block(cx.function, "match.err");
    let merge = cx.context.append_basic_block(cx.function, "match.merge");

    let zero = cx.i64t.const_int(0, false);
    let is_ok = cx
        .builder
        .build_int_compare(IntPredicate::EQ, tag, zero, "is_ok")
        .expect("cmp tag");
    let _ = cx.builder.build_conditional_branch(is_ok, ok_bb, err_bb);

    cx.builder.position_at_end(ok_bb);
    if let Some(name) = ok_name {
        let slot = ensure_local(cx, name);
        let _ = cx.builder.build_store(slot, payload);
    }
    emit_block(cx, ok_body);
    if cx
        .builder
        .get_insert_block()
        .and_then(|bb| bb.get_terminator())
        .is_none()
    {
        let _ = cx.builder.build_unconditional_branch(merge);
    }

    cx.builder.position_at_end(err_bb);
    if let Some(name) = err_name {
        let slot = ensure_local(cx, name);
        let _ = cx.builder.build_store(slot, payload);
    }
    emit_block(cx, err_body);
    if cx
        .builder
        .get_insert_block()
        .and_then(|bb| bb.get_terminator())
        .is_none()
    {
        let _ = cx.builder.build_unconditional_branch(merge);
    }

    cx.builder.position_at_end(merge);
}

#[allow(dead_code)]
fn emit_if<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    arms: &[(MirExpr, Vec<MirStmt>)],
    else_body: Option<&[MirStmt]>,
) {
    if arms.is_empty() {
        if let Some(body) = else_body {
            emit_block(cx, body);
        }
        return;
    }

    let merge = cx.context.append_basic_block(cx.function, "if.merge");
    let mut remaining = arms;
    loop {
        let (cond_e, then_body) = &remaining[0];
        let rest = &remaining[1..];

        let cond_v = match emit_expr_i64(cx, cond_e) {
            Some(v) => v,
            None => {
                if cx
                    .builder
                    .get_insert_block()
                    .and_then(|bb| bb.get_terminator())
                    .is_none()
                {
                    let _ = cx.builder.build_unconditional_branch(merge);
                }
                break;
            }
        };
        let zero = cx.i64t.const_int(0, false);
        let is_true = cx
            .builder
            .build_int_compare(IntPredicate::NE, cond_v, zero, "cond")
            .expect("cmp");

        let then_bb = cx.context.append_basic_block(cx.function, "if.then");
        let else_bb = cx.context.append_basic_block(cx.function, "if.else");
        let _ = cx
            .builder
            .build_conditional_branch(is_true, then_bb, else_bb);

        cx.builder.position_at_end(then_bb);
        emit_block(cx, then_body);
        if cx
            .builder
            .get_insert_block()
            .and_then(|bb| bb.get_terminator())
            .is_none()
        {
            let _ = cx.builder.build_unconditional_branch(merge);
        }

        cx.builder.position_at_end(else_bb);
        if rest.is_empty() {
            if let Some(body) = else_body {
                emit_block(cx, body);
            }
            if cx
                .builder
                .get_insert_block()
                .and_then(|bb| bb.get_terminator())
                .is_none()
            {
                let _ = cx.builder.build_unconditional_branch(merge);
            }
            break;
        }
        remaining = rest;
    }

    cx.builder.position_at_end(merge);
}

#[allow(dead_code)]
fn ensure_local<'ctx>(cx: &mut EmitCx<'_, 'ctx>, name: &str) -> PointerValue<'ctx> {
    if let Some(p) = cx.locals.get(name) {
        return *p;
    }
    let slot = cx
        .builder
        .build_alloca(cx.i64t, name)
        .expect("alloca local");
    let _ = cx.builder.build_store(slot, cx.i64t.const_int(0, false));
    cx.locals.insert(name.to_string(), slot);
    slot
}

/// Emit expression coerced to representation `want`.
fn emit_expr_as<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    expr: &MirExpr,
    want: MirRepr,
) -> Option<BasicValueEnum<'ctx>> {
    match expr {
        MirExpr::BoxValue { value, from } => {
            let v = emit_expr_as(cx, value, *from)?;
            Some(box_value(cx, v, *from)?.as_basic_value_enum())
        }
        MirExpr::UnboxValue { value, to } => {
            let v = emit_expr_as(cx, value, MirRepr::Boxed)?;
            unbox_value(cx, v, *to)
        }
        MirExpr::Call { ret, .. } if ret.is_tagged() => {
            cx.diags.push(
                Diagnostic::error(
                    "result/option-shaped call must be handled with `|` match (not used as plain value)",
                )
                .with_code("cg-unhandled"),
            );
            None
        }
        MirExpr::Call {
            target,
            args,
            ret: _,
        } => {
            let iv = emit_call(cx, target, args, false)?;
            coerce_basic(cx, iv.as_basic_value_enum(), MirRepr::Boxed, want)
        }
        other => {
            let (v, have) = emit_scalar_typed(cx, other)?;
            coerce_basic(cx, v, have, want)
        }
    }
}

/// Emit as universal Echo `i64` (ABI / boxed).
fn emit_expr_i64<'ctx>(cx: &mut EmitCx<'_, 'ctx>, expr: &MirExpr) -> Option<IntValue<'ctx>> {
    let v = emit_expr_as(cx, expr, MirRepr::Boxed)?;
    Some(v.into_int_value())
}

fn emit_condition<'ctx>(cx: &mut EmitCx<'_, 'ctx>, expr: &MirExpr) -> Option<IntValue<'ctx>> {
    let (v, rep) = match expr {
        MirExpr::BoxValue { .. } | MirExpr::UnboxValue { .. } | MirExpr::Call { .. } => {
            let b = emit_expr_as(cx, expr, MirRepr::Bool)?;
            return Some(b.into_int_value());
        }
        other => emit_scalar_typed(cx, other)?,
    };
    match rep {
        MirRepr::Bool => Some(v.into_int_value()),
        MirRepr::Int64 | MirRepr::Boxed | MirRepr::Unknown => {
            let iv = v.into_int_value();
            let zero = cx.i64t.const_int(0, false);
            Some(
                cx.builder
                    .build_int_compare(IntPredicate::NE, iv, zero, "br.cond")
                    .expect("cmp"),
            )
        }
        _ => {
            let iv = emit_expr_i64(cx, expr)?;
            let zero = cx.i64t.const_int(0, false);
            Some(
                cx.builder
                    .build_int_compare(IntPredicate::NE, iv, zero, "br.cond")
                    .expect("cmp"),
            )
        }
    }
}

/// Coerce a value to native f64 (heap float handle or already-f64).
fn coerce_to_f64<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    v: BasicValueEnum<'ctx>,
    rep: MirRepr,
) -> Option<inkwell::values::FloatValue<'ctx>> {
    match rep {
        MirRepr::Float64 => Some(v.into_float_value()),
        MirRepr::Int64 | MirRepr::Boxed | MirRepr::Unknown => {
            let iv = v.into_int_value();
            let to_f = cx
                .module
                .get_function(RT_FLOAT_TO_F64)
                .expect("float_to_f64");
            let call = cx
                .builder
                .build_call(to_f, &[iv.into()], "as.f64")
                .expect("float_to_f64");
            Some(call.try_as_basic_value().unwrap_basic().into_float_value())
        }
        _ => None,
    }
}

fn box_value<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    v: BasicValueEnum<'ctx>,
    from: MirRepr,
) -> Option<IntValue<'ctx>> {
    // Prefer actual LLVM type when MIR repr is under-specified (Boxed/Unknown
    // after float arith) so we never `into_int_value` on a double.
    if v.is_float_value()
        && matches!(
            from,
            MirRepr::Float64 | MirRepr::Float32 | MirRepr::Boxed | MirRepr::Unknown
        )
    {
        let f = v.into_float_value();
        let f64v = if f.get_type() == cx.f64t {
            f
        } else {
            cx.builder
                .build_float_ext(f, cx.f64t, "box.fpext")
                .expect("fpext")
        };
        let from_f = cx
            .module
            .get_function(RT_FLOAT_FROM_F64)
            .expect("float_from_f64");
        let call = cx
            .builder
            .build_call(from_f, &[f64v.into()], "box.f64")
            .expect("float_from_f64");
        return Some(call.try_as_basic_value().unwrap_basic().into_int_value());
    }
    match from {
        MirRepr::Int64 | MirRepr::Duration | MirRepr::Boxed | MirRepr::Unknown => {
            Some(v.into_int_value())
        }
        MirRepr::Int32 | MirRepr::Int16 | MirRepr::Int8 => {
            let i = v.into_int_value();
            Some(
                cx.builder
                    .build_int_s_extend(i, cx.i64t, "box.sint")
                    .expect("sext"),
            )
        }
        MirRepr::UInt32 | MirRepr::UInt16 | MirRepr::UInt8 => {
            let i = v.into_int_value();
            Some(
                cx.builder
                    .build_int_z_extend(i, cx.i64t, "box.uint")
                    .expect("zext"),
            )
        }
        MirRepr::UInt64 => Some(v.into_int_value()),
        MirRepr::Bool => {
            let b = v.into_int_value();
            Some(
                cx.builder
                    .build_int_z_extend(b, cx.i64t, "box.bool")
                    .expect("zext"),
            )
        }
        MirRepr::Float64 => {
            let f = v.into_float_value();
            let from_f = cx
                .module
                .get_function(RT_FLOAT_FROM_F64)
                .expect("float_from_f64");
            let call = cx
                .builder
                .build_call(from_f, &[f.into()], "box.f64")
                .expect("float_from_f64");
            Some(call.try_as_basic_value().unwrap_basic().into_int_value())
        }
        MirRepr::Float32 => {
            let f = v.into_float_value();
            let f64v = cx
                .builder
                .build_float_ext(f, cx.f64t, "f32.fpext")
                .expect("fpext");
            let from_f = cx
                .module
                .get_function(RT_FLOAT_FROM_F64)
                .expect("float_from_f64");
            let call = cx
                .builder
                .build_call(from_f, &[f64v.into()], "box.f32")
                .expect("float_from_f64");
            Some(call.try_as_basic_value().unwrap_basic().into_int_value())
        }
        MirRepr::StringRef
        | MirRepr::BytesRef
        | MirRepr::LocatorRef
        | MirRepr::ObjectRef
        | MirRepr::ListRef => {
            // Handles are already i64 bits in this ABI.
            Some(v.into_int_value())
        }
    }
}

fn unbox_value<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    v: BasicValueEnum<'ctx>,
    to: MirRepr,
) -> Option<BasicValueEnum<'ctx>> {
    let iv = v.into_int_value();
    match to {
        MirRepr::Int64 | MirRepr::Duration | MirRepr::Boxed | MirRepr::Unknown => {
            Some(iv.as_basic_value_enum())
        }
        MirRepr::Int32 | MirRepr::UInt32 => Some(
            cx.builder
                .build_int_truncate(iv, cx.i32t, "unbox.i32")
                .expect("trunc")
                .as_basic_value_enum(),
        ),
        MirRepr::Int16 | MirRepr::UInt16 => Some(
            cx.builder
                .build_int_truncate(iv, cx.i16t, "unbox.i16")
                .expect("trunc")
                .as_basic_value_enum(),
        ),
        MirRepr::Int8 | MirRepr::UInt8 => Some(
            cx.builder
                .build_int_truncate(iv, cx.i8t, "unbox.i8")
                .expect("trunc")
                .as_basic_value_enum(),
        ),
        MirRepr::UInt64 => Some(iv.as_basic_value_enum()),
        MirRepr::Bool => {
            let zero = cx.i64t.const_int(0, false);
            let b = cx
                .builder
                .build_int_compare(IntPredicate::NE, iv, zero, "unbox.bool")
                .expect("cmp");
            Some(b.as_basic_value_enum())
        }
        MirRepr::Float64 => {
            let to_f = cx
                .module
                .get_function(RT_FLOAT_TO_F64)
                .expect("float_to_f64");
            let call = cx
                .builder
                .build_call(to_f, &[iv.into()], "unbox.f64")
                .expect("float_to_f64");
            Some(call.try_as_basic_value().unwrap_basic())
        }
        MirRepr::Float32 => {
            let to_f = cx
                .module
                .get_function(RT_FLOAT_TO_F64)
                .expect("float_to_f64");
            let call = cx
                .builder
                .build_call(to_f, &[iv.into()], "unbox.f32.f64")
                .expect("float_to_f64");
            let f64v = call.try_as_basic_value().unwrap_basic().into_float_value();
            Some(
                cx.builder
                    .build_float_trunc(f64v, cx.f32t, "unbox.f32")
                    .expect("fptrunc")
                    .as_basic_value_enum(),
            )
        }
        MirRepr::StringRef
        | MirRepr::BytesRef
        | MirRepr::LocatorRef
        | MirRepr::ObjectRef
        | MirRepr::ListRef => Some(iv.as_basic_value_enum()),
    }
}

fn coerce_basic<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    v: BasicValueEnum<'ctx>,
    have: MirRepr,
    want: MirRepr,
) -> Option<BasicValueEnum<'ctx>> {
    if have == want || want == MirRepr::Unknown {
        return Some(v);
    }
    if want.is_universal() {
        return Some(box_value(cx, v, have)?.as_basic_value_enum());
    }
    if have.is_universal() {
        return unbox_value(cx, v, want);
    }
    // Same family already handled; refuse silent native↔native coerce
    if have == want {
        return Some(v);
    }
    Some(v)
}

/// Emit a tagged call (or error if not tagged).
fn emit_expr_tagged<'ctx>(cx: &mut EmitCx<'_, 'ctx>, expr: &MirExpr) -> Option<IntValue<'ctx>> {
    match expr {
        // Indirect call: ret shape lives on the function value (runtime).
        MirExpr::Call {
            target: target @ CallTarget::Indirect { .. },
            args,
            ..
        } => emit_call(cx, target, args, true),
        MirExpr::Call { target, args, ret } if ret.is_tagged() => emit_call(cx, target, args, true),
        MirExpr::Call { .. } => {
            cx.diags.push(
                Diagnostic::error("match scrutinee is not result/option-shaped")
                    .with_code("cg-match"),
            );
            None
        }
        // Task join bind (`- name = { … }`) stores packed i128 under `__i128_name`.
        MirExpr::Name(n) if cx.tagged_locals.contains(n) => {
            let key = format!("__i128_{n}");
            let slot = cx.locals.get(&key).copied().or_else(|| {
                // Fall back: reconstruct wide local if only i64 exists.
                None
            })?;
            let loaded = cx
                .builder
                .build_load(cx.i128t, slot, "task.pack")
                .expect("load i128");
            Some(loaded.into_int_value())
        }
        _ => {
            cx.diags.push(
                Diagnostic::error(
                    "match scrutinee must be a result/option-shaped call or task join bind",
                )
                .with_code("cg-match"),
            );
            None
        }
    }
}

fn emit_call<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    target: &CallTarget,
    args: &[MirExpr],
    expect_tagged: bool,
) -> Option<IntValue<'ctx>> {
    match target {
        CallTarget::Runtime { export } => {
            let Some(native) = runtime_native_symbol(export) else {
                cx.diags.push(
                    Diagnostic::error(format!("unknown runtime primitive `{export}`"))
                        .with_code("cg-runtime"),
                );
                return None;
            };
            // print / eprint: void, return 0 — strings only (runtime ignores non-strings).
            if native == RT_PRINT_I64 || native == RT_EPRINT {
                if args.len() != 1 {
                    cx.diags.push(
                        Diagnostic::error(format!("runtime.{export} expects one argument"))
                            .with_code("cg-runtime"),
                    );
                    return None;
                }
                let v = emit_expr_i64(cx, &args[0])?;
                let f = cx
                    .module
                    .get_function(native)
                    .unwrap_or_else(|| panic!("missing runtime symbol {native}"));
                let _ = cx.builder.build_call(f, &[v.into()], "").expect("print");
                return Some(cx.i64t.const_int(0, false));
            }
            // Close sockets: void, return 0.
            if native == RT_TCP_CLOSE || native == RT_UDP_CLOSE {
                if args.len() != 1 {
                    cx.diags.push(
                        Diagnostic::error(format!("runtime.{export} expects one argument"))
                            .with_code("cg-runtime"),
                    );
                    return None;
                }
                let v = emit_expr_i64(cx, &args[0])?;
                let f = cx
                    .module
                    .get_function(native)
                    .unwrap_or_else(|| panic!("missing runtime symbol {native}"));
                let _ = cx.builder.build_call(f, &[v.into()], "").expect("rt_close");
                return Some(cx.i64t.const_int(0, false));
            }
            // Suite: register(name, body) / bench_register(name, body) void.
            if native == RT_TEST_REGISTER || native == RT_TEST_BENCH_REGISTER {
                if args.len() != 2 {
                    let which = if native == RT_TEST_REGISTER {
                        "test_register"
                    } else {
                        "test_bench_register"
                    };
                    cx.diags.push(
                        Diagnostic::error(format!("runtime.{which} expects two arguments"))
                            .with_code("cg-runtime"),
                    );
                    return None;
                }
                let name = emit_expr_i64(cx, &args[0])?;
                let body = emit_expr_i64(cx, &args[1])?;
                let f = cx.module.get_function(native).expect("test register");
                let _ = cx
                    .builder
                    .build_call(f, &[name.into(), body.into()], "")
                    .expect("test register");
                return Some(cx.i64t.const_int(0, false));
            }
            // Suite: fail(msg) void.
            if native == RT_TEST_FAIL {
                if args.len() != 1 {
                    cx.diags.push(
                        Diagnostic::error("runtime.test_fail expects one argument")
                            .with_code("cg-runtime"),
                    );
                    return None;
                }
                let msg = emit_expr_i64(cx, &args[0])?;
                let f = cx.module.get_function(RT_TEST_FAIL).expect("test_fail");
                let _ = cx
                    .builder
                    .build_call(f, &[msg.into()], "")
                    .expect("test_fail");
                return Some(cx.i64t.const_int(0, false));
            }
            // list_reserve(list, additional): void, return 0.
            if native == RT_LIST_RESERVE {
                if args.len() != 2 {
                    cx.diags.push(
                        Diagnostic::error("runtime.list_reserve expects two arguments")
                            .with_code("cg-runtime"),
                    );
                    return None;
                }
                let list = emit_expr_i64(cx, &args[0])?;
                let add = emit_expr_i64(cx, &args[1])?;
                let f = cx
                    .module
                    .get_function(RT_LIST_RESERVE)
                    .expect("list_reserve");
                let _ = cx
                    .builder
                    .build_call(f, &[list.into(), add.into()], "")
                    .expect("list_reserve");
                return Some(cx.i64t.const_int(0, false));
            }
            // sleep_ms: void, return 0.
            if native == RT_RANDOM_SEED {
                if args.len() != 1 {
                    cx.diags.push(
                        Diagnostic::error("runtime.random_seed expects one argument")
                            .with_code("cg-runtime"),
                    );
                    return None;
                }
                let a0 = emit_expr_i64(cx, &args[0])?;
                let f = cx.module.get_function(RT_RANDOM_SEED).expect("random_seed");
                let _ = cx
                    .builder
                    .build_call(f, &[a0.into()], "")
                    .expect("random_seed");
                return Some(cx.i64t.const_int(0, false));
            }
            if native == RT_SLEEP_MS {
                if args.len() != 1 {
                    cx.diags.push(
                        Diagnostic::error("runtime.sleep_ms expects one argument")
                            .with_code("cg-runtime"),
                    );
                    return None;
                }
                let ms = emit_expr_i64(cx, &args[0])?;
                let f = cx.module.get_function(RT_SLEEP_MS).expect("sleep_ms");
                let _ = cx
                    .builder
                    .build_call(f, &[ms.into()], "")
                    .expect("sleep_ms");
                return Some(cx.i64t.const_int(0, false));
            }
            // process env_set / env_unset / exit: void.
            if native == RT_PROCESS_ENV_SET {
                if args.len() != 2 {
                    cx.diags.push(
                        Diagnostic::error("runtime.process_env_set expects two arguments")
                            .with_code("cg-runtime"),
                    );
                    return None;
                }
                let a0 = emit_expr_i64(cx, &args[0])?;
                let a1 = emit_expr_i64(cx, &args[1])?;
                let f = cx
                    .module
                    .get_function(RT_PROCESS_ENV_SET)
                    .expect("process_env_set");
                let _ = cx
                    .builder
                    .build_call(f, &[a0.into(), a1.into()], "")
                    .expect("process_env_set");
                return Some(cx.i64t.const_int(0, false));
            }
            if native == RT_PROCESS_ENV_UNSET
                || native == RT_PROCESS_EXIT
                || native == RT_FS_FILE_CLOSE
                || native == RT_TLS_CLOSE
                || native == RT_TLS_CLOSE_LISTENER
                || native == RT_UNIX_CLOSE
                || native == RT_PROCESS_PIPE_CLOSE
            {
                if args.len() != 1 {
                    cx.diags.push(
                        Diagnostic::error(format!("runtime.{export} expects one argument"))
                            .with_code("cg-runtime"),
                    );
                    return None;
                }
                let a0 = emit_expr_i64(cx, &args[0])?;
                let f = cx.module.get_function(native).expect("void1");
                let _ = cx.builder.build_call(f, &[a0.into()], "").expect("void1");
                return Some(cx.i64t.const_int(0, false));
            }
            // Arity for i64… → i64 runtime exports.
            let arity = if native == RT_NOW_MS
                || native == RT_TEST_FINISH
                || native == RT_PROCESS_ARGS
                || native == RT_RANDOM_U64
                || native == RT_RANDOM_FLOAT
                || native == RT_CRYPTO_RANDOM_U64
                || native == RT_OS_PID
                || native == RT_OS_CWD
                || native == RT_OS_HOSTNAME
                || native == RT_OS_PLATFORM
                || native == RT_NOW_MONO_MS
                || native == RT_FS_TEMP_DIR
                || native == RT_STDIN_READ
                || native == RT_STORE_MEMORY_NEW
                || native == RT_STORE_S3_NEW
            {
                0
            } else if native == RT_STR_FROM_INT
                || native == RT_STR_FROM_FLOAT
                || native == RT_STR_FROM_BYTES
                || native == RT_STR_FROM_DURATION
                || native == RT_STR_FROM_LOCATOR
                || native == RT_STR_FROM_DEBUG
                || native == RT_STR_LEN
                || native == RT_BYTES_LEN
                || native == RT_LIST_LEN
                || native == RT_LIST_NEW_EMPTY_LISTS
                || native == RT_BYTES_FROM_I64
                || native == RT_BYTES_FROM_STR
                || native == RT_BYTES_FROM_VALUE
                || native == RT_REFLECT_KIND
                || native == RT_REFLECT_KIND_NAME
                || native == RT_REFLECT_KEY_BYTES
                || native == RT_HTTP_PARSE_REQUEST
                || native == RT_HTTP_HEADERS_COMPLETE
                || native == RT_HTTP_REQUEST_COMPLETE
                || native == RT_TCP_LISTEN
                || native == RT_TCP_ACCEPT
                || native == RT_TCP_CONNECT
                || native == RT_UDP_BIND
                || native == RT_PROCESS_ENV_HAS
                || native == RT_PROCESS_ENV_GET
                || native == RT_FS_EXISTS
                || native == RT_FS_IS_FILE
                || native == RT_FS_IS_DIR
                || native == RT_FS_READ
                || native == RT_FS_REMOVE
                || native == RT_FS_CREATE_DIR
                || native == RT_FS_CREATE_DIR_ALL
                || native == RT_FS_READ_DIR
                || native == RT_FS_REMOVE_DIR
                || native == RT_FS_METADATA
                || native == RT_FS_OPEN_READ
                || native == RT_FS_OPEN_WRITE
                || native == RT_FS_OPEN_APPEND
                || native == RT_MATH_SQRT
                || native == RT_MATH_SIN
                || native == RT_MATH_COS
                || native == RT_MATH_TAN
                || native == RT_MATH_FLOOR
                || native == RT_MATH_CEIL
                || native == RT_MATH_ABS_F
                || native == RT_MATH_ABS_I
                || native == RT_JSON_PARSE
                || native == RT_JSON_STRINGIFY
                || native == RT_DNS_LOOKUP
                || native == RT_SHA256
                || native == RT_CRYPTO_RANDOM_BYTES
                || native == RT_OS_CHDIR
                || native == RT_FS_CREATE_TEMP
                || native == RT_STR_TO_LOWER
                || native == RT_STR_TO_UPPER
                || native == RT_STR_TRIM
                || native == RT_HEX_ENCODE
                || native == RT_HEX_DECODE
                || native == RT_BASE64_ENCODE
                || native == RT_BASE64_DECODE
                || native == RT_UTF8_VALID
                || native == RT_UTF8_DECODE
                || native == RT_TLS_LISTEN
                || native == RT_PARSE_I64
                || native == RT_PARSE_F64
                || native == RT_URL_PARSE
                || native == RT_GZIP_COMPRESS
                || native == RT_GZIP_DECOMPRESS
                || native == RT_ZIP_UNPACK_FIRST
                || native == RT_SHA512
                || native == RT_PATH_CLEAN
                || native == RT_LOCATOR_CLASS
                || native == RT_UNIX_LISTEN
                || native == RT_UNIX_ACCEPT
                || native == RT_UNIX_CONNECT
                || native == RT_PROCESS_WAIT
                || native == RT_STORE_CLOSE
                || native == RT_STORE_KEYS
                || native == RT_GIT_PACK_VALID
                || native == RT_GIT_PACK_COUNT
                || native == RT_GIT_INDEX_PACK
                || native == RT_GIT_RECEIVE_PACK_SPLIT
                || native == RT_GIT_UPLOAD_PACK_WANTS
            {
                1
            } else if native == RT_TCP_READ
                || native == RT_TCP_WRITE
                || native == RT_UDP_RECV_FROM
                || native == RT_STR_CAT
                || native == RT_STR_CONTAINS
                || native == RT_STR_STARTS_WITH
                || native == RT_STR_ENDS_WITH
                || native == RT_STR_REPEAT
                || native == RT_BYTES_GET
                || native == RT_STR_GET
                || native == RT_BYTES_CAT
                || native == RT_LIST_GET
                || native == RT_PROCESS_RUN
                || native == RT_FS_JOIN
                || native == RT_FS_WRITE
                || native == RT_FS_COPY
                || native == RT_FS_RENAME
                || native == RT_FS_FILE_READ
                || native == RT_FS_FILE_WRITE
                || native == RT_FS_FILE_SEEK
                || native == RT_MATH_POW
                || native == RT_STR_SPLIT
                || native == RT_FS_SYMLINK
                || native == RT_TLS_READ
                || native == RT_TLS_WRITE
                || native == RT_PROCESS_RUN_CAPTURE
                || native == RT_TIME_FORMAT
                || native == RT_TIME_PARSE
                || native == RT_ZIP_PACK
                || native == RT_HMAC_SHA256
                || native == RT_FS_CHMOD
                || native == RT_PATH_REL
                || native == RT_UNIX_READ
                || native == RT_UNIX_WRITE
                || native == RT_PROCESS_SPAWN_PIPES
                || native == RT_PROCESS_PIPE_WRITE
                || native == RT_PROCESS_PIPE_READ
                || native == RT_STORE_GET
                || native == RT_STORE_HEAD
            {
                2
            } else if native == RT_UDP_SEND_TO
                || native == RT_STR_SLICE
                || native == RT_BYTES_SLICE
                || native == RT_STR_REPLACE
                || native == RT_TLS_ACCEPT
                || native == RT_AES_GCM_ENCRYPT
                || native == RT_AES_GCM_DECRYPT
                || native == RT_PROCESS_RUN_CWD
                || native == RT_STORE_GET_IF_NONE_MATCH
                || native == RT_STORE_PUT
                || native == RT_STORE_CAS_CREATE
            {
                3
            } else if native == RT_TLS_CONNECT
                || native == RT_STORE_GET_RANGE
                || native == RT_STORE_CAS
            {
                4
            } else {
                cx.diags.push(
                    Diagnostic::error(format!(
                        "runtime primitive `{export}` not implemented in codegen"
                    ))
                    .with_code("cg-runtime"),
                );
                return None;
            };
            if args.len() != arity {
                cx.diags.push(
                    Diagnostic::error(format!(
                        "runtime.{export} expects {arity} argument(s), got {}",
                        args.len()
                    ))
                    .with_code("cg-runtime"),
                );
                return None;
            }
            let mut argv = Vec::with_capacity(arity);
            for a in args {
                argv.push(emit_expr_i64(cx, a)?.into());
            }
            let f = cx
                .module
                .get_function(native)
                .unwrap_or_else(|| panic!("missing runtime symbol {native}"));
            let call = cx.builder.build_call(f, &argv, "rt").expect("rt");
            Some(call.try_as_basic_value().unwrap_basic().into_int_value())
        }
        CallTarget::Function { module_path, name } => {
            let key = mangle_fn(module_path, name);
            let (fv, ret) = match cx.fn_map.get(&key) {
                Some(x) => *x,
                None => {
                    cx.diags.push(
                        Diagnostic::error(format!("unknown function `{name}` ({key})"))
                            .with_code("cg-call"),
                    );
                    return None;
                }
            };
            if expect_tagged != ret.is_tagged() {
                if !expect_tagged && ret.is_tagged() {
                    cx.diags.push(
                        Diagnostic::error(
                            "result/option-shaped call must be handled with `|` match",
                        )
                        .with_code("cg-unhandled"),
                    );
                } else {
                    cx.diags.push(
                        Diagnostic::error("match scrutinee is not result/option-shaped")
                            .with_code("cg-match"),
                    );
                }
                return None;
            }
            let mut vals: Vec<BasicMetadataValueEnum> = Vec::new();
            for a in args {
                vals.push(emit_expr_i64(cx, a)?.as_basic_value_enum().into());
            }
            let call = cx
                .builder
                .build_call(fv, &vals, if expect_tagged { "call_t" } else { "call" })
                .expect("call");
            Some(call.try_as_basic_value().unwrap_basic().into_int_value())
        }
        CallTarget::Indirect { callee } => {
            // Function value handle: { code ptr, ret shape }.
            // Kernel logs for past suite crashes showed `ip == 0` (null call);
            // refuse to inttoptr/call a zero code pointer.
            let handle = emit_expr_i64(cx, callee)?;
            let code_f = cx.module.get_function(RT_FN_CODE).expect("fn_code");
            let shape_f = cx.module.get_function(RT_FN_SHAPE).expect("fn_shape");
            let code = cx
                .builder
                .build_call(code_f, &[handle.into()], "fn.code")
                .expect("fn_code")
                .try_as_basic_value()
                .unwrap_basic()
                .into_int_value();
            let shape = cx
                .builder
                .build_call(shape_f, &[handle.into()], "fn.shape")
                .expect("fn_shape")
                .try_as_basic_value()
                .unwrap_basic()
                .into_int_value();
            let zero = cx.i64t.const_zero();
            let is_null = cx
                .builder
                .build_int_compare(IntPredicate::EQ, code, zero, "fn.null")
                .expect("fn.null");
            let parent = cx
                .builder
                .get_insert_block()
                .expect("block")
                .get_parent()
                .expect("fn");
            let null_bb = cx.context.append_basic_block(parent, "fn.null");
            let call_bb = cx.context.append_basic_block(parent, "fn.call");
            let join_bb = cx.context.append_basic_block(parent, "fn.join");
            cx.builder
                .build_conditional_branch(is_null, null_bb, call_bb)
                .expect("br null");

            cx.builder.position_at_end(null_bb);
            // Soft-fail: return 0 rather than jump to address 0.
            cx.builder.build_unconditional_branch(join_bb).expect("br");
            let null_bb_end = cx.builder.get_insert_block().expect("null end");

            cx.builder.position_at_end(call_bb);
            let ptr_ty = cx.context.ptr_type(AddressSpace::default());
            let fptr = cx
                .builder
                .build_int_to_ptr(code, ptr_ty, "fnptr")
                .expect("inttoptr");
            let params: Vec<BasicMetadataTypeEnum> = args.iter().map(|_| cx.i64t.into()).collect();
            let mut vals: Vec<BasicMetadataValueEnum> = Vec::new();
            for a in args {
                vals.push(emit_expr_i64(cx, a)?.as_basic_value_enum().into());
            }
            // shape 0 = plain (i64), 1|2 = result|option (i128)
            let call_val = if expect_tagged {
                let fty = cx.i128t.fn_type(&params, false);
                let call = cx
                    .builder
                    .build_indirect_call(fty, fptr, &vals, "icall_t")
                    .expect("icall_t");
                call.try_as_basic_value().unwrap_basic().into_int_value()
            } else {
                let _ = shape;
                let fty = cx.i64t.fn_type(&params, false);
                let call = cx
                    .builder
                    .build_indirect_call(fty, fptr, &vals, "icall")
                    .expect("icall");
                call.try_as_basic_value().unwrap_basic().into_int_value()
            };
            cx.builder.build_unconditional_branch(join_bb).expect("br");
            let call_bb_end = cx.builder.get_insert_block().expect("call end");

            cx.builder.position_at_end(join_bb);
            let phi_ty = if expect_tagged { cx.i128t } else { cx.i64t };
            let phi = cx.builder.build_phi(phi_ty, "icall.r").expect("phi");
            let zero_ret = if expect_tagged {
                cx.i128t.const_zero().as_basic_value_enum()
            } else {
                zero.as_basic_value_enum()
            };
            phi.add_incoming(&[
                (&zero_ret, null_bb_end),
                (&call_val.as_basic_value_enum(), call_bb_end),
            ]);
            Some(phi.as_basic_value().into_int_value())
        }
    }
}

#[allow(dead_code)]
fn emit_scalar<'ctx>(cx: &mut EmitCx<'_, 'ctx>, expr: &MirExpr) -> Option<IntValue<'ctx>> {
    let (v, _) = emit_scalar_typed(cx, expr)?;
    match v {
        BasicValueEnum::IntValue(iv) => {
            if iv.get_type().get_bit_width() == 1 {
                Some(
                    cx.builder
                        .build_int_z_extend(iv, cx.i64t, "bool.i64")
                        .expect("zext"),
                )
            } else {
                Some(iv)
            }
        }
        other => box_value(cx, other, MirRepr::Unknown),
    }
}

fn emit_scalar_typed<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    expr: &MirExpr,
) -> Option<(BasicValueEnum<'ctx>, MirRepr)> {
    match expr {
        MirExpr::ConstI64(v) => Some((
            cx.i64t.const_int(*v as u64, true).as_basic_value_enum(),
            MirRepr::Int64,
        )),
        MirExpr::ConstI32(v) => Some((
            cx.i32t.const_int(*v as u64, true).as_basic_value_enum(),
            MirRepr::Int32,
        )),
        MirExpr::ConstInt { value, width } => {
            let rep = width_to_repr(*width);
            let ty = int_ty_for_repr(cx, rep)?;
            let bits = *value as u64;
            Some((
                ty.const_int(bits, width.is_signed_int())
                    .as_basic_value_enum(),
                rep,
            ))
        }
        MirExpr::Cast { to, expr } => {
            let (v, from) = emit_scalar_typed(cx, expr)?;
            emit_int_cast(cx, v, from, *to)
        }
        MirExpr::ConstF32(v) => Some((
            cx.f32t.const_float(*v as f64).as_basic_value_enum(),
            MirRepr::Float32,
        )),
        MirExpr::ConstDuration(v) => Some((
            cx.i64t.const_int(*v as u64, true).as_basic_value_enum(),
            MirRepr::Duration,
        )),
        MirExpr::ConstBool(b) => Some((
            cx.i1t.const_int(u64::from(*b), false).as_basic_value_enum(),
            MirRepr::Bool,
        )),
        MirExpr::ConstF64(v) => Some((
            cx.f64t.const_float(*v).as_basic_value_enum(),
            MirRepr::Float64,
        )),
        MirExpr::Name(n) => {
            if n.ends_with("@undef") {
                return Some((
                    cx.i64t.const_int(0, false).as_basic_value_enum(),
                    MirRepr::Int64,
                ));
            }
            if let Some(v) = cx.values.get(n) {
                let rep = cx.reprs.get(n).copied().unwrap_or(MirRepr::Unknown);
                return Some((*v, rep));
            }
            // Fall back to base name (pre-SSA) if present.
            if let Some(base) = n.split('@').next() {
                if base != n {
                    if let Some(v) = cx.values.get(base) {
                        let rep = cx.reprs.get(base).copied().unwrap_or(MirRepr::Unknown);
                        return Some((*v, rep));
                    }
                }
            }
            let slot = match cx
                .locals
                .get(n)
                .or_else(|| n.split('@').next().and_then(|b| cx.locals.get(b)))
            {
                Some(s) => *s,
                None => {
                    cx.diags.push(
                        Diagnostic::error(format!("unknown name `{n}` in codegen"))
                            .with_code("cg-name"),
                    );
                    return None;
                }
            };
            let loaded = cx.builder.build_load(cx.i64t, slot, n).expect("load");
            Some((loaded, MirRepr::Boxed))
        }
        MirExpr::PrimCall { prim, args } => {
            let iv = emit_prim(cx, *prim, args)?;
            let rep = match prim {
                MirPrim::ListLen => MirRepr::Int64,
                MirPrim::ListGetChecked => MirRepr::Boxed,
            };
            Some((iv.as_basic_value_enum(), rep))
        }
        MirExpr::Unary { op, expr } => {
            let (v, rep) = emit_scalar_typed(cx, expr)?;
            match op {
                UnaryOp::Neg if rep == MirRepr::Int64 => {
                    let iv = v.into_int_value();
                    Some((
                        cx.builder
                            .build_int_neg(iv, "neg")
                            .expect("neg")
                            .as_basic_value_enum(),
                        MirRepr::Int64,
                    ))
                }
                UnaryOp::Neg if rep == MirRepr::Int32 => {
                    let iv = v.into_int_value();
                    Some((
                        cx.builder
                            .build_int_neg(iv, "neg")
                            .expect("neg")
                            .as_basic_value_enum(),
                        MirRepr::Int32,
                    ))
                }
                UnaryOp::Neg if rep == MirRepr::Float64 => {
                    let fv = v.into_float_value();
                    Some((
                        cx.builder
                            .build_float_neg(fv, "fneg")
                            .expect("fneg")
                            .as_basic_value_enum(),
                        MirRepr::Float64,
                    ))
                }
                UnaryOp::Neg if rep == MirRepr::Float32 => {
                    let fv = v.into_float_value();
                    Some((
                        cx.builder
                            .build_float_neg(fv, "fneg")
                            .expect("fneg")
                            .as_basic_value_enum(),
                        MirRepr::Float32,
                    ))
                }
                UnaryOp::Not if rep == MirRepr::Bool => {
                    let b = v.into_int_value();
                    let one = cx.i1t.const_int(1, false);
                    Some((
                        cx.builder
                            .build_xor(b, one, "not")
                            .expect("not")
                            .as_basic_value_enum(),
                        MirRepr::Bool,
                    ))
                }
                UnaryOp::Not => {
                    let iv = box_value(cx, v, rep)?;
                    let z = cx.i64t.const_int(0, false);
                    let is_zero = cx
                        .builder
                        .build_int_compare(IntPredicate::EQ, iv, z, "not")
                        .expect("not");
                    Some((is_zero.as_basic_value_enum(), MirRepr::Bool))
                }
                UnaryOp::BitNot if rep.is_native_int() => {
                    let iv = v.into_int_value();
                    let bits = iv.get_type().get_bit_width();
                    let all = if bits == 64 {
                        u64::MAX
                    } else {
                        (1u64 << bits) - 1
                    };
                    let all = iv.get_type().const_int(all, false);
                    Some((
                        cx.builder
                            .build_xor(iv, all, "bnot")
                            .expect("bnot")
                            .as_basic_value_enum(),
                        rep,
                    ))
                }
                UnaryOp::BitNot => {
                    let iv = box_value(cx, v, rep)?;
                    let all = cx.i64t.const_int(u64::MAX, false);
                    Some((
                        cx.builder
                            .build_xor(iv, all, "bnot")
                            .expect("bnot")
                            .as_basic_value_enum(),
                        MirRepr::Int64,
                    ))
                }
                UnaryOp::Neg => {
                    let iv = box_value(cx, v, rep)?;
                    Some((
                        cx.builder
                            .build_int_neg(iv, "neg")
                            .expect("neg")
                            .as_basic_value_enum(),
                        MirRepr::Int64,
                    ))
                }
            }
        }
        MirExpr::Binary { op, left, right } => emit_binary_typed(cx, *op, left, right),
        MirExpr::Call { target, args, ret } => {
            if ret.is_tagged() {
                cx.diags.push(
                    Diagnostic::error("result/option-shaped call must be handled with `|` match")
                        .with_code("cg-unhandled"),
                );
                return None;
            }
            let iv = emit_call(cx, target, args, false)?;
            Some((iv.as_basic_value_enum(), MirRepr::Boxed))
        }
        MirExpr::FnValue {
            module_path,
            symbol,
        } => {
            let key = mangle_fn(module_path, symbol);
            let (fv, ret) = match cx.fn_map.get(&key) {
                Some(x) => *x,
                None => {
                    cx.diags.push(
                        Diagnostic::error(format!("unknown function value `{symbol}` ({key})"))
                            .with_code("cg-call"),
                    );
                    return None;
                }
            };
            let ptr = fv.as_global_value().as_pointer_value();
            let code = cx
                .builder
                .build_ptr_to_int(ptr, cx.i64t, "fn.codebits")
                .expect("ptrtoint");
            // 0 plain, 1 result, 2 option — matches echo_runtime FN_SHAPE_*.
            let shape_code: u64 = match ret {
                MirRetShape::Plain => 0,
                MirRetShape::Result => 1,
                MirRetShape::Option => 2,
            };
            let shape = cx.i64t.const_int(shape_code, false);
            let new_f = cx.module.get_function(RT_FN_NEW).expect("fn_new");
            let call = cx
                .builder
                .build_call(new_f, &[code.into(), shape.into()], "fnval")
                .expect("fn_new");
            Some((
                call.try_as_basic_value()
                    .unwrap_basic()
                    .into_int_value()
                    .as_basic_value_enum(),
                MirRepr::Boxed,
            ))
        }
        MirExpr::Range { start, end } => {
            let lo = emit_expr_i64(cx, start)?;
            let hi = emit_expr_i64(cx, end)?;
            let f = cx.module.get_function(RT_RANGE_NEW).expect("range_new");
            let call = cx
                .builder
                .build_call(f, &[lo.into(), hi.into()], "range")
                .expect("range_new");
            Some((
                call.try_as_basic_value()
                    .unwrap_basic()
                    .into_int_value()
                    .as_basic_value_enum(),
                MirRepr::Boxed,
            ))
        }
        MirExpr::ListLit(elems) => {
            let iv = emit_list_lit(cx, elems)?;
            // Handle bits as i64 ABI; fact is ListRef when stored, here raw new list.
            Some((iv.as_basic_value_enum(), MirRepr::ListRef))
        }
        MirExpr::StringLit { bytes } => {
            let iv = emit_string_lit(cx, bytes)?;
            Some((iv.as_basic_value_enum(), MirRepr::StringRef))
        }
        MirExpr::BytesLit { bytes } => {
            let iv = emit_bytes_lit(cx, bytes)?;
            Some((iv.as_basic_value_enum(), MirRepr::BytesRef))
        }
        MirExpr::BytesInterp { parts } => {
            let iv = emit_bytes_interp(cx, parts)?;
            Some((iv.as_basic_value_enum(), MirRepr::BytesRef))
        }
        MirExpr::LocatorLit { text } => {
            let iv = emit_locator_lit(cx, text.as_bytes())?;
            Some((iv.as_basic_value_enum(), MirRepr::LocatorRef))
        }
        MirExpr::LocatorInterp { parts } => {
            let iv = emit_locator_interp(cx, parts)?;
            Some((iv.as_basic_value_enum(), MirRepr::LocatorRef))
        }
        MirExpr::StringInterp { parts } => {
            let iv = emit_string_interp(cx, parts)?;
            Some((iv.as_basic_value_enum(), MirRepr::StringRef))
        }
        MirExpr::Index { base, index } => {
            let list = emit_expr_i64(cx, base)?;
            let idx = emit_expr_i64(cx, index)?;
            let get_f = cx.module.get_function(RT_LIST_GET).expect("list_get");
            let call = cx
                .builder
                .build_call(get_f, &[list.into(), idx.into()], "idx")
                .expect("get");
            Some((
                call.try_as_basic_value()
                    .unwrap_basic()
                    .into_int_value()
                    .as_basic_value_enum(),
                MirRepr::Boxed,
            ))
        }
        MirExpr::StructLit { type_name, fields } => {
            let iv = emit_struct_lit(cx, type_name, fields)?;
            Some((iv.as_basic_value_enum(), MirRepr::ObjectRef))
        }
        MirExpr::StructTypeIs { value, type_name } => {
            let handle = emit_expr_i64(cx, value)?;
            let (ptr, len) = emit_const_bytes(cx, type_name.as_bytes());
            let f = cx
                .module
                .get_function(RT_STRUCT_TYPE_IS)
                .expect("struct_type_is");
            let call = cx
                .builder
                .build_call(f, &[handle.into(), ptr.into(), len.into()], "type_is")
                .expect("struct_type_is");
            let iv = call.try_as_basic_value().unwrap_basic().into_int_value();
            // Runtime returns i64 0/1; conds want i1-style bool repr.
            let b = cx
                .builder
                .build_int_compare(IntPredicate::NE, iv, cx.i64t.const_zero(), "type_is_b")
                .expect("type_is ne");
            Some((b.as_basic_value_enum(), MirRepr::Bool))
        }
        MirExpr::FieldGet { base, field } => {
            let handle = emit_expr_i64(cx, base)?;
            let (ptr, len) = emit_const_bytes(cx, field.as_bytes());
            let get_f = cx.module.get_function(RT_STRUCT_GET).expect("struct_get");
            let call = cx
                .builder
                .build_call(get_f, &[handle.into(), ptr.into(), len.into()], "field")
                .expect("struct_get");
            Some((
                call.try_as_basic_value()
                    .unwrap_basic()
                    .into_int_value()
                    .as_basic_value_enum(),
                MirRepr::Boxed,
            ))
        }
        MirExpr::BoxValue { value, from } => {
            let v = emit_expr_as(cx, value, *from)?;
            let boxed = box_value(cx, v, *from)?;
            Some((boxed.as_basic_value_enum(), MirRepr::Boxed))
        }
        MirExpr::UnboxValue { value, to } => {
            let v = emit_expr_as(cx, value, MirRepr::Boxed)?;
            let u = unbox_value(cx, v, *to)?;
            Some((u, *to))
        }
    }
}

fn emit_binary_typed<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    op: BinaryOp,
    left: &MirExpr,
    right: &MirExpr,
) -> Option<(BasicValueEnum<'ctx>, MirRepr)> {
    let (lv, lr) = emit_scalar_typed(cx, left)?;
    let (rv, rr) = emit_scalar_typed(cx, right)?;

    // Native integer same-width arith / compares / bitwise (`i*` / `ui*`).
    // Default i64 / universal ABI yields to a more specific width (untagged lit,
    // ui64 lane). Boxed free-fn params must not force signed `>>` (ashr).
    let int_lane = if lr == rr && lr.is_native_int() {
        Some(lr)
    } else if lr == MirRepr::Int64 && rr.is_native_int() && rr != MirRepr::Int64 {
        Some(rr)
    } else if rr == MirRepr::Int64 && lr.is_native_int() && lr != MirRepr::Int64 {
        Some(lr)
    } else if lr.is_unsigned_int() && rr.is_universal() {
        Some(lr)
    } else if rr.is_unsigned_int() && lr.is_universal() {
        Some(rr)
    } else {
        None
    };
    if let Some(lane) = int_lane {
        let l = lv.into_int_value();
        let r = rv.into_int_value();
        // Coerce i64 operand to the specific lane's LLVM int type when needed.
        let lty = int_ty_for_repr(cx, lane)?;
        let l = if l.get_type() == lty {
            l
        } else if lty.get_bit_width() < l.get_type().get_bit_width() {
            cx.builder
                .build_int_truncate(l, lty, "i.trunc")
                .expect("trunc")
        } else if lty.get_bit_width() > l.get_type().get_bit_width() {
            if lane.is_unsigned_int() {
                cx.builder
                    .build_int_z_extend(l, lty, "i.zext")
                    .expect("zext")
            } else {
                cx.builder
                    .build_int_s_extend(l, lty, "i.sext")
                    .expect("sext")
            }
        } else {
            l
        };
        let r = if r.get_type() == lty {
            r
        } else if lty.get_bit_width() < r.get_type().get_bit_width() {
            cx.builder
                .build_int_truncate(r, lty, "i.trunc")
                .expect("trunc")
        } else if lty.get_bit_width() > r.get_type().get_bit_width() {
            if lane.is_unsigned_int() {
                cx.builder
                    .build_int_z_extend(r, lty, "i.zext")
                    .expect("zext")
            } else {
                cx.builder
                    .build_int_s_extend(r, lty, "i.sext")
                    .expect("sext")
            }
        } else {
            r
        };
        let unsigned = lane.is_unsigned_int();
        return match op {
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Rem
            | BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => Some((
                emit_binop_signedness(cx, op, l, r, unsigned).as_basic_value_enum(),
                lane,
            )),
            BinaryOp::Eq | BinaryOp::EqEqEq => Some((
                cx.builder
                    .build_int_compare(IntPredicate::EQ, l, r, "eq")
                    .expect("eq")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::NotEq | BinaryOp::NotEqEq => Some((
                cx.builder
                    .build_int_compare(IntPredicate::NE, l, r, "ne")
                    .expect("ne")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::Lt => {
                let p = if unsigned {
                    IntPredicate::ULT
                } else {
                    IntPredicate::SLT
                };
                Some((
                    cx.builder
                        .build_int_compare(p, l, r, "lt")
                        .expect("lt")
                        .as_basic_value_enum(),
                    MirRepr::Bool,
                ))
            }
            BinaryOp::Gt => {
                let p = if unsigned {
                    IntPredicate::UGT
                } else {
                    IntPredicate::SGT
                };
                Some((
                    cx.builder
                        .build_int_compare(p, l, r, "gt")
                        .expect("gt")
                        .as_basic_value_enum(),
                    MirRepr::Bool,
                ))
            }
            BinaryOp::LtEq => {
                let p = if unsigned {
                    IntPredicate::ULE
                } else {
                    IntPredicate::SLE
                };
                Some((
                    cx.builder
                        .build_int_compare(p, l, r, "le")
                        .expect("le")
                        .as_basic_value_enum(),
                    MirRepr::Bool,
                ))
            }
            BinaryOp::GtEq => {
                let p = if unsigned {
                    IntPredicate::UGE
                } else {
                    IntPredicate::SGE
                };
                Some((
                    cx.builder
                        .build_int_compare(p, l, r, "ge")
                        .expect("ge")
                        .as_basic_value_enum(),
                    MirRepr::Bool,
                ))
            }
            BinaryOp::And | BinaryOp::Or => {
                let z = l.get_type().const_int(0, false);
                let lb = cx
                    .builder
                    .build_int_compare(IntPredicate::NE, l, z, "and.l")
                    .expect("l");
                let rb = cx
                    .builder
                    .build_int_compare(IntPredicate::NE, r, z, "and.r")
                    .expect("r");
                let b = if matches!(op, BinaryOp::And) {
                    cx.builder.build_and(lb, rb, "and").expect("and")
                } else {
                    cx.builder.build_or(lb, rb, "or").expect("or")
                };
                Some((b.as_basic_value_enum(), MirRepr::Bool))
            }
        };
    }

    // Duration as i64 nanoseconds: add/sub + compares
    if lr == MirRepr::Duration && rr == MirRepr::Duration {
        let l = lv.into_int_value();
        let r = rv.into_int_value();
        return match op {
            BinaryOp::Add | BinaryOp::Sub => Some((
                emit_binop(cx, op, l, r).as_basic_value_enum(),
                MirRepr::Duration,
            )),
            BinaryOp::Eq | BinaryOp::EqEqEq => Some((
                cx.builder
                    .build_int_compare(IntPredicate::EQ, l, r, "deq")
                    .expect("eq")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::NotEq | BinaryOp::NotEqEq => Some((
                cx.builder
                    .build_int_compare(IntPredicate::NE, l, r, "dne")
                    .expect("ne")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::Lt => Some((
                cx.builder
                    .build_int_compare(IntPredicate::SLT, l, r, "dlt")
                    .expect("lt")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::Gt => Some((
                cx.builder
                    .build_int_compare(IntPredicate::SGT, l, r, "dgt")
                    .expect("gt")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::LtEq => Some((
                cx.builder
                    .build_int_compare(IntPredicate::SLE, l, r, "dle")
                    .expect("le")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::GtEq => Some((
                cx.builder
                    .build_int_compare(IntPredicate::SGE, l, r, "dge")
                    .expect("ge")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            _ => None,
        };
    }

    // Native i64 arithmetic / compares / bitwise
    if lr == MirRepr::Int64 && rr == MirRepr::Int64 {
        let l = lv.into_int_value();
        let r = rv.into_int_value();
        return match op {
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Rem
            | BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => Some((
                emit_binop(cx, op, l, r).as_basic_value_enum(),
                MirRepr::Int64,
            )),
            BinaryOp::Eq | BinaryOp::EqEqEq => Some((
                cx.builder
                    .build_int_compare(IntPredicate::EQ, l, r, "eq")
                    .expect("eq")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::NotEq | BinaryOp::NotEqEq => Some((
                cx.builder
                    .build_int_compare(IntPredicate::NE, l, r, "ne")
                    .expect("ne")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::Lt => Some((
                cx.builder
                    .build_int_compare(IntPredicate::SLT, l, r, "lt")
                    .expect("lt")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::Gt => Some((
                cx.builder
                    .build_int_compare(IntPredicate::SGT, l, r, "gt")
                    .expect("gt")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::LtEq => Some((
                cx.builder
                    .build_int_compare(IntPredicate::SLE, l, r, "le")
                    .expect("le")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::GtEq => Some((
                cx.builder
                    .build_int_compare(IntPredicate::SGE, l, r, "ge")
                    .expect("ge")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::And | BinaryOp::Or => {
                // truthiness on ints → i1
                let z = cx.i64t.const_int(0, false);
                let lb = cx
                    .builder
                    .build_int_compare(IntPredicate::NE, l, z, "and.l")
                    .expect("l");
                let rb = cx
                    .builder
                    .build_int_compare(IntPredicate::NE, r, z, "and.r")
                    .expect("r");
                let b = if matches!(op, BinaryOp::And) {
                    cx.builder.build_and(lb, rb, "and").expect("and")
                } else {
                    cx.builder.build_or(lb, rb, "or").expect("or")
                };
                Some((b.as_basic_value_enum(), MirRepr::Bool))
            }
        };
    }

    // Native bool logic / compares
    if lr == MirRepr::Bool && rr == MirRepr::Bool {
        let l = lv.into_int_value();
        let r = rv.into_int_value();
        return match op {
            BinaryOp::And => Some((
                cx.builder
                    .build_and(l, r, "and")
                    .expect("and")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::Or => Some((
                cx.builder
                    .build_or(l, r, "or")
                    .expect("or")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::Eq | BinaryOp::EqEqEq => Some((
                cx.builder
                    .build_int_compare(IntPredicate::EQ, l, r, "eq")
                    .expect("eq")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::NotEq | BinaryOp::NotEqEq => Some((
                cx.builder
                    .build_int_compare(IntPredicate::NE, l, r, "ne")
                    .expect("ne")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            _ => None,
        };
    }

    // Native f32 arith / compares
    if lr == MirRepr::Float32 && rr == MirRepr::Float32 {
        let l = lv.into_float_value();
        let r = rv.into_float_value();
        return match op {
            BinaryOp::Add => Some((
                cx.builder
                    .build_float_add(l, r, "fadd")
                    .expect("fadd")
                    .as_basic_value_enum(),
                MirRepr::Float32,
            )),
            BinaryOp::Sub => Some((
                cx.builder
                    .build_float_sub(l, r, "fsub")
                    .expect("fsub")
                    .as_basic_value_enum(),
                MirRepr::Float32,
            )),
            BinaryOp::Mul => Some((
                cx.builder
                    .build_float_mul(l, r, "fmul")
                    .expect("fmul")
                    .as_basic_value_enum(),
                MirRepr::Float32,
            )),
            BinaryOp::Div => Some((
                cx.builder
                    .build_float_div(l, r, "fdiv")
                    .expect("fdiv")
                    .as_basic_value_enum(),
                MirRepr::Float32,
            )),
            BinaryOp::Rem => Some((
                cx.builder
                    .build_float_rem(l, r, "frem")
                    .expect("frem")
                    .as_basic_value_enum(),
                MirRepr::Float32,
            )),
            BinaryOp::Lt => Some((
                cx.builder
                    .build_float_compare(FloatPredicate::OLT, l, r, "flt")
                    .expect("flt")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::Gt => Some((
                cx.builder
                    .build_float_compare(FloatPredicate::OGT, l, r, "fgt")
                    .expect("fgt")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::LtEq => Some((
                cx.builder
                    .build_float_compare(FloatPredicate::OLE, l, r, "fle")
                    .expect("fle")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::GtEq => Some((
                cx.builder
                    .build_float_compare(FloatPredicate::OGE, l, r, "fge")
                    .expect("fge")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::Eq | BinaryOp::EqEqEq => Some((
                cx.builder
                    .build_float_compare(FloatPredicate::OEQ, l, r, "feq")
                    .expect("feq")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::NotEq | BinaryOp::NotEqEq => Some((
                cx.builder
                    .build_float_compare(FloatPredicate::ONE, l, r, "fne")
                    .expect("fne")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::And
            | BinaryOp::Or
            | BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => None,
        };
    }

    // Native f64 arith / compares when either side is a proven float (or both).
    // Boxed params that hold heap floats are unboxed via float_to_f64.
    if lr == MirRepr::Float64 || rr == MirRepr::Float64 {
        let l = coerce_to_f64(cx, lv, lr)?;
        let r = coerce_to_f64(cx, rv, rr)?;
        return match op {
            BinaryOp::Add => Some((
                cx.builder
                    .build_float_add(l, r, "fadd")
                    .expect("fadd")
                    .as_basic_value_enum(),
                MirRepr::Float64,
            )),
            BinaryOp::Sub => Some((
                cx.builder
                    .build_float_sub(l, r, "fsub")
                    .expect("fsub")
                    .as_basic_value_enum(),
                MirRepr::Float64,
            )),
            BinaryOp::Mul => Some((
                cx.builder
                    .build_float_mul(l, r, "fmul")
                    .expect("fmul")
                    .as_basic_value_enum(),
                MirRepr::Float64,
            )),
            BinaryOp::Div => Some((
                cx.builder
                    .build_float_div(l, r, "fdiv")
                    .expect("fdiv")
                    .as_basic_value_enum(),
                MirRepr::Float64,
            )),
            BinaryOp::Rem => Some((
                cx.builder
                    .build_float_rem(l, r, "frem")
                    .expect("frem")
                    .as_basic_value_enum(),
                MirRepr::Float64,
            )),
            BinaryOp::Lt => Some((
                cx.builder
                    .build_float_compare(FloatPredicate::OLT, l, r, "flt")
                    .expect("flt")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::Gt => Some((
                cx.builder
                    .build_float_compare(FloatPredicate::OGT, l, r, "fgt")
                    .expect("fgt")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::LtEq => Some((
                cx.builder
                    .build_float_compare(FloatPredicate::OLE, l, r, "fle")
                    .expect("fle")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::GtEq => Some((
                cx.builder
                    .build_float_compare(FloatPredicate::OGE, l, r, "fge")
                    .expect("fge")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::Eq | BinaryOp::EqEqEq => Some((
                cx.builder
                    .build_float_compare(FloatPredicate::OEQ, l, r, "feq")
                    .expect("feq")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::NotEq | BinaryOp::NotEqEq => Some((
                cx.builder
                    .build_float_compare(FloatPredicate::ONE, l, r, "fne")
                    .expect("fne")
                    .as_basic_value_enum(),
                MirRepr::Bool,
            )),
            BinaryOp::And
            | BinaryOp::Or
            | BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => None,
        };
    }

    // Fallback: box both and use runtime deep (`==`) or identity (`===`) eq
    let l = box_value(cx, lv, lr)?;
    let r = box_value(cx, rv, rr)?;
    match op {
        BinaryOp::Eq => {
            let f = cx.module.get_function(RT_EQ).expect("eq");
            let call = cx
                .builder
                .build_call(f, &[l.into(), r.into()], "eq")
                .expect("eq");
            let iv = call.try_as_basic_value().unwrap_basic().into_int_value();
            // Runtime returns i64 0/1; narrow to i1 for Bool fact
            let z = cx.i64t.const_int(0, false);
            let b = cx
                .builder
                .build_int_compare(IntPredicate::NE, iv, z, "eq.b")
                .expect("cmp");
            Some((b.as_basic_value_enum(), MirRepr::Bool))
        }
        BinaryOp::EqEqEq => {
            let f = cx.module.get_function(RT_EQ_ID).expect("eq_id");
            let call = cx
                .builder
                .build_call(f, &[l.into(), r.into()], "eqid")
                .expect("eq_id");
            let iv = call.try_as_basic_value().unwrap_basic().into_int_value();
            let z = cx.i64t.const_int(0, false);
            let b = cx
                .builder
                .build_int_compare(IntPredicate::NE, iv, z, "eqid.b")
                .expect("cmp");
            Some((b.as_basic_value_enum(), MirRepr::Bool))
        }
        BinaryOp::NotEq => {
            let f = cx.module.get_function(RT_NE).expect("ne");
            let call = cx
                .builder
                .build_call(f, &[l.into(), r.into()], "ne")
                .expect("ne");
            let iv = call.try_as_basic_value().unwrap_basic().into_int_value();
            let z = cx.i64t.const_int(0, false);
            let b = cx
                .builder
                .build_int_compare(IntPredicate::NE, iv, z, "ne.b")
                .expect("cmp");
            Some((b.as_basic_value_enum(), MirRepr::Bool))
        }
        BinaryOp::NotEqEq => {
            let f = cx.module.get_function(RT_NE_ID).expect("ne_id");
            let call = cx
                .builder
                .build_call(f, &[l.into(), r.into()], "neid")
                .expect("ne_id");
            let iv = call.try_as_basic_value().unwrap_basic().into_int_value();
            let z = cx.i64t.const_int(0, false);
            let b = cx
                .builder
                .build_int_compare(IntPredicate::NE, iv, z, "neid.b")
                .expect("cmp");
            Some((b.as_basic_value_enum(), MirRepr::Bool))
        }
        _ => {
            let iv = emit_binop(cx, op, l, r);
            let rep = match op {
                BinaryOp::Lt
                | BinaryOp::Gt
                | BinaryOp::LtEq
                | BinaryOp::GtEq
                | BinaryOp::And
                | BinaryOp::Or => {
                    // emit_binop zexts compares to i64; treat as Int64 truthy
                    MirRepr::Int64
                }
                _ => MirRepr::Int64,
            };
            Some((iv.as_basic_value_enum(), rep))
        }
    }
}

fn emit_prim<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    prim: MirPrim,
    args: &[MirExpr],
) -> Option<IntValue<'ctx>> {
    match prim {
        MirPrim::ListLen => {
            if args.len() != 1 {
                cx.diags
                    .push(Diagnostic::error("list_len expects 1 arg").with_code("cg-prim"));
                return None;
            }
            let list = emit_expr_i64(cx, &args[0])?;
            let f = cx.module.get_function(RT_LIST_LEN).expect("list_len");
            let call = cx
                .builder
                .build_call(f, &[list.into()], "len")
                .expect("len");
            Some(call.try_as_basic_value().unwrap_basic().into_int_value())
        }
        MirPrim::ListGetChecked => {
            if args.len() != 2 {
                cx.diags
                    .push(Diagnostic::error("list_get expects 2 args").with_code("cg-prim"));
                return None;
            }
            let list = emit_expr_i64(cx, &args[0])?;
            let idx = emit_expr_i64(cx, &args[1])?;
            // Soft OOB semantics live in `echo_runtime_list_get` (not MIR BCE).
            let f = cx.module.get_function(RT_LIST_GET).expect("list_get");
            let call = cx
                .builder
                .build_call(f, &[list.into(), idx.into()], "get")
                .expect("get");
            Some(call.try_as_basic_value().unwrap_basic().into_int_value())
        }
    }
}

fn emit_struct_lit<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    type_name: &str,
    fields: &[(String, MirExpr)],
) -> Option<IntValue<'ctx>> {
    let set_f = cx.module.get_function(RT_STRUCT_SET).expect("struct_set");
    let handle = if type_name.is_empty() {
        let new_f = cx.module.get_function(RT_STRUCT_NEW).expect("struct_new");
        cx.builder
            .build_call(new_f, &[], "st")
            .expect("struct_new")
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value()
    } else {
        let new_f = cx
            .module
            .get_function(RT_STRUCT_NEW_NAMED)
            .expect("struct_new_named");
        let (ptr, len) = emit_const_bytes(cx, type_name.as_bytes());
        cx.builder
            .build_call(new_f, &[ptr.into(), len.into()], "st")
            .expect("struct_new_named")
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value()
    };
    for (name, val) in fields {
        let v = emit_expr_i64(cx, val)?;
        let (ptr, len) = emit_const_bytes(cx, name.as_bytes());
        let _ = cx
            .builder
            .build_call(
                set_f,
                &[handle.into(), ptr.into(), len.into(), v.into()],
                "",
            )
            .expect("struct_set");
    }
    Some(handle)
}

fn emit_string_lit<'ctx>(cx: &mut EmitCx<'_, 'ctx>, bytes: &[u8]) -> Option<IntValue<'ctx>> {
    let (ptr, len) = emit_const_bytes(cx, bytes);
    let f = cx
        .module
        .get_function(RT_STRING_FROM_UTF8)
        .expect("string_from_utf8");
    let call = cx
        .builder
        .build_call(f, &[ptr.into(), len.into()], "str")
        .expect("string_from_utf8 call");
    Some(call.try_as_basic_value().unwrap_basic().into_int_value())
}

fn emit_bytes_lit<'ctx>(cx: &mut EmitCx<'_, 'ctx>, bytes: &[u8]) -> Option<IntValue<'ctx>> {
    let (ptr, len) = emit_const_bytes(cx, bytes);
    let f = cx
        .module
        .get_function(RT_BYTES_FROM_PTR)
        .expect("bytes_from_ptr");
    let call = cx
        .builder
        .build_call(f, &[ptr.into(), len.into()], "bytes")
        .expect("bytes_from_ptr call");
    Some(call.try_as_basic_value().unwrap_basic().into_int_value())
}

fn emit_locator_lit<'ctx>(cx: &mut EmitCx<'_, 'ctx>, bytes: &[u8]) -> Option<IntValue<'ctx>> {
    let (ptr, len) = emit_const_bytes(cx, bytes);
    let f = cx
        .module
        .get_function(RT_LOCATOR_FROM_UTF8)
        .expect("locator_from_utf8");
    let call = cx
        .builder
        .build_call(f, &[ptr.into(), len.into()], "locator")
        .expect("locator_from_utf8 call");
    Some(call.try_as_basic_value().unwrap_basic().into_int_value())
}

fn emit_const_bytes<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    bytes: &[u8],
) -> (PointerValue<'ctx>, IntValue<'ctx>) {
    let i64t = cx.i64t;
    let init = cx.context.const_string(bytes, false);
    let arr_ty = init.get_type();
    let global = cx.module.add_global(arr_ty, None, "strlit");
    global.set_initializer(&init);
    global.set_constant(true);
    global.set_linkage(inkwell::module::Linkage::Private);
    let zero = i64t.const_int(0, false);
    let ptr = unsafe {
        cx.builder
            .build_in_bounds_gep(arr_ty, global.as_pointer_value(), &[zero, zero], "str.ptr")
            .expect("gep")
    };
    let len = i64t.const_int(bytes.len() as u64, false);
    (ptr, len)
}

fn emit_bytes_interp<'ctx>(cx: &mut EmitCx<'_, 'ctx>, parts: &[StrPart]) -> Option<IntValue<'ctx>> {
    let from_val = cx
        .module
        .get_function(RT_BYTES_FROM_VALUE)
        .expect("bytes_from_value");
    let cat_f = cx.module.get_function(RT_BYTES_CAT).expect("bytes_cat");
    let mut acc: Option<IntValue<'ctx>> = None;
    for part in parts {
        let chunk = match part {
            StrPart::Lit(bytes) => emit_bytes_lit(cx, bytes)?,
            StrPart::Name(name) => {
                let expr = if let Some(field) = name.strip_prefix('.') {
                    MirExpr::FieldGet {
                        base: Box::new(MirExpr::Name(echo_hir::RECV_PARAM.into())),
                        field: field.to_string(),
                    }
                } else {
                    MirExpr::Name(name.clone())
                };
                let v = emit_expr_i64(cx, &expr)?;
                cx.builder
                    .build_call(from_val, &[v.into()], "binterp")
                    .expect("bytes_from_value")
                    .try_as_basic_value()
                    .unwrap_basic()
                    .into_int_value()
            }
        };
        acc = Some(match acc {
            None => chunk,
            Some(prev) => cx
                .builder
                .build_call(cat_f, &[prev.into(), chunk.into()], "bcat")
                .expect("bytes_cat")
                .try_as_basic_value()
                .unwrap_basic()
                .into_int_value(),
        });
    }
    match acc {
        Some(v) => Some(v),
        None => emit_bytes_lit(cx, b""),
    }
}

fn emit_locator_interp<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    parts: &[StrPart],
) -> Option<IntValue<'ctx>> {
    let s = emit_string_interp(cx, parts)?;
    let f = cx
        .module
        .get_function(RT_LOCATOR_FROM_STRING)
        .expect("locator_from_string");
    let call = cx
        .builder
        .build_call(f, &[s.into()], "locator_interp")
        .expect("locator_from_string call");
    Some(call.try_as_basic_value().unwrap_basic().into_int_value())
}

fn emit_string_interp<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    parts: &[StrPart],
) -> Option<IntValue<'ctx>> {
    let new_f = cx
        .module
        .get_function(RT_STR_BUILDER_NEW)
        .expect("builder_new");
    let push_str = cx
        .module
        .get_function(RT_STR_BUILDER_PUSH_STR)
        .expect("push_str");
    let push_val = cx
        .module
        .get_function(RT_STR_BUILDER_PUSH_VALUE)
        .expect("push_val");
    let finish = cx
        .module
        .get_function(RT_STR_BUILDER_FINISH)
        .expect("finish");

    let b = cx
        .builder
        .build_call(new_f, &[], "sb")
        .expect("new")
        .try_as_basic_value()
        .unwrap_basic()
        .into_int_value();

    for part in parts {
        match part {
            StrPart::Lit(bytes) => {
                let (ptr, len) = emit_const_bytes(cx, bytes);
                let _ = cx
                    .builder
                    .build_call(push_str, &[b.into(), ptr.into(), len.into()], "")
                    .expect("push_str");
            }
            StrPart::Name(name) => {
                let expr = if let Some(field) = name.strip_prefix('.') {
                    // `{.field}` → field get on method receiver.
                    MirExpr::FieldGet {
                        base: Box::new(MirExpr::Name(echo_hir::RECV_PARAM.into())),
                        field: field.to_string(),
                    }
                } else {
                    MirExpr::Name(name.clone())
                };
                let v = emit_expr_i64(cx, &expr)?;
                let _ = cx
                    .builder
                    .build_call(push_val, &[b.into(), v.into()], "")
                    .expect("push_val");
            }
        }
    }

    let call = cx
        .builder
        .build_call(finish, &[b.into()], "interp")
        .expect("finish");
    Some(call.try_as_basic_value().unwrap_basic().into_int_value())
}

fn emit_list_lit<'ctx>(cx: &mut EmitCx<'_, 'ctx>, elems: &[MirExpr]) -> Option<IntValue<'ctx>> {
    let new_f = cx.module.get_function(RT_LIST_NEW).expect("list_new");
    let push_f = cx.module.get_function(RT_LIST_PUSH).expect("list_push");
    let list = cx
        .builder
        .build_call(new_f, &[], "list")
        .expect("new")
        .try_as_basic_value()
        .unwrap_basic()
        .into_int_value();
    for e in elems {
        let v = emit_expr_i64(cx, e)?;
        let _ = cx
            .builder
            .build_call(push_f, &[list.into(), v.into()], "")
            .expect("push");
    }
    Some(list)
}

fn emit_binop<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    op: BinaryOp,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
) -> IntValue<'ctx> {
    emit_binop_signedness(cx, op, l, r, /*unsigned*/ false)
}

fn emit_binop_signedness<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    op: BinaryOp,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    unsigned: bool,
) -> IntValue<'ctx> {
    match op {
        BinaryOp::Add => cx.builder.build_int_add(l, r, "add").expect("add"),
        BinaryOp::Sub => cx.builder.build_int_sub(l, r, "sub").expect("sub"),
        BinaryOp::Mul => cx.builder.build_int_mul(l, r, "mul").expect("mul"),
        BinaryOp::Div => {
            if unsigned {
                cx.builder
                    .build_int_unsigned_div(l, r, "udiv")
                    .expect("udiv")
            } else {
                cx.builder.build_int_signed_div(l, r, "div").expect("div")
            }
        }
        BinaryOp::Rem => {
            if unsigned {
                cx.builder
                    .build_int_unsigned_rem(l, r, "urem")
                    .expect("urem")
            } else {
                cx.builder.build_int_signed_rem(l, r, "rem").expect("rem")
            }
        }
        BinaryOp::BitAnd => cx.builder.build_and(l, r, "band").expect("band"),
        BinaryOp::BitOr => cx.builder.build_or(l, r, "bor").expect("bor"),
        BinaryOp::BitXor => cx.builder.build_xor(l, r, "bxor").expect("bxor"),
        BinaryOp::Shl => {
            // Mask shift count to bit-width (defined for all counts; matches wrapping_shl).
            let bits = l.get_type().get_bit_width();
            let mask = l.get_type().const_int((bits as u64) - 1, false);
            let amt = cx.builder.build_and(r, mask, "shlamt").expect("shlamt");
            cx.builder.build_left_shift(l, amt, "shl").expect("shl")
        }
        BinaryOp::Shr => {
            let bits = l.get_type().get_bit_width();
            let mask = l.get_type().const_int((bits as u64) - 1, false);
            let amt = cx.builder.build_and(r, mask, "shramt").expect("shramt");
            // Signed: arithmetic; unsigned: logical.
            cx.builder
                .build_right_shift(l, amt, !unsigned, "shr")
                .expect("shr")
        }
        BinaryOp::Eq | BinaryOp::EqEqEq => {
            let c = cx
                .builder
                .build_int_compare(IntPredicate::EQ, l, r, "eq")
                .expect("eq");
            cx.builder
                .build_int_z_extend(c, cx.i64t, "eq.i64")
                .expect("zext")
        }
        BinaryOp::NotEq | BinaryOp::NotEqEq => {
            let c = cx
                .builder
                .build_int_compare(IntPredicate::NE, l, r, "ne")
                .expect("ne");
            cx.builder
                .build_int_z_extend(c, cx.i64t, "ne.i64")
                .expect("zext")
        }
        BinaryOp::Lt => {
            let p = if unsigned {
                IntPredicate::ULT
            } else {
                IntPredicate::SLT
            };
            cmp_zext(cx, p, l, r, "lt")
        }
        BinaryOp::Gt => {
            let p = if unsigned {
                IntPredicate::UGT
            } else {
                IntPredicate::SGT
            };
            cmp_zext(cx, p, l, r, "gt")
        }
        BinaryOp::LtEq => {
            let p = if unsigned {
                IntPredicate::ULE
            } else {
                IntPredicate::SLE
            };
            cmp_zext(cx, p, l, r, "le")
        }
        BinaryOp::GtEq => {
            let p = if unsigned {
                IntPredicate::UGE
            } else {
                IntPredicate::SGE
            };
            cmp_zext(cx, p, l, r, "ge")
        }
        BinaryOp::And => {
            let z = cx.i64t.const_int(0, false);
            let lb = cx
                .builder
                .build_int_compare(IntPredicate::NE, l, z, "and.l")
                .expect("and.l");
            let rb = cx
                .builder
                .build_int_compare(IntPredicate::NE, r, z, "and.r")
                .expect("and.r");
            let b = cx.builder.build_and(lb, rb, "and").expect("and");
            cx.builder
                .build_int_z_extend(b, cx.i64t, "and.i64")
                .expect("zext")
        }
        BinaryOp::Or => {
            let z = cx.i64t.const_int(0, false);
            let lb = cx
                .builder
                .build_int_compare(IntPredicate::NE, l, z, "or.l")
                .expect("or.l");
            let rb = cx
                .builder
                .build_int_compare(IntPredicate::NE, r, z, "or.r")
                .expect("or.r");
            let b = cx.builder.build_or(lb, rb, "or").expect("or");
            cx.builder
                .build_int_z_extend(b, cx.i64t, "or.i64")
                .expect("zext")
        }
    }
}

fn cmp_zext<'ctx>(
    cx: &mut EmitCx<'_, 'ctx>,
    pred: IntPredicate,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    let c = cx.builder.build_int_compare(pred, l, r, name).expect("cmp");
    cx.builder
        .build_int_z_extend(c, cx.i64t, &format!("{name}.i64"))
        .expect("zext")
}

#[derive(Debug)]
pub struct AotArtifact {
    pub binary: PathBuf,
    pub ir_path: PathBuf,
}

#[derive(Debug)]
pub struct LinkError {
    pub message: String,
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LinkError {}

/// How clang links the AOT binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkMode {
    /// Host dynamic (default `xo run` / `xo build`).
    Dynamic,
    /// Fully static ELF (`xo build --static` / `--image`). Must have no `PT_INTERP`.
    Static,
}

impl LinkMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dynamic => "dynamic",
            Self::Static => "static",
        }
    }
}

/// Link already-optimized (or O0) LLVM IR text into a native binary.
///
/// Mid-end opts run in [`emit_llvm_with`]; clang only lowers IR to object and
/// links `libecho_runtime` (`-O0` so the mid-end is not re-run).
pub fn link_aot(ir: &str, work_dir: &Path, binary_name: &str) -> Result<AotArtifact, LinkError> {
    link_aot_with(ir, work_dir, binary_name, LinkMode::Dynamic)
}

/// Like [`link_aot`] with an explicit [`LinkMode`].
pub fn link_aot_with(
    ir: &str,
    work_dir: &Path,
    binary_name: &str,
    mode: LinkMode,
) -> Result<AotArtifact, LinkError> {
    fs::create_dir_all(work_dir).map_err(|e| LinkError {
        message: format!("create work dir {}: {e}", work_dir.display()),
    })?;

    let ir_path = work_dir.join("program.ll");
    fs::write(&ir_path, ir).map_err(|e| LinkError {
        message: format!("write IR: {e}"),
    })?;

    let binary = work_dir.join(binary_name);
    let runtime =
        runtime_lib::find_runtime_staticlib_for(mode).map_err(|message| LinkError { message })?;
    let clang = find_clang_for(mode).map_err(|message| LinkError { message })?;

    let mut cmd = Command::new(&clang);
    cmd.arg(&ir_path)
        .arg("-O0")
        .arg("-g")
        .arg("-o")
        .arg(&binary)
        .arg(&runtime);
    if mode == LinkMode::Static {
        cmd.arg("-static");
        cmd.arg("-lm");
        if clang_is_musl(&clang) {
            if let Some(target) = musl_clang_target() {
                cmd.arg(format!("--target={target}"));
            }
        }
    } else {
        cmd.arg("-lpthread").arg("-ldl").arg("-lm");
    }
    cmd.arg("-Wno-override-module");
    let output = cmd.output().map_err(|e| LinkError {
        message: format!("spawn clang ({}): {e}", clang.display()),
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut message = format!("clang failed ({}):\n{}", output.status, stderr);
        if let Some(hint) = runtime_lib::missing_runtime_symbol_hint(&stderr) {
            message.push('\n');
            message.push_str(hint);
        }
        return Err(LinkError { message });
    }

    if mode == LinkMode::Static {
        let bytes = fs::read(&binary).map_err(|e| LinkError {
            message: format!("read static binary: {e}"),
        })?;
        if elf::elf_needs_interpreter(&bytes) {
            return Err(LinkError {
                message: "static link produced a dynamic ELF (PT_INTERP). Build libecho_runtime for musl (`cargo build -p echo_runtime --target x86_64-unknown-linux-musl`) and set ECHO_RUNTIME_LIB, or install musl-clang.".into(),
            });
        }
    }

    Ok(AotArtifact { binary, ir_path })
}

fn clang_is_musl(clang: &Path) -> bool {
    clang
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|n| n.contains("musl"))
}

fn musl_clang_target() -> Option<&'static str> {
    if cfg!(target_arch = "x86_64") {
        Some("x86_64-unknown-linux-musl")
    } else if cfg!(target_arch = "aarch64") {
        Some("aarch64-unknown-linux-musl")
    } else {
        None
    }
}

fn find_clang_for(mode: LinkMode) -> Result<PathBuf, String> {
    if mode == LinkMode::Static {
        if let Ok(p) = std::env::var("ECHO_MUSL_CLANG") {
            let path = PathBuf::from(p);
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!("ECHO_MUSL_CLANG not a file: {}", path.display()));
        }
        if let Ok(p) = which("musl-clang") {
            return Ok(p);
        }
        if let Ok(p) = which("musl-gcc") {
            return Ok(p);
        }
    }
    find_clang()
}

fn find_clang() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("ECHO_CLANG") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!("ECHO_CLANG not a file: {}", path.display()));
    }
    which("clang").or_else(|_| which("clang-22"))
}

fn which(name: &str) -> Result<PathBuf, String> {
    let output = Command::new("which")
        .arg(name)
        .output()
        .map_err(|e| format!("which {name}: {e}"))?;
    if !output.status.success() {
        return Err(format!("`{name}` not found on PATH"));
    }
    let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(p))
}

// --- Durable LLVM IR artifact cache (infra v3) ---

/// Format version for [`encode_ir_artifact`] / [`decode_ir_artifact`].
/// Bumped when IR payload shape changes (opt-level-aware cache keys live outside).
pub const IR_ARTIFACT_FORMAT: u32 = 1;

const IR_MAGIC: &[u8] = b"ECHOIR01";

/// Encode successful LLVM IR text for the codegen phase cache.
#[must_use]
pub fn encode_ir_artifact(ir: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(IR_MAGIC.len() + 4 + ir.len());
    out.extend_from_slice(IR_MAGIC);
    out.extend_from_slice(&IR_ARTIFACT_FORMAT.to_le_bytes());
    out.extend_from_slice(ir.as_bytes());
    out
}

/// Decode IR produced by [`encode_ir_artifact`].
#[must_use]
pub fn decode_ir_artifact(bytes: &[u8]) -> Option<String> {
    if bytes.len() < IR_MAGIC.len() + 4 {
        return None;
    }
    if &bytes[..IR_MAGIC.len()] != IR_MAGIC {
        return None;
    }
    let ver = u32::from_le_bytes(bytes[IR_MAGIC.len()..IR_MAGIC.len() + 4].try_into().ok()?);
    if ver != IR_ARTIFACT_FORMAT {
        return None;
    }
    String::from_utf8(bytes[IR_MAGIC.len() + 4..].to_vec()).ok()
}

#[cfg(test)]
mod ir_cache_tests {
    use super::*;

    #[test]
    fn ir_artifact_roundtrip() {
        let ir = "; ModuleID = 'echo'\ndefine i64 @echo_entry() {\n  ret i64 0\n}\n";
        let bytes = encode_ir_artifact(ir);
        assert_eq!(decode_ir_artifact(&bytes).as_deref(), Some(ir));
        assert!(decode_ir_artifact(b"junk").is_none());
    }
}

#[cfg(test)]
mod native_repr_tests {
    use super::*;
    use echo_mir::{
        CallTarget, MirExpr, MirFn, MirPrim, MirProgram, MirRetShape, MirStmt, analyze_escapes,
        analyze_reprs, construct_ssa, inject_lifetime, simplify_local, structured_to_cfg,
    };
    use std::path::PathBuf;

    fn finish_mir(
        params: &[String],
        stmts: &[MirStmt],
    ) -> (
        echo_mir::MirCfg,
        std::collections::HashMap<String, echo_mir::MirRepr>,
        std::collections::HashMap<String, echo_mir::EscapeClass>,
    ) {
        let cfg = structured_to_cfg(stmts, MirRetShape::Plain);
        let cfg = construct_ssa(cfg, params);
        let (cfg, reprs) = analyze_reprs(cfg, params);
        let (cfg, reprs) = simplify_local(cfg, reprs);
        let (cfg, reprs, escapes) = analyze_escapes(cfg, reprs);
        let (cfg, reprs) = simplify_local(cfg, reprs);
        (cfg, reprs, escapes)
    }

    fn emit_fn(params: &[&str], stmts: Vec<MirStmt>) -> String {
        let params: Vec<String> = params.iter().map(|s| (*s).to_string()).collect();
        let (cfg, reprs, escapes) = finish_mir(&params, &stmts);
        let f = MirFn {
            module_path: PathBuf::from("/t.echo"),
            name: "f".into(),
            params: params.clone(),
            body: stmts,
            cfg,
            reprs,
            escapes,
            ret: MirRetShape::Plain,
            debug_vars: vec![],
        };
        let prog = MirProgram {
            functions: vec![f],
            entry_path: PathBuf::from("/t.echo"),
        };
        let emitted = emit_llvm(&prog);
        assert_eq!(
            emitted.diagnostics.error_count(),
            0,
            "{:?}",
            emitted.diagnostics.items()
        );
        emitted.ir
    }

    #[test]
    fn ir_has_line_table_debug_info() {
        let ir = emit_stmts(vec![MirStmt::ReturnOk(MirExpr::ConstI64(0), None)]);
        assert!(
            ir.contains("!DICompileUnit")
                && ir.contains("!DISubprogram")
                && ir.contains("!DILocation"),
            "expected DWARF line-table metadata; ir=\n{ir}"
        );
        assert!(
            ir.contains("echo_entry") && ir.contains("producer: \"xo\""),
            "expected xo compile unit and echo_entry; ir=\n{ir}"
        );
        assert!(
            ir.contains("t.echo"),
            "expected source file name in DI; ir=\n{ir}"
        );
    }

    #[test]
    fn bytes_interp_calls_bytes_from_value() {
        use echo_mir::StrPart;
        let ir = emit_fn(
            &["n"],
            vec![MirStmt::ReturnOk(
                MirExpr::BytesInterp {
                    parts: vec![StrPart::Lit(b"x=".to_vec()), StrPart::Name("n".into())],
                },
                None,
            )],
        );
        assert!(
            ir.contains("echo_runtime_bytes_from_value"),
            "expected bytes_from_value in IR; ir=\n{ir}"
        );
        assert!(
            ir.contains("echo_runtime_bytes_from_ptr") || ir.contains("echo_runtime_bytes_cat"),
            "expected bytes lit/cat for live bytes interp; ir=\n{ir}"
        );
    }

    #[test]
    fn locator_interp_calls_locator_from_string() {
        use echo_mir::StrPart;
        let ir = emit_fn(
            &["host"],
            vec![MirStmt::ReturnOk(
                MirExpr::LocatorInterp {
                    parts: vec![
                        StrPart::Lit(b"http://".to_vec()),
                        StrPart::Name("host".into()),
                    ],
                },
                None,
            )],
        );
        assert!(
            ir.contains("echo_runtime_locator_from_string"),
            "expected locator_from_string in IR; ir=\n{ir}"
        );
        assert!(
            ir.contains("echo_runtime_string_builder_new"),
            "expected string builder for live locator interp; ir=\n{ir}"
        );
    }

    #[test]
    fn ir_has_per_instruction_line_not_only_one() {
        use echo_source::{BytePos, SourceId, Span};
        let dir = std::env::temp_dir().join(format!("echo-di-line-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("multi.echo");
        let src = "$ a = 1\n$ b = 2\n^ b\n";
        std::fs::write(&path, src).unwrap();
        let off = src.find('^').expect("^") as u32;
        let span = Span::new(SourceId::from_u32(0), BytePos(off), BytePos(off + 3));
        let params: Vec<String> = vec![];
        let stmts = inject_lifetime(vec![
            MirStmt::Set {
                name: "b".into(),
                value: MirExpr::ConstI64(2),
                span: Some(span),
            },
            MirStmt::ReturnOk(MirExpr::Name("b".into()), Some(span)),
        ]);
        let (cfg, reprs, escapes) = finish_mir(&params, &stmts);
        let f = MirFn {
            module_path: path.clone(),
            name: "f".into(),
            params,
            body: stmts,
            cfg,
            reprs,
            escapes,
            ret: MirRetShape::Plain,
            debug_vars: vec![],
        };
        let prog = MirProgram {
            functions: vec![f],
            entry_path: path,
        };
        let ir = emit_llvm(&prog).ir;
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            ir.contains("!DILocation(line: 3,"),
            "expected a location on source line 3, not only line 1; ir=\n{ir}"
        );
        let only_line_1 = ir
            .lines()
            .filter(|l| l.contains("!DILocation(line:"))
            .all(|l| l.contains("!DILocation(line: 1,"));
        assert!(!only_line_1, "every DILocation was line 1; ir=\n{ir}");
    }

    #[test]
    fn ir_has_local_variable_and_type_di() {
        let params = vec!["n".to_string()];
        let stmts = vec![MirStmt::ReturnOk(MirExpr::Name("n".into()), None)];
        let (cfg, reprs, escapes) = finish_mir(&params, &stmts);
        let f = MirFn {
            module_path: PathBuf::from("/t.echo"),
            name: "f".into(),
            params: params.clone(),
            body: stmts,
            cfg,
            reprs,
            escapes,
            ret: MirRetShape::Plain,
            debug_vars: vec![("n".into(), "i64".into())],
        };
        let ir = emit_llvm(&MirProgram {
            functions: vec![f],
            entry_path: PathBuf::from("/t.echo"),
        })
        .ir;
        assert!(
            ir.contains("!DILocalVariable") && ir.contains("name: \"n\""),
            "expected DILocalVariable for n; ir=\n{ir}"
        );
        assert!(
            ir.contains("!DIBasicType") && ir.contains("name: \"i64\""),
            "expected checker kind label as DIBasicType; ir=\n{ir}"
        );
    }

    fn emit_stmts(stmts: Vec<MirStmt>) -> String {
        emit_fn(&[], stmts)
    }

    #[test]
    fn int_arith_emits_native_i64_add() {
        // Params are ABI-boxed; unbox + native add (avoids LLVM const-fold of 1+2).
        let ir = emit_fn(
            &["a", "b"],
            vec![
                MirStmt::Set {
                    name: "c".into(),
                    value: MirExpr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(MirExpr::Name("a".into())),
                        right: Box::new(MirExpr::Name("b".into())),
                    },
                    span: None,
                },
                MirStmt::ReturnOk(MirExpr::Name("c".into()), None),
            ],
        );
        assert!(
            ir.contains("add i64") || ir.contains("add nsw i64"),
            "expected native i64 add; ir=\n{ir}"
        );
        assert!(
            !ir.contains("call i64 @echo_runtime_struct_new")
                && !ir.contains("call i64 @echo_runtime_list_new"),
            "unexpected heap alloc call in pure int arith; ir=\n{ir}"
        );
    }

    #[test]
    fn comparison_emits_native_i1() {
        let ir = emit_fn(
            &["a", "b"],
            vec![
                MirStmt::Set {
                    name: "t".into(),
                    value: MirExpr::Binary {
                        op: BinaryOp::Lt,
                        left: Box::new(MirExpr::Name("a".into())),
                        right: Box::new(MirExpr::Name("b".into())),
                    },
                    span: None,
                },
                MirStmt::ReturnOk(MirExpr::Name("t".into()), None),
            ],
        );
        assert!(
            ir.contains("icmp slt i64") || ir.contains("icmp ult i64"),
            "expected native icmp; ir=\n{ir}"
        );
        assert!(
            ir.contains("zext i1") || ir.contains("icmp"),
            "bool path should involve i1; ir=\n{ir}"
        );
    }

    #[test]
    fn same_type_phi_stays_i64() {
        // Non-constant condition so both arms stay live.
        let ir = emit_fn(
            &["p"],
            vec![
                MirStmt::If {
                    arms: vec![(
                        MirExpr::Name("p".into()),
                        vec![MirStmt::Set {
                            name: "x".into(),
                            value: MirExpr::ConstI64(10),
                            span: None,
                        }],
                    )],
                    else_body: Some(vec![MirStmt::Set {
                        name: "x".into(),
                        value: MirExpr::ConstI64(20),
                        span: None,
                    }]),
                },
                MirStmt::ReturnOk(MirExpr::Name("x".into()), None),
            ],
        );
        assert!(
            ir.contains("phi i64"),
            "same-type int phi should be i64; ir=\n{ir}"
        );
        assert!(
            !ir.contains("phi i1"),
            "should not use i1 phi for int merge"
        );
    }

    #[test]
    fn mixed_phi_is_boxed_i64() {
        let ir = emit_fn(
            &["p"],
            vec![
                MirStmt::If {
                    arms: vec![(
                        MirExpr::Name("p".into()),
                        vec![MirStmt::Set {
                            name: "x".into(),
                            value: MirExpr::ConstI64(1),
                            span: None,
                        }],
                    )],
                    else_body: Some(vec![MirStmt::Set {
                        name: "x".into(),
                        value: MirExpr::ConstBool(true),
                        span: None,
                    }]),
                },
                MirStmt::ReturnOk(MirExpr::Name("x".into()), None),
            ],
        );
        assert!(
            ir.contains("phi i64"),
            "mixed phi falls back to boxed i64; ir=\n{ir}"
        );
        // Const true may fold to i64 1 without a visible zext; φ must not be i1.
        assert!(
            !ir.contains("phi i1"),
            "mixed phi must not stay native i1; ir=\n{ir}"
        );
    }

    #[test]
    fn print_boxes_int_at_abi_boundary_only() {
        let ir = emit_fn(
            &["n"],
            vec![
                MirStmt::Set {
                    name: "m".into(),
                    value: MirExpr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(MirExpr::Name("n".into())),
                        right: Box::new(MirExpr::ConstI64(1)),
                    },
                    span: None,
                },
                MirStmt::Eval(MirExpr::Call {
                    target: CallTarget::Runtime {
                        export: "print".into(),
                    },
                    args: vec![MirExpr::Name("m".into())],
                    ret: MirRetShape::Plain,
                }),
                MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
            ],
        );
        assert!(
            ir.contains("add i64") || ir.contains("add nsw i64"),
            "arith stays native; ir=\n{ir}"
        );
        assert!(
            ir.contains("echo_runtime_print_i64"),
            "print uses runtime ABI; ir=\n{ir}"
        );
        assert!(!ir.contains("call i64 @echo_runtime_list_new"));
        assert!(!ir.contains("call i64 @echo_runtime_struct_new"));
    }

    #[test]
    fn no_redundant_alloc_for_proven_scalars() {
        let ir = emit_fn(
            &["a"],
            vec![
                MirStmt::Set {
                    name: "b".into(),
                    value: MirExpr::Binary {
                        op: BinaryOp::Mul,
                        left: Box::new(MirExpr::Name("a".into())),
                        right: Box::new(MirExpr::ConstI64(4)),
                    },
                    span: None,
                },
                MirStmt::ReturnOk(MirExpr::Name("b".into()), None),
            ],
        );
        assert!(
            ir.contains("mul ") || ir.contains("mul i64"),
            "expected native mul; ir=\n{ir}"
        );
        assert!(!ir.contains("malloc"));
        assert!(!ir.contains("call i64 @echo_runtime_list_new"));
        assert!(!ir.contains("call i64 @echo_runtime_struct_new"));
        assert!(!ir.contains("call i64 @echo_runtime_string_from_utf8"));
    }

    #[test]
    fn loop_with_native_add_has_clean_control_flow() {
        // while p { t = x + y }; return t — native ops + loop shape (LLVM LICM residual).
        let ir = emit_fn(
            &["p", "x", "y"],
            vec![
                MirStmt::Loop {
                    cond: Some(MirExpr::Name("p".into())),
                    body: vec![MirStmt::Set {
                        name: "t".into(),
                        value: MirExpr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(MirExpr::Name("x".into())),
                            right: Box::new(MirExpr::Name("y".into())),
                        },
                        span: None,
                    }],
                },
                MirStmt::ReturnOk(MirExpr::Name("t".into()), None),
            ],
        );
        assert!(
            ir.contains("add i64") || ir.contains("add nsw i64"),
            "expected native add; ir=\n{ir}"
        );
        assert!(
            ir.contains("br i1") || ir.matches("br label").count() >= 2,
            "expected loop control flow; ir=\n{ir}"
        );
    }

    #[test]
    fn repeated_native_adds_are_real_llvm_ops() {
        // Two x+y binds — MIR does not GVN; handoff is still native `add` for LLVM.
        let ir = emit_fn(
            &["x", "y"],
            vec![
                MirStmt::Set {
                    name: "s1".into(),
                    value: MirExpr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(MirExpr::Name("x".into())),
                        right: Box::new(MirExpr::Name("y".into())),
                    },
                    span: None,
                },
                MirStmt::Set {
                    name: "s2".into(),
                    value: MirExpr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(MirExpr::Name("x".into())),
                        right: Box::new(MirExpr::Name("y".into())),
                    },
                    span: None,
                },
                MirStmt::ReturnOk(
                    MirExpr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(MirExpr::Name("s1".into())),
                        right: Box::new(MirExpr::Name("s2".into())),
                    },
                    None,
                ),
            ],
        );
        let add_count = ir.matches("add i64").count() + ir.matches("add nsw i64").count();
        assert!(
            add_count >= 2,
            "expected multiple native adds for LLVM to CSE; got {add_count}; ir=\n{ir}"
        );
        assert!(!ir.contains("call i64 @echo_runtime_eq"));
    }

    #[test]
    fn constant_native_expr_has_no_runtime_arith() {
        // 1+2 may remain as `add` at O0 (LLVM folds residual); never a runtime helper.
        let ir = emit_stmts(vec![
            MirStmt::Set {
                name: "c".into(),
                value: MirExpr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(MirExpr::ConstI64(1)),
                    right: Box::new(MirExpr::ConstI64(2)),
                },
                span: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("c".into()), None),
        ]);
        assert!(
            ir.contains("add i64")
                || ir.contains("add nsw i64")
                || ir.contains("ret i64 3")
                || ir.contains("i64 3"),
            "expected native const arith or fold; ir=\n{ir}"
        );
        assert!(!ir.contains("call i64 @echo_runtime_eq"));
        assert!(!ir.contains("call i64 @echo_runtime_list_new"));
    }

    #[test]
    fn simplified_scalar_flow_no_redundant_box_unbox_ops() {
        // After simplify: unbox params → add → box once for print; no box/unbox pairs.
        let params = vec!["a".into(), "b".into()];
        let stmts = vec![
            MirStmt::Set {
                name: "c".into(),
                value: MirExpr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(MirExpr::Name("a".into())),
                    right: Box::new(MirExpr::Name("b".into())),
                },
                span: None,
            },
            MirStmt::Eval(MirExpr::Call {
                target: CallTarget::Runtime {
                    export: "print".into(),
                },
                args: vec![MirExpr::Name("c".into())],
                ret: MirRetShape::Plain,
            }),
            MirStmt::ReturnOk(MirExpr::Name("a".into()), None),
        ];
        let (cfg, reprs, escapes) = finish_mir(&params, &stmts);
        // Count nested BoxValue∘UnboxValue / UnboxValue∘BoxValue in the CFG text
        let dump = format!("{cfg:?}");
        assert!(
            !dump.contains("UnboxValue { value: BoxValue")
                && !dump.contains("BoxValue { value: UnboxValue"),
            "redundant pairs should be gone; {dump}"
        );
        let f = MirFn {
            module_path: PathBuf::from("/t.echo"),
            name: "f".into(),
            params: params.clone(),
            body: stmts,
            cfg,
            reprs,
            escapes,
            ret: MirRetShape::Plain,
            debug_vars: vec![],
        };
        let prog = MirProgram {
            functions: vec![f],
            entry_path: PathBuf::from("/t.echo"),
        };
        let ir = emit_llvm(&prog).ir;
        assert!(ir.contains("add i64") || ir.contains("add nsw i64"), "{ir}");
        assert!(ir.contains("echo_runtime_print_i64"), "{ir}");
        // i64↔i64 box is a no-op bit-identity; no heap runtime for scalars
        assert!(!ir.contains("call i64 @echo_runtime_list_new"));
        assert!(!ir.contains("call i64 @echo_runtime_struct_new"));
    }

    #[test]
    fn eprint_emits_runtime_stderr_symbol() {
        let ir = emit_fn(
            &["s"],
            vec![
                MirStmt::Eval(MirExpr::Call {
                    target: CallTarget::Runtime {
                        export: "eprint".into(),
                    },
                    args: vec![MirExpr::Name("s".into())],
                    ret: MirRetShape::Plain,
                }),
                MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
            ],
        );
        assert!(
            ir.contains("echo_runtime_eprint"),
            "eprint uses stderr ABI; ir=\n{ir}"
        );
        assert!(
            !ir.contains("call void @echo_runtime_print_i64"),
            "eprint must not lower to stdout print; ir=\n{ir}"
        );
    }

    #[test]
    fn stdin_read_emits_runtime_symbol() {
        let ir = emit_fn(
            &[],
            vec![
                MirStmt::Eval(MirExpr::Call {
                    target: CallTarget::Runtime {
                        export: "stdin_read".into(),
                    },
                    args: vec![],
                    ret: MirRetShape::Plain,
                }),
                MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
            ],
        );
        assert!(
            ir.contains("echo_runtime_stdin_read"),
            "stdin_read uses stdin ABI; ir=\n{ir}"
        );
    }

    #[test]
    fn list_get_emits_runtime_checked_get() {
        let ir = emit_fn(
            &["xs", "i"],
            vec![
                MirStmt::Set {
                    name: "v".into(),
                    value: MirExpr::PrimCall {
                        prim: MirPrim::ListGetChecked,
                        args: vec![MirExpr::Name("xs".into()), MirExpr::Name("i".into())],
                    },
                    span: None,
                },
                MirStmt::ReturnOk(MirExpr::Name("v".into()), None),
            ],
        );
        assert!(
            ir.contains("call i64 @echo_runtime_list_get"),
            "list get is runtime soft-check; ir=\n{ir}"
        );
    }

    #[test]
    fn loop_native_phi_add_icmp_br() {
        // i = 0; while i < 10 { i = i + 1 }; return i — SSA + native scalar handoff.
        let ir = emit_fn(
            &[],
            vec![
                MirStmt::Set {
                    name: "i".into(),
                    value: MirExpr::ConstI64(0),
                    span: None,
                },
                MirStmt::Loop {
                    cond: Some(MirExpr::Binary {
                        op: BinaryOp::Lt,
                        left: Box::new(MirExpr::Name("i".into())),
                        right: Box::new(MirExpr::ConstI64(10)),
                    }),
                    body: vec![MirStmt::Set {
                        name: "i".into(),
                        value: MirExpr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(MirExpr::Name("i".into())),
                            right: Box::new(MirExpr::ConstI64(1)),
                        },
                        span: None,
                    }],
                },
                MirStmt::ReturnOk(MirExpr::Name("i".into()), None),
            ],
        );
        assert!(ir.contains("phi i64"), "expected native i64 phi; ir=\n{ir}");
        assert!(
            ir.contains("add i64") || ir.contains("add nsw i64"),
            "expected native add; ir=\n{ir}"
        );
        assert!(
            ir.contains("icmp slt i64") || ir.contains("icmp ult i64"),
            "expected icmp; ir=\n{ir}"
        );
        assert!(
            ir.contains("br i1"),
            "expected conditional branch; ir=\n{ir}"
        );
        assert!(!ir.contains("call i64 @echo_runtime_eq"));
        assert!(!ir.contains("call i64 @echo_runtime_list_new"));
        assert!(!ir.contains("call i64 @echo_runtime_struct_new"));
    }
}

/// Cache key for a linked AOT binary derived from **post-opt** LLVM IR text.
///
/// Hosts pass IR already optimized at the selected [`OptLevel`]. The key
/// includes: IR bytes (opt participates via IR content), explicit `opt` token
/// (defense in depth if two levels emit identical text), runtime ABI, and
/// lower/codegen fingerprints so ABI or emitter changes never reuse a binary.
#[must_use]
pub fn aot_binary_cache_key(ir: &str) -> echo_cache::PhaseCacheKey {
    aot_binary_cache_key_with_opt(ir, OptLevel::O0)
}

/// Like [`aot_binary_cache_key`] but records the requested optimization level.
#[must_use]
pub fn aot_binary_cache_key_with_opt(ir: &str, opt: OptLevel) -> echo_cache::PhaseCacheKey {
    aot_binary_cache_key_with(ir, opt, LinkMode::Dynamic)
}

/// AOT cache key including link mode (static vs dynamic binaries must not collide).
#[must_use]
pub fn aot_binary_cache_key_with(
    ir: &str,
    opt: OptLevel,
    mode: LinkMode,
) -> echo_cache::PhaseCacheKey {
    use echo_cache::PhaseCacheKey;
    use echo_fingerprint::{ArtifactPhase, RUNTIME_ABI_VERSION, phase_fingerprint};
    let abi = RUNTIME_ABI_VERSION.to_string();
    let lower = phase_fingerprint(ArtifactPhase::Lower, &[]);
    let codegen = phase_fingerprint(ArtifactPhase::Codegen, &[]);
    let lower_s = lower.fingerprint.as_str().to_string();
    let codegen_s = codegen.fingerprint.as_str().to_string();
    let opt_s = opt.as_str();
    PhaseCacheKey::for_source(
        ArtifactPhase::Codegen,
        ir.as_bytes(),
        &[
            ("artifact", "aot_binary"),
            ("runtime_abi", abi.as_str()),
            ("lower_fp", lower_s.as_str()),
            ("codegen_fp", codegen_s.as_str()),
            ("opt", opt_s),
            ("link", mode.as_str()),
        ],
    )
}

#[cfg(test)]
mod llvm_opt_tests {
    //! LLVM opt tests use the production MIR handoff only:
    //! CFG → SSA → repr → simplify → escape → simplify → emit (± LLVM opt).

    use super::*;
    use echo_mir::{
        CallTarget, MirExpr, MirFn, MirProgram, MirRetShape, MirStmt, analyze_escapes,
        analyze_reprs, construct_ssa, simplify_local, structured_to_cfg,
    };
    use std::path::PathBuf;
    use std::time::Instant;

    fn finish_handoff(
        params: &[String],
        stmts: &[MirStmt],
    ) -> (
        echo_mir::MirCfg,
        std::collections::HashMap<String, echo_mir::MirRepr>,
        std::collections::HashMap<String, echo_mir::EscapeClass>,
    ) {
        let cfg = structured_to_cfg(stmts, MirRetShape::Plain);
        let cfg = construct_ssa(cfg, params);
        let (cfg, reprs) = analyze_reprs(cfg, params);
        let (cfg, reprs) = simplify_local(cfg, reprs);
        let (cfg, reprs, escapes) = analyze_escapes(cfg, reprs);
        let (cfg, reprs) = simplify_local(cfg, reprs);
        (cfg, reprs, escapes)
    }

    fn mir_fn(module: &PathBuf, name: &str, params: &[&str], stmts: Vec<MirStmt>) -> MirFn {
        let params: Vec<String> = params.iter().map(|s| (*s).to_string()).collect();
        let (cfg, reprs, escapes) = finish_handoff(&params, &stmts);
        MirFn {
            module_path: module.clone(),
            name: name.into(),
            params,
            body: stmts,
            cfg,
            reprs,
            escapes,
            ret: MirRetShape::Plain,
            debug_vars: vec![],
        }
    }

    fn prog_toplevel(stmts: Vec<MirStmt>) -> MirProgram {
        let path = PathBuf::from("/t.echo");
        MirProgram {
            functions: vec![mir_fn(&path, "__toplevel", &[], stmts)],
            entry_path: path,
        }
    }

    /// Two functions: call edge remains after MIR; LLVM O2 may inline.
    fn prog_with_call() -> MirProgram {
        let path = PathBuf::from("/t.echo");
        let add1 = mir_fn(
            &path,
            "add1",
            &["a"],
            vec![
                MirStmt::Set {
                    name: "r".into(),
                    value: MirExpr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(MirExpr::Name("a".into())),
                        right: Box::new(MirExpr::ConstI64(1)),
                    },
                    span: None,
                },
                MirStmt::ReturnOk(MirExpr::Name("r".into()), None),
            ],
        );
        let top = mir_fn(
            &path,
            "__toplevel",
            &[],
            vec![
                MirStmt::Set {
                    name: "v".into(),
                    value: MirExpr::Call {
                        target: CallTarget::Function {
                            module_path: path.clone(),
                            name: "add1".into(),
                        },
                        args: vec![MirExpr::ConstI64(41)],
                        ret: MirRetShape::Plain,
                    },
                    span: None,
                },
                MirStmt::ReturnOk(MirExpr::Name("v".into()), None),
            ],
        );
        MirProgram {
            functions: vec![add1, top],
            entry_path: path,
        }
    }

    #[test]
    fn emit_verifies_and_o0_matches_pre_opt() {
        let prog = prog_toplevel(vec![MirStmt::ReturnOk(MirExpr::ConstI64(0), None)]);
        let e = emit_llvm_with(&prog, OptLevel::O0);
        assert_eq!(
            e.diagnostics.error_count(),
            0,
            "{:?}",
            e.diagnostics.items()
        );
        assert_eq!(e.ir, e.ir_pre_opt);
        assert!(e.ir.contains("define"));
    }

    #[test]
    fn handoff_shape_is_native_scalar_not_runtime_arith() {
        // Params keep MIR from folding the add; codegen must still emit native i64.
        let path = PathBuf::from("/t.echo");
        let prog = MirProgram {
            functions: vec![mir_fn(
                &path,
                "f",
                &["a", "b"],
                vec![
                    MirStmt::Set {
                        name: "c".into(),
                        value: MirExpr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(MirExpr::Name("a".into())),
                            right: Box::new(MirExpr::Name("b".into())),
                        },
                        span: None,
                    },
                    MirStmt::ReturnOk(MirExpr::Name("c".into()), None),
                ],
            )],
            entry_path: path,
        };
        let e = emit_llvm_with(&prog, OptLevel::O0);
        assert_eq!(
            e.diagnostics.error_count(),
            0,
            "{:?}",
            e.diagnostics.items()
        );
        assert!(
            e.ir.contains("add i64") || e.ir.contains("add nsw i64"),
            "hyper-optimizable handoff needs native add; ir=\n{}",
            e.ir
        );
        assert!(
            !e.ir.contains("call i64 @echo_runtime_eq"),
            "no runtime arith helper; ir=\n{}",
            e.ir
        );
    }

    #[test]
    fn o2_verifies_and_preserves_jit_semantics_on_call() {
        let prog = prog_with_call();
        let o0 = emit_llvm_with(&prog, OptLevel::O0);
        let o2 = emit_llvm_with(&prog, OptLevel::O2);
        assert_eq!(
            o0.diagnostics.error_count(),
            0,
            "{:?}",
            o0.diagnostics.items()
        );
        assert_eq!(
            o2.diagnostics.error_count(),
            0,
            "{:?}",
            o2.diagnostics.items()
        );
        // O0 post-emit equals pre-opt; O2 may rewrite (e.g. inline) but must verify.
        assert_eq!(o0.ir, o0.ir_pre_opt);
        let s0 = run_jit_ir(&o0.ir).expect("jit o0");
        let s2 = run_jit_ir(&o2.ir).expect("jit o2");
        assert_eq!(s0, s2);
        assert_eq!(s0, 42);
    }

    #[test]
    fn all_opt_levels_verify_and_match_jit_semantics() {
        let prog = prog_with_call();
        let mut statuses = Vec::new();
        for opt in [
            OptLevel::O0,
            OptLevel::O1,
            OptLevel::O2,
            OptLevel::O3,
            OptLevel::Oz,
        ] {
            let e = emit_llvm_with(&prog, opt);
            assert_eq!(
                e.diagnostics.error_count(),
                0,
                "opt={opt}: {:?}",
                e.diagnostics.items()
            );
            assert_eq!(e.opt, opt);
            if opt == OptLevel::O0 {
                assert_eq!(e.ir, e.ir_pre_opt, "O0 must not run mid-end");
            }
            let status = run_jit_ir(&e.ir).unwrap_or_else(|err| panic!("jit {opt}: {err}"));
            statuses.push((opt, status));
        }
        let expected = statuses[0].1;
        for (opt, status) in &statuses {
            assert_eq!(*status, expected, "opt {opt} diverged from O0 semantics");
        }
        assert_eq!(expected, 42);
    }

    #[test]
    fn oz_pipeline_is_not_o2_alias_in_config() {
        assert_eq!(OptLevel::Oz.pass_pipeline(), Some("default<Oz>"));
        assert_ne!(OptLevel::Oz.as_str(), OptLevel::O2.as_str());
    }

    #[test]
    fn aot_binary_keys_differ_when_ir_differs_by_opt() {
        let prog = prog_with_call();
        let o0 = emit_llvm_with(&prog, OptLevel::O0);
        let o2 = emit_llvm_with(&prog, OptLevel::O2);
        let oz = emit_llvm_with(&prog, OptLevel::Oz);
        assert_eq!(o0.diagnostics.error_count(), 0);
        assert_eq!(o2.diagnostics.error_count(), 0);
        assert_eq!(oz.diagnostics.error_count(), 0);
        let k0 = aot_binary_cache_key(&o0.ir);
        let k2 = aot_binary_cache_key(&o2.ir);
        let kz = aot_binary_cache_key(&oz.ir);
        // When mid-end rewrites IR, AOT keys must not collide across levels.
        if o0.ir != o2.ir {
            assert_ne!(k0.blob_name(), k2.blob_name());
        }
        if o2.ir != oz.ir {
            assert_ne!(k2.blob_name(), kz.blob_name());
        }
        // Same IR always same AOT key (deterministic).
        assert_eq!(aot_binary_cache_key(&o0.ir).blob_name(), k0.blob_name());
        // Explicit opt token distinguishes even if IR text matched.
        assert_ne!(
            aot_binary_cache_key_with_opt(&o0.ir, OptLevel::O0).blob_name(),
            aot_binary_cache_key_with_opt(&o0.ir, OptLevel::O2).blob_name()
        );
        assert_ne!(
            aot_binary_cache_key_with(&o0.ir, OptLevel::O0, LinkMode::Dynamic).blob_name(),
            aot_binary_cache_key_with(&o0.ir, OptLevel::O0, LinkMode::Static).blob_name()
        );
    }

    #[test]
    fn i32_width_emits_native_i32_add() {
        // Non-constant operands so LLVM does not fold away the add.
        let path = PathBuf::from("/t.echo");
        let params = vec!["a".into(), "b".into()];
        let stmts = vec![
            MirStmt::Set {
                name: "c".into(),
                value: MirExpr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(MirExpr::UnboxValue {
                        value: Box::new(MirExpr::Name("a".into())),
                        to: MirRepr::Int32,
                    }),
                    right: Box::new(MirExpr::UnboxValue {
                        value: Box::new(MirExpr::Name("b".into())),
                        to: MirRepr::Int32,
                    }),
                },
                span: None,
            },
            MirStmt::ReturnOk(
                MirExpr::BoxValue {
                    value: Box::new(MirExpr::Name("c".into())),
                    from: MirRepr::Int32,
                },
                None,
            ),
        ];
        let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
        let cfg = construct_ssa(cfg, &params);
        let (cfg, reprs) = analyze_reprs(cfg, &params);
        let f = MirFn {
            module_path: path.clone(),
            name: "f".into(),
            params: params.clone(),
            body: stmts,
            cfg,
            reprs,
            escapes: Default::default(),
            ret: MirRetShape::Plain,
            debug_vars: vec![],
        };
        let prog = MirProgram {
            functions: vec![f],
            entry_path: path,
        };
        let e = emit_llvm(&prog);
        assert_eq!(
            e.diagnostics.error_count(),
            0,
            "{:?}",
            e.diagnostics.items()
        );
        assert!(
            e.ir.contains("add i32") || e.ir.contains("add nsw i32"),
            "expected native i32 add; ir=\n{}",
            e.ir
        );
        assert!(
            e.ir.contains("sext i32") || e.ir.contains("trunc i64"),
            "expected i32↔i64 edge; ir=\n{}",
            e.ir
        );
    }

    #[test]
    fn bytes_lit_emits_runtime_bytes_from_ptr() {
        let path = PathBuf::from("/t.echo");
        let prog = MirProgram {
            functions: vec![mir_fn(
                &path,
                "__toplevel",
                &[],
                vec![
                    MirStmt::Set {
                        name: "b".into(),
                        value: MirExpr::BytesLit {
                            bytes: b"raw".to_vec(),
                        },
                        span: None,
                    },
                    MirStmt::ReturnOk(MirExpr::Name("b".into()), None),
                ],
            )],
            entry_path: path,
        };
        let e = emit_llvm(&prog);
        assert_eq!(
            e.diagnostics.error_count(),
            0,
            "{:?}",
            e.diagnostics.items()
        );
        assert!(
            e.ir.contains("echo_runtime_bytes_from_ptr"),
            "expected bytes lit runtime call; ir=\n{}",
            e.ir
        );
    }

    #[test]
    fn f32_width_emits_native_f32_add() {
        // Non-constant operands so LLVM does not fold away the fadd.
        let path = PathBuf::from("/t.echo");
        let params = vec!["a".into(), "b".into()];
        let stmts = vec![
            MirStmt::Set {
                name: "c".into(),
                value: MirExpr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(MirExpr::UnboxValue {
                        value: Box::new(MirExpr::Name("a".into())),
                        to: MirRepr::Float32,
                    }),
                    right: Box::new(MirExpr::UnboxValue {
                        value: Box::new(MirExpr::Name("b".into())),
                        to: MirRepr::Float32,
                    }),
                },
                span: None,
            },
            MirStmt::ReturnOk(
                MirExpr::BoxValue {
                    value: Box::new(MirExpr::Name("c".into())),
                    from: MirRepr::Float32,
                },
                None,
            ),
        ];
        let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
        let cfg = construct_ssa(cfg, &params);
        let (cfg, reprs) = analyze_reprs(cfg, &params);
        let f = MirFn {
            module_path: path.clone(),
            name: "f".into(),
            params: params.clone(),
            body: stmts,
            cfg,
            reprs,
            escapes: Default::default(),
            ret: MirRetShape::Plain,
            debug_vars: vec![],
        };
        let prog = MirProgram {
            functions: vec![f],
            entry_path: path,
        };
        let e = emit_llvm(&prog);
        assert_eq!(
            e.diagnostics.error_count(),
            0,
            "{:?}",
            e.diagnostics.items()
        );
        assert!(
            e.ir.contains("fadd float") || e.ir.contains("fadd nnan float"),
            "expected native f32 add; ir=\n{}",
            e.ir
        );
        assert!(
            e.ir.contains("fpext float") || e.ir.contains("fptrunc double"),
            "expected f32↔f64 edge; ir=\n{}",
            e.ir
        );
    }

    #[test]
    fn list_index_set_emits_runtime_list_set() {
        let path = PathBuf::from("/t.echo");
        let prog = MirProgram {
            functions: vec![mir_fn(
                &path,
                "__toplevel",
                &[],
                vec![
                    MirStmt::Set {
                        name: "xs".into(),
                        value: MirExpr::ListLit(vec![MirExpr::ConstI64(1), MirExpr::ConstI64(2)]),
                        span: None,
                    },
                    MirStmt::IndexSet {
                        base: MirExpr::Name("xs".into()),
                        index: MirExpr::ConstI64(0),
                        value: MirExpr::ConstI64(9),
                    },
                    MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
                ],
            )],
            entry_path: path,
        };
        let ir = emit_llvm(&prog).ir;
        assert!(
            ir.contains("echo_runtime_list_set"),
            "expected list_set; ir=\n{ir}"
        );
    }

    #[test]
    fn list_push_emits_runtime_list_push() {
        let path = PathBuf::from("/t.echo");
        let prog = MirProgram {
            functions: vec![mir_fn(
                &path,
                "__toplevel",
                &[],
                vec![
                    MirStmt::Set {
                        name: "xs".into(),
                        value: MirExpr::ListLit(vec![]),
                        span: None,
                    },
                    MirStmt::ListPush {
                        base: MirExpr::Name("xs".into()),
                        value: MirExpr::ConstI64(1),
                    },
                    MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
                ],
            )],
            entry_path: path,
        };
        let ir = emit_llvm(&prog).ir;
        assert!(
            ir.contains("echo_runtime_list_push"),
            "expected list_push; ir=\n{ir}"
        );
    }

    #[test]
    fn small_opt_bench_records_metrics_on_full_pipeline() {
        let prog = prog_with_call();
        let t0 = Instant::now();
        let o0 = emit_llvm_with(&prog, OptLevel::O0);
        let d0 = t0.elapsed();
        let t2 = Instant::now();
        let o2 = emit_llvm_with(&prog, OptLevel::O2);
        let d2 = t2.elapsed();
        assert_eq!(o0.diagnostics.error_count(), 0);
        assert_eq!(o2.diagnostics.error_count(), 0);
        let m0 = measure_ir(&o0.ir);
        let m2 = measure_ir(&o2.ir);
        assert!(m0.ir_bytes > 0 && m2.ir_bytes > 0);
        eprintln!(
            "bench opt (full MIR): O0 inst={} calls={} rt={} bb={} bytes={} time={:?} | O2 inst={} calls={} rt={} bb={} bytes={} time={:?}",
            m0.instruction_lines,
            m0.call_count,
            m0.runtime_call_count,
            m0.basic_block_count,
            m0.ir_bytes,
            d0,
            m2.instruction_lines,
            m2.call_count,
            m2.runtime_call_count,
            m2.basic_block_count,
            m2.ir_bytes,
            d2
        );
    }
}
