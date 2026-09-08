//! Browser check/run host: shared frontend plus a playground MIR runner.
//!
//! LLVM AOT/JIT stays native (`xo run` / `xo build`). The playground calls
//! [`check_source`], [`format_source_text`], and [`run::run_source`].
//! Playground run is a host demo, not `xo run`.

#![forbid(unsafe_code)]

mod run;

pub use run::{
    HOMEPAGE_SUM, RunResult, SAMPLE_RESULT, SAMPLE_STRUCT, SAMPLE_SUM, run_json, run_source,
};

use std::path::{Path, PathBuf};

use echo_diagnostics::{Diagnostics, Severity};
use echo_parser::format_source;
use echo_resolver::{SearchPaths, VirtualSources, check_entry_virtual};
use echo_source::{LineMap, SourceFile, SourceId};
use serde::Serialize;

include!(concat!(env!("OUT_DIR"), "/std_files.rs"));

/// Logical package root used for bundled std + the playground entry.
pub const VIRTUAL_ROOT: &str = "/echo";
/// User buffer path inside [`VIRTUAL_ROOT`].
pub const PLAYGROUND_PATH: &str = "/echo/playground.echo";

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CheckDiagnostic {
    pub severity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CheckResult {
    pub ok: bool,
    pub diagnostics: Vec<CheckDiagnostic>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FormatResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<CheckDiagnostic>,
}

/// Number of bundled `std/**/*.echo` files (build-time embed).
#[must_use]
pub fn bundled_std_file_count() -> usize {
    STD_FILES.len()
}

/// Check `source` as `/echo/playground.echo` with the embedded std tree.
#[must_use]
pub fn check_source(source: &str) -> CheckResult {
    let (sources, search) = playground_workspace(source);
    let checked = check_entry_virtual(Path::new(PLAYGROUND_PATH), &search, &sources);
    let diagnostics = locate_diagnostics(&checked.diagnostics, &checked.graph.modules, &sources);
    CheckResult {
        ok: checked.diagnostics.error_count() == 0,
        diagnostics,
    }
}

/// Pretty-print `source` through the shared formatter (`xo fmt`).
#[must_use]
pub fn format_source_text(source: &str) -> FormatResult {
    let file = SourceFile::new(SourceId::from_u32(0), PLAYGROUND_PATH, source.to_string());
    match format_source(&file) {
        Ok(text) => FormatResult {
            ok: true,
            text: Some(text),
            diagnostics: Vec::new(),
        },
        Err(diags) => {
            let map = file.line_map();
            FormatResult {
                ok: false,
                text: None,
                diagnostics: diags
                    .items()
                    .iter()
                    .map(|d| diagnostic_from_parts(d, "playground.echo", Some(map)))
                    .collect(),
            }
        }
    }
}

/// JSON for the wasm-bindgen surface (`check`).
#[must_use]
pub fn check_json(source: &str) -> String {
    serde_json::to_string(&check_source(source)).expect("check json")
}

/// JSON for the wasm-bindgen surface (`format`).
#[must_use]
pub fn format_json(source: &str) -> String {
    serde_json::to_string(&format_source_text(source)).expect("format json")
}

pub(crate) fn playground_workspace(source: &str) -> (VirtualSources, SearchPaths) {
    let mut sources = VirtualSources::new();
    for (rel, text) in STD_FILES {
        let path = format!("{VIRTUAL_ROOT}/std/{rel}");
        sources.insert(path, *text);
    }
    sources.insert(PLAYGROUND_PATH, source.to_string());
    let search = SearchPaths {
        package_roots: vec![PathBuf::from(VIRTUAL_ROOT)],
        declared_deps: Default::default(),
    };
    (sources, search)
}

fn locate_diagnostics(
    diags: &Diagnostics,
    modules: &[echo_resolver::ModuleUnit],
    sources: &VirtualSources,
) -> Vec<CheckDiagnostic> {
    diags
        .items()
        .iter()
        .map(|d| {
            let located = d.span.and_then(|span| {
                let unit = modules.iter().find(|m| m.source_id == span.source)?;
                let text = sources.get(&unit.path).unwrap_or("");
                Some((display_path(&unit.path), LineMap::from_text(text)))
            });
            match located {
                Some((path, map)) => diagnostic_from_parts(d, &path, Some(&map)),
                None => diagnostic_from_parts(d, "", None),
            }
        })
        .collect()
}

fn diagnostic_from_parts(
    d: &echo_diagnostics::Diagnostic,
    path: &str,
    map: Option<&LineMap>,
) -> CheckDiagnostic {
    let (line, column, end_line, end_column) = match (d.span, map) {
        (Some(span), Some(map)) => {
            let (sl, sc) = map.line_col_1based(span.start);
            let (el, ec) = map.line_col_1based(span.end);
            (sl, sc, el, ec)
        }
        _ => (0, 0, 0, 0),
    };
    CheckDiagnostic {
        severity: match d.severity {
            Severity::Error => "error".into(),
            Severity::Warning => "warning".into(),
            Severity::Note => "note".into(),
        },
        code: d.code.clone(),
        message: d.message.clone(),
        path: path.to_string(),
        line,
        column,
        end_line,
        end_column,
    }
}

fn display_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    raw.strip_prefix("/echo/")
        .unwrap_or(raw.as_ref())
        .to_string()
}

#[cfg(target_arch = "wasm32")]
mod wasm_api {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub fn check(source: &str) -> String {
        super::check_json(source)
    }

    #[wasm_bindgen]
    pub fn format(source: &str) -> String {
        super::format_json(source)
    }

    #[wasm_bindgen(js_name = stdFileCount)]
    pub fn std_file_count() -> u32 {
        super::bundled_std_file_count() as u32
    }

    #[wasm_bindgen(js_name = playgroundRun)]
    pub fn playground_run(source: &str) -> String {
        super::run_json(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_std_tree() {
        assert!(
            bundled_std_file_count() >= 40,
            "expected bundled std, got {}",
            bundled_std_file_count()
        );
        assert!(STD_FILES.iter().any(|(p, _)| *p == "io.echo"));
        assert!(STD_FILES.iter().any(|(p, _)| *p == "str.echo"));
        assert!(STD_FILES.iter().any(|(p, _)| *p == "net/http.echo"));
    }

    #[test]
    fn check_ok_std_io() {
        let result = check_source(
            r#"/ std/io

$ xs = [1, 2, 3]
~ sum = 0
* x : xs {
    ~ sum = sum + x
}
io.print("sum={sum}")
"#,
        );
        assert!(result.ok, "{result:?}");
        assert!(result.diagnostics.is_empty(), "{result:?}");
    }

    #[test]
    fn check_rejects_file_scope_error_return() {
        let result = check_source("! 1\n");
        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("sem-error-return")),
            "{result:?}"
        );
        assert!(result.diagnostics.iter().any(|d| d.line >= 1), "{result:?}");
    }

    #[test]
    fn check_rejects_userland_runtime() {
        let result = check_source("/ runtime\n");
        assert!(!result.ok);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("res-runtime-forbidden")),
            "{result:?}"
        );
    }

    #[test]
    fn format_rewrites_simple_bind() {
        let result = format_source_text("$ x=1\n");
        assert!(result.ok, "{result:?}");
        assert_eq!(result.text.as_deref(), Some("$ x = 1\n"));
    }

    #[test]
    fn format_reports_parse_errors() {
        let result = format_source_text("$ = \n");
        assert!(!result.ok);
        assert!(result.text.is_none());
        assert!(!result.diagnostics.is_empty());
    }

    #[test]
    fn json_round_trips() {
        let json = check_json("$ x = 1\n");
        assert!(json.contains("\"ok\":true"), "{json}");
    }

    #[test]
    fn run_sum_sample_prints_sum() {
        let result = run_source(SAMPLE_SUM);
        assert!(result.ok, "{result:?}");
        assert_eq!(result.printed.as_deref(), Some("sum=6\n"), "{result:?}");
        assert!(result.host_error.is_none(), "{result:?}");
    }

    #[test]
    fn run_homepage_sum_echo_prints_sum() {
        let result = run_source(HOMEPAGE_SUM);
        assert!(result.ok, "{result:?}");
        assert_eq!(result.printed.as_deref(), Some("sum=6\n"), "{result:?}");
        assert!(result.host_error.is_none(), "{result:?}");
    }

    #[test]
    fn run_result_sample_prints_ok_arm() {
        let result = run_source(SAMPLE_RESULT);
        assert!(result.ok, "{result:?}");
        assert_eq!(result.printed.as_deref(), Some("7\n"), "{result:?}");
    }

    #[test]
    fn run_struct_sample_prints_field_updates() {
        let result = run_source(SAMPLE_STRUCT);
        assert!(result.ok, "{result:?}");
        assert_eq!(result.printed.as_deref(), Some("3\n13\n"), "{result:?}");
    }

    #[test]
    fn run_rejects_file_scope_error_return_without_executing() {
        let result = run_source("! 1\n");
        assert!(!result.ok, "{result:?}");
        assert!(result.printed.is_none(), "{result:?}");
        assert!(result.host_error.is_none(), "{result:?}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("sem-error-return")),
            "{result:?}"
        );
    }

    #[test]
    fn run_refuses_fs_as_playground_host() {
        assert_playground_host("/ std/fs\n/ std/io\n$ ok = fs.exists(\"x\")\n");
    }

    #[test]
    fn run_refuses_net_as_playground_host() {
        assert_playground_host("/ std/net/tcp\n$ c = tcp.connect(\"127.0.0.1:1\")\n");
    }

    #[test]
    fn run_refuses_process_as_playground_host() {
        assert_playground_host("/ std/process\n$ xs = process.args()\n");
    }

    #[test]
    fn run_refuses_tasks_as_playground_host() {
        assert_playground_host("+ job = {\n    ^ 1\n}\n- job\n");
    }

    fn assert_playground_host(source: &str) {
        let result = run_source(source);
        assert!(!result.ok, "{result:?}");
        assert!(result.printed.is_none(), "{result:?}");
        let err = result.host_error.as_deref().unwrap_or("");
        assert!(
            err.starts_with("playground-host:"),
            "expected playground-host error, got {result:?}"
        );
    }

    #[test]
    fn run_json_exposes_printed_field() {
        let json = run_json(SAMPLE_SUM);
        assert!(json.contains("\"ok\":true"), "{json}");
        assert!(json.contains("sum=6"), "{json}");
    }
}
