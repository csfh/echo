//! Parse lexer tokens into a source-shaped AST (chumsky; ADR 0011).

#![forbid(unsafe_code)]

mod grammar;

use echo_ast::{File, remap_source_ids};
use echo_cache::{ArtifactStore, PhaseCacheKey};
use echo_diagnostics::{Diagnostic, Diagnostics, decode_diagnostics, encode_diagnostics};
use echo_fingerprint::{ArtifactPhase, phase_fingerprint};
use echo_lexer::{Lexed, format_diag_codes, lex};
use echo_source::SourceFile;
use serde::{Deserialize, Serialize};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// Full frontend result through parse.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub file: Option<File>,
    pub lexed: Lexed,
    pub diagnostics: Diagnostics,
}

/// Whether a parse used the on-disk artifact cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseCacheOutcome {
    Bypass,
    Hit,
    Miss,
    StoreError,
}

const PARSE_BLOB_FORMAT: u32 = 1;

#[derive(Serialize, Deserialize)]
struct CachedParseBlob {
    format: u32,
    file: Option<File>,
    diags: Vec<u8>,
}

/// Lex then parse `source` (shared pipeline entry for `xo ast` / hosts).
#[must_use]
pub fn parse(source: &SourceFile) -> Parsed {
    parse_uncached(source)
}

/// Format Echo source via shared parse + AST pretty-print (`xo fmt`).
///
/// Returns `Ok(canonical)` when parse succeeds with a file and no error
/// diagnostics. On failure, returns diagnostics (caller must not write success).
pub fn format_source(source: &SourceFile) -> Result<String, Diagnostics> {
    let parsed = parse(source);
    if parsed.diagnostics.error_count() > 0 {
        return Err(parsed.diagnostics);
    }
    let Some(file) = parsed.file.as_ref() else {
        let mut d = Diagnostics::new();
        d.push(echo_diagnostics::Diagnostic::error("no AST produced").with_code("fmt-no-ast"));
        return Err(d);
    };
    Ok(echo_ast::format_file(file))
}

fn parse_uncached(source: &SourceFile) -> Parsed {
    let lexed = lex(source);
    let mut diagnostics = Diagnostics::new();
    for d in lexed.diagnostics.items() {
        diagnostics.push(d.clone());
    }

    let (file, parse_diags) = grammar::parse_tokens(source, &lexed.tokens);
    for d in parse_diags {
        diagnostics.push(d);
    }

    Parsed {
        file,
        lexed,
        diagnostics,
    }
}

/// Parse with optional durable cache (AST + diagnostics; tokens not stored).
///
/// Key: source bytes + parse/index component fingerprints. On hit, tokens in
/// [`Parsed::lexed`] are empty (callers that need tokens should use [`parse`]).
#[must_use]
pub fn parse_with_cache(
    source: &SourceFile,
    store: Option<&ArtifactStore>,
) -> (Parsed, ParseCacheOutcome) {
    let Some(store) = store else {
        return (parse_uncached(source), ParseCacheOutcome::Bypass);
    };

    let key = parse_phase_key(source);
    if let Ok(Some(bytes)) = store.get(&key) {
        if let Some(parsed) = decode_parsed(&bytes, source) {
            return (parsed, ParseCacheOutcome::Hit);
        }
    }

    let parsed = parse_uncached(source);
    // Do not persist failed parses: a forgotten version bump must not stick
    // a transient (or pre-fix) parse-error blob to a source path forever.
    if parsed.diagnostics.error_count() > 0 || parsed.file.is_none() {
        return (parsed, ParseCacheOutcome::Miss);
    }
    let blob = encode_parsed(&parsed);
    let outcome = match store.put(&key, &blob) {
        Ok(_) => ParseCacheOutcome::Miss,
        Err(_) => ParseCacheOutcome::StoreError,
    };
    (parsed, outcome)
}

fn parse_phase_key(source: &SourceFile) -> PhaseCacheKey {
    let index_fp = phase_fingerprint(ArtifactPhase::Index, &[]);
    let path = source.path().to_string_lossy();
    PhaseCacheKey::for_source(
        ArtifactPhase::Parse,
        source.text().as_bytes(),
        &[
            ("path", path.as_ref()),
            ("index_fp", index_fp.fingerprint.as_str()),
        ],
    )
}

fn encode_parsed(parsed: &Parsed) -> Vec<u8> {
    let blob = CachedParseBlob {
        format: PARSE_BLOB_FORMAT,
        file: parsed.file.clone(),
        diags: encode_diagnostics(&parsed.diagnostics),
    };
    bincode::serialize(&blob).unwrap_or_default()
}

fn decode_parsed(bytes: &[u8], source: &SourceFile) -> Option<Parsed> {
    let mut blob: CachedParseBlob = bincode::deserialize(bytes).ok()?;
    if blob.format != PARSE_BLOB_FORMAT {
        return None;
    }
    if let Some(file) = &mut blob.file {
        remap_source_ids(file, source.id());
    }
    let diagnostics = decode_diagnostics(&blob.diags);
    Some(Parsed {
        file: blob.file,
        lexed: Lexed {
            tokens: vec![],
            diagnostics: Diagnostics::new(),
        },
        diagnostics,
    })
}

/// Format parse diagnostic codes (one per line) for fixtures.
#[must_use]
pub fn format_parse_diag_codes(diagnostics: &Diagnostics) -> String {
    format_diag_codes(diagnostics)
}

/// Map a chumsky-style message into a diagnostic.
pub(crate) fn parse_error(message: impl Into<String>, span: echo_source::Span) -> Diagnostic {
    Diagnostic::error(message)
        .with_span(span)
        .with_code("parse-error")
}

#[cfg(test)]
mod tests {
    use super::*;
    use echo_ast::{BindLeader, Stmt, format_ast_kinds};
    use echo_cache::{ArtifactStore, CacheLayout};
    use echo_source::SourceMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn parse_str(src: &str) -> Parsed {
        let mut map = SourceMap::new();
        let id = map.add("t.echo", src);
        parse(map.get(id).unwrap())
    }

    #[test]
    fn parse_cache_hit_roundtrip() {
        let mut root = std::env::temp_dir();
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("echo-parse-cache-{t}"));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("t.echo");
        fs::write(&path, "$ name = 42\n").unwrap();

        let layout = CacheLayout::for_project(&root);
        let store = ArtifactStore::new(layout);
        let mut map = SourceMap::new();
        let id = map.load(&path).unwrap();
        let src = map.get(id).unwrap();

        let (p1, o1) = parse_with_cache(src, Some(&store));
        assert_eq!(o1, ParseCacheOutcome::Miss);
        assert!(p1.file.is_some());

        let (p2, o2) = parse_with_cache(src, Some(&store));
        assert_eq!(o2, ParseCacheOutcome::Hit);
        assert!(p2.file.is_some());
        assert_eq!(
            format_ast_kinds(p1.file.as_ref().unwrap()),
            format_ast_kinds(p2.file.as_ref().unwrap())
        );

        store.layout().clean().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dollar_bind() {
        let p = parse_str("$ name = 42\n");
        assert_eq!(
            p.diagnostics.error_count(),
            0,
            "{:?}",
            p.diagnostics.items()
        );
        let file = p.file.expect("ast");
        match &file.stmts[0] {
            Stmt::Bind(b) => {
                assert_eq!(b.leader, BindLeader::Dollar);
                assert_eq!(b.name.name, "name");
            }
            other => panic!("expected bind, got {other:?}"),
        }
        let kinds = format_ast_kinds(&file);
        assert!(kinds.contains("bind_dollar"), "{kinds}");
        assert!(kinds.contains("name name"), "{kinds}");
    }

    #[test]
    fn match_bool_literal_arms() {
        let p = parse_str(
            "\
| t {
    | {
        ^ 1
    }
    _ {
        ^ 0
    }
}
",
        );
        assert_eq!(
            p.diagnostics.error_count(),
            0,
            "{:?}",
            p.diagnostics.items()
        );
        let file = p.file.expect("ast");
        match &file.stmts[0] {
            Stmt::Match(m) => {
                assert_eq!(m.arms.len(), 2);
                assert!(matches!(
                    &m.arms[0].kind,
                    echo_ast::MatchArmKind::Values(ps)
                        if ps.len() == 1
                            && matches!(ps[0], echo_ast::Expr::Bool { value: true, .. })
                ));
                assert!(matches!(
                    &m.arms[1].kind,
                    echo_ast::MatchArmKind::Values(ps)
                        if ps.len() == 1
                            && matches!(ps[0], echo_ast::Expr::Bool { value: false, .. })
                ));
            }
            other => panic!("expected match, got {other:?}"),
        }
    }

    #[test]
    fn match_multi_value_arm() {
        let p = parse_str(
            r#"| x {
    1, 2, 3 {
        ^ 1
    }
    : {
        ^ 0
    }
}
"#,
        );
        assert_eq!(
            p.diagnostics.error_count(),
            0,
            "{:?}",
            p.diagnostics.items()
        );
        let file = p.file.expect("ast");
        match &file.stmts[0] {
            Stmt::Match(m) => {
                assert!(matches!(
                    &m.arms[0].kind,
                    echo_ast::MatchArmKind::Values(ps) if ps.len() == 3
                ));
            }
            other => panic!("expected match, got {other:?}"),
        }
    }

    #[test]
    fn value_field_assign() {
        let p = parse_str("~ p.x = 9\n");
        assert_eq!(
            p.diagnostics.error_count(),
            0,
            "{:?}",
            p.diagnostics.items()
        );
        let file = p.file.expect("ast");
        match &file.stmts[0] {
            Stmt::Assign(a) => match &a.target {
                echo_ast::AssignTarget::Field { base, field } => {
                    match base {
                        echo_ast::Expr::Name(n) => assert_eq!(n.name, "p"),
                        other => panic!("expected name base, got {other:?}"),
                    }
                    assert_eq!(field.name, "x");
                }
                other => panic!("expected field assign, got {other:?}"),
            },
            other => panic!("expected assign, got {other:?}"),
        }
    }

    #[test]
    fn width_tag_signed_literal_after_tag() {
        for src in ["$ a = <i32>-32\n", "$ a = <i32> -32\n", "$ a = <i32> 32\n"] {
            let p = parse_str(src);
            assert_eq!(
                p.diagnostics.error_count(),
                0,
                "{src}: {:?}",
                p.diagnostics.items()
            );
            let file = p.file.expect("ast");
            match &file.stmts[0] {
                Stmt::Bind(b) => match b.init.as_ref() {
                    Some(echo_ast::Expr::Number { text, width, .. }) => {
                        assert!(
                            matches!(width, Some(echo_ast::Width::I32)),
                            "{src}: {width:?}"
                        );
                        if src.contains('-') {
                            assert_eq!(text, "-32", "{src}");
                        } else {
                            assert_eq!(text, "32", "{src}");
                        }
                    }
                    other => panic!("{src}: expected number, got {other:?}"),
                },
                other => panic!("{src}: expected bind, got {other:?}"),
            }
        }
    }

    #[test]
    fn unknown_width_tag_is_width_cast() {
        let p = parse_str("$ a = <u8> 1\n");
        assert_eq!(
            p.diagnostics.error_count(),
            0,
            "{:?}",
            p.diagnostics.items()
        );
        let file = p.file.expect("ast");
        match &file.stmts[0] {
            Stmt::Bind(b) => match b.init.as_ref() {
                Some(echo_ast::Expr::WidthCast { width, tag, .. }) => {
                    assert!(width.is_none());
                    assert_eq!(tag, "u8");
                }
                other => panic!("expected WidthCast, got {other:?}"),
            },
            other => panic!("expected bind, got {other:?}"),
        }
    }

    #[test]
    fn list_index_assign() {
        let p = parse_str("~ xs[0] = 9\n");
        assert_eq!(
            p.diagnostics.error_count(),
            0,
            "{:?}",
            p.diagnostics.items()
        );
        let file = p.file.expect("ast");
        match &file.stmts[0] {
            Stmt::Assign(a) => match &a.target {
                echo_ast::AssignTarget::Index { base, index } => {
                    match base {
                        echo_ast::Expr::Name(n) => assert_eq!(n.name, "xs"),
                        other => panic!("expected name base, got {other:?}"),
                    }
                    match index {
                        Some(echo_ast::Expr::Number { text, .. }) => assert_eq!(text, "0"),
                        other => panic!("expected index 0, got {other:?}"),
                    }
                }
                other => panic!("expected index assign, got {other:?}"),
            },
            other => panic!("expected assign, got {other:?}"),
        }
    }

    #[test]
    fn list_push_assign() {
        let p = parse_str("~ xs[] = 1\n");
        assert_eq!(
            p.diagnostics.error_count(),
            0,
            "{:?}",
            p.diagnostics.items()
        );
        let file = p.file.expect("ast");
        match &file.stmts[0] {
            Stmt::Assign(a) => match &a.target {
                echo_ast::AssignTarget::Index { base, index: None } => match base {
                    echo_ast::Expr::Name(n) => assert_eq!(n.name, "xs"),
                    other => panic!("expected name base, got {other:?}"),
                },
                other => panic!("expected push assign, got {other:?}"),
            },
            other => panic!("expected assign, got {other:?}"),
        }
    }

    #[test]
    fn field_list_push_assign() {
        let p = parse_str("~ m.entries[] = 1\n");
        assert_eq!(
            p.diagnostics.error_count(),
            0,
            "{:?}",
            p.diagnostics.items()
        );
        let file = p.file.expect("ast");
        match &file.stmts[0] {
            Stmt::Assign(a) => match &a.target {
                echo_ast::AssignTarget::Index { base, index: None } => match base {
                    echo_ast::Expr::Field { field, .. } => assert_eq!(field.name, "entries"),
                    other => panic!("expected field base, got {other:?}"),
                },
                other => panic!("expected push assign, got {other:?}"),
            },
            other => panic!("expected assign, got {other:?}"),
        }
    }

    #[test]
    fn field_then_index_assign() {
        let p = parse_str("~ row.xs[0] = 5\n");
        assert_eq!(
            p.diagnostics.error_count(),
            0,
            "{:?}",
            p.diagnostics.items()
        );
        let file = p.file.expect("ast");
        match &file.stmts[0] {
            Stmt::Assign(a) => match &a.target {
                echo_ast::AssignTarget::Index { base, .. } => match base {
                    echo_ast::Expr::Field {
                        base: inner, field, ..
                    } => {
                        assert_eq!(field.name, "xs");
                        match inner.as_ref() {
                            echo_ast::Expr::Name(n) => assert_eq!(n.name, "row"),
                            other => panic!("expected name, got {other:?}"),
                        }
                    }
                    other => panic!("expected field base, got {other:?}"),
                },
                other => panic!("expected index assign, got {other:?}"),
            },
            other => panic!("expected assign, got {other:?}"),
        }
    }

    #[test]
    fn nested_value_field_assign() {
        let p = parse_str("~ p.nested.y = 9\n");
        assert_eq!(
            p.diagnostics.error_count(),
            0,
            "{:?}",
            p.diagnostics.items()
        );
        let file = p.file.expect("ast");
        match &file.stmts[0] {
            Stmt::Assign(a) => match &a.target {
                echo_ast::AssignTarget::Field { base, field } => {
                    assert_eq!(field.name, "y");
                    match base {
                        echo_ast::Expr::Field {
                            base: inner,
                            field: mid,
                            ..
                        } => {
                            assert_eq!(mid.name, "nested");
                            match inner.as_ref() {
                                echo_ast::Expr::Name(n) => assert_eq!(n.name, "p"),
                                other => panic!("expected name root, got {other:?}"),
                            }
                        }
                        other => panic!("expected nested Field base, got {other:?}"),
                    }
                }
                other => panic!("expected field assign, got {other:?}"),
            },
            other => panic!("expected assign, got {other:?}"),
        }
    }

    #[test]
    fn all_leader_families_smoke() {
        let src = "\
$ a = 1
~ b = 2
# C = 3
% user {
    $ name
}
@ user {
    $ greet = () {
        ^ \"hi\"
    }
}
? ready {
    ^ 1
}
: {
    ^ 0
}
! \"x\"
^ 1
* {
    <
}
>
| x {
    1 {
        ^ 1
    }
    : {
        ^ 0
    }
}
/ std/io
\\ name
";
        let p = parse_str(src);
        assert_eq!(
            p.diagnostics.error_count(),
            0,
            "{:?}",
            p.diagnostics.items()
        );
        let file = p.file.expect("ast");
        assert!(file.stmts.len() >= 12, "stmts={}", file.stmts.len());
    }

    fn format_twice(src: &str) -> (String, String) {
        let mut map = SourceMap::new();
        let id = map.add("fmt.echo", src);
        let src_file = map.get(id).unwrap();
        let once = format_source(src_file).expect("format once");
        let id2 = map.add("fmt2.echo", &once);
        let twice = format_source(map.get(id2).unwrap()).expect("format twice");
        (once, twice)
    }

    #[test]
    fn format_idempotent_core_surface() {
        // Valid Echo with irregular spacing (leaders always have required whitespace).
        let messy = r#"
$  x = 1
~ y = x + 2
$ f = (a, b){
    ^ a + b
}
% box {
    ~ n = 0
    $ value = (){
        ^ .n
    }
}
| x {
    1, 2 {
        ^ 1
    }
    : {
        ^ 0
    }
}
/  std/io
\ f, box
"#;
        let (once, twice) = format_twice(messy);
        assert_eq!(once, twice, "not idempotent:\n{once}");
        assert!(once.contains("$ x = 1\n"), "{once}");
        assert!(once.contains("% box {"), "{once}");
        assert!(once.contains("/ std/io\n"), "{once}");
        assert!(once.contains("$ f = (a, b) {"), "{once}");
    }

    #[test]
    fn format_idempotent_match_and_task() {
        let src = r#"
| t {
    | {
        ^ 1
    }
    _ {
        ^ 0
    }
}
+ job = () {
    ^ 1
}
- job
"#;
        let (once, twice) = format_twice(src);
        assert_eq!(once, twice, "{once}");
    }

    #[test]
    fn format_rejects_invalid() {
        let mut map = SourceMap::new();
        let id = map.add("bad.echo", "$ = \n");
        let err = format_source(map.get(id).unwrap());
        assert!(err.is_err());
        let d = err.unwrap_err();
        assert!(d.error_count() > 0);
    }

    #[test]
    fn bare_reassign_is_readable_not_chumsky_debug() {
        // Common REPL/user footgun: `a = 3` instead of `~ a = 3`.
        let p = parse_str("$ a = 1\na = 2\n");
        assert!(
            p.diagnostics.error_count() > 0,
            "{:?}",
            p.diagnostics.items()
        );
        let msg = &p.diagnostics.items()[0].message;
        assert!(!msg.contains("Simple {"), "raw chumsky dump leaked: {msg}");
        assert!(
            msg.contains("~ name =") || msg.contains("reassign"),
            "expected reassignment hint, got: {msg}"
        );
        assert!(msg.contains('='), "{msg}");
    }

    #[test]
    fn parent_relative_import_path() {
        let p = parse_str("/ ../lib/util\n");
        assert_eq!(
            p.diagnostics.error_count(),
            0,
            "{:?}",
            p.diagnostics.items()
        );
        let file = p.file.expect("ast");
        match &file.stmts[0] {
            Stmt::Import(imp) => {
                assert_eq!(imp.path.len(), 3);
                assert!(matches!(imp.path[0], echo_ast::ImportPathSeg::DotDot));
                match &imp.path[1] {
                    echo_ast::ImportPathSeg::Name(n) => assert_eq!(n.name, "lib"),
                    other => panic!("expected name, got {other:?}"),
                }
                match &imp.path[2] {
                    echo_ast::ImportPathSeg::Name(n) => assert_eq!(n.name, "util"),
                    other => panic!("expected name, got {other:?}"),
                }
            }
            other => panic!("expected import, got {other:?}"),
        }
    }

    #[test]
    fn trailing_comma_in_list_is_parse_error() {
        let p = parse_str("$ xs = [1, 2,]\n");
        assert!(
            p.diagnostics
                .items()
                .iter()
                .any(|d| d.code.as_deref() == Some("parse-error")),
            "{:?}",
            p.diagnostics.items()
        );
    }
}
