//! Formatter invariants over every `.echo` file in the repository.
//!
//! For each file that parses cleanly, `format_source` must:
//! 1. succeed and produce source that parses cleanly,
//! 2. keep the program: the `xo ast --kinds` dump of input and output match,
//! 3. keep every `;` comment, in source order,
//! 4. be idempotent.
//!
//! `xo fmt --write` rewrites files in place, so a violation here means data loss.

use std::fs;
use std::path::{Path, PathBuf};

use echo_ast::format_ast_kinds;
use echo_lexer::lex;
use echo_parser::{format_source, parse};
use echo_source::SourceMap;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "echo") {
            out.push(path);
        }
    }
}

fn comment_texts(text: &str) -> Vec<String> {
    let mut map = SourceMap::new();
    let id = map.add("c.echo", text);
    let src = map.get(id).unwrap();
    lex(src)
        .comments
        .iter()
        .map(|c| {
            text[c.start.0 as usize..c.end.0 as usize]
                .trim_end()
                .to_string()
        })
        .collect()
}

fn kinds(text: &str) -> Option<String> {
    let mut map = SourceMap::new();
    let id = map.add("k.echo", text);
    let parsed = parse(map.get(id).unwrap());
    if parsed.diagnostics.error_count() > 0 {
        return None;
    }
    parsed.file.as_ref().map(format_ast_kinds)
}

#[test]
fn formatter_preserves_meaning_and_comments_across_the_corpus() {
    let root = repo_root();
    let mut files = Vec::new();
    for dir in ["examples", "echo26", "std"] {
        collect(&root.join(dir), &mut files);
    }
    files.sort();
    assert!(files.len() > 100, "corpus too small: {}", files.len());

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for path in &files {
        let text = fs::read_to_string(path).expect("read corpus file");
        let rel = path.strip_prefix(&root).unwrap().display().to_string();
        let Some(before) = kinds(&text) else {
            continue; // reject fixtures that do not parse are out of scope
        };
        checked += 1;

        let mut map = SourceMap::new();
        let id = map.add("in.echo", &text);
        let once = match format_source(map.get(id).unwrap()) {
            Ok(out) => out,
            Err(_) => {
                failures.push(format!("{rel}: format failed"));
                continue;
            }
        };

        match kinds(&once) {
            None => failures.push(format!("{rel}: output does not parse")),
            Some(after) if after != before => {
                failures.push(format!("{rel}: output changes the AST"));
            }
            Some(_) => {}
        }
        if comment_texts(&once) != comment_texts(&text) {
            failures.push(format!("{rel}: comments not preserved"));
        }

        let mut map2 = SourceMap::new();
        let id2 = map2.add("out.echo", &once);
        match format_source(map2.get(id2).unwrap()) {
            Ok(twice) if twice == once => {}
            _ => failures.push(format!("{rel}: not idempotent")),
        }
    }

    assert!(checked > 100, "too few parseable files: {checked}");
    assert!(
        failures.is_empty(),
        "{} of {} files violate formatter invariants:\n{}",
        failures.len(),
        checked,
        failures.join("\n")
    );
}
