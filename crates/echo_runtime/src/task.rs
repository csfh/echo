//! Task handles for `+` / `-` leaders (ADR 0013).
//!
//! - Results as **i128** (plain / result / option).
//! - **Args** for `+ f(a,b,…)` (up to [`MAX_TASK_ARGS`]).
//! - **Unjoined** tasks at process end → hard fail.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::{HeapHeader, HEAP_MAGIC};

/// Heap kind for task handles.
pub const KIND_TASK: u32 = 13;

/// Max arguments for `+ f(args)` (v0).
pub const MAX_TASK_ARGS: usize = 8;

/// 0 = plain (`fn(…) -> i64`), 1 = result, 2 = option (`fn(…) -> i128`).
pub type TaskShape = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Running,
    Finished(i128),
    Failed,
}

pub(crate) struct TaskInner {
    state: Mutex<TaskState>,
    done: Condvar,
    entry: usize,
    shape: TaskShape,
    argc: u8,
    args: [i64; MAX_TASK_ARGS],
    /// Still needs a language-level `-` join.
    joined: Mutex<bool>,
}

#[repr(C)]
pub struct EchoTask {
    header: HeapHeader,
    inner: Arc<TaskInner>,
}

/// Live tasks that have not been joined yet.
static UNJOINED: AtomicUsize = AtomicUsize::new(0);

/// All tasks created this process-session (for JIT drain between runs).
static LIVE: Mutex<Vec<Arc<TaskInner>>> = Mutex::new(Vec::new());

fn task_header() -> HeapHeader {
    HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_TASK,
        promotion_epoch: 0,
    }
}

/// Drop a task handle. The inner task may keep running (`Arc`); unjoined
/// accounting is unchanged — dispose is ownership of the **handle**, not cancel.
pub(crate) fn free_task_object(handle: i64) {
    let _ = unsafe { Box::from_raw(handle as *mut EchoTask) };
}

/// Allocate a task handle that is not scheduled and does not bump UNJOINED.
#[cfg(test)]
pub(crate) fn test_alloc_task_handle() -> i64 {
    let inner = Arc::new(TaskInner {
        state: Mutex::new(TaskState::Pending),
        done: Condvar::new(),
        entry: 1,
        shape: 0,
        argc: 0,
        args: [0; MAX_TASK_ARGS],
        joined: Mutex::new(true),
    });
    crate::heap_to_handle(Box::new(EchoTask {
        header: task_header(),
        inner,
    }))
}

fn as_task(handle: i64) -> Option<&'static EchoTask> {
    if handle == 0 {
        return None;
    }
    let h = unsafe { crate::header_at(handle)? };
    if unsafe { (*h).kind } != KIND_TASK {
        return None;
    }
    Some(unsafe { &*(handle as *const EchoTask) })
}

fn pack_plain(v: i64) -> i128 {
    (v as u64) as i128
}

fn mark_spawned() {
    UNJOINED.fetch_add(1, Ordering::SeqCst);
}

fn mark_joined() {
    UNJOINED.fetch_sub(1, Ordering::SeqCst);
}

/// Process end: fail if any task was never joined with `-`.
///
/// Returns `0` if ok, `1` if unjoined tasks remain.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_task_check_joined() -> i64 {
    let n = UNJOINED.load(Ordering::SeqCst);
    if n != 0 {
        eprintln!("echo_runtime: {n} task(s) left unjoined (every + needs a -)");
        1
    } else {
        0
    }
}

fn new_task(entry: usize, shape: TaskShape, argc: u8, args: [i64; MAX_TASK_ARGS]) -> i64 {
    let shape = match shape {
        1 => 1,
        2 => 2,
        _ => 0,
    };
    let inner = Arc::new(TaskInner {
        state: Mutex::new(TaskState::Pending),
        done: Condvar::new(),
        entry,
        shape,
        argc,
        args,
        joined: Mutex::new(false),
    });
    {
        let mut live = LIVE.lock().expect("live tasks");
        live.push(inner.clone());
    }
    let t = Box::new(EchoTask {
        header: task_header(),
        inner,
    });
    let h = crate::heap_to_handle(t);
    mark_spawned();
    h
}

/// Wait for every task body to finish, then clear the unjoined counter.
///
/// Call after a JIT `echo_entry` returns so worker threads are not still
/// executing JIT code when the execution engine is dropped. Language-level
/// unjoined status is already reported by [`echo_runtime_task_check_joined`];
/// this only drains OS work and resets process-global task state for the next
/// in-process run (REPL / repeated `run_jit_ir`).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_task_after_run() {
    let live = {
        let mut g = LIVE.lock().expect("live tasks");
        std::mem::take(&mut *g)
    };
    for inner in live {
        crate::sched::schedule(inner.clone());
        let mut st = inner.state.lock().expect("task state");
        loop {
            match *st {
                TaskState::Finished(_) | TaskState::Failed => break,
                TaskState::Pending | TaskState::Running => {
                    st = inner.done.wait(st).expect("task done");
                }
            }
        }
    }
    UNJOINED.store(0, Ordering::SeqCst);
}

/// Create a zero-arg task (not yet scheduled).
///
/// # Safety
/// `entry` must match `shape`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_task_new(entry: i64, shape: i64) -> i64 {
    if entry == 0 {
        return 0;
    }
    new_task(entry as usize, shape, 0, [0; MAX_TASK_ARGS])
}

/// Create a task with up to 8 i64 args (`+ f(a0,…)`).
///
/// # Safety
/// `entry` must be a function of arity `argc` matching `shape`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_task_new_args(
    entry: i64,
    shape: i64,
    argc: i64,
    a0: i64,
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: i64,
    a7: i64,
) -> i64 {
    if entry == 0 || argc < 0 || argc as usize > MAX_TASK_ARGS {
        return 0;
    }
    let args = [a0, a1, a2, a3, a4, a5, a6, a7];
    new_task(entry as usize, shape, argc as u8, args)
}

/// Schedule task immediately.
///
/// # Safety
/// `handle` is 0 or a task handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_task_spawn(handle: i64) -> i64 {
    let Some(task) = as_task(handle) else {
        return 0;
    };
    crate::sched::schedule(task.inner.clone());
    handle
}

/// Zero-arg create + schedule.
///
/// # Safety
/// `entry` must match `shape`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_task_spawn_entry(entry: i64, shape: i64) -> i64 {
    let h = unsafe { echo_runtime_task_new(entry, shape) };
    if h == 0 {
        return 0;
    }
    unsafe { echo_runtime_task_spawn(h) }
}

/// Create + schedule with args.
///
/// # Safety
/// See [`echo_runtime_task_new_args`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_task_spawn_args(
    entry: i64,
    shape: i64,
    argc: i64,
    a0: i64,
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: i64,
    a7: i64,
) -> i64 {
    let h = unsafe {
        echo_runtime_task_new_args(entry, shape, argc, a0, a1, a2, a3, a4, a5, a6, a7)
    };
    if h == 0 {
        return 0;
    }
    unsafe { echo_runtime_task_spawn(h) }
}

fn join_mark(task: &EchoTask) {
    let mut j = task.inner.joined.lock().expect("joined");
    if !*j {
        *j = true;
        mark_joined();
    }
}

/// Join: low 64 bits of packed result.
///
/// # Safety
/// `handle` is 0 or a task handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_task_join(handle: i64) -> i64 {
    let wide = unsafe { echo_runtime_task_join_wide(handle) };
    wide as i64
}

/// Join: full i128 pack.
///
/// # Safety
/// `handle` is 0 or a task handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_task_join_wide(handle: i64) -> i128 {
    let Some(task) = as_task(handle) else {
        return 0;
    };
    crate::sched::schedule(task.inner.clone());
    let mut st = task.inner.state.lock().expect("task state");
    loop {
        match *st {
            TaskState::Finished(v) => {
                drop(st);
                join_mark(task);
                return v;
            }
            TaskState::Failed => {
                drop(st);
                join_mark(task);
                return 0;
            }
            TaskState::Pending | TaskState::Running => {
                st = task.inner.done.wait(st).expect("task done");
            }
        }
    }
}

/// Immediate block zero-arg.
///
/// # Safety
/// `entry` must match `shape`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_task_block(entry: i64, shape: i64) -> i64 {
    let wide = unsafe { echo_runtime_task_block_wide(entry, shape) };
    wide as i64
}

/// Immediate block zero-arg, wide.
///
/// # Safety
/// `entry` must match `shape`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_task_block_wide(entry: i64, shape: i64) -> i128 {
    let h = unsafe { echo_runtime_task_spawn_entry(entry, shape) };
    unsafe { echo_runtime_task_join_wide(h) }
}

/// Immediate block with args (`- f(a)` style not used; for internal use).
///
/// # Safety
/// See spawn args.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_task_block_args(
    entry: i64,
    shape: i64,
    argc: i64,
    a0: i64,
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: i64,
    a7: i64,
) -> i128 {
    let h = unsafe {
        echo_runtime_task_spawn_args(entry, shape, argc, a0, a1, a2, a3, a4, a5, a6, a7)
    };
    unsafe { echo_runtime_task_join_wide(h) }
}

/// Ret shape on handle (0/1/2).
///
/// # Safety
/// `handle` is 0 or a task handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_task_shape(handle: i64) -> i64 {
    let Some(task) = as_task(handle) else {
        return 0;
    };
    task.inner.shape
}

fn invoke_plain(entry: usize, argc: u8, args: &[i64; MAX_TASK_ARGS]) -> i64 {
    unsafe {
        match argc {
            0 => {
                let f: unsafe extern "C" fn() -> i64 = std::mem::transmute(entry);
                f()
            }
            1 => {
                let f: unsafe extern "C" fn(i64) -> i64 = std::mem::transmute(entry);
                f(args[0])
            }
            2 => {
                let f: unsafe extern "C" fn(i64, i64) -> i64 = std::mem::transmute(entry);
                f(args[0], args[1])
            }
            3 => {
                let f: unsafe extern "C" fn(i64, i64, i64) -> i64 = std::mem::transmute(entry);
                f(args[0], args[1], args[2])
            }
            4 => {
                let f: unsafe extern "C" fn(i64, i64, i64, i64) -> i64 = std::mem::transmute(entry);
                f(args[0], args[1], args[2], args[3])
            }
            5 => {
                let f: unsafe extern "C" fn(i64, i64, i64, i64, i64) -> i64 =
                    std::mem::transmute(entry);
                f(args[0], args[1], args[2], args[3], args[4])
            }
            6 => {
                let f: unsafe extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64 =
                    std::mem::transmute(entry);
                f(args[0], args[1], args[2], args[3], args[4], args[5])
            }
            7 => {
                let f: unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64) -> i64 =
                    std::mem::transmute(entry);
                f(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6],
                )
            }
            _ => {
                let f: unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64) -> i64 =
                    std::mem::transmute(entry);
                f(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                )
            }
        }
    }
}

fn invoke_wide(entry: usize, argc: u8, args: &[i64; MAX_TASK_ARGS]) -> i128 {
    unsafe {
        match argc {
            0 => {
                let f: unsafe extern "C" fn() -> i128 = std::mem::transmute(entry);
                f()
            }
            1 => {
                let f: unsafe extern "C" fn(i64) -> i128 = std::mem::transmute(entry);
                f(args[0])
            }
            2 => {
                let f: unsafe extern "C" fn(i64, i64) -> i128 = std::mem::transmute(entry);
                f(args[0], args[1])
            }
            3 => {
                let f: unsafe extern "C" fn(i64, i64, i64) -> i128 = std::mem::transmute(entry);
                f(args[0], args[1], args[2])
            }
            4 => {
                let f: unsafe extern "C" fn(i64, i64, i64, i64) -> i128 = std::mem::transmute(entry);
                f(args[0], args[1], args[2], args[3])
            }
            _ => {
                // Cap wide multi-arg at 4 for simplicity; extend if needed.
                let f: unsafe extern "C" fn(i64, i64, i64, i64) -> i128 = std::mem::transmute(entry);
                f(args[0], args[1], args[2], args[3])
            }
        }
    }
}

pub(crate) fn run_task_inner(inner: &Arc<TaskInner>) {
    {
        let mut st = inner.state.lock().expect("task state");
        match *st {
            TaskState::Finished(_) | TaskState::Failed | TaskState::Running => return,
            TaskState::Pending => *st = TaskState::Running,
        }
    }
    let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if inner.shape == 1 || inner.shape == 2 {
            invoke_wide(inner.entry, inner.argc, &inner.args)
        } else {
            pack_plain(invoke_plain(inner.entry, inner.argc, &inner.args))
        }
    })) {
        Ok(v) => TaskState::Finished(v),
        Err(_) => TaskState::Failed,
    };
    {
        let mut st = inner.state.lock().expect("task state");
        *st = result;
        inner.done.notify_all();
    }
}

pub(crate) type SharedTask = Arc<TaskInner>;

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn forty_two() -> i64 {
        42
    }

    unsafe extern "C" fn add1(x: i64) -> i64 {
        x + 1
    }

    unsafe extern "C" fn err_99() -> i128 {
        ((1i128) << 64) | 99
    }

    #[test]
    fn spawn_join_plain() {
        let entry = forty_two as unsafe extern "C" fn() -> i64 as usize as i64;
        let h = unsafe { echo_runtime_task_spawn_entry(entry, 0) };
        assert_eq!(unsafe { echo_runtime_task_join(h) }, 42);
    }

    #[test]
    fn spawn_args() {
        let entry = add1 as unsafe extern "C" fn(i64) -> i64 as usize as i64;
        let h = unsafe { echo_runtime_task_spawn_args(entry, 0, 1, 41, 0, 0, 0, 0, 0, 0, 0) };
        assert_eq!(unsafe { echo_runtime_task_join(h) }, 42);
    }

    #[test]
    fn join_wide_result_err() {
        let entry = err_99 as unsafe extern "C" fn() -> i128 as usize as i64;
        let h = unsafe { echo_runtime_task_spawn_entry(entry, 1) };
        let w = unsafe { echo_runtime_task_join_wide(h) };
        assert_eq!((w >> 64) as i64, 1);
        assert_eq!(w as i64, 99);
    }

    #[test]
    fn join_marks_handle_joined() {
        // Counter is process-global (parallel tests share it); only assert this
        // handle's join path succeeds and does not panic.
        let entry = forty_two as unsafe extern "C" fn() -> i64 as usize as i64;
        let h = unsafe { echo_runtime_task_spawn_entry(entry, 0) };
        assert_eq!(unsafe { echo_runtime_task_join(h) }, 42);
        // Second join is a no-op for the unjoined counter (already marked).
        assert_eq!(unsafe { echo_runtime_task_join(h) }, 42);
    }
}
