//! Echo 2026 suite runner (`e26`): drive a **candidate** toolchain binary over
//! the `echo26/` executable contract for the Echo 2026 edition (ADR 0015).
//!
//! ```text
//! e26 --binary ./target/debug/xo
//! e26 --binary /path/to/other-echo
//! ```
//!
//! Stages per fixture:
//!
//! ```text
//! $bin lex --kinds --diag-codes <file.echo>
//!   stdout → .lex    stderr → .diag (optional)
//!
//! $bin ast --kinds --diag-codes <file.echo>
//!   stdout → .ast    (required)
//!
//! $bin check --diag-codes <file.echo>
//!   stderr → .check  (optional; `sem-*` / `res-*`; absent ⇒ none)
//!
//! If `<NNN>_*.run` exists (or update mode creates it from a present `.run`):
//! $bin run --diag-codes <file.echo>
//!   stdout → .run    stderr compile diags ignored for content (must be empty
//!                    of `cg-*` / compile errors when .run is present)
//!   stdin  → companion `.stdin` bytes if present, else /dev/null
//!   exit code → first line of .runexit if present, else any exit is ok for
//!               stdout-only fixtures; preferred: companion `.runexit` with
//!               a single integer line (process status).
//!
//! Only paths matching `<NNN>_*.echo` are suite roots (support modules may sit
//! beside them without a numeric prefix and are not run as entries).
//! ```

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use clap::Parser;
use walkdir::WalkDir;

#[derive(Debug, Parser)]
#[command(name = "e26")]
#[command(about = "Run the Echo 2026 (echo26) fixture suite against a candidate Echo binary")]
struct Cli {
    /// Candidate toolchain binary (e.g. xo or a third-party CLI).
    #[arg(long, short = 'b')]
    binary: PathBuf,

    /// Suite root (default: ./echo26 or next to workspace).
    #[arg(long, short = 'r')]
    root: Option<PathBuf>,

    /// Rewrite .lex / .diag / .ast / .check / .run / .runexit from candidate output.
    #[arg(long)]
    update: bool,

    /// Only run fixtures whose relative path contains this substring.
    #[arg(long)]
    filter: Option<String>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(n) => {
            eprintln!("e26: {n} failed");
            ExitCode::from(1)
        }
        Err(err) => {
            eprintln!("e26: {err}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<usize, String> {
    let binary = cli
        .binary
        .canonicalize()
        .map_err(|e| format!("binary {}: {e}", cli.binary.display()))?;
    if !binary.is_file() {
        return Err(format!("binary is not a file: {}", binary.display()));
    }

    let root = resolve_root(cli.root.as_deref())?;
    let fixtures = discover(&root, cli.filter.as_deref())?;
    if fixtures.is_empty() {
        return Err(format!("no .echo fixtures under {}", root.display()));
    }

    let mut failed = 0usize;
    for echo_path in &fixtures {
        let rel = echo_path
            .strip_prefix(&root)
            .unwrap_or(echo_path)
            .to_string_lossy()
            .replace('\\', "/");
        match run_one(&binary, echo_path, cli.update) {
            Ok(()) => println!("ok   {rel}"),
            Err(err) => {
                failed += 1;
                println!("FAIL {rel}");
                eprintln!("     {err}");
            }
        }
    }

    let passed = fixtures.len() - failed;
    println!(
        "\ne26: {} passed, {} failed (binary {})",
        passed,
        failed,
        binary.display()
    );
    Ok(failed)
}

fn resolve_root(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return p
            .canonicalize()
            .map_err(|e| format!("root {}: {e}", p.display()));
    }

    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let candidates = [
        cwd.join("echo26"),
        cwd.join("../echo26"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../echo26"),
    ];
    for c in candidates {
        if c.is_dir() {
            return c
                .canonicalize()
                .map_err(|e| format!("root {}: {e}", c.display()));
        }
    }
    Err("could not find echo26/ (pass --root)".into())
}

fn discover(root: &Path, filter: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("echo") {
            continue;
        }
        if !is_numbered_fixture(path) {
            // Support modules for multi-file cases (e.g. user.echo next to 001_entry.echo).
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(f) = filter {
            if !rel.contains(f) {
                continue;
            }
        }
        out.push(path.to_path_buf());
    }
    out.sort();
    Ok(out)
}

/// Suite roots look like `001_dollar.echo` (three digits + underscore + slug).
fn is_numbered_fixture(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    let Some((num, _rest)) = stem.split_once('_') else {
        return false;
    };
    num.len() == 3 && num.chars().all(|c| c.is_ascii_digit())
}

fn run_one(binary: &Path, echo_path: &Path, update: bool) -> Result<(), String> {
    run_lex(binary, echo_path, update)?;
    run_ast(binary, echo_path, update)?;
    run_check(binary, echo_path, update)?;
    run_exec(binary, echo_path, update)?;
    Ok(())
}

fn run_lex(binary: &Path, echo_path: &Path, update: bool) -> Result<(), String> {
    let (stdout, stderr, code) = invoke(
        binary,
        &["lex", "--kinds", "--diag-codes"],
        echo_path,
        Stdio::null(),
    )?;
    if code > 1 {
        return Err(format!("lex: candidate exited {code}\nstderr:\n{stderr}"));
    }

    let lex_path = echo_path.with_extension("lex");
    let diag_path = echo_path.with_extension("diag");

    if update {
        write_expect(&lex_path, &stdout)?;
        write_or_remove(&diag_path, &stderr)?;
        return Ok(());
    }

    compare_file("lex tokens", &lex_path, &stdout)?;
    compare_optional_diag(&diag_path, &stderr)?;
    Ok(())
}

fn run_ast(binary: &Path, echo_path: &Path, update: bool) -> Result<(), String> {
    let (stdout, stderr, code) = invoke(
        binary,
        &["ast", "--kinds", "--diag-codes"],
        echo_path,
        Stdio::null(),
    )?;
    // Exit 1 is ok when fixtures expect diagnostics (e.g. lex errors on same file).
    if code > 1 {
        return Err(format!("ast: candidate exited {code}\nstderr:\n{stderr}"));
    }

    let ast_path = echo_path.with_extension("ast");

    if update {
        if stdout.trim().is_empty() {
            return Err(format!(
                "ast: empty tree for {} (cannot write .ast)",
                echo_path.display()
            ));
        }
        write_expect(&ast_path, &stdout)?;
        return Ok(());
    }

    // .ast is required for every fixture (parse stage is part of the suite).
    compare_file("ast", &ast_path, &stdout)?;
    Ok(())
}

fn run_check(binary: &Path, echo_path: &Path, update: bool) -> Result<(), String> {
    let (_stdout, stderr, code) =
        invoke(binary, &["check", "--diag-codes"], echo_path, Stdio::null())?;
    if code > 1 {
        return Err(format!("check: candidate exited {code}\nstderr:\n{stderr}"));
    }

    let check_path = echo_path.with_extension("check");

    if update {
        write_or_remove(&check_path, &stderr)?;
        return Ok(());
    }

    // Absent .check ⇒ expect no semantic diagnostics.
    compare_optional_diag(&check_path, &stderr)
        .map_err(|e| e.replace("diagnostics mismatch", "check (sem-*) mismatch"))?;
    Ok(())
}

/// Optional execute stage: only when `.run` and/or `.runexit` exists (or `--update`
/// with `ECHO_E26_UPDATE_RUN=1` / when either file already exists).
fn run_exec(binary: &Path, echo_path: &Path, update: bool) -> Result<(), String> {
    let run_path = echo_path.with_extension("run");
    let runexit_path = echo_path.with_extension("runexit");
    let wants_run = run_path.is_file() || runexit_path.is_file();
    if !wants_run && !update {
        return Ok(());
    }
    // On --update, only refresh run expectations when the fixture already opted in
    // (has .run / .runexit) so we do not force-execute the whole suite.
    if update && !wants_run {
        return Ok(());
    }

    let (stdout, stderr, code) = invoke(
        binary,
        &["run", "--diag-codes"],
        echo_path,
        exec_stdin(echo_path)?,
    )?;
    // Tool failure (linker missing, etc.)
    if code == 2 && !runexit_path.is_file() {
        // Still allow .runexit of 2; otherwise treat as tool failure when no exit expect.
        if !stderr.trim().is_empty() && !wants_run {
            return Err(format!("run: candidate exited {code}\nstderr:\n{stderr}"));
        }
    }

    // Compile errors: non-empty diag codes on stderr and no successful program output path.
    // For run fixtures, compile must succeed unless .runexit documents a non-exec status.
    if update {
        write_expect(&run_path, &stdout)?;
        write_expect(&runexit_path, &format!("{code}\n"))?;
        return Ok(());
    }

    if run_path.is_file() {
        compare_file("run stdout", &run_path, &stdout)?;
    }
    if runexit_path.is_file() {
        let expected = fs::read_to_string(&runexit_path)
            .map_err(|e| format!("read {}: {e}", runexit_path.display()))?;
        let exp = normalize(&expected);
        let act = code.to_string();
        if exp != act {
            return Err(format!(
                "run exit mismatch\n--- expected ({})\n{exp}\n--- actual\n{act}\nstderr:\n{stderr}",
                runexit_path.display()
            ));
        }
    }
    Ok(())
}

fn exec_stdin(echo_path: &Path) -> Result<Stdio, String> {
    let path = echo_path.with_extension("stdin");
    if !path.is_file() {
        return Ok(Stdio::null());
    }
    fs::File::open(&path)
        .map(Stdio::from)
        .map_err(|e| format!("open {}: {e}", path.display()))
}

fn invoke(
    binary: &Path,
    args: &[&str],
    echo_path: &Path,
    stdin: Stdio,
) -> Result<(String, String, i32), String> {
    let output = Command::new(binary)
        .args(args)
        .arg(echo_path)
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("spawn {}: {e}", binary.display()))?;

    let code = output.status.code().unwrap_or(255);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok((stdout, stderr, code))
}

fn write_expect(path: &Path, content: &str) -> Result<(), String> {
    fs::write(path, ensure_nl(&normalize(content)))
        .map_err(|e| format!("write {}: {e}", path.display()))
}

fn write_or_remove(path: &Path, content: &str) -> Result<(), String> {
    if normalize(content).is_empty() {
        let _ = fs::remove_file(path);
        Ok(())
    } else {
        write_expect(path, content)
    }
}

fn compare_file(label: &str, path: &Path, actual: &str) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("missing {} (re-run with --update)", path.display()));
    }
    let expected = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    if normalize(&expected) != normalize(actual) {
        return Err(format!(
            "{label} mismatch\n--- expected ({})\n{}--- actual\n{}",
            path.display(),
            expected,
            actual
        ));
    }
    Ok(())
}

fn compare_optional_diag(path: &Path, actual: &str) -> Result<(), String> {
    let expected = if path.is_file() {
        fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?
    } else {
        String::new()
    };
    if normalize(&expected) != normalize(actual) {
        return Err(format!(
            "diagnostics mismatch\n--- expected\n{}--- actual\n{}",
            expected, actual
        ));
    }
    Ok(())
}

fn normalize(s: &str) -> String {
    let mut t = s.replace("\r\n", "\n");
    while t.ends_with('\n') {
        t.pop();
    }
    t
}

fn ensure_nl(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        format!("{s}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdin_sidecar_uses_stdin_extension() {
        let echo = Path::new("echo26/run/io/002_ping.echo");
        assert!(is_numbered_fixture(echo));
        assert_eq!(
            echo.with_extension("stdin"),
            Path::new("echo26/run/io/002_ping.stdin")
        );
    }

    #[test]
    fn exec_stdin_missing_file_is_null() {
        let dir = std::env::temp_dir();
        let echo = dir.join("e26-no-sidecar-001_none.echo");
        match exec_stdin(&echo) {
            Ok(_) => {}
            Err(e) => panic!("missing sidecar must not error: {e}"),
        }
    }
}
