//! Model A test suite: register cases during load, run after entry body.
//!
//! Enabled when the host sets `XO_TEST` in the process environment (AOT child
//! via `xo test`) or calls [`echo_runtime_test_enable`] (in-process JIT).
//! Registration is a no-op otherwise so co-located `test.it` / `test.bench`
//! calls do not run under ordinary `xo run`.
//!
//! Bench mode (`XO_BENCH` or [`echo_runtime_test_enable_bench`]) runs only
//! registered benchmarks with auto-N harness loops; plain suite mode runs only
//! `test.it` cases.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{echo_runtime_fn_code, echo_runtime_fn_shape, string_data, FN_SHAPE_PLAIN};

/// Serialize appends to bench JSONL output.
static BENCH_OUT_LOCK: Mutex<()> = Mutex::new(());

/// In-process override for hosts that cannot set env (e.g. `xo` with forbid(unsafe)).
static BENCH_OUT_CFG: Mutex<BenchOutCfg> = Mutex::new(BenchOutCfg {
    path: None,
    file: None,
    opt: None,
});

struct BenchOutCfg {
    path: Option<std::path::PathBuf>,
    file: Option<String>,
    /// LLVM opt token (`O0`…`Oz`) for JSONL provenance.
    opt: Option<String>,
}

/// Configure streaming JSONL output for this process (JIT / in-process hosts).
///
/// AOT children still use `XO_BENCH_OUT` / `XO_BENCH_FILE` / `XO_BENCH_OPT`.
pub fn echo_runtime_bench_configure(
    out: Option<&std::path::Path>,
    file: Option<&str>,
    opt: Option<&str>,
) {
    let mut g = BENCH_OUT_CFG.lock().unwrap_or_else(|e| e.into_inner());
    g.path = out.map(|p| p.to_path_buf());
    g.file = file.map(|s| s.to_string());
    g.opt = opt.map(|s| s.to_string());
}

/// Case-body timings: integer nanoseconds (suite cases are usually sub-ms).
fn fmt_ns(d: Duration) -> String {
    format!("{}ns", d.as_nanos())
}

/// Target wall time for one measured bench run (Go-style auto-N).
const BENCH_TARGET: Duration = Duration::from_secs(1);
/// Cap iterations so a zero-cost body cannot hang.
const BENCH_MAX_N: u64 = 1_000_000_000;

struct Case {
    name: String,
    /// Native code pointer bits (from function value).
    code: i64,
    shape: i64,
}

struct Suite {
    cases: Vec<Case>,
    benches: Vec<Case>,
    /// Failures in the case currently executing.
    case_failed: bool,
}

static SUITE: Mutex<Suite> = Mutex::new(Suite {
    cases: Vec::new(),
    benches: Vec::new(),
    case_failed: false,
});

static ENABLED: AtomicBool = AtomicBool::new(false);
static BENCH_MODE: AtomicBool = AtomicBool::new(false);

/// Turn on suite mode for this process (JIT / in-process hosts) — tests only.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_test_enable() {
    ENABLED.store(true, Ordering::SeqCst);
    BENCH_MODE.store(false, Ordering::SeqCst);
}

/// Turn on suite mode and run benchmarks only (JIT / in-process hosts).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_test_enable_bench() {
    ENABLED.store(true, Ordering::SeqCst);
    BENCH_MODE.store(true, Ordering::SeqCst);
}

fn suite_mode() -> bool {
    ENABLED.load(Ordering::SeqCst) || std::env::var_os("XO_TEST").is_some()
}

fn bench_mode() -> bool {
    BENCH_MODE.load(Ordering::SeqCst) || std::env::var_os("XO_BENCH").is_some()
}

fn push_case(list: &mut Vec<Case>, name: i64, body: i64, kind: &str) {
    let label = string_data(name).unwrap_or_else(|| "<unnamed>".into());
    let code = unsafe { echo_runtime_fn_code(body) };
    let shape = unsafe { echo_runtime_fn_shape(body) };
    if code == 0 {
        eprintln!("xo test: skip register {kind} {label:?}: not a function value");
        return;
    }
    list.push(Case {
        name: label,
        code,
        shape,
    });
}

/// Register a named zero-arg function value as a test case.
///
/// No-op unless suite mode is on.
///
/// # Safety
/// `name` must be 0 or a string handle; `body` must be 0 or a function-value handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_test_register(name: i64, body: i64) {
    if !suite_mode() {
        return;
    }
    let mut g = SUITE.lock().expect("test suite lock");
    // SAFETY: caller contract — name/body are runtime handles or 0.
    push_case(&mut g.cases, name, body, "test");
}

/// Register a named zero-arg function value as a benchmark.
///
/// The harness invokes the body N times (auto-scaled). No-op unless suite mode
/// is on. Cases only run when bench mode is also on (`XO_BENCH` / enable_bench).
///
/// # Safety
/// `name` must be 0 or a string handle; `body` must be 0 or a function-value handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_test_bench_register(name: i64, body: i64) {
    if !suite_mode() {
        return;
    }
    let mut g = SUITE.lock().expect("test suite lock");
    // SAFETY: caller contract — name/body are runtime handles or 0.
    push_case(&mut g.benches, name, body, "bench");
}

/// Record a failure in the current case and print `msg` (string handle).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_test_fail(msg: i64) {
    if !suite_mode() {
        return;
    }
    {
        let mut g = SUITE.lock().expect("test suite lock");
        g.case_failed = true;
    }
    if let Some(s) = string_data(msg) {
        eprintln!("  fail: {s}");
    } else {
        eprintln!("  fail");
    }
}

/// Run registered tests or benches. Returns failure count, or `-1` when suite
/// mode is off.
///
/// - Bench mode: run only benchmarks (auto-N, ns/op).
/// - Otherwise: run only `test.it` cases.
///
/// When suite mode is on and zero matching cases registered, prints a notice
/// and returns 0.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_test_finish() -> i64 {
    if !suite_mode() {
        return -1;
    }

    let (cases, benches) = {
        let mut g = SUITE.lock().expect("test suite lock");
        (std::mem::take(&mut g.cases), std::mem::take(&mut g.benches))
    };

    if bench_mode() {
        return run_benches(benches);
    }
    run_tests(cases)
}

fn run_tests(cases: Vec<Case>) -> i64 {
    if cases.is_empty() {
        eprintln!("xo test: 0 cases registered");
        return 0;
    }

    let mut failed = 0i64;
    let total = cases.len();
    let suite_start = Instant::now();
    for case in &cases {
        {
            let mut g = SUITE.lock().expect("test suite lock");
            g.case_failed = false;
        }
        let case_start = Instant::now();
        // SAFETY: code bits come from fn values created by this process's LLVM.
        let call_ok = unsafe { invoke_case(case.code, case.shape) };
        let case_failed = SUITE.lock().expect("test suite lock").case_failed;
        let elapsed = fmt_ns(case_start.elapsed());
        if !call_ok || case_failed {
            failed += 1;
            eprintln!("FAIL  {} ({elapsed})", case.name);
        } else {
            eprintln!("ok    {} ({elapsed})", case.name);
        }
    }

    let passed = total as i64 - failed;
    let suite_elapsed = fmt_ns(suite_start.elapsed());
    eprintln!("xo test: {passed} passed, {failed} failed, {total} total ({suite_elapsed})");
    failed
}

fn run_benches(benches: Vec<Case>) -> i64 {
    if benches.is_empty() {
        eprintln!("xo test --bench: 0 benchmarks registered");
        return 0;
    }

    let mut failed = 0i64;
    let total = benches.len();
    let suite_start = Instant::now();
    for case in &benches {
        match run_one_bench(case) {
            Ok((n, ns_per_op, total_d)) => {
                eprintln!(
                    "bench {}  N={n}  {}ns/op  ({})",
                    case.name,
                    ns_per_op,
                    fmt_ns(total_d)
                );
                append_bench_jsonl(Ok(BenchRecord {
                    name: &case.name,
                    n,
                    ns_per_op,
                    total_ns: total_d.as_nanos(),
                }));
            }
            Err(()) => {
                failed += 1;
                eprintln!("FAIL  bench {}", case.name);
                append_bench_jsonl(Err(&case.name));
            }
        }
    }

    let passed = total as i64 - failed;
    let suite_elapsed = fmt_ns(suite_start.elapsed());
    eprintln!(
        "xo test --bench: {passed} passed, {failed} failed, {total} total ({suite_elapsed})"
    );
    failed
}

struct BenchRecord<'a> {
    name: &'a str,
    n: u64,
    ns_per_op: u128,
    total_ns: u128,
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Append one JSONL record when output is configured (env or [`echo_runtime_bench_configure`]).
///
/// Optional file label from `XO_BENCH_FILE` / configure labels the suite entry.
fn append_bench_jsonl(result: Result<BenchRecord<'_>, &str>) {
    let (path, file, opt) = {
        let g = BENCH_OUT_CFG.lock().unwrap_or_else(|e| e.into_inner());
        let path = g
            .path
            .clone()
            .or_else(|| std::env::var_os("XO_BENCH_OUT").map(std::path::PathBuf::from));
        let file = g
            .file
            .clone()
            .or_else(|| std::env::var("XO_BENCH_FILE").ok())
            .unwrap_or_default();
        let opt = g
            .opt
            .clone()
            .or_else(|| std::env::var("XO_BENCH_OPT").ok())
            .unwrap_or_else(|| "O0".into());
        (path, file, opt)
    };
    let Some(path) = path else {
        return;
    };
    let line = match result {
        Ok(r) => format!(
            "{{\"v\":1,\"file\":\"{}\",\"name\":\"{}\",\"opt\":\"{}\",\"status\":\"ok\",\"n\":{},\"ns_per_op\":{},\"total_ns\":{}}}\n",
            json_escape(&file),
            json_escape(r.name),
            json_escape(&opt),
            r.n,
            r.ns_per_op,
            r.total_ns
        ),
        Err(name) => format!(
            "{{\"v\":1,\"file\":\"{}\",\"name\":\"{}\",\"opt\":\"{}\",\"status\":\"fail\"}}\n",
            json_escape(&file),
            json_escape(name),
            json_escape(&opt)
        ),
    };
    let _guard = BENCH_OUT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut f) => {
            if let Err(e) = f.write_all(line.as_bytes()) {
                eprintln!("xo test --bench: cannot write {}: {e}", path.to_string_lossy());
            }
        }
        Err(e) => {
            eprintln!("xo test --bench: cannot open {}: {e}", path.to_string_lossy());
        }
    }
}

/// Auto-scale N until one measured run lasts about [`BENCH_TARGET`] (or hit
/// [`BENCH_MAX_N`]). Returns `(N, ns/op, measured_duration)`.
fn run_one_bench(case: &Case) -> Result<(u64, u128, Duration), ()> {
    let mut n: u64 = 1;
    loop {
        {
            let mut g = SUITE.lock().expect("test suite lock");
            g.case_failed = false;
        }
        let start = Instant::now();
        for _ in 0..n {
            // SAFETY: code bits from fn values created by this process's LLVM.
            let call_ok = unsafe { invoke_case(case.code, case.shape) };
            if !call_ok {
                return Err(());
            }
            if SUITE.lock().expect("test suite lock").case_failed {
                return Err(());
            }
        }
        let d = start.elapsed();
        if d >= BENCH_TARGET || n >= BENCH_MAX_N {
            let ns_per_op = if n == 0 {
                0
            } else {
                d.as_nanos() / u128::from(n)
            };
            return Ok((n, ns_per_op, d));
        }
        // Grow N toward the target wall time; avoid stalling on zero-cost work.
        let next = if d.as_nanos() == 0 {
            n.saturating_mul(100).max(n + 1)
        } else {
            let target_ns = BENCH_TARGET.as_nanos();
            let scaled = (u128::from(n) * target_ns) / d.as_nanos();
            let scaled = scaled.max(u128::from(n) + 1);
            u64::try_from(scaled).unwrap_or(BENCH_MAX_N)
        };
        n = next.min(BENCH_MAX_N).max(n + 1);
    }
}

/// Invoke a zero-arg function value. Returns false if the pointer was null.
///
/// # Safety
/// `code` must be a valid native function pointer for this process (or 0).
unsafe fn invoke_case(code: i64, shape: i64) -> bool {
    if code == 0 {
        return false;
    }
    if shape == FN_SHAPE_PLAIN {
        type Plain = unsafe extern "C" fn() -> i64;
        let f: Plain = unsafe { std::mem::transmute(code) };
        let _ = unsafe { f() };
        return true;
    }
    // Result / option shaped: ignore tag, just force the body to run.
    type Wide = unsafe extern "C" fn() -> i128;
    let f: Wide = unsafe { std::mem::transmute(code) };
    let _ = unsafe { f() };
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::echo_runtime_fn_new;
    use std::sync::Mutex as StdMutex;

    /// Suite globals are process-wide; serialize unit tests that touch them.
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    fn reset_suite() {
        ENABLED.store(false, Ordering::SeqCst);
        BENCH_MODE.store(false, Ordering::SeqCst);
        let _ = SUITE.lock().map(|mut g| {
            g.cases.clear();
            g.benches.clear();
            g.case_failed = false;
        });
    }

    #[test]
    fn register_noop_without_suite_mode() {
        let _g = TEST_LOCK.lock().expect("test lock");
        reset_suite();
        // SAFETY: nullish handles; register must no-op without suite mode.
        unsafe {
            echo_runtime_test_register(0, 0);
            echo_runtime_test_bench_register(0, 0);
        }
        assert_eq!(echo_runtime_test_finish(), -1);
        reset_suite();
    }

    #[test]
    fn finish_zero_cases_when_enabled() {
        let _g = TEST_LOCK.lock().expect("test lock");
        reset_suite();
        echo_runtime_test_enable();
        assert_eq!(echo_runtime_test_finish(), 0);
        reset_suite();
    }

    #[test]
    fn finish_zero_benches_when_bench_enabled() {
        let _g = TEST_LOCK.lock().expect("test lock");
        reset_suite();
        echo_runtime_test_enable_bench();
        assert_eq!(echo_runtime_test_finish(), 0);
        reset_suite();
    }

    #[test]
    fn bench_mode_ignores_test_cases() {
        let _g = TEST_LOCK.lock().expect("test lock");
        reset_suite();
        echo_runtime_test_enable_bench();
        extern "C" fn sample() -> i64 {
            1
        }
        let handle = echo_runtime_fn_new(sample as *const () as i64, FN_SHAPE_PLAIN);
        // SAFETY: handle is a real fn value; name 0 → "<unnamed>".
        unsafe {
            echo_runtime_test_register(0, handle);
        }
        // No benches registered → 0 cases message path, not running the test.
        assert_eq!(echo_runtime_test_finish(), 0);
        reset_suite();
    }

    #[test]
    fn test_mode_ignores_benches() {
        let _g = TEST_LOCK.lock().expect("test lock");
        reset_suite();
        echo_runtime_test_enable();
        extern "C" fn sample() -> i64 {
            1
        }
        let handle = echo_runtime_fn_new(sample as *const () as i64, FN_SHAPE_PLAIN);
        // SAFETY: handle is a real fn value.
        unsafe {
            echo_runtime_test_bench_register(0, handle);
        }
        assert_eq!(echo_runtime_test_finish(), 0);
        reset_suite();
    }

    #[test]
    fn bench_runs_and_reports_ok() {
        let _g = TEST_LOCK.lock().expect("test lock");
        reset_suite();
        echo_runtime_test_enable_bench();
        extern "C" fn sample() -> i64 {
            1
        }
        let handle = echo_runtime_fn_new(sample as *const () as i64, FN_SHAPE_PLAIN);
        // SAFETY: handle is a real fn value.
        unsafe {
            echo_runtime_test_bench_register(0, handle);
        }
        // Empty body is very fast; should complete with 0 failures.
        // Cap: still may run up to ~1s of wall time — acceptable for unit test.
        assert_eq!(echo_runtime_test_finish(), 0);
        reset_suite();
    }

    #[test]
    fn fn_new_roundtrip_for_register() {
        extern "C" fn sample() -> i64 {
            42
        }
        let code = sample as *const () as i64;
        let handle = echo_runtime_fn_new(code, FN_SHAPE_PLAIN);
        let back = unsafe { echo_runtime_fn_code(handle) };
        assert_eq!(back, code);
    }
}
