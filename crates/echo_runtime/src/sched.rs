//! Runtime event loop driven by **mio** (ADR 0013).
//!
//! - **Poller thread** owns `mio::Poll` and signals I/O waiters.
//! - **Worker pool** runs task bodies.
//! - Park protocol: try I/O → on WouldBlock arm interest → retry → then wait
//!   (closes edge-trigger races).
//!
//! **Unix:** interest is registered via `mio::unix::SourceFd` on OS file descriptors.
//! **Windows:** `SourceFd` is unavailable; park uses a short yield so nonblocking
//! net ops can retry. Full WSA/mio socket registration can replace this later.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use mio::{Events, Poll, Registry, Token, Waker};
#[cfg(unix)]
use mio::Interest;

use crate::task::{run_task_inner, SharedTask};

/// OS handle used for I/O parking (fd on Unix, socket on Windows).
#[cfg(unix)]
pub type IoRaw = std::os::fd::RawFd;
#[cfg(windows)]
pub type IoRaw = std::os::windows::io::RawSocket;
#[cfg(not(any(unix, windows)))]
pub type IoRaw = i64;

const TOKEN_WAKER: Token = Token(0);
const TOKEN_IO_BASE: usize = 1;

struct IoWaiter {
    ready: Mutex<bool>,
    cv: Condvar,
}

impl IoWaiter {
    fn new() -> Self {
        Self {
            ready: Mutex::new(false),
            cv: Condvar::new(),
        }
    }

    fn wait(&self) {
        let mut g = self.ready.lock().expect("io waiter");
        while !*g {
            g = self.cv.wait(g).expect("io waiter wait");
        }
        *g = false;
    }

    fn signal(&self) {
        let mut g = self.ready.lock().expect("io waiter");
        *g = true;
        self.cv.notify_one();
    }
}

struct LoopState {
    runnable: Mutex<VecDeque<SharedTask>>,
    work: Condvar,
    waker: Waker,
    registry: Registry,
    next_token: AtomicUsize,
    waiters: Mutex<HashMap<usize, Arc<IoWaiter>>>,
}

static LOOP: OnceLock<Arc<LoopState>> = OnceLock::new();
static WORKER_STARTED: AtomicBool = AtomicBool::new(false);

fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 8)
}

fn ensure_loop() -> &'static Arc<LoopState> {
    LOOP.get_or_init(|| {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Arc<LoopState>>();
        thread::Builder::new()
            .name("echo-mio-poller".into())
            .spawn(move || {
                let mut poll = match Poll::new() {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("echo_runtime: mio Poll::new failed: {e}");
                        return;
                    }
                };
                let waker = match Waker::new(poll.registry(), TOKEN_WAKER) {
                    Ok(w) => w,
                    Err(e) => {
                        eprintln!("echo_runtime: mio Waker::new failed: {e}");
                        return;
                    }
                };
                let registry = match poll.registry().try_clone() {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("echo_runtime: registry clone failed: {e}");
                        return;
                    }
                };
                let state = Arc::new(LoopState {
                    runnable: Mutex::new(VecDeque::new()),
                    work: Condvar::new(),
                    waker,
                    registry,
                    next_token: AtomicUsize::new(TOKEN_IO_BASE),
                    waiters: Mutex::new(HashMap::new()),
                });
                let _ = ready_tx.send(state.clone());
                WORKER_STARTED.store(true, Ordering::SeqCst);

                let n = worker_count();
                for i in 0..n {
                    let st = state.clone();
                    thread::Builder::new()
                        .name(format!("echo-task-worker-{i}"))
                        .spawn(move || worker_main(st))
                        .expect("spawn task worker");
                }

                poller_main(state, &mut poll);
            })
            .expect("spawn echo mio poller");

        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("echo event loop failed to start")
    })
}

fn worker_main(state: Arc<LoopState>) {
    loop {
        let task = {
            let mut q = state.runnable.lock().expect("runnable");
            while q.is_empty() {
                q = state.work.wait(q).expect("work wait");
            }
            q.pop_front().expect("non-empty")
        };
        run_task_inner(&task);
    }
}

fn poller_main(state: Arc<LoopState>, poll: &mut Poll) {
    let mut events = Events::with_capacity(1024);
    loop {
        if let Err(e) = poll.poll(&mut events, None) {
            if e.kind() != std::io::ErrorKind::Interrupted {
                eprintln!("echo_runtime: mio poll error: {e}");
            }
            continue;
        }
        for ev in events.iter() {
            if ev.token() == TOKEN_WAKER {
                state.work.notify_all();
                continue;
            }
            let id = ev.token().0;
            let waiter = {
                let mut map = state.waiters.lock().expect("waiters");
                map.remove(&id)
            };
            if let Some(w) = waiter {
                w.signal();
            }
        }
    }
}

/// Enqueue a task and wake workers immediately (`+` spawn).
pub fn schedule(task: SharedTask) {
    let state = ensure_loop();
    {
        let mut q = state.runnable.lock().expect("runnable");
        q.push_back(task);
    }
    if let Err(e) = state.waker.wake() {
        eprintln!("echo_runtime: mio waker.wake failed: {e}");
    }
    state.work.notify_one();
}

#[cfg(unix)]
fn interest_bits(readable: bool, writable: bool) -> Interest {
    match (readable, writable) {
        (true, true) => Interest::READABLE | Interest::WRITABLE,
        (true, false) => Interest::READABLE,
        (false, true) => Interest::WRITABLE,
        (false, false) => Interest::READABLE,
    }
}

/// Arm mio interest on `fd`, then block until the poller signals readiness.
///
/// Caller protocol (edge-safe):
/// 1. try I/O
/// 2. on WouldBlock call this
/// 3. retry I/O (this function arms, waits; you retry after return)
///
/// Internally: register → wait → deregister. Net ops should **retry once
/// after arm without wait** — use [`arm_fd`] + [`wait_fd`] for that.
#[allow(dead_code)] // available for simple park; net uses arm/wait/disarm
pub fn park_fd(fd: IoRaw, readable: bool, writable: bool) {
    let token_id = arm_fd(fd, readable, writable);
    wait_fd(token_id, fd);
}

/// Register interest; returns token id for [`wait_fd`].
pub fn arm_fd(fd: IoRaw, readable: bool, writable: bool) -> usize {
    let state = ensure_loop();
    let token_id = state.next_token.fetch_add(1, Ordering::Relaxed);
    let waiter = Arc::new(IoWaiter::new());
    {
        let mut map = state.waiters.lock().expect("waiters");
        map.insert(token_id, waiter);
    }
    arm_register(state, token_id, fd, readable, writable);
    token_id
}

#[cfg(unix)]
fn arm_register(state: &LoopState, token_id: usize, fd: IoRaw, readable: bool, writable: bool) {
    use mio::unix::SourceFd;
    let interest = interest_bits(readable, writable);
    let mut source = SourceFd(&fd);
    if let Err(e) = state
        .registry
        .register(&mut source, Token(token_id), interest)
    {
        let _ = state
            .registry
            .reregister(&mut source, Token(token_id), interest);
        let _ = e;
    }
}

#[cfg(windows)]
fn arm_register(_state: &LoopState, _token_id: usize, _fd: IoRaw, _readable: bool, _writable: bool) {
    // mio::unix::SourceFd is not available; waiter is still installed so wait_fd
    // can park briefly (see wait_fd_windows) while net retries.
}

#[cfg(not(any(unix, windows)))]
fn arm_register(_state: &LoopState, _token_id: usize, _fd: IoRaw, _readable: bool, _writable: bool) {}

/// Wait until token is signaled, then deregister `fd`.
pub fn wait_fd(token_id: usize, fd: IoRaw) {
    #[cfg(unix)]
    {
        let state = ensure_loop();
        let waiter = {
            let map = state.waiters.lock().expect("waiters");
            map.get(&token_id).cloned()
        };
        if let Some(w) = waiter {
            w.wait();
        }
        disarm_fd(token_id, fd);
    }
    #[cfg(windows)]
    {
        // No SourceFd registration: yield so the nonblocking retry loop can spin
        // without busy-burning a core. Edge-driven parking needs WSA/mio socket
        // registration (follow-up).
        let _ = fd;
        thread::sleep(Duration::from_millis(1));
        disarm_fd(token_id, fd);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (token_id, fd);
    }
}

/// Drop waiter + deregister without waiting (I/O succeeded after arm).
pub fn disarm_fd(token_id: usize, fd: IoRaw) {
    let state = ensure_loop();
    #[cfg(unix)]
    {
        use mio::unix::SourceFd;
        let mut source = SourceFd(&fd);
        let _ = state.registry.deregister(&mut source);
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
    }
    let mut map = state.waiters.lock().expect("waiters");
    map.remove(&token_id);
}

#[cfg(test)]
pub fn worker_started() -> bool {
    WORKER_STARTED.load(Ordering::SeqCst) || LOOP.get().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{echo_runtime_task_join, echo_runtime_task_spawn_entry};

    unsafe extern "C" fn seven() -> i64 {
        7
    }

    #[test]
    fn mio_schedule_runs_on_worker() {
        let entry = seven as unsafe extern "C" fn() -> i64 as usize as i64;
        let h = unsafe { echo_runtime_task_spawn_entry(entry, 0) };
        let v = unsafe { echo_runtime_task_join(h) };
        assert_eq!(v, 7);
        assert!(worker_started());
    }
}
