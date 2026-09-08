//! Interactive REPL: shared pipeline + LLVM JIT (same as `xo run --jit`).
//!
//! Pattern follows the prior Echo tooling setup (`rustyline` + multi-line brace
//! buffer + session statements + JIT), re-synthesized for this tree (no PHP,
//! no private interpreter).

use std::cell::RefCell;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use echo_ast::Stmt;
use echo_parser::parse;
use echo_pipeline::{compile_to_llvm_with, AnalyzeOptions, OptLevel};
use echo_semantics::infer_last_expr_type;
use echo_source::SourceMap;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hint, Hinter};
use rustyline::history::{DefaultHistory, SearchDirection};
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Context, Editor, Helper};

/// Meta-commands offered as completion-style hints when the line starts with `:`.
const META_HINTS: &[&str] = &[":help", ":session", ":clear", ":quit", ":exit", ":q", ":?"];

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_GREEN: &str = "\x1b[32m";

/// Entry from `xo repl`.
pub fn run_repl() -> std::process::ExitCode {
    let interactive = io::stdin().is_terminal();
    let mut session = ReplSession::default();

    if interactive {
        run_interactive(&mut session);
    } else {
        run_piped(&mut session);
    }
    std::process::ExitCode::SUCCESS
}

fn run_interactive(session: &mut ReplSession) {
    let mut editor: Editor<ReplHelper, DefaultHistory> = match Editor::new() {
        Ok(e) => e,
        Err(err) => {
            eprintln!("xo repl: failed to initialize editor: {err}");
            std::process::exit(1);
        }
    };
    let history_path = repl_history_path();
    if let Some(path) = history_path.as_deref() {
        let _ = editor.load_history(path);
    }

    println!("{ANSI_DIM}Echo REPL — shared pipeline + JIT. :quit / :exit / Ctrl-D to leave.{ANSI_RESET}");
    println!("{ANSI_DIM}Multi-line: keep braces open. Session keeps successful statements.{ANSI_RESET}");
    println!("{ANSI_DIM}Eager hints: complete int expressions show  → i32|i64 <value> (dim).{ANSI_RESET}");

    let mut pending = String::new();
    let mut pending_brace_depth = 0i32;

    loop {
        // Refresh helper so eager-eval sees current session binds.
        editor.set_helper(Some(ReplHelper::new(session.clone())));
        let prompt = if pending.is_empty() {
            format!("{ANSI_GREEN}xo){ANSI_RESET} ")
        } else {
            format!("{ANSI_GREEN}...){ANSI_RESET} ")
        };
        let line = match editor.readline(&prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => break,
            Err(err) => {
                eprintln!("xo repl: read error: {err}");
                break;
            }
        };

        let input = line.trim_end();
        if pending.is_empty() && input.trim().is_empty() {
            continue;
        }
        if pending.is_empty() {
            let t = input.trim();
            if t == ":quit" || t == ":exit" || t == ":q" {
                break;
            }
            if t == ":help" || t == ":?" {
                print_help();
                continue;
            }
            if t == ":clear" {
                session.chunks.clear();
                println!("{ANSI_DIM}(session cleared){ANSI_RESET}");
                continue;
            }
            if t == ":session" {
                if session.chunks.is_empty() {
                    println!("{ANSI_DIM}(empty session){ANSI_RESET}");
                } else {
                    print!("{}", session.source());
                }
                continue;
            }
        }

        let _ = editor.add_history_entry(input);
        if let Some(chunk) = buffer_repl_input(input, &mut pending, &mut pending_brace_depth) {
            eval_chunk(session, &chunk, true);
        }
    }

    if let Some(path) = history_path.as_deref() {
        let _ = editor.save_history(path);
    }
}

fn run_piped(session: &mut ReplSession) {
    let mut line = String::new();
    let mut pending = String::new();
    let mut pending_brace_depth = 0i32;
    let stdin = io::stdin();

    loop {
        line.clear();
        match stdin.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(err) => {
                eprintln!("xo repl: read error: {err}");
                break;
            }
        }

        let input = line.trim_end();
        if pending.is_empty() && input.trim().is_empty() {
            continue;
        }
        if pending.is_empty() {
            let t = input.trim();
            if t == ":quit" || t == ":exit" || t == ":q" {
                break;
            }
            if t == ":clear" {
                session.chunks.clear();
                continue;
            }
            if t == ":help" || t == ":?" || t == ":session" {
                continue;
            }
        }

        if let Some(chunk) = buffer_repl_input(input, &mut pending, &mut pending_brace_depth) {
            eval_chunk(session, &chunk, false);
        }
    }
}

fn print_help() {
    println!(
        "{ANSI_DIM}\
:help / :?     this help\n\
:session       print accumulated session source\n\
:clear         clear session\n\
:quit / :exit  leave\n\
\n\
Inline hints (dim, end-of-line):\n\
  — eager eval: `5 + 3` →  i64 8; `<i32>` binds show i32  (Right does not insert)\n\
  — meta: `:hel` → p  (Right accepts)\n\
  — history prefix match (Right accepts)\n\
\n\
Statements accumulate in the session and re-JIT with each input.\n\
Bare expressions print via str.from_int / str.from_float + io.print.\n\
{ANSI_RESET}"
    );
}

#[derive(Debug, Default, Clone)]
struct ReplSession {
    /// Successful statement chunks (source text), replayed before each eval.
    chunks: Vec<String>,
}

impl ReplSession {
    fn source(&self) -> String {
        self.chunks.join("\n")
    }

    fn has_import(&self, path: &str) -> bool {
        let needle = format!("/ {path}");
        self.chunks.iter().any(|c| c.contains(&needle))
    }
}

fn buffer_repl_input(input: &str, pending: &mut String, brace_depth: &mut i32) -> Option<String> {
    if pending.is_empty() && brace_delta(input) <= 0 {
        return Some(input.to_string());
    }

    if !pending.is_empty() {
        pending.push('\n');
    }
    pending.push_str(input);
    *brace_depth += brace_delta(input);

    if *brace_depth > 0 {
        return None;
    }

    *brace_depth = 0;
    Some(std::mem::take(pending))
}

/// Net open-brace depth, ignoring strings (pure/rich).
fn brace_delta(input: &str) -> i32 {
    let mut delta = 0i32;
    let mut chars = input.chars().peekable();
    let mut quote: Option<char> = None;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote.is_some() && ch == '\\' {
            escaped = true;
            continue;
        }
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch == '{' => delta += 1,
            None if ch == '}' => delta -= 1,
            None => {}
        }
    }
    delta
}

fn eval_chunk(session: &mut ReplSession, chunk: &str, interactive: bool) {
    let trimmed = chunk.trim();
    if trimmed.is_empty() {
        return;
    }

    let kind = classify_chunk(chunk);
    // Bare expressions with a display path are not stored (ephemeral preview).
    // Side-effecting bare calls (`io.print(...)`) and statements accumulate.
    let mut store_on_ok = true;
    let program = match &kind {
        ChunkKind::Expression(expr) => {
            match infer_expr_type(session, expr).and_then(|ty| {
                let conv = display_conv_for_type(&ty)?;
                Some((ty, conv))
            }) {
                Some((_ty, conv)) => {
                    store_on_ok = false;
                    build_expression_program(session, expr, conv)
                }
                None => {
                    // No auto-display (unit/unknown/side-effect call): run as a
                    // statement against the session, but do not persist the line
                    // unless it is a real declaration-style form. Persist only
                    // when the chunk is not a pure call-for-effect later; for
                    // `io.print` we still must not store (re-print on every eval).
                    store_on_ok = false;
                    let mut src = session.source();
                    if !src.is_empty() {
                        src.push('\n');
                    }
                    src.push_str(chunk);
                    src
                }
            }
        }
        ChunkKind::Statement => {
            let mut src = session.source();
            if !src.is_empty() {
                src.push('\n');
            }
            src.push_str(chunk);
            src
        }
    };

    match execute_source(&program) {
        Ok(status) => {
            if status != 0 && interactive {
                eprintln!("{ANSI_DIM}(exit {status}){ANSI_RESET}");
            }
            // Persist statements even when exit status is non-zero so a `+` spawn
            // that reports unjoined tasks is still available for a later `-` join
            // in the cumulative session (each eval re-JITs full session source).
            if store_on_ok {
                session.chunks.push(chunk.to_string());
            }
        }
        Err(err) => {
            eprint!("{err}");
            if !err.ends_with('\n') {
                eprintln!();
            }
        }
    }
}

#[derive(Debug)]
enum ChunkKind {
    Statement,
    Expression(String),
}

/// Heuristic: whole buffer parses as a single bare expression statement.
fn classify_chunk(chunk: &str) -> ChunkKind {
    let mut map = SourceMap::new();
    let id = map.add("repl-chunk.echo", chunk);
    let parsed = parse(map.get(id).unwrap());
    if parsed.diagnostics.error_count() > 0 {
        return ChunkKind::Statement;
    }
    let Some(file) = parsed.file.as_ref() else {
        return ChunkKind::Statement;
    };
    if file.stmts.len() == 1 && matches!(file.stmts[0], Stmt::Expr(_)) {
        // Use original text as expression body (keep operators/parens).
        return ChunkKind::Expression(chunk.trim().to_string());
    }
    ChunkKind::Statement
}

/// How to turn a bare-expr value into a printable string.
#[derive(Debug, Clone, Copy)]
enum DisplayConv {
    /// `str.from_int` — `i32` / `i64`
    FromInt,
    /// `str.from_float` — `f32` / `f64`
    FromFloat,
    /// Direct `io.print` — already a string
    String,
    /// Language bool glyphs `|` / `_`
    Bool,
    /// `str.from_debug` — structs, lists, and other heap shapes
    Debug,
}

fn display_conv_for_type(ty: &echo_semantics::Type) -> Option<DisplayConv> {
    match ty {
        echo_semantics::Type::Int | echo_semantics::Type::Int32 => Some(DisplayConv::FromInt),
        echo_semantics::Type::Float | echo_semantics::Type::Float32 => Some(DisplayConv::FromFloat),
        echo_semantics::Type::String => Some(DisplayConv::String),
        echo_semantics::Type::Bool => Some(DisplayConv::Bool),
        echo_semantics::Type::Named(_)
        | echo_semantics::Type::Anon(_)
        | echo_semantics::Type::List(_)
        | echo_semantics::Type::Bytes
        | echo_semantics::Type::Duration
        | echo_semantics::Type::Range => Some(DisplayConv::Debug),
        _ => None,
    }
}

fn build_expression_program(session: &ReplSession, expr: &str, conv: DisplayConv) -> String {
    let mut src = String::new();
    if !session.has_import("std/io") {
        src.push_str("/ std/io\n");
    }
    let needs_str = matches!(
        conv,
        DisplayConv::FromInt | DisplayConv::FromFloat | DisplayConv::Debug
    );
    if needs_str && !session.has_import("std/str") {
        src.push_str("/ std/str\n");
    }
    let body = session.source();
    if !body.is_empty() {
        src.push_str(&body);
        src.push('\n');
    }
    match conv {
        DisplayConv::FromInt => {
            src.push_str("io.print(str.from_int((");
            src.push_str(expr);
            src.push_str(")))\n");
        }
        DisplayConv::FromFloat => {
            src.push_str("io.print(str.from_float((");
            src.push_str(expr);
            src.push_str(")))\n");
        }
        DisplayConv::String => {
            src.push_str("io.print((");
            src.push_str(expr);
            src.push_str("))\n");
        }
        DisplayConv::Bool => {
            // Surface glyphs: `|` true, `_` false.
            // Do not wrap the condition in extra parens — that breaks `?`/`:` chaining.
            src.push_str("? ");
            src.push_str(expr);
            src.push_str(" {\n");
            src.push_str("    io.print(\"|\")\n");
            src.push_str("}\n");
            src.push_str(": {\n");
            src.push_str("    io.print(\"_\")\n");
            src.push_str("}\n");
        }
        DisplayConv::Debug => {
            src.push_str("io.print(str.from_debug((");
            src.push_str(expr);
            src.push_str(")))\n");
        }
    }
    src
}

/// Work dir under a tree that can see `std/` (SearchPaths walks parents of entry).
fn repl_work_root() -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        for dir in cwd.ancestors() {
            if dir.join("std").is_dir() {
                return dir.join(".xo").join("repl");
            }
        }
    }
    // Unit tests / binary: crates/xo → workspace root via compile-time manifest dir.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(ws) = manifest.parent().and_then(|c| c.parent()) {
        if ws.join("std").is_dir() {
            return ws.join(".xo").join("repl");
        }
    }
    std::env::temp_dir().join("echo-repl")
}

fn execute_source(source: &str) -> Result<i64, String> {
    let mut root = repl_work_root();
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    root.push(format!("s-{t}"));
    std::fs::create_dir_all(&root).map_err(|e| format!("xo repl: temp dir: {e}"))?;
    let path = root.join("repl.echo");
    std::fs::write(&path, source).map_err(|e| format!("xo repl: write: {e}"))?;

    let compiled = compile_to_llvm_with(
        &path,
        &AnalyzeOptions {
            use_cache: false,
            overlays: Default::default(),
        },
        OptLevel::O0,
    );

    if compiled.diagnostics.error_count() > 0 {
        let mut msg = String::new();
        let line_map = echo_source::LineMap::from_text(source);
        for d in compiled.diagnostics.items() {
            let sev = match d.severity {
                echo_diagnostics::Severity::Error => "error",
                echo_diagnostics::Severity::Warning => "warning",
                echo_diagnostics::Severity::Note => "note",
            };
            let code = d.code.as_deref().unwrap_or("-");
            if let Some(span) = d.span {
                let (line, col) = line_map.line_col_1based(span.start);
                msg.push_str(&format!(
                    "{sev}[{code}] {line}:{col}: {}\n",
                    d.message
                ));
            } else {
                msg.push_str(&format!("{sev}[{code}]: {}\n", d.message));
            }
        }
        let _ = std::fs::remove_dir_all(&root);
        return Err(msg);
    }

    let status = echo_codegen::run_jit_ir(&compiled.ir).map_err(|e| {
        let _ = std::fs::remove_dir_all(&root);
        format!("xo repl: JIT: {e}\n")
    })?;

    let _ = std::fs::remove_dir_all(&root);
    Ok(status)
}

/// REPL history: `$XDG_STATE_HOME/xo/history` (default `~/.local/state/xo/history`).
///
/// XDG state is the right home for history/logs (not cache). Creates the
/// directory when missing so `save_history` can write.
fn repl_history_path() -> Option<PathBuf> {
    let state_home = std::env::var_os("XDG_STATE_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join(".local").join("state"))
        })?;
    let dir = state_home.join("xo");
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("xo repl: cannot create history dir {}: {err}", dir.display());
        return None;
    }
    Some(dir.join("history"))
}

// ── rustyline helper (eager-eval + completion hints + multi-line) ──────────

/// Dim ghost text to the right of the cursor.
///
/// Eval previews are display-only (`accept = false`) so Right-arrow does not
/// insert ` → int64 8` into the buffer. Meta/history suffixes set `accept`.
struct ReplHint {
    display: String,
    accept: bool,
}

impl Hint for ReplHint {
    fn display(&self) -> &str {
        &self.display
    }

    fn completion(&self) -> Option<&str> {
        if self.accept {
            Some(&self.display)
        } else {
            None
        }
    }
}

struct ReplHelper {
    session: ReplSession,
    /// Cache last eager-eval: `(line, display_or_none)`.
    eval_cache: RefCell<Option<(String, Option<String>)>>,
}

impl ReplHelper {
    fn new(session: ReplSession) -> Self {
        Self {
            session,
            eval_cache: RefCell::new(None),
        }
    }
}

impl Completer for ReplHelper {
    type Candidate = Pair;
}

impl Helper for ReplHelper {}

impl Hinter for ReplHelper {
    type Hint = ReplHint;

    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<Self::Hint> {
        // Only show a ghost suffix when the cursor is at end-of-line.
        if line.is_empty() || pos < line.len() {
            return None;
        }
        // Eager expression value (primary): `5 + 3` → dim `  → int64 8`.
        if let Some(display) = self.eager_eval_display(line) {
            return Some(ReplHint {
                display,
                accept: false,
            });
        }
        if let Some(h) = meta_hint_suffix(line) {
            return Some(ReplHint {
                display: h,
                accept: true,
            });
        }
        history_hint_suffix(line, pos, ctx).map(|h| ReplHint {
            display: h,
            accept: true,
        })
    }
}

impl ReplHelper {
    fn eager_eval_display(&self, line: &str) -> Option<String> {
        // Multi-line pending buffer is not in `line` (rustyline single field).
        if brace_delta(line) != 0 {
            return None;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(':') {
            return None;
        }

        {
            let cache = self.eval_cache.borrow();
            if let Some((ref prev, ref result)) = *cache {
                if prev == line {
                    return result.clone();
                }
            }
        }

        let display = eager_eval_hint(&self.session, line);
        *self.eval_cache.borrow_mut() = Some((line.to_string(), display.clone()));
        display
    }
}

/// JIT-evaluate a complete bare expression in session context; format ` → i32 N`.
///
/// Kind label + `str.from_int` / `str.from_float` come from shared inference.
/// Failures yield no hint — silent while typing incomplete code.
fn eager_eval_hint(session: &ReplSession, line: &str) -> Option<String> {
    let ChunkKind::Expression(expr) = classify_chunk(line) else {
        return None;
    };
    let ty = infer_expr_type(session, &expr)?;
    let conv = display_conv_for_type(&ty)?;
    let kind = ty.to_string();
    let prog = build_expression_program(session, &expr, conv);
    let (exec, out) = echo_runtime::with_print_capture(|| execute_source(&prog));
    if exec.is_err() {
        return None;
    }
    let value = out.lines().next()?.trim();
    if value.is_empty() {
        return None;
    }
    // Leading spaces separate the ghost from the expression text.
    Some(format!("  → {kind} {value}"))
}

/// Infer the kind of `expr` after replaying session statements (file-local).
fn infer_expr_type(session: &ReplSession, expr: &str) -> Option<echo_semantics::Type> {
    let mut src = session.source();
    if !src.is_empty() {
        src.push('\n');
    }
    src.push_str(expr);
    src.push('\n');
    let mut map = SourceMap::new();
    let id = map.add("repl-hint.echo", &src);
    let parsed = parse(map.get(id).unwrap());
    if parsed.diagnostics.error_count() > 0 {
        return None;
    }
    let file = parsed.file.as_ref()?;
    let ty = infer_last_expr_type(file, &[])?;
    // Drop unresolved inference vars — not a stable preview kind.
    match &ty {
        echo_semantics::Type::Var(_)
        | echo_semantics::Type::Unknown
        | echo_semantics::Type::Error => None,
        _ => Some(ty),
    }
}

/// Remaining characters to complete a meta-command (`:hel` → `p`).
///
/// Among commands that start with the typed prefix, prefer the **shortest**
/// full command (tightest match), then return only the untyped suffix.
fn meta_hint_suffix(line: &str) -> Option<String> {
    let t = line.trim_start();
    if !t.starts_with(':') {
        return None;
    }
    // Exact meta command — nothing left to suggest (e.g. `:q` is complete).
    if META_HINTS.iter().any(|c| *c == t) {
        return None;
    }
    // Among longer commands that still match the prefix, prefer the shortest.
    let mut match_cmd: Option<&str> = None;
    for cmd in META_HINTS {
        if cmd.starts_with(t) {
            match match_cmd {
                None => match_cmd = Some(cmd),
                Some(prev) if cmd.len() < prev.len() => match_cmd = Some(cmd),
                Some(_) => {}
            }
        }
    }
    match_cmd.map(|cmd| cmd[t.len()..].to_string())
}

/// Ghost suffix from the newest history entry that starts with `line`.
fn history_hint_suffix(line: &str, pos: usize, ctx: &Context<'_>) -> Option<String> {
    let start = if ctx.history_index() == ctx.history().len() {
        ctx.history_index().saturating_sub(1)
    } else {
        ctx.history_index()
    };
    let sr = ctx
        .history()
        .starts_with(line, start, SearchDirection::Reverse)
        .ok()
        .flatten()?;
    if sr.entry == line {
        return None;
    }
    if pos > sr.entry.len() {
        return None;
    }
    Some(sr.entry[pos..].to_owned())
}

impl Highlighter for ReplHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        std::borrow::Cow::Owned(format!("{ANSI_DIM}{hint}{ANSI_RESET}"))
    }
}

impl Validator for ReplHelper {
    fn validate(&self, ctx: &mut ValidationContext) -> rustyline::Result<ValidationResult> {
        if brace_delta(ctx.input()) > 0 {
            Ok(ValidationResult::Incomplete)
        } else {
            Ok(ValidationResult::Valid(None))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brace_delta_tracks_blocks() {
        assert_eq!(brace_delta("$ f = () {"), 1);
        assert_eq!(brace_delta("  ^ 1"), 0);
        assert_eq!(brace_delta("}"), -1);
        assert_eq!(brace_delta("'{ not a brace }'"), 0);
        assert_eq!(brace_delta("\"{ still string }\""), 0);
    }

    #[test]
    fn buffer_waits_for_close_brace() {
        let mut pending = String::new();
        let mut depth = 0;
        assert!(buffer_repl_input("$ f = () {", &mut pending, &mut depth).is_none());
        assert_eq!(depth, 1);
        let done = buffer_repl_input("  ^ 1\n}", &mut pending, &mut depth);
        assert!(done.is_some());
        assert!(done.unwrap().contains("^ 1"));
    }

    #[test]
    fn classify_bare_expression() {
        match classify_chunk("1 + 2") {
            ChunkKind::Expression(e) => assert!(e.contains('+')),
            ChunkKind::Statement => panic!("expected expression"),
        }
        match classify_chunk("$ x = 1") {
            ChunkKind::Statement => {}
            ChunkKind::Expression(_) => panic!("expected statement"),
        }
    }

    #[test]
    fn execute_simple_print() {
        let src = "/ std/io\n/ std/str\nio.print(str.from_int(1 + 2))\n";
        let st = execute_source(src).expect("exec");
        assert_eq!(st, 0);
    }

    #[test]
    fn session_keeps_bind_for_later_expr() {
        let mut session = ReplSession::default();
        eval_chunk(&mut session, "$ x = 40", false);
        assert_eq!(session.chunks.len(), 1);
        // expression uses session
        let prog = build_expression_program(&session, "x + 2", DisplayConv::FromInt);
        assert!(prog.contains("$ x = 40"));
        assert!(prog.contains("x + 2"));
        assert!(prog.contains("from_int"));
        let st = execute_source(&prog).expect("exec");
        assert_eq!(st, 0);
    }

    #[test]
    fn float_expr_uses_from_float_not_from_int() {
        let mut session = ReplSession::default();
        eval_chunk(&mut session, "$ b = 5.4", false);
        assert_eq!(session.chunks.len(), 1);
        let ty = infer_expr_type(&session, "b").expect("type");
        assert!(matches!(ty, echo_semantics::Type::Float));
        let prog = build_expression_program(&session, "b", DisplayConv::FromFloat);
        assert!(prog.contains("from_float"), "{prog}");
        assert!(!prog.contains("from_int"), "{prog}");
        let (st, out) = echo_runtime::with_print_capture(|| execute_source(&prog));
        assert_eq!(st.expect("exec"), 0);
        assert_eq!(out.trim(), "5.4");

        let prog2 = build_expression_program(&session, "b + 3.5", DisplayConv::FromFloat);
        let (st2, out2) = echo_runtime::with_print_capture(|| execute_source(&prog2));
        assert_eq!(st2.expect("exec"), 0);
        assert_eq!(out2.trim(), "8.9");
    }

    #[test]
    fn meta_hint_completes_prefix() {
        assert_eq!(meta_hint_suffix(":hel").as_deref(), Some("p"));
        assert_eq!(meta_hint_suffix(":ses").as_deref(), Some("sion"));
        assert_eq!(meta_hint_suffix(":c").as_deref(), Some("lear"));
        assert_eq!(meta_hint_suffix(":qu").as_deref(), Some("it"));
        assert_eq!(meta_hint_suffix(":q"), None); // exact short command
        assert_eq!(meta_hint_suffix(":quit"), None);
        assert_eq!(meta_hint_suffix("$ x = 1"), None);
        assert_eq!(meta_hint_suffix(""), None);
    }

    #[test]
    fn eager_eval_hint_int_expression() {
        let session = ReplSession::default();
        let h = eager_eval_hint(&session, "5 + 3").expect("hint");
        assert_eq!(h, "  → i64 8");
        assert!(eager_eval_hint(&session, "5 +").is_none());
        assert!(eager_eval_hint(&session, "$ x = 1").is_none());
    }

    #[test]
    fn eager_eval_hint_uses_session_binds() {
        let mut session = ReplSession::default();
        eval_chunk(&mut session, "$ x = 40", false);
        let h = eager_eval_hint(&session, "x + 2").expect("hint");
        assert_eq!(h, "  → i64 42");
    }

    #[test]
    fn eager_eval_hint_preserves_i32_width() {
        let mut session = ReplSession::default();
        eval_chunk(&mut session, "$ a = <i32> 5", false);
        assert_eq!(session.chunks.len(), 1);
        let h = eager_eval_hint(&session, "a").expect("hint");
        assert_eq!(h, "  → i32 5");
    }

    #[test]
    fn eager_eval_hint_float() {
        let mut session = ReplSession::default();
        eval_chunk(&mut session, "$ b = 5.4", false);
        let h = eager_eval_hint(&session, "b").expect("hint");
        assert_eq!(h, "  → f64 5.4");
        let h2 = eager_eval_hint(&session, "b + 3.5").expect("hint");
        assert_eq!(h2, "  → f64 8.9");
    }

    #[test]
    fn bare_expr_bool_string_struct_display() {
        let mut session = ReplSession::default();
        // bool — bare `|` at line start is match leader; use a bound name.
        eval_chunk(&mut session, "$ flag = |", false);
        let ty = infer_expr_type(&session, "flag").expect("bool");
        assert!(matches!(ty, echo_semantics::Type::Bool));
        let prog = build_expression_program(&session, "flag", DisplayConv::Bool);
        assert!(prog.contains("io.print(\"|\")"), "{prog}");
        let (st, out) = echo_runtime::with_print_capture(|| execute_source(&prog));
        assert_eq!(st.expect("exec"), 0);
        assert_eq!(out.trim(), "|");

        // string
        let prog_s = build_expression_program(&session, "\"hi\"", DisplayConv::String);
        let (st_s, out_s) = echo_runtime::with_print_capture(|| execute_source(&prog_s));
        assert_eq!(st_s.expect("exec"), 0);
        assert_eq!(out_s.trim(), "hi");

        // named struct
        eval_chunk(&mut session, "% point {\n    $ x\n    $ y\n}\n", false);
        eval_chunk(&mut session, "$ p = point { x: 1, y: 2 }", false);
        let ty_p = infer_expr_type(&session, "p").expect("struct");
        assert!(matches!(ty_p, echo_semantics::Type::Named(_)));
        let prog_p = build_expression_program(&session, "p", DisplayConv::Debug);
        assert!(prog_p.contains("from_debug"), "{prog_p}");
        let (st_p, out_p) = echo_runtime::with_print_capture(|| execute_source(&prog_p));
        assert_eq!(st_p.expect("exec"), 0);
        assert!(
            out_p.contains("point") && out_p.contains("x") && out_p.contains("1"),
            "got {out_p:?}"
        );
    }

    /// Drive multi-line buffer + `eval_chunk` like piped `xo repl` (real JIT).
    fn repl_lines(lines: &[&str]) -> ReplSession {
        let mut session = ReplSession::default();
        let mut pending = String::new();
        let mut depth = 0i32;
        for line in lines {
            if let Some(chunk) = buffer_repl_input(line, &mut pending, &mut depth) {
                eval_chunk(&mut session, &chunk, false);
            }
        }
        assert!(
            pending.is_empty() && depth == 0,
            "unfinished multi-line buffer: {pending:?} depth={depth}"
        );
        session
    }

    fn eval_display(session: &mut ReplSession, expr: &str) -> String {
        let (st, out) = echo_runtime::with_print_capture(|| {
            eval_chunk(session, expr, false);
        });
        let _ = st;
        out
    }

    #[test]
    fn forms_const_function_list_loop() {
        let mut s = repl_lines(&["# A = 21", "# B = A + A"]);
        let out = eval_display(&mut s, "B");
        assert_eq!(out.trim(), "42");

        let mut s = repl_lines(&[
            "$ add = (a, b) {",
            "    ^ a + b",
            "}",
        ]);
        let out = eval_display(&mut s, "add(20, 22)");
        assert_eq!(out.trim(), "42");

        let mut s = repl_lines(&[
            "$ xs = [1, 2, 3]",
            "~ sum = 0",
            "* x : xs {",
            "    ~ sum = sum + x",
            "}",
        ]);
        let out = eval_display(&mut s, "sum");
        assert_eq!(out.trim(), "6");

        let mut s = repl_lines(&["~ sum = 0", "* n : 1..3 {", "    ~ sum = sum + n", "}"]);
        let out = eval_display(&mut s, "sum");
        assert_eq!(out.trim(), "6");
    }

    #[test]
    fn forms_import_print_and_strings() {
        let mut s = ReplSession::default();
        let (st, out) = echo_runtime::with_print_capture(|| {
            eval_chunk(&mut s, "/ std/io", false);
            eval_chunk(&mut s, "io.print(\"hello\")", false);
        });
        let _ = st;
        assert_eq!(out.trim(), "hello");

        let mut s = ReplSession::default();
        eval_chunk(&mut s, "$ name = 'Ada'", false);
        eval_chunk(&mut s, "$ s = \"hi {name}\"", false);
        let out = eval_display(&mut s, "s");
        assert_eq!(out.trim(), "hi Ada");

        let mut s = ReplSession::default();
        eval_chunk(&mut s, "$ s = 'hello pure'", false);
        let out = eval_display(&mut s, "s");
        assert_eq!(out.trim(), "hello pure");
    }

    #[test]
    fn forms_struct_match_result() {
        let mut s = repl_lines(&[
            "% point {",
            "    ~ x",
            "    ~ y",
            "}",
            "$ p = point { x: 3, y: 4 }",
        ]);
        let out = eval_display(&mut s, "p.x");
        assert_eq!(out.trim(), "3");
        eval_chunk(&mut s, "~ p.x = p.x + 10", false);
        let out = eval_display(&mut s, "p.x");
        assert_eq!(out.trim(), "13");

        // Match arm binds are block-local in Echo; drive via print in arms.
        let mut s = ReplSession::default();
        let (st, out) = echo_runtime::with_print_capture(|| {
            eval_chunk(&mut s, "/ std/io", false);
            eval_chunk(
                &mut s,
                "$ number = 5\n| number {\n    4..9 {\n        io.print(\"mid\")\n    }\n    : {\n        io.print(\"other\")\n    }\n}",
                false,
            );
        });
        let _ = st;
        assert_eq!(out.trim(), "mid");

        let mut s = ReplSession::default();
        let (st, out) = echo_runtime::with_print_capture(|| {
            eval_chunk(&mut s, "/ std/io", false);
            eval_chunk(&mut s, "/ std/str", false);
            eval_chunk(
                &mut s,
                "$ checked = (x) {\n    ? x < 0 {\n        ! 99\n    }\n    ^ x\n}",
                false,
            );
            eval_chunk(
                &mut s,
                "| checked(7) {\n    $ v {\n        io.print(str.from_int(v))\n    }\n    ! e {\n        io.print(str.from_int(e))\n    }\n}",
                false,
            );
        });
        let _ = st;
        assert_eq!(out.trim(), "7");
    }

    #[test]
    fn forms_task_spawn_join_and_later_expr() {
        let mut s = ReplSession::default();
        // Spawn alone leaves unjoined status but must stay in session for join.
        eval_chunk(&mut s, "+ job = {\n    ^ 7\n}", false);
        assert!(
            s.chunks.iter().any(|c| c.contains('+')),
            "spawn must be stored for later join: {:?}",
            s.chunks
        );
        eval_chunk(&mut s, "- v = job", false);
        assert!(s.chunks.iter().any(|c| c.contains("- v")), "{:?}", s.chunks);

        let out = eval_display(&mut s, "v");
        assert_eq!(out.trim(), "7", "joined payload should display");

        eval_chunk(&mut s, "$ x = 1", false);
        let out = eval_display(&mut s, "x + v");
        assert_eq!(out.trim(), "8", "later inputs must not leave unjoined tasks");
    }

    #[test]
    fn unjoined_task_jit_does_not_abort_process() {
        // Status 1 (unjoined) is ok; must not SIGSEGV on the host process.
        let src = "+ job = {\n    ^ 7\n}\n";
        let st = execute_source(src).expect("jit must return, not crash");
        assert_ne!(st, 0, "unjoined tasks report non-zero status");
        // Second JIT in-process must also be safe (REPL re-eval path).
        let st2 = execute_source("/ std/io\nio.print(\"ok\")\n").expect("second jit");
        assert_eq!(st2, 0);
    }
}

