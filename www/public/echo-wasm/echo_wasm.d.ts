/* tslint:disable */
/* eslint-disable */

export function check(source: string): string;

export function format(source: string): string;

export function playgroundRun(source: string): string;

export function stdFileCount(): number;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly echo_runtime_float_from_f64: (a: number) => bigint;
    readonly echo_runtime_str_from_int: (a: bigint) => bigint;
    readonly echo_runtime_print_i64: (a: bigint) => void;
    readonly echo_runtime_str_from_debug: (a: bigint) => bigint;
    readonly echo_runtime_list_new: () => bigint;
    readonly echo_runtime_range_new: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_string_builder_new: () => bigint;
    readonly echo_runtime_string_builder_push_value: (a: bigint, b: bigint) => void;
    readonly echo_runtime_string_builder_finish: (a: bigint) => bigint;
    readonly echo_runtime_scope_enter: (a: bigint) => void;
    readonly echo_runtime_scope_exit: (a: bigint) => void;
    readonly echo_runtime_scope_register: (a: bigint) => void;
    readonly echo_runtime_scope_promote: (a: bigint, b: bigint) => void;
    readonly echo_runtime_scope_disown: (a: bigint) => void;
    readonly echo_runtime_scope_release: (a: bigint) => void;
    readonly check: (a: number, b: number) => [number, number];
    readonly format: (a: number, b: number) => [number, number];
    readonly playgroundRun: (a: number, b: number) => [number, number];
    readonly stdFileCount: () => number;
    readonly echo_runtime_struct_get: (a: bigint, b: number, c: number) => bigint;
    readonly echo_runtime_struct_new_named: (a: number, b: number) => bigint;
    readonly echo_runtime_struct_set: (a: bigint, b: number, c: number, d: bigint) => void;
    readonly echo_runtime_string_from_utf8: (a: number, b: number) => bigint;
    readonly echo_runtime_abort: (a: number, b: number) => void;
    readonly echo_runtime_bytes_cat: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_bytes_from_i64: (a: bigint) => bigint;
    readonly echo_runtime_bytes_from_ptr: (a: number, b: number) => bigint;
    readonly echo_runtime_bytes_from_str: (a: bigint) => bigint;
    readonly echo_runtime_bytes_get: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_bytes_len: (a: bigint) => bigint;
    readonly echo_runtime_bytes_slice: (a: bigint, b: bigint, c: bigint) => bigint;
    readonly echo_runtime_eq: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_eq_id: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_float_to_f64: (a: bigint) => number;
    readonly echo_runtime_fn_code: (a: bigint) => bigint;
    readonly echo_runtime_fn_new: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_fn_shape: (a: bigint) => bigint;
    readonly echo_runtime_http_headers_complete: (a: bigint) => bigint;
    readonly echo_runtime_http_parse_request: (a: bigint) => bigint;
    readonly echo_runtime_http_request_complete: (a: bigint) => bigint;
    readonly echo_runtime_list_new_empty_lists: (a: bigint) => bigint;
    readonly echo_runtime_list_reserve: (a: bigint, b: bigint) => void;
    readonly echo_runtime_locator_class: (a: bigint) => bigint;
    readonly echo_runtime_locator_from_utf8: (a: number, b: number) => bigint;
    readonly echo_runtime_ne: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_ne_id: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_now_ms: () => bigint;
    readonly echo_runtime_reflect_key_bytes: (a: bigint) => bigint;
    readonly echo_runtime_reflect_kind: (a: bigint) => bigint;
    readonly echo_runtime_reflect_kind_name: (a: bigint) => bigint;
    readonly echo_runtime_sleep_ms: (a: bigint) => void;
    readonly echo_runtime_str_cat: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_str_contains: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_str_ends_with: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_str_from_bytes: (a: bigint) => bigint;
    readonly echo_runtime_str_from_duration: (a: bigint) => bigint;
    readonly echo_runtime_str_from_float: (a: bigint) => bigint;
    readonly echo_runtime_str_from_locator: (a: bigint) => bigint;
    readonly echo_runtime_str_get: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_str_len: (a: bigint) => bigint;
    readonly echo_runtime_str_repeat: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_str_slice: (a: bigint, b: bigint, c: bigint) => bigint;
    readonly echo_runtime_str_starts_with: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_string_builder_push_str: (a: bigint, b: number, c: number) => void;
    readonly echo_runtime_struct_new: () => bigint;
    readonly echo_runtime_struct_type_is: (a: bigint, b: number, c: number) => bigint;
    readonly echo_runtime_list_get: (a: bigint, b: bigint) => bigint;
    readonly echo_runtime_list_len: (a: bigint) => bigint;
    readonly echo_runtime_list_push: (a: bigint, b: bigint) => void;
    readonly echo_runtime_list_set: (a: bigint, b: bigint, c: bigint) => void;
    readonly echo_runtime_scope_drain_deferred: () => void;
    readonly echo_runtime_scope_enqueue_release: (a: bigint) => void;
    readonly echo_runtime_scope_promote_graph: (a: bigint, b: bigint) => void;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
