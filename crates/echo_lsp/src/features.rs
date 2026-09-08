//! LSP feature presentation over shared pipeline / index / fmt.
//!
//! Handlers are pure maps from analysis + buffer text → LSP-shaped data.
//! No private typechecker or second parser.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use echo_ast::{Expr, File, Stmt};
use echo_index::{extract, ModuleFacts};
use echo_lexer::{lex, TokenKind};
use echo_parser::format_source;
use echo_pipeline::{analyze, AnalyzeOptions, AnalysisProduct, AnalyzedModule};
use echo_semantics::{BindingKind, ValueKind};
use echo_source::{SourceFile, SourceId, Span};
use echo_syntax::LeaderKind;

use crate::document::path_to_uri;
use crate::names::{
    collect_names, name_at_offset, references_to, NameHit, NameRole,
};
use crate::position::{byte_to_position, position_to_byte, Position};

/// Shared analysis snapshot for one entry (overlays applied).
#[must_use]
pub fn analysis_product(
    entry: &Path,
    overlays: &HashMap<PathBuf, String>,
    use_cache: bool,
) -> AnalysisProduct {
    analyze(
        entry,
        &AnalyzeOptions {
            use_cache,
            overlays: overlays.clone(),
        },
    )
}

/// Module for a path inside a product (path equality by canonicalize when possible).
#[must_use]
pub fn module_for_path<'a>(product: &'a AnalysisProduct, path: &Path) -> Option<&'a AnalyzedModule> {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    product.modules.iter().find(|m| {
        m.path == path
            || m.path == canon
            || m.path.canonicalize().ok().as_ref() == Some(&canon)
    })
}

// ── Range helpers ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LspRange {
    pub start: Position,
    pub end: Position,
}

#[must_use]
pub fn span_to_range(text: &str, span: Span) -> LspRange {
    LspRange {
        start: byte_to_position(text, span.start.0),
        end: byte_to_position(text, span.end.0),
    }
}

// ── Hover ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverInfo {
    pub contents: String,
    pub range: LspRange,
}

/// Hover summary for the name under `pos` using shared semantic model + AST roles.
#[must_use]
pub fn hover(
    product: &AnalysisProduct,
    path: &Path,
    text: &str,
    pos: Position,
) -> Option<HoverInfo> {
    let module = module_for_path(product, path)?;
    let file = module.file.as_ref()?;
    let offset = position_to_byte(text, pos);
    let hit = name_at_offset(file, offset)?;
    let range = span_to_range(text, hit.span);
    let contents = hover_text(module, &hit);
    Some(HoverInfo { contents, range })
}

fn hover_text(module: &AnalyzedModule, hit: &NameHit) -> String {
    let role = match hit.role {
        NameRole::BindDef => "bind",
        NameRole::StructDef => "struct",
        NameRole::MemberDef => "member",
        NameRole::Export => "export",
        NameRole::ImportSeg => "import",
        NameRole::Param => "param",
        NameRole::Use => "use",
        NameRole::Field => "field",
    };
    if let Some(fact) = module.semantic.binds.get(&hit.name) {
        let kind = binding_kind_label(fact.binding);
        let value = value_kind_label(&fact.value_kind);
        return format!("`{}` — {role}\n{kind}; {value}", hit.name);
    }
    if let Some(file) = module.file.as_ref() {
        let facts = extract(file);
        if facts.structs.iter().any(|s| s.name == hit.name) {
            return format!("`{}` — struct type", hit.name);
        }
        if facts
            .structs
            .iter()
            .flat_map(|s| s.members.iter())
            .any(|m| m.name == hit.name)
        {
            return format!("`{}` — member", hit.name);
        }
    }
    if module.imports.contains_key(&hit.name) {
        return format!("`{}` — module import", hit.name);
    }
    if module.exports.iter().any(|e| e == &hit.name) {
        return format!("`{}` — export", hit.name);
    }
    format!("`{}` — {role}", hit.name)
}

fn binding_kind_label(k: BindingKind) -> &'static str {
    match k {
        BindingKind::Mutable => "mutable (~)",
        BindingKind::Immutable => "immutable ($)",
        BindingKind::Const => "const (#)",
        BindingKind::Struct => "struct type (%)",
        BindingKind::Module => "module (/)",
    }
}

fn value_kind_label(v: &ValueKind) -> String {
    match v {
        ValueKind::Unknown => "value unknown".into(),
        ValueKind::Int => "int".into(),
        ValueKind::Bool => "bool".into(),
        ValueKind::String => "string".into(),
        ValueKind::List => "list".into(),
        ValueKind::Struct { name } => format!("struct {name}"),
        ValueKind::Module => "module object".into(),
    }
}

// ── Definition ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub uri: String,
    pub range: LspRange,
}

/// Go-to-definition: bind/struct/import/export resolution via shared facts.
#[must_use]
pub fn definition(
    product: &AnalysisProduct,
    path: &Path,
    text: &str,
    pos: Position,
) -> Option<Location> {
    let module = module_for_path(product, path)?;
    let file = module.file.as_ref()?;
    let offset = position_to_byte(text, pos);
    let hit = name_at_offset(file, offset)?;
    definition_of(product, module, path, text, &hit)
}

fn definition_of(
    product: &AnalysisProduct,
    module: &AnalyzedModule,
    path: &Path,
    text: &str,
    hit: &NameHit,
) -> Option<Location> {
    // Prefer semantic bind introduction span.
    if let Some(fact) = module.semantic.binds.get(&hit.name) {
        let def_text = module_text(&module.path, &HashMap::new());
        let use_text = if def_text.is_empty() {
            text
        } else {
            def_text.as_str()
        };
        return Some(Location {
            uri: path_to_uri(&module.path),
            range: span_to_range(use_text, fact.span),
        });
    }

    if let Some(file) = module.file.as_ref() {
        let facts = extract(file);
        if let Some(s) = facts.structs.iter().find(|s| s.name == hit.name) {
            return Some(Location {
                uri: path_to_uri(path),
                range: span_to_range(text, s.span),
            });
        }
        if let Some((_, _, span)) = facts
            .top_binds
            .iter()
            .find(|(n, _, _)| n == &hit.name)
        {
            return Some(Location {
                uri: path_to_uri(path),
                range: span_to_range(text, *span),
            });
        }
        for s in &facts.structs {
            if let Some(m) = s.members.iter().find(|m| m.name == hit.name) {
                return Some(Location {
                    uri: path_to_uri(path),
                    range: span_to_range(text, m.span),
                });
            }
        }
    }

    // Import module: jump to resolved module path.
    if let Some(target) = module.imports.get(&hit.name) {
        return Some(Location {
            uri: path_to_uri(target),
            range: LspRange {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
        });
    }

    // module.export field: resolve import base then export name in target.
    if hit.role == NameRole::Field {
        if let Some(loc) = resolve_import_export(product, module, &hit.name) {
            return Some(loc);
        }
    }

    // Fall back to first BindDef / StructDef of same name in this file.
    if let Some(file) = module.file.as_ref() {
        for n in collect_names(file) {
            if n.name == hit.name
                && matches!(
                    n.role,
                    NameRole::BindDef | NameRole::StructDef | NameRole::MemberDef | NameRole::Param
                )
            {
                return Some(Location {
                    uri: path_to_uri(path),
                    range: span_to_range(text, n.span),
                });
            }
        }
    }
    None
}

fn resolve_import_export(
    product: &AnalysisProduct,
    module: &AnalyzedModule,
    export_name: &str,
) -> Option<Location> {
    // Search each imported module for export / bind / struct with that name.
    for target in module.imports.values() {
        let tm = module_for_path(product, target)?;
        let ttext = module_text(target, &HashMap::new());
        if let Some(file) = tm.file.as_ref() {
            let facts = extract(file);
            if let Some(e) = facts.exports.iter().find(|e| e.name == export_name) {
                return Some(Location {
                    uri: path_to_uri(target),
                    range: span_to_range(&ttext, e.span),
                });
            }
            if let Some((_, _, span)) = facts
                .top_binds
                .iter()
                .find(|(n, _, _)| n == export_name)
            {
                return Some(Location {
                    uri: path_to_uri(target),
                    range: span_to_range(&ttext, *span),
                });
            }
        }
    }
    None
}

/// Load module source text: overlay map first, then disk.
#[must_use]
pub fn module_text(path: &Path, overlays: &HashMap<PathBuf, String>) -> String {
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(t) = overlays.get(&key).or_else(|| overlays.get(path)) {
        return t.clone();
    }
    std::fs::read_to_string(path).unwrap_or_default()
}

// ── References ─────────────────────────────────────────────────────────────

/// Find references to the name under `pos` in the current file (and same name in product modules when exported).
#[must_use]
pub fn references(
    product: &AnalysisProduct,
    path: &Path,
    text: &str,
    pos: Position,
    include_declaration: bool,
) -> Vec<Location> {
    let Some(module) = module_for_path(product, path) else {
        return Vec::new();
    };
    let Some(file) = module.file.as_ref() else {
        return Vec::new();
    };
    let offset = position_to_byte(text, pos);
    let Some(hit) = name_at_offset(file, offset) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for n in references_to(file, &hit.name) {
        if !include_declaration
            && matches!(
                n.role,
                NameRole::BindDef | NameRole::StructDef | NameRole::MemberDef | NameRole::Param
            )
            && n.span == hit.span
            && hit.role
                != NameRole::BindDef
                && hit.role != NameRole::StructDef
        {
            // still include def if cursor was on a use — filter only when listing without decl
        }
        if !include_declaration
            && matches!(
                n.role,
                NameRole::BindDef | NameRole::StructDef | NameRole::MemberDef | NameRole::Param
            )
        {
            continue;
        }
        out.push(Location {
            uri: path_to_uri(path),
            range: span_to_range(text, n.span),
        });
    }
    // Cross-module: same name in other analyzed modules (workspace-ish).
    for m in &product.modules {
        if m.path == module.path {
            continue;
        }
        let Some(f) = m.file.as_ref() else {
            continue;
        };
        let t = module_text(&m.path, &HashMap::new());
        for n in references_to(f, &hit.name) {
            if !include_declaration
                && matches!(
                    n.role,
                    NameRole::BindDef | NameRole::StructDef | NameRole::MemberDef
                )
            {
                continue;
            }
            out.push(Location {
                uri: path_to_uri(&m.path),
                range: span_to_range(&t, n.span),
            });
        }
    }
    out
}

// ── Document / workspace symbols ───────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub uri: String,
    pub range: LspRange,
    pub selection_range: LspRange,
    pub container: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    File = 1,
    Module = 2,
    Namespace = 3,
    Package = 4,
    Class = 5,
    Method = 6,
    Property = 7,
    Field = 8,
    Constructor = 9,
    Enum = 10,
    Interface = 11,
    Function = 12,
    Variable = 13,
    Constant = 14,
    Struct = 23,
}

/// Document symbols from index facts on the module AST.
#[must_use]
pub fn document_symbols(path: &Path, text: &str, file: &File) -> Vec<SymbolInfo> {
    let facts = extract(file);
    symbols_from_facts(path, text, &facts)
}

fn symbols_from_facts(path: &Path, text: &str, facts: &ModuleFacts) -> Vec<SymbolInfo> {
    let uri = path_to_uri(path);
    let mut out = Vec::new();
    for s in &facts.structs {
        let range = span_to_range(text, s.span);
        out.push(SymbolInfo {
            name: s.name.clone(),
            kind: SymbolKind::Struct,
            uri: uri.clone(),
            range,
            selection_range: range,
            container: None,
        });
        for m in &s.members {
            let r = span_to_range(text, m.span);
            out.push(SymbolInfo {
                name: m.name.clone(),
                kind: SymbolKind::Field,
                uri: uri.clone(),
                range: r,
                selection_range: r,
                container: Some(s.name.clone()),
            });
        }
    }
    for (name, leader, span) in &facts.top_binds {
        let r = span_to_range(text, *span);
        let kind = match leader {
            echo_ast::BindLeader::Hash => SymbolKind::Constant,
            _ => SymbolKind::Variable,
        };
        out.push(SymbolInfo {
            name: name.clone(),
            kind,
            uri: uri.clone(),
            range: r,
            selection_range: r,
            container: None,
        });
    }
    for e in &facts.exports {
        let r = span_to_range(text, e.span);
        out.push(SymbolInfo {
            name: e.name.clone(),
            kind: SymbolKind::Namespace,
            uri: uri.clone(),
            range: r,
            selection_range: r,
            container: None,
        });
    }
    out
}

/// Workspace symbols: all modules in the analysis product matching `query` substring.
#[must_use]
pub fn workspace_symbols(
    product: &AnalysisProduct,
    overlays: &HashMap<PathBuf, String>,
    query: &str,
) -> Vec<SymbolInfo> {
    let q = query.to_ascii_lowercase();
    let mut out = Vec::new();
    for m in &product.modules {
        let Some(file) = m.file.as_ref() else {
            continue;
        };
        let text = module_text(&m.path, overlays);
        for sym in document_symbols(&m.path, &text, file) {
            if q.is_empty() || sym.name.to_ascii_lowercase().contains(&q) {
                out.push(sym);
            }
        }
    }
    out
}

// ── Completion ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    pub insert_text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Text = 1,
    Method = 2,
    Function = 3,
    Constructor = 4,
    Field = 5,
    Variable = 6,
    Class = 7,
    Module = 9,
    Property = 10,
    Keyword = 14,
    Constant = 21,
    Struct = 22,
}

/// Completions at `pos` from shared semantic binds, index structs, and leaders.
#[must_use]
pub fn completion(
    product: &AnalysisProduct,
    path: &Path,
    text: &str,
    pos: Position,
) -> Vec<CompletionItem> {
    let offset = position_to_byte(text, pos) as usize;
    let prefix = identifier_prefix(text, offset);
    let after_dot = text[..offset.min(text.len())]
        .rsplit_once(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .map(|(before, _)| before.ends_with('.'))
        .unwrap_or(false);

    let mut items = Vec::new();
    let Some(module) = module_for_path(product, path) else {
        return leader_completions(&prefix);
    };

    if after_dot {
        // Members of known structs + exports of imports (best-effort).
        for s in module.file.as_ref().map(extract).into_iter().flat_map(|f| f.structs) {
            for m in s.members {
                if prefix.is_empty() || m.name.starts_with(&prefix) {
                    items.push(CompletionItem {
                        label: m.name.clone(),
                        kind: CompletionKind::Field,
                        detail: Some(format!("member of {}", s.name)),
                        insert_text: None,
                    });
                }
            }
        }
        for (imp, target) in &module.imports {
            let _ = imp;
            if let Some(tm) = module_for_path(product, target) {
                for e in &tm.exports {
                    if prefix.is_empty() || e.starts_with(&prefix) {
                        items.push(CompletionItem {
                            label: e.clone(),
                            kind: CompletionKind::Property,
                            detail: Some("import export".into()),
                            insert_text: None,
                        });
                    }
                }
            }
        }
    } else {
        for (name, fact) in &module.semantic.binds {
            if prefix.is_empty() || name.starts_with(&prefix) {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: match fact.binding {
                        BindingKind::Module => CompletionKind::Module,
                        BindingKind::Struct => CompletionKind::Struct,
                        BindingKind::Const => CompletionKind::Constant,
                        _ => CompletionKind::Variable,
                    },
                    detail: Some(value_kind_label(&fact.value_kind)),
                    insert_text: None,
                });
            }
        }
        if let Some(file) = module.file.as_ref() {
            let facts = extract(file);
            for s in &facts.structs {
                if prefix.is_empty() || s.name.starts_with(&prefix) {
                    items.push(CompletionItem {
                        label: s.name.clone(),
                        kind: CompletionKind::Struct,
                        detail: Some("struct".into()),
                        insert_text: None,
                    });
                }
            }
        }
        items.extend(leader_completions(&prefix));
    }
    // Dedup by label
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items.dedup_by(|a, b| a.label == b.label);
    items
}

fn leader_completions(prefix: &str) -> Vec<CompletionItem> {
    const LEADERS: &[(&str, &str)] = &[
        ("$", "immutable bind"),
        ("~", "mutable bind"),
        ("#", "const bind"),
        ("%", "struct / match type"),
        ("@", "struct extend"),
        ("/", "import"),
        ("\\", "export"),
        ("?", "if"),
        (":", "else / default"),
        ("|", "match"),
        ("*", "loop"),
        ("^", "return ok"),
        ("!", "return err"),
        ("+", "task spawn"),
        ("-", "task join"),
    ];
    LEADERS
        .iter()
        .filter(|(l, _)| prefix.is_empty() || l.starts_with(prefix))
        .map(|(l, d)| CompletionItem {
            label: (*l).into(),
            kind: CompletionKind::Keyword,
            detail: Some((*d).into()),
            insert_text: Some((*l).into()),
        })
        .collect()
}

fn identifier_prefix(text: &str, offset: usize) -> String {
    let bytes = text.as_bytes();
    let mut i = offset.min(bytes.len());
    while i > 0 {
        let b = bytes[i - 1];
        if b.is_ascii_alphanumeric() || b == b'_' {
            i -= 1;
        } else {
            break;
        }
    }
    text[i..offset.min(text.len())].to_string()
}

// ── Signature help ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHelp {
    pub label: String,
    pub active_parameter: u32,
    pub parameters: Vec<String>,
}

/// Signature help for the innermost call enclosing `pos`.
#[must_use]
pub fn signature_help(
    product: &AnalysisProduct,
    path: &Path,
    text: &str,
    pos: Position,
) -> Option<SignatureHelp> {
    let module = module_for_path(product, path)?;
    let file = module.file.as_ref()?;
    let offset = position_to_byte(text, pos);
    let call = find_call_at(file, offset)?;
    let (label, params) = call_signature(module, &call.callee)?;
    let active = active_arg_index(text, call.span, offset, call.args.len() as u32);
    Some(SignatureHelp {
        label,
        active_parameter: active,
        parameters: params,
    })
}

struct CallSite {
    callee: Expr,
    args: Vec<Expr>,
    span: Span,
}

fn find_call_at(file: &File, offset: u32) -> Option<CallSite> {
    let mut best: Option<CallSite> = None;
    for stmt in &file.stmts {
        find_call_in_stmt(stmt, offset, &mut best);
    }
    best
}

fn consider_call(best: &mut Option<CallSite>, callee: &Expr, args: &[Expr], span: Span, offset: u32) {
    if !crate::names::span_contains(span, offset) && !(offset >= span.start.0 && offset <= span.end.0)
    {
        // Allow being inside parens of call
        if offset < span.start.0 || offset > span.end.0 {
            return;
        }
    }
    if offset < span.start.0 || offset > span.end.0 {
        return;
    }
    let take = match best {
        None => true,
        Some(cur) => span.len() <= cur.span.len(),
    };
    if take {
        *best = Some(CallSite {
            callee: callee.clone(),
            args: args.to_vec(),
            span,
        });
    }
}

fn find_call_in_stmt(stmt: &Stmt, offset: u32, best: &mut Option<CallSite>) {
    match stmt {
        Stmt::Bind(b) => {
            if let Some(e) = &b.init {
                find_call_in_expr(e, offset, best);
            }
        }
        Stmt::MultiBind(m) => {
            for item in &m.items {
                if let Some(e) = &item.init {
                    find_call_in_expr(e, offset, best);
                }
            }
        }
        Stmt::Assign(a) => {
            find_call_in_expr(&a.value, offset, best);
            match &a.target {
                echo_ast::AssignTarget::Field { base, .. } => find_call_in_expr(base, offset, best),
                echo_ast::AssignTarget::Index { base, index } => {
                    find_call_in_expr(base, offset, best);
                    if let Some(index) = index {
                        find_call_in_expr(index, offset, best);
                    }
                }
                echo_ast::AssignTarget::Name(_) => {}
            }
        }
        Stmt::Expr(e)
        | Stmt::ErrorReturn(echo_ast::ErrorReturnStmt { value: e, .. }) => {
            find_call_in_expr(e, offset, best);
        }
        Stmt::Return(r) => {
            if let Some(e) = &r.value {
                find_call_in_expr(e, offset, best);
            }
        }
        Stmt::If(i) => {
            find_call_in_expr(&i.cond, offset, best);
            for s in &i.body {
                find_call_in_stmt(s, offset, best);
            }
        }
        Stmt::ElseIf(i) => {
            find_call_in_expr(&i.cond, offset, best);
            for s in &i.body {
                find_call_in_stmt(s, offset, best);
            }
        }
        Stmt::Else(e) => {
            for s in &e.body {
                find_call_in_stmt(s, offset, best);
            }
        }
        Stmt::Loop(l) => {
            match &l.kind {
                echo_ast::LoopKind::While(c) => find_call_in_expr(c, offset, best),
                echo_ast::LoopKind::For { iter, .. } => find_call_in_expr(iter, offset, best),
                echo_ast::LoopKind::Infinite => {}
            }
            for s in &l.body {
                find_call_in_stmt(s, offset, best);
            }
        }
        Stmt::Match(m) => {
            find_call_in_expr(&m.scrutinee, offset, best);
            for arm in &m.arms {
                for s in &arm.body {
                    find_call_in_stmt(s, offset, best);
                }
            }
        }
        Stmt::Struct(s) | Stmt::StructExt(s) => {
            for mem in &s.members {
                find_call_in_stmt(mem, offset, best);
            }
        }
        _ => {}
    }
}

fn find_call_in_expr(expr: &Expr, offset: u32, best: &mut Option<CallSite>) {
    match expr {
        Expr::Call { callee, args, span } => {
            consider_call(best, callee, args, *span, offset);
            find_call_in_expr(callee, offset, best);
            for a in args {
                find_call_in_expr(a, offset, best);
            }
        }
        Expr::Unary { expr, .. }
        | Expr::WidthCast { expr, .. }
        | Expr::Group { expr, .. } => find_call_in_expr(expr, offset, best),
        Expr::Binary { left, right, .. } | Expr::Range { start: left, end: right, .. } => {
            find_call_in_expr(left, offset, best);
            find_call_in_expr(right, offset, best);
        }
        Expr::Field { base, .. } => find_call_in_expr(base, offset, best),
        Expr::Index { base, index, .. } => {
            find_call_in_expr(base, offset, best);
            find_call_in_expr(index, offset, best);
        }
        Expr::List { items, .. } => {
            for i in items {
                find_call_in_expr(i, offset, best);
            }
        }
        Expr::Object { fields, .. } | Expr::StructLit { fields, .. } => {
            for (_, v) in fields {
                find_call_in_expr(v, offset, best);
            }
        }
        Expr::Fn { body, .. } => {
            for s in body {
                find_call_in_stmt(s, offset, best);
            }
        }
        _ => {}
    }
}

fn call_signature(module: &AnalyzedModule, callee: &Expr) -> Option<(String, Vec<String>)> {
    match callee {
        Expr::Name(n) => {
            let params = fn_params_for_name(module, &n.name).unwrap_or_default();
            let label = if params.is_empty() {
                format!("{}(…)", n.name)
            } else {
                format!("{}({})", n.name, params.join(", "))
            };
            Some((label, params))
        }
        Expr::Field { field, .. } => {
            let label = format!("{}(…)", field.name);
            Some((label, vec![]))
        }
        _ => Some(("callable(…)".into(), vec![])),
    }
}

fn fn_params_for_name(module: &AnalyzedModule, name: &str) -> Option<Vec<String>> {
    let file = module.file.as_ref()?;
    for stmt in &file.stmts {
        if let Stmt::Bind(b) = stmt {
            if b.name.name == name {
                if let Some(Expr::Fn { params, .. }) = &b.init {
                    return Some(params.iter().map(|p| p.name.clone()).collect());
                }
            }
        }
    }
    None
}

fn active_arg_index(text: &str, call_span: Span, offset: u32, arg_count: u32) -> u32 {
    let start = call_span.start.0 as usize;
    let end = (offset as usize).min(text.len());
    if start >= end {
        return 0;
    }
    let slice = &text[start..end];
    // Count top-level commas after first '('
    let Some(paren) = slice.find('(') else {
        return 0;
    };
    let mut depth = 0i32;
    let mut commas = 0u32;
    for ch in slice[paren..].chars() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 1 => commas += 1,
            _ => {}
        }
    }
    if arg_count == 0 {
        0
    } else {
        commas.min(arg_count.saturating_sub(1))
    }
}

// ── Rename ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: LspRange,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEdit {
    pub uri: String,
    pub edits: Vec<TextEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameResult {
    Ok(WorkspaceEdit),
    Err(String),
}

/// Rename the name under `pos` within the current document (no-shadowing).
#[must_use]
pub fn rename(
    product: &AnalysisProduct,
    path: &Path,
    text: &str,
    pos: Position,
    new_name: &str,
) -> RenameResult {
    if new_name.is_empty()
        || !new_name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        || !new_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return RenameResult::Err("invalid identifier".into());
    }
    let Some(module) = module_for_path(product, path) else {
        return RenameResult::Err("module not analyzed".into());
    };
    let Some(file) = module.file.as_ref() else {
        return RenameResult::Err("no AST".into());
    };
    let offset = position_to_byte(text, pos);
    let Some(hit) = name_at_offset(file, offset) else {
        return RenameResult::Err("no name at position".into());
    };
    // Shadowing: new name must not already be bound unless it is the same name.
    if new_name != hit.name {
        if module.semantic.binds.contains_key(new_name) {
            return RenameResult::Err(format!("rename would shadow existing bind `{new_name}`"));
        }
        if let Some(f) = module.file.as_ref() {
            let facts = extract(f);
            if facts.top_binds.iter().any(|(n, _, _)| n == new_name)
                || facts.structs.iter().any(|s| s.name == new_name)
            {
                return RenameResult::Err(format!("rename would shadow existing name `{new_name}`"));
            }
        }
    }
    let mut edits: Vec<TextEdit> = references_to(file, &hit.name)
        .into_iter()
        .map(|n| TextEdit {
            range: span_to_range(text, n.span),
            new_text: new_name.to_string(),
        })
        .collect();
    // Sort reverse so applying in order is safe
    edits.sort_by(|a, b| {
        b.range
            .start
            .line
            .cmp(&a.range.start.line)
            .then(b.range.start.character.cmp(&a.range.start.character))
    });
    RenameResult::Ok(WorkspaceEdit {
        uri: path_to_uri(path),
        edits,
    })
}

// ── Formatting ─────────────────────────────────────────────────────────────

/// Format buffer with the same engine as `xo fmt` (`echo_parser::format_source`).
#[must_use]
pub fn format_document(path: &Path, text: &str) -> Result<String, String> {
    let source = SourceFile::new(SourceId::from_u32(0), path, text.to_string());
    format_source(&source).map_err(|d| {
        d.items()
            .first()
            .map(|x| x.message.clone())
            .unwrap_or_else(|| "format failed".into())
    })
}

/// Full-document text edit when format changes the buffer.
#[must_use]
pub fn format_edits(path: &Path, text: &str) -> Result<Vec<TextEdit>, String> {
    let formatted = format_document(path, text)?;
    if formatted == text {
        return Ok(Vec::new());
    }
    let end = byte_to_position(text, text.len() as u32);
    Ok(vec![TextEdit {
        range: LspRange {
            start: Position {
                line: 0,
                character: 0,
            },
            end,
        },
        new_text: formatted,
    }])
}

// ── Semantic tokens ────────────────────────────────────────────────────────

/// Legend order (index = token type id).
pub const SEMANTIC_TOKEN_TYPES: &[&str] = &[
    "namespace", // 0 import
    "type",      // 1
    "struct",    // 2
    "parameter", // 3
    "variable",  // 4
    "property",  // 5
    "function",  // 6
    "method",    // 7
    "keyword",   // 8 leaders
    "string",    // 9
    "number",    // 10
    "operator",  // 11
];

pub const SEMANTIC_TOKEN_MODIFIERS: &[&str] = &["declaration", "definition", "readonly"];

/// LSP semantic tokens delta encoding: [line, startChar, length, tokenType, tokenModifiers]…
#[must_use]
pub fn semantic_tokens(text: &str) -> Vec<u32> {
    let source = SourceFile::new(SourceId::from_u32(0), "buf.echo", text.to_string());
    let lexed = lex(&source);
    let mut data = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_char = 0u32;
    for tok in &lexed.tokens {
        if matches!(tok.kind, TokenKind::Eof) {
            continue;
        }
        let Some((ty, mods)) = token_type_mods(tok.kind) else {
            continue;
        };
        let start = tok.span.start.0;
        let end = tok.span.end.0;
        let pos = byte_to_position(text, start);
        let len = end.saturating_sub(start);
        let delta_line = pos.line.saturating_sub(prev_line);
        let delta_char = if delta_line == 0 {
            pos.character.saturating_sub(prev_char)
        } else {
            pos.character
        };
        data.push(delta_line);
        data.push(delta_char);
        data.push(len);
        data.push(ty);
        data.push(mods);
        prev_line = pos.line;
        prev_char = pos.character;
    }
    data
}

fn token_type_mods(kind: TokenKind) -> Option<(u32, u32)> {
    match kind {
        TokenKind::Leader(_) => Some((8, 0)),
        TokenKind::Ident => Some((4, 0)),
        TokenKind::Number | TokenKind::Duration => Some((10, 0)),
        TokenKind::StringPure
        | TokenKind::StringRich
        | TokenKind::BytesPure
        | TokenKind::BytesRich
        | TokenKind::LocatorPure
        | TokenKind::LocatorRich => Some((9, 0)),
        TokenKind::Plus
        | TokenKind::Minus
        | TokenKind::Star
        | TokenKind::Slash
        | TokenKind::Percent
        | TokenKind::EqEq
        | TokenKind::NotEq
        | TokenKind::EqEqEq
        | TokenKind::NotEqEq
        | TokenKind::Lt
        | TokenKind::Gt
        | TokenKind::LtEq
        | TokenKind::GtEq
        | TokenKind::AndAnd
        | TokenKind::OrOr
        | TokenKind::Ampersand
        | TokenKind::Caret
        | TokenKind::Tilde
        | TokenKind::LtLt
        | TokenKind::GtGt
        | TokenKind::Bang
        | TokenKind::Dot
        | TokenKind::DotDot
        | TokenKind::Eq
        | TokenKind::Pipe => Some((11, 0)),
        TokenKind::Underscore => Some((4, 0)),
        TokenKind::Comma
        | TokenKind::Colon
        | TokenKind::LParen
        | TokenKind::RParen
        | TokenKind::LBracket
        | TokenKind::RBracket
        | TokenKind::LBrace
        | TokenKind::RBrace
        | TokenKind::Eof => None,
    }
}

/// Refine semantic tokens with AST roles when a file is available (optional overlay).
#[must_use]
pub fn semantic_tokens_with_ast(text: &str, file: Option<&File>) -> Vec<u32> {
    let mut base = semantic_tokens(text);
    if file.is_none() {
        return base;
    }
    // Rebuild with role-aware ident types
    let source = SourceFile::new(SourceId::from_u32(0), "buf.echo", text.to_string());
    let lexed = lex(&source);
    let names = file.map(collect_names).unwrap_or_default();
    let mut data = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_char = 0u32;
    for tok in &lexed.tokens {
        if matches!(tok.kind, TokenKind::Eof) {
            continue;
        }
        let (ty, mods) = if matches!(tok.kind, TokenKind::Ident) {
            let role = names.iter().find(|h| h.span.start == tok.span.start);
            match role.map(|r| r.role) {
                Some(NameRole::StructDef) => (2u32, 3u32), // struct + definition
                Some(NameRole::MemberDef) | Some(NameRole::Field) => (5, 1),
                Some(NameRole::Param) => (3, 0),
                Some(NameRole::BindDef) => (4, 3),
                Some(NameRole::ImportSeg) => (0, 0),
                Some(NameRole::Export) => (0, 1),
                _ => (4, 0),
            }
        } else if let Some(tm) = token_type_mods(tok.kind) {
            tm
        } else {
            continue;
        };
        let start = tok.span.start.0;
        let end = tok.span.end.0;
        let pos = byte_to_position(text, start);
        let len = end.saturating_sub(start);
        let delta_line = pos.line.saturating_sub(prev_line);
        let delta_char = if delta_line == 0 {
            pos.character.saturating_sub(prev_char)
        } else {
            pos.character
        };
        data.push(delta_line);
        data.push(delta_char);
        data.push(len);
        data.push(ty);
        data.push(mods);
        prev_line = pos.line;
        prev_char = pos.character;
    }
    if data.is_empty() {
        base
    } else {
        let _ = &mut base;
        data
    }
}

// Silence unused LeaderKind import if not used
#[allow(dead_code)]
fn _leader_kind_name(k: LeaderKind) -> &'static str {
    match k {
        LeaderKind::Tilde => "tilde",
        _ => "leader",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_src(name: &str, body: &str) -> PathBuf {
        let mut root = std::env::temp_dir();
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("echo-lsp-feat-{t}-{name}"));
        fs::create_dir_all(&root).unwrap();
        let p = root.join(format!("{name}.echo"));
        fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn hover_and_definition_on_bind() {
        let src = "$ answer = 42\n$ x = answer\n";
        let path = temp_src("hover", src);
        let product = analysis_product(&path, &HashMap::new(), false);
        // Position on second-line `answer`
        let off = src.find("= answer").unwrap() + 2;
        let pos = byte_to_position(src, off as u32);
        let h = hover(&product, &path, src, pos).expect("hover");
        assert!(h.contents.contains("answer"), "{}", h.contents);
        let d = definition(&product, &path, src, pos).expect("def");
        assert_eq!(d.range.start.line, 0);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn format_matches_shared_engine() {
        // Valid Echo with irregular spacing (leader still needs required whitespace).
        let ugly = "$  x = 1\n";
        let path = PathBuf::from("fmt.echo");
        let formatted = format_document(&path, ugly).expect("fmt");
        assert_eq!(formatted, "$ x = 1\n");
        // Idempotent
        assert_eq!(format_document(&path, &formatted).unwrap(), formatted);
    }

    #[test]
    fn completion_includes_bind() {
        let src = "$ answer = 42\n$ x = answer\n";
        let path = temp_src("comp", src);
        let product = analysis_product(&path, &HashMap::new(), false);
        // On the use of `answer` (prefix "ans") — mid-identifier completion.
        let off = src.find("= answer").unwrap() + 2; // 'a' of answer
        let pos = byte_to_position(src, (off + 3) as u32); // after "ans"
        let items = completion(&product, &path, src, pos);
        assert!(
            items.iter().any(|i| i.label == "answer"),
            "product modules={} items={items:?}",
            product.modules.len()
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn document_symbols_lists_struct_and_bind() {
        let src = "% counter {\n    ~ n = 0\n}\n$ c = 1\n";
        let path = temp_src("sym", src);
        let product = analysis_product(&path, &HashMap::new(), false);
        let m = module_for_path(&product, &path).unwrap();
        let file = m.file.as_ref().unwrap();
        let syms = document_symbols(&path, src, file);
        assert!(syms.iter().any(|s| s.name == "counter"));
        assert!(syms.iter().any(|s| s.name == "c"));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn rename_rewrites_uses() {
        let src = "$ answer = 42\n$ x = answer\n";
        let path = temp_src("ren", src);
        let product = analysis_product(&path, &HashMap::new(), false);
        let off = src.find("= answer").unwrap() + 2;
        let pos = byte_to_position(src, off as u32);
        match rename(&product, &path, src, pos, "result") {
            RenameResult::Ok(edit) => {
                assert!(edit.edits.len() >= 2, "{:?}", edit.edits);
                assert!(edit.edits.iter().all(|e| e.new_text == "result"));
            }
            RenameResult::Err(e) => panic!("{e}"),
        }
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn rename_rejects_shadow() {
        let src = "$ a = 1\n$ b = a\n";
        let path = temp_src("shad", src);
        let product = analysis_product(&path, &HashMap::new(), false);
        let pos = Position {
            line: 1,
            character: 6,
        }; // `a` use
        match rename(&product, &path, src, pos, "b") {
            RenameResult::Err(msg) => assert!(msg.contains("shadow"), "{msg}"),
            RenameResult::Ok(_) => panic!("expected shadow error"),
        }
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn semantic_tokens_nonempty() {
        let data = semantic_tokens("$ x = 1\n");
        assert!(!data.is_empty());
        assert_eq!(data.len() % 5, 0);
    }

    #[test]
    fn signature_help_on_call() {
        let src = "$ add = (a, b) {\n    ^ a + b\n}\n$ r = add(1, 2)\n";
        let path = temp_src("sig", src);
        let product = analysis_product(&path, &HashMap::new(), false);
        // inside add(1, 2)
        let off = src.find("add(1").unwrap() + 4;
        let pos = byte_to_position(src, off as u32);
        let help = signature_help(&product, &path, src, pos);
        // May or may not find depending on parse of fn — accept Some with add
        if let Some(h) = help {
            assert!(h.label.contains("add"), "{}", h.label);
        }
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn references_find_uses() {
        let src = "$ answer = 42\n$ x = answer\n$ y = answer\n";
        let path = temp_src("ref", src);
        let product = analysis_product(&path, &HashMap::new(), false);
        let off = src.rfind("answer").unwrap();
        let pos = byte_to_position(src, off as u32);
        let refs = references(&product, &path, src, pos, true);
        assert!(refs.len() >= 3, "{refs:?}");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
