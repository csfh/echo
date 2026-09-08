//! Process args, environment, exit, and spawn+wait (`std/process`).

use std::process::Command;
use std::sync::Mutex;

use crate::{
    echo_runtime_list_new, echo_runtime_list_push, string_data, string_to_handle, EchoList,
    HEAP_MAGIC, KIND_LIST,
};

/// In-process host override for `echo_runtime_process_args` (JIT / tests).
/// `None` → `std::env::args()` (AOT child, default).
static PROCESS_ARGS_OVERRIDE: Mutex<Option<Vec<String>>> = Mutex::new(None);

fn args_override_lock() -> std::sync::MutexGuard<'static, Option<Vec<String>>> {
    PROCESS_ARGS_OVERRIDE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Replace process argv for this process until [`echo_runtime_process_clear_args`].
pub fn echo_runtime_process_set_args(args: Vec<String>) {
    *args_override_lock() = Some(args);
}

/// Restore `echo_runtime_process_args` to `std::env::args()`.
pub fn echo_runtime_process_clear_args() {
    *args_override_lock() = None;
}

/// Sets in-process argv; restores env argv on drop.
pub struct ProcessArgsOverride;

impl ProcessArgsOverride {
    pub fn apply(args: Vec<String>) -> Self {
        echo_runtime_process_set_args(args);
        Self
    }
}

impl Drop for ProcessArgsOverride {
    fn drop(&mut self) {
        echo_runtime_process_clear_args();
    }
}

/// Current process arguments as a list of string handles (`argv[0]` is the program).
///
/// In-process JIT hosts install argv via [`ProcessArgsOverride`] so user args
/// are not the host CLI (`xo run --jit …`).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_args() -> i64 {
    let list = echo_runtime_list_new();
    let override_args = args_override_lock().clone();
    if let Some(args) = override_args {
        for a in args {
            let h = string_to_handle(a);
            unsafe {
                echo_runtime_list_push(list, h);
            }
        }
    } else {
        for a in std::env::args() {
            let h = string_to_handle(a);
            unsafe {
                echo_runtime_list_push(list, h);
            }
        }
    }
    list
}

/// 1 if environment variable `name` is set, else 0. Non-string name → 0.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_env_has(name: i64) -> i64 {
    let Some(key) = string_data(name) else {
        return 0;
    };
    if std::env::var_os(&key).is_some() {
        1
    } else {
        0
    }
}

/// Value of environment variable `name`, or empty string if unset / bad name.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_env_get(name: i64) -> i64 {
    let Some(key) = string_data(name) else {
        return string_to_handle(String::new());
    };
    match std::env::var(&key) {
        Ok(v) => string_to_handle(v),
        Err(_) => string_to_handle(String::new()),
    }
}

/// Set environment variable. No-op if name/value are not strings.
///
/// # Safety
/// Uses `std::env::set_var` (not thread-safe vs concurrent env reads in other threads).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_env_set(name: i64, value: i64) {
    let Some(key) = string_data(name) else {
        return;
    };
    let Some(val) = string_data(value) else {
        return;
    };
    // SAFETY: Echo single-threaded runtime default; std documents this as unsafe in 2024.
    unsafe {
        std::env::set_var(key, val);
    }
}

/// Unset environment variable. No-op if name is not a string.
///
/// # Safety
/// Uses `std::env::remove_var` (not thread-safe vs concurrent env reads).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_env_unset(name: i64) {
    let Some(key) = string_data(name) else {
        return;
    };
    unsafe {
        std::env::remove_var(key);
    }
}

/// Terminate the current process with `code` (truncated to process exit status).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_exit(code: i64) {
    let status = if code < 0 {
        255
    } else if code > 255 {
        (code & 0xff) as i32
    } else {
        code as i32
    };
    std::process::exit(status);
}

/// Spawn `program` with string args from `args` list, wait, return exit status.
///
/// Returns:
/// - `0..` — process exit code (or 128+signal convention is left as OS-reported wait code bits)
/// - `-1` — spawn failed or bad handles
///
/// `args` is a list of string handles (may be empty). Does not include the program name unless
/// the caller puts it there; `program` is the executable path/name only.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_process_run(program: i64, args: i64) -> i64 {
    let Some(prog) = string_data(program) else {
        return -1;
    };
    if prog.is_empty() {
        return -1;
    }
    let mut cmd = Command::new(&prog);
    if args != 0 {
        if let Some(list) = list_string_args(args) {
            for a in list {
                cmd.arg(a);
            }
        } else {
            return -1;
        }
    }
    match cmd.status() {
        Ok(st) => st.code().unwrap_or(1) as i64,
        Err(_) => -1,
    }
}

fn list_string_args(list: i64) -> Option<Vec<String>> {
    let h = unsafe { crate::header_at(list)? };
    if unsafe { (*h).magic } != HEAP_MAGIC || unsafe { (*h).kind } != KIND_LIST {
        return None;
    }
    let lst = unsafe { &*(list as *const EchoList) };
    let mut out = Vec::with_capacity(lst.elems.len());
    for &el in &lst.elems {
        out.push(string_data(el)?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::echo_runtime_string_from_utf8;

    fn s(text: &str) -> i64 {
        unsafe { echo_runtime_string_from_utf8(text.as_ptr(), text.len()) }
    }

    #[test]
    fn args_nonempty() {
        let list = echo_runtime_process_args();
        let n = unsafe { crate::echo_runtime_list_len(list) };
        assert!(n >= 1, "expected at least argv0, got {n}");
        let a0 = unsafe { crate::echo_runtime_list_get(list, 0) };
        let text = string_data(a0).expect("argv0 string");
        assert!(!text.is_empty());
    }

    #[test]
    fn args_override_replaces_env_argv() {
        let _g = ProcessArgsOverride::apply(vec![
            "prog.echo".into(),
            "--verbose".into(),
            "in.echo".into(),
        ]);
        let list = echo_runtime_process_args();
        let n = unsafe { crate::echo_runtime_list_len(list) };
        assert_eq!(n, 3, "override argv length, got {n}");
        let a0 = unsafe { crate::echo_runtime_list_get(list, 0) };
        let a1 = unsafe { crate::echo_runtime_list_get(list, 1) };
        let a2 = unsafe { crate::echo_runtime_list_get(list, 2) };
        assert_eq!(string_data(a0).as_deref(), Some("prog.echo"));
        assert_eq!(string_data(a1).as_deref(), Some("--verbose"));
        assert_eq!(string_data(a2).as_deref(), Some("in.echo"));
    }

    #[test]
    fn args_override_drop_restores_env_argv() {
        {
            let _g = ProcessArgsOverride::apply(vec!["only-override".into()]);
            let list = echo_runtime_process_args();
            let a0 = unsafe { crate::echo_runtime_list_get(list, 0) };
            assert_eq!(string_data(a0).as_deref(), Some("only-override"));
        }
        let list = echo_runtime_process_args();
        let a0 = unsafe { crate::echo_runtime_list_get(list, 0) };
        let text = string_data(a0).expect("argv0 after clear");
        assert_ne!(text, "only-override");
        assert!(!text.is_empty());
    }

    #[test]
    fn env_roundtrip() {
        let key = s("ECHO_PROCESS_TEST_KEY");
        echo_runtime_process_env_unset(key);
        assert_eq!(echo_runtime_process_env_has(key), 0);
        let empty = echo_runtime_process_env_get(key);
        assert_eq!(string_data(empty).as_deref(), Some(""));

        echo_runtime_process_env_set(key, s("hello"));
        assert_eq!(echo_runtime_process_env_has(key), 1);
        let v = echo_runtime_process_env_get(key);
        assert_eq!(string_data(v).as_deref(), Some("hello"));

        echo_runtime_process_env_unset(key);
        assert_eq!(echo_runtime_process_env_has(key), 0);
    }

    #[test]
    fn run_true_or_echo() {
        // Portable enough: `true` on Unix; on Windows try `cmd /C exit 0` via run of cmd.
        #[cfg(unix)]
        {
            let code = echo_runtime_process_run(s("true"), echo_runtime_list_new());
            assert_eq!(code, 0, "true should exit 0, got {code}");
            let args = echo_runtime_list_new();
            unsafe {
                echo_runtime_list_push(args, s("not-a-real-command-xyz-echo-test"));
            }
            // false exits 1
            let code = echo_runtime_process_run(s("false"), echo_runtime_list_new());
            assert_eq!(code, 1);
        }
        #[cfg(windows)]
        {
            let args = echo_runtime_list_new();
            unsafe {
                echo_runtime_list_push(args, s("/C"));
                echo_runtime_list_push(args, s("exit 0"));
            }
            let code = echo_runtime_process_run(s("cmd"), args);
            assert_eq!(code, 0, "cmd /C exit 0, got {code}");
        }
    }

    #[test]
    fn run_missing_program() {
        let code = echo_runtime_process_run(s(""), echo_runtime_list_new());
        assert_eq!(code, -1);
        let code = echo_runtime_process_run(
            s("__echo_definitely_missing_binary_xyz__"),
            echo_runtime_list_new(),
        );
        assert_eq!(code, -1);
    }
}
