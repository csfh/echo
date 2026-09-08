//! Scope-owned memory lowering (ADR 0016) — slice 2.
//!
//! Injects explicit `ScopeEnter` / `ScopeExit` / `ScopeRegister` / `ScopePromote`
//! / `ScopeDisown` into structured MIR.
//!
//! **Slice 2:** promote to the **true owning ancestor** of the destination bind
//! (not always function root); demote only when analysis proves unique ownership
//! and a nested/shorter life (inward `ScopePromote`); leave-scope exits on
//! return/break/continue and match/if/loop arms.
//!
//! **Alias safety:** demotion is refused when a name may share a heap handle with
//! another live name (e.g. `~ b = a`). Name-use-only demotion under immediate free
//! is use-after-free.

use std::collections::{HashMap, HashSet};

use crate::{MirExpr, MirStmt, StrPart};

/// Function root scope id.
pub const ROOT_SCOPE: u32 = 0;

struct Ctx {
    open: Vec<u32>,
    loop_scopes: Vec<u32>,
    next_id: u32,
    /// Bind name → scope that **owns** the binding (introduction scope).
    bind_scope: HashMap<String, u32>,
    /// Names that still hold a **unique** fresh allocation (never aliased).
    /// Demotion is only legal for these — AC1 "proves shorter life".
    unique_fresh: HashSet<String>,
    /// Union-find parent for may-alias names (same handle).
    alias_parent: HashMap<String, String>,
}

impl Ctx {
    fn current(&self) -> u32 {
        *self.open.last().unwrap_or(&ROOT_SCOPE)
    }

    fn alias_find(&mut self, name: &str) -> String {
        let p = self
            .alias_parent
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string());
        if p == name {
            return p;
        }
        let root = self.alias_find(&p);
        self.alias_parent.insert(name.to_string(), root.clone());
        root
    }

    fn alias_union(&mut self, a: &str, b: &str) {
        let ra = self.alias_find(a);
        let rb = self.alias_find(b);
        if ra != rb {
            self.alias_parent.insert(ra, rb.clone());
        }
        // Shared handle ⇒ neither is a unique fresh owner.
        self.unique_fresh.remove(a);
        self.unique_fresh.remove(b);
        let root = self.alias_find(a);
        // Clear unique on entire class (scan known names).
        let keys: Vec<String> = self.alias_parent.keys().cloned().collect();
        for k in keys {
            if self.alias_find(&k) == root {
                self.unique_fresh.remove(&k);
            }
        }
        self.unique_fresh.remove(&root);
    }

    fn note_unique_fresh(&mut self, name: &str) {
        // Rebind to a fresh alloc: sole owner of a new object.
        self.unique_fresh.insert(name.to_string());
        self.alias_parent.insert(name.to_string(), name.to_string());
    }

    fn note_not_unique(&mut self, name: &str) {
        self.unique_fresh.remove(name);
    }

    /// Link `name` into `peer`'s alias class without clearing peer's unique_fresh.
    /// Used when nesting a name into a fresh container the peer uniquely owns.
    fn soft_alias(&mut self, name: &str, peer: &str) {
        if name == peer {
            return;
        }
        self.unique_fresh.remove(name);
        let ra = self.alias_find(name);
        let rb = self.alias_find(peer);
        if ra != rb {
            self.alias_parent.insert(ra, rb);
        }
    }
}

/// Exhaustive collection of `Name` leaves that may denote managed heap handles
/// embedded in `e`. New `MirExpr` variants that embed sub-expressions must be
/// added here (non-exhaustive match will fail to compile).
///
/// Includes call arguments (for alias / escape analysis). **Do not** use this
/// alone for `ScopePromote` of nested storage — call args are not owned by the
/// call result (see [`nested_owned_names_in_expr`]).
#[must_use]
pub fn managed_names_in_expr(e: &MirExpr) -> Vec<&str> {
    let mut out = Vec::new();
    collect_managed_names(e, &mut out);
    out
}

/// Names whose handles are **stored inside** a freshly constructed value
/// (list/struct/interp/range nests). Empty for `Call` / `PrimCall`: arguments
/// are borrowed by the callee, not nested into the return value.
///
/// Promoting call-arg names into the caller's/callee's bind scope caused
/// use-after-free: `$ st = runtime.fs_create_dir_all(path)` re-homed `path`
/// into the callee frame, then `ScopeExit` freed the caller's string.
#[must_use]
pub fn nested_owned_names_in_expr(e: &MirExpr) -> Vec<&str> {
    let mut out = Vec::new();
    collect_nested_owned_names(e, &mut out);
    out
}

fn collect_managed_names<'a>(e: &'a MirExpr, out: &mut Vec<&'a str>) {
    match e {
        MirExpr::Name(n) => out.push(n.as_str()),
        MirExpr::Unary { expr, .. }
        | MirExpr::Cast { expr, .. }
        | MirExpr::BoxValue { value: expr, .. }
        | MirExpr::UnboxValue { value: expr, .. }
        | MirExpr::StructTypeIs { value: expr, .. }
        | MirExpr::FieldGet { base: expr, .. } => collect_managed_names(expr, out),
        MirExpr::Binary { left, right, .. }
        | MirExpr::Range {
            start: left,
            end: right,
        } => {
            collect_managed_names(left, out);
            collect_managed_names(right, out);
        }
        MirExpr::Index { base, index } => {
            collect_managed_names(base, out);
            collect_managed_names(index, out);
        }
        MirExpr::Call { args, .. } | MirExpr::PrimCall { args, .. } => {
            for a in args {
                collect_managed_names(a, out);
            }
        }
        MirExpr::ListLit(xs) => {
            for x in xs {
                collect_managed_names(x, out);
            }
        }
        MirExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_managed_names(v, out);
            }
        }
        MirExpr::StringInterp { parts }
        | MirExpr::LocatorInterp { parts }
        | MirExpr::BytesInterp { parts } => {
            for p in parts {
                if let StrPart::Name(n) = p {
                    out.push(n.as_str());
                }
            }
        }
        // Non-embedding leaves — no managed names.
        MirExpr::ConstI64(_)
        | MirExpr::ConstI32(_)
        | MirExpr::ConstInt { .. }
        | MirExpr::ConstBool(_)
        | MirExpr::ConstF64(_)
        | MirExpr::ConstF32(_)
        | MirExpr::ConstDuration(_)
        | MirExpr::StringLit { .. }
        | MirExpr::BytesLit { .. }
        | MirExpr::LocatorLit { .. }
        | MirExpr::FnValue { .. } => {}
    }
}

fn collect_nested_owned_names<'a>(e: &'a MirExpr, out: &mut Vec<&'a str>) {
    match e {
        // Call / prim results do not store arg handles; do not walk args.
        MirExpr::Call { .. } | MirExpr::PrimCall { .. } => {}
        MirExpr::ListLit(xs) => {
            for x in xs {
                collect_managed_names(x, out);
            }
        }
        MirExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_managed_names(v, out);
            }
        }
        MirExpr::StringInterp { parts }
        | MirExpr::LocatorInterp { parts }
        | MirExpr::BytesInterp { parts } => {
            for p in parts {
                if let StrPart::Name(n) = p {
                    out.push(n.as_str());
                }
            }
        }
        MirExpr::Range { start, end } => {
            collect_managed_names(start, out);
            collect_managed_names(end, out);
        }
        MirExpr::BoxValue { value, .. } => collect_nested_owned_names(value, out),
        // Other forms: no nested storage into a new owner object.
        MirExpr::Name(_)
        | MirExpr::Unary { .. }
        | MirExpr::Cast { .. }
        | MirExpr::UnboxValue { .. }
        | MirExpr::StructTypeIs { .. }
        | MirExpr::FieldGet { .. }
        | MirExpr::Binary { .. }
        | MirExpr::Index { .. }
        | MirExpr::ConstI64(_)
        | MirExpr::ConstI32(_)
        | MirExpr::ConstInt { .. }
        | MirExpr::ConstBool(_)
        | MirExpr::ConstF64(_)
        | MirExpr::ConstF32(_)
        | MirExpr::ConstDuration(_)
        | MirExpr::StringLit { .. }
        | MirExpr::BytesLit { .. }
        | MirExpr::LocatorLit { .. }
        | MirExpr::FnValue { .. } => {}
    }
}

/// Clear unique ownership for `names`; optionally soft-alias each with `peer`
/// (container / destination bind). Does **not** clear `peer`'s unique_fresh.
fn note_may_share_handles(ctx: &mut Ctx, names: &[&str], peer: Option<&str>) {
    for n in names {
        ctx.note_not_unique(n);
        if let Some(p) = peer {
            ctx.soft_alias(n, p);
        }
    }
}

/// Rewrite a function body with scope ownership ops.
#[must_use]
pub fn inject_lifetime(body: Vec<MirStmt>) -> Vec<MirStmt> {
    let mut ctx = Ctx {
        open: vec![ROOT_SCOPE],
        loop_scopes: Vec::new(),
        next_id: 1,
        bind_scope: HashMap::new(),
        unique_fresh: HashSet::new(),
        alias_parent: HashMap::new(),
    };
    let mut out = Vec::with_capacity(body.len() + 4);
    out.push(MirStmt::ScopeEnter { id: ROOT_SCOPE });
    out.extend(rewrite_seq(&body, &mut ctx));
    out.push(MirStmt::ScopeExit { id: ROOT_SCOPE });
    out
}

fn rewrite_seq(stmts: &[MirStmt], ctx: &mut Ctx) -> Vec<MirStmt> {
    let mut out = Vec::new();
    for (i, s) in stmts.iter().enumerate() {
        let after = &stmts[i + 1..];
        out.extend(rewrite_stmt(s, ctx, after));
    }
    out
}

fn rewrite_stmt(s: &MirStmt, ctx: &mut Ctx, after: &[MirStmt]) -> Vec<MirStmt> {
    match s {
        MirStmt::Set { name, value, span } => rewrite_set(name, value, *span, ctx),
        MirStmt::FieldSet { base, field, value } => {
            let mut v = Vec::new();
            note_store_escape(base, value, ctx);
            maybe_promote_store(&mut v, base, value, ctx);
            v.push(MirStmt::FieldSet {
                base: base.clone(),
                field: field.clone(),
                value: value.clone(),
            });
            v
        }
        MirStmt::ListPush { base, value } => {
            let mut v = Vec::new();
            note_store_escape(base, value, ctx);
            maybe_promote_store(&mut v, base, value, ctx);
            v.push(MirStmt::ListPush {
                base: base.clone(),
                value: value.clone(),
            });
            v
        }
        MirStmt::IndexSet { base, index, value } => {
            let mut v = Vec::new();
            note_store_escape(base, value, ctx);
            maybe_promote_store(&mut v, base, value, ctx);
            v.push(MirStmt::IndexSet {
                base: base.clone(),
                index: index.clone(),
                value: value.clone(),
            });
            v
        }
        MirStmt::If { arms, else_body } => {
            let mut new_arms = Vec::new();
            for (cond, body) in arms {
                // If arms enter once: demote parent-owned names used only in this arm.
                // Demote disabled: under immediate free, name-keyed demote is
                // unsound with shared strings/aliases; graph promote handles escape.
                new_arms.push((cond.clone(), rewrite_once_block(body, ctx, after, false)));
            }
            let else_body = else_body
                .as_ref()
                .map(|body| rewrite_once_block(body, ctx, after, false));
            vec![MirStmt::If {
                arms: new_arms,
                else_body,
            }]
        }
        MirStmt::Loop { cond, body } => {
            // Outer once-scope for demotion; per-iteration scope inside body for break.
            rewrite_loop_with_demote_wrap(ctx, after, None, body, |body| MirStmt::Loop {
                cond: cond.clone(),
                body,
            })
        }
        MirStmt::ForIn { item, iter, body } => {
            rewrite_loop_with_demote_wrap(ctx, after, Some(item.as_str()), body, |body| {
                MirStmt::ForIn {
                    item: item.clone(),
                    iter: iter.clone(),
                    body,
                }
            })
        }
        MirStmt::MatchTagged {
            scrutinee,
            ok_name,
            ok_body,
            err_name,
            err_body,
        } => {
            let ok_sid = ctx.next_id;
            ctx.next_id += 1;
            let err_sid = ctx.next_id;
            ctx.next_id += 1;

            ctx.open.push(ok_sid);
            let mut ok_b = Vec::new();
            ok_b.push(MirStmt::ScopeEnter { id: ok_sid });
            if let Some(n) = ok_name {
                ctx.bind_scope.insert(n.clone(), ok_sid);
                ok_b.push(MirStmt::ScopeRegister {
                    value: MirExpr::Name(n.clone()),
                });
            }
            ok_b.extend(rewrite_seq(ok_body, ctx));
            ok_b.push(MirStmt::ScopeExit { id: ok_sid });
            if let Some(n) = ok_name {
                ctx.bind_scope.remove(n);
            }
            ctx.open.pop();

            ctx.open.push(err_sid);
            let mut err_b = Vec::new();
            err_b.push(MirStmt::ScopeEnter { id: err_sid });
            if let Some(n) = err_name {
                ctx.bind_scope.insert(n.clone(), err_sid);
                err_b.push(MirStmt::ScopeRegister {
                    value: MirExpr::Name(n.clone()),
                });
            }
            err_b.extend(rewrite_seq(err_body, ctx));
            err_b.push(MirStmt::ScopeExit { id: err_sid });
            if let Some(n) = err_name {
                ctx.bind_scope.remove(n);
            }
            ctx.open.pop();

            vec![MirStmt::MatchTagged {
                scrutinee: scrutinee.clone(),
                ok_name: ok_name.clone(),
                ok_body: ok_b,
                err_name: err_name.clone(),
                err_body: err_b,
            }]
        }
        MirStmt::ReturnOk(e, span) => exit_then_return(ctx, Some(ReturnKind::Ok(e.clone(), *span))),
        MirStmt::ReturnErr(e) => exit_then_return(ctx, Some(ReturnKind::Err(e.clone()))),
        MirStmt::ReturnNone => exit_then_return(ctx, Some(ReturnKind::None)),
        MirStmt::Break => {
            let mut v = exit_scopes_to_loop(ctx);
            v.push(MirStmt::Break);
            v
        }
        MirStmt::Continue => {
            let mut v = exit_scopes_to_loop(ctx);
            v.push(MirStmt::Continue);
            v
        }
        MirStmt::TaskSpawn { bind, .. } | MirStmt::TaskSpawnFn { bind, .. } => {
            let mut v = vec![s.clone()];
            if let Some(name) = bind {
                let current = ctx.current();
                ctx.bind_scope.insert(name.clone(), current);
                v.push(MirStmt::ScopeRegister {
                    value: MirExpr::Name(name.clone()),
                });
            }
            v
        }
        other => vec![other.clone()],
    }
}

fn rewrite_set(
    name: &str,
    value: &MirExpr,
    span: Option<echo_source::Span>,
    ctx: &mut Ctx,
) -> Vec<MirStmt> {
    let current = ctx.current();
    let mut v = Vec::new();

    if !ctx.bind_scope.contains_key(name) {
        ctx.bind_scope.insert(name.to_string(), current);
    }
    let target = *ctx.bind_scope.get(name).unwrap_or(&current);

    let fresh = expr_is_fresh_alloc(value);
    let managed = expr_is_managed(value);
    // Promote only true nests (list/struct/interp), never call arguments.
    let nested = nested_owned_names_in_expr(value);
    // Alias analysis still sees call args (may share after `^ path` returns).
    let all_embedded = managed_names_in_expr(value);

    // Ownership / alias tracking — single transfer API for all RHS shapes.
    if let MirExpr::Name(other) = value {
        // Pure alias: both names share one handle.
        ctx.note_not_unique(name);
        ctx.alias_union(name, other);
    } else if fresh {
        // Dest uniquely owns the new container/value object…
        ctx.note_unique_fresh(name);
        // Names nested *into* the RHS may share a handle with dest (ListLit /
        // StructLit). Call args are not nested — do not soft-alias them away.
        note_may_share_handles(ctx, &nested, Some(name));
    } else if managed {
        ctx.note_not_unique(name);
        note_may_share_handles(ctx, &all_embedded, Some(name));
    }

    v.push(MirStmt::Set {
        name: name.to_string(),
        value: value.clone(),
        span,
    });

    if fresh {
        // Register into current frame, then promote to bind owner if nested deeper.
        v.push(MirStmt::ScopeRegister {
            value: MirExpr::Name(name.to_string()),
        });
        if managed && target != current {
            v.push(MirStmt::ScopePromote {
                value: MirExpr::Name(name.to_string()),
                target,
            });
        }
        // Nested Names stored into this value must be owned at dest's bind scope
        // (same as ListPush): otherwise a prior demote into this region + exit
        // frees them while dest (promoted outward) still holds the handles.
        // Call arguments are intentionally excluded (see nested_owned_names_in_expr).
        for n in &nested {
            if *n != name {
                v.push(MirStmt::ScopePromote {
                    value: MirExpr::Name((*n).to_string()),
                    target,
                });
            }
        }
    } else if managed && target != current {
        // Reassign outer bind from nested scope: ownership moves to bind owner.
        v.push(MirStmt::ScopePromote {
            value: MirExpr::Name(name.to_string()),
            target,
        });
        for n in &nested {
            if *n != name {
                v.push(MirStmt::ScopePromote {
                    value: MirExpr::Name((*n).to_string()),
                    target,
                });
            }
        }
    }
    v
}

fn maybe_promote_store(v: &mut Vec<MirStmt>, base: &MirExpr, value: &MirExpr, ctx: &Ctx) {
    if !expr_is_managed(value) {
        return;
    }
    let current = ctx.current();
    let Some(target) = store_promote_target(base, ctx) else {
        return;
    };
    if target != current {
        v.push(MirStmt::ScopePromote {
            value: value.clone(),
            target,
        });
    }
}

/// Container store escape: every managed Name in `value` may share with `base`.
fn note_store_escape(base: &MirExpr, value: &MirExpr, ctx: &mut Ctx) {
    let names = managed_names_in_expr(value);
    if names.is_empty() {
        return;
    }
    note_may_share_handles(ctx, &names, base_name(base));
}

fn base_name(e: &MirExpr) -> Option<&str> {
    match e {
        MirExpr::Name(n) => Some(n.as_str()),
        MirExpr::FieldGet { base, .. } | MirExpr::Index { base, .. } => base_name(base),
        _ => None,
    }
}

/// Scope entered at most once (if/match arm). Optional demotion of parent-owned names.
fn rewrite_once_block(
    body: &[MirStmt],
    ctx: &mut Ctx,
    after: &[MirStmt],
    demote: bool,
) -> Vec<MirStmt> {
    let sid = ctx.next_id;
    ctx.next_id += 1;
    ctx.open.push(sid);
    let mut b = Vec::new();
    b.push(MirStmt::ScopeEnter { id: sid });
    if demote {
        // Escapes *inside* the body must clear unique_fresh before demote chooses.
        prescan_body_escapes(body, ctx);
        b.extend(demote_into_scope(sid, body, after, ctx));
    }
    b.extend(rewrite_seq(body, ctx));
    b.push(MirStmt::ScopeExit { id: sid });
    ctx.open.pop();
    b
}

/// Loop / for-in: demote into a **once** wrapper scope around the whole loop;
/// per-iteration body still gets its own scope for break/continue cleanup.
fn rewrite_loop_with_demote_wrap(
    ctx: &mut Ctx,
    _after: &[MirStmt],
    for_item: Option<&str>,
    body: &[MirStmt],
    make: impl FnOnce(Vec<MirStmt>) -> MirStmt,
) -> Vec<MirStmt> {
    let wrap = ctx.next_id;
    ctx.next_id += 1;
    ctx.open.push(wrap);

    let mut out = Vec::new();
    out.push(MirStmt::ScopeEnter { id: wrap });
    // Demote into loop wrap disabled (see if-arm note); graph promote owns escape.

    // Per-iteration body scope (re-entered); tracks break/continue exits.
    let body_sid = ctx.next_id;
    ctx.next_id += 1;
    ctx.open.push(body_sid);
    ctx.loop_scopes.push(body_sid);
    if let Some(item) = for_item {
        ctx.bind_scope.insert(item.to_string(), body_sid);
    }
    let mut b = Vec::new();
    b.push(MirStmt::ScopeEnter { id: body_sid });
    b.extend(rewrite_seq(body, ctx));
    b.push(MirStmt::ScopeExit { id: body_sid });
    if let Some(item) = for_item {
        ctx.bind_scope.remove(item);
    }
    ctx.loop_scopes.pop();
    ctx.open.pop();

    out.push(make(b));
    out.push(MirStmt::ScopeExit { id: wrap });
    ctx.open.pop();
    out
}

/// Dry-run escape transfer for a statement list (and nested control) so demotion
/// sees in-body stores/nests before choosing candidates. Idempotent with the
/// real rewrite that follows.
fn prescan_body_escapes(stmts: &[MirStmt], ctx: &mut Ctx) {
    for s in stmts {
        match s {
            MirStmt::Set { name, value, .. } => {
                let embedded = managed_names_in_expr(value);
                if let MirExpr::Name(other) = value {
                    ctx.note_not_unique(name);
                    ctx.alias_union(name, other);
                } else if expr_is_fresh_alloc(value) {
                    // Dest may be outer bind; nested Names escape into it.
                    note_may_share_handles(ctx, &embedded, Some(name.as_str()));
                } else if expr_is_managed(value) {
                    ctx.note_not_unique(name);
                    note_may_share_handles(ctx, &embedded, Some(name.as_str()));
                }
            }
            MirStmt::FieldSet { base, value, .. }
            | MirStmt::ListPush { base, value }
            | MirStmt::IndexSet { base, value, .. } => {
                note_store_escape(base, value, ctx);
            }
            MirStmt::If { arms, else_body } => {
                for (_, b) in arms {
                    prescan_body_escapes(b, ctx);
                }
                if let Some(b) = else_body {
                    prescan_body_escapes(b, ctx);
                }
            }
            MirStmt::Loop { body, .. } | MirStmt::ForIn { body, .. } => {
                prescan_body_escapes(body, ctx);
            }
            MirStmt::MatchTagged {
                ok_body, err_body, ..
            } => {
                prescan_body_escapes(ok_body, ctx);
                prescan_body_escapes(err_body, ctx);
            }
            _ => {}
        }
    }
}

/// Promote target when storing into a container: the container bind's owning scope.
fn store_promote_target(base: &MirExpr, ctx: &Ctx) -> Option<u32> {
    match base {
        MirExpr::Name(n) => Some(ctx.bind_scope.get(n).copied().unwrap_or(ROOT_SCOPE)),
        MirExpr::FieldGet { base, .. } => store_promote_target(base, ctx),
        MirExpr::Index { base, .. } => store_promote_target(base, ctx),
        _ => {
            let cur = ctx.current();
            if cur == ROOT_SCOPE {
                None
            } else if ctx.open.len() >= 2 {
                Some(ctx.open[ctx.open.len() - 2])
            } else {
                Some(ROOT_SCOPE)
            }
        }
    }
}

/// Demote parent-owned names used in `body` but not after: inward `ScopePromote`.
///
/// **Alias-safe (required under immediate free):** only demote when the name is a
/// unique fresh owner and no may-alias peer is used after the nested region.
/// Demoting `b` while `a` aliases the same handle and is used later is UAF.
fn demote_into_scope(
    nested_id: u32,
    body: &[MirStmt],
    after: &[MirStmt],
    ctx: &Ctx,
) -> Vec<MirStmt> {
    let parent = if ctx.open.len() >= 2 {
        ctx.open[ctx.open.len() - 2]
    } else {
        ROOT_SCOPE
    };
    let used_in = names_used_in_stmts(body);
    let used_after = names_used_in_stmts(after);
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    // Need mutable alias_find — clone alias state into a temp Ctx view.
    let mut alias_parent = ctx.alias_parent.clone();
    let unique_fresh = ctx.unique_fresh.clone();
    let bind_scope = ctx.bind_scope.clone();

    fn find(parent: &mut HashMap<String, String>, name: &str) -> String {
        let p = parent
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string());
        if p == name {
            return p;
        }
        let root = find(parent, &p);
        parent.insert(name.to_string(), root.clone());
        root
    }

    for (name, &owner) in &bind_scope {
        if owner != parent {
            continue;
        }
        if !used_in.contains(name.as_str()) {
            continue;
        }
        if used_after.contains(name.as_str()) {
            continue;
        }
        // Unique ownership only — never demote aliases (e.g. `~ b = a`).
        if !unique_fresh.contains(name) {
            continue;
        }
        // Whole may-alias class must be unused after (belt; unique_fresh should
        // already imply singleton class for name-assign tracking).
        let root = find(&mut alias_parent, name);
        let mut peer_used_after = false;
        for (other, _) in &bind_scope {
            if find(&mut alias_parent, other) == root && used_after.contains(other.as_str()) {
                peer_used_after = true;
                break;
            }
        }
        if peer_used_after {
            continue;
        }
        if !seen.insert(name.clone()) {
            continue;
        }
        out.push(MirStmt::ScopePromote {
            value: MirExpr::Name(name.clone()),
            target: nested_id,
        });
    }
    out
}

fn names_used_in_stmts(stmts: &[MirStmt]) -> HashSet<String> {
    let mut set = HashSet::new();
    for s in stmts {
        collect_names_stmt(s, &mut set);
    }
    set
}

fn collect_names_stmt(s: &MirStmt, set: &mut HashSet<String>) {
    match s {
        MirStmt::Set { name, value, .. } => {
            set.insert(name.clone());
            collect_names_expr(value, set);
        }
        MirStmt::ScopeRegister { value }
        | MirStmt::ScopePromote { value, .. }
        | MirStmt::ScopeDisown { value }
        | MirStmt::ScopeRelease { value }
        | MirStmt::ReturnOk(value, _)
        | MirStmt::ReturnErr(value)
        | MirStmt::Eval(value) => collect_names_expr(value, set),
        MirStmt::FieldSet { base, value, .. } => {
            collect_names_expr(base, set);
            collect_names_expr(value, set);
        }
        MirStmt::IndexSet { base, index, value } => {
            collect_names_expr(base, set);
            collect_names_expr(index, set);
            collect_names_expr(value, set);
        }
        MirStmt::ListPush { base, value } => {
            collect_names_expr(base, set);
            collect_names_expr(value, set);
        }
        MirStmt::If { arms, else_body } => {
            for (c, b) in arms {
                collect_names_expr(c, set);
                for s in b {
                    collect_names_stmt(s, set);
                }
            }
            if let Some(b) = else_body {
                for s in b {
                    collect_names_stmt(s, set);
                }
            }
        }
        MirStmt::Loop { cond, body } => {
            if let Some(c) = cond {
                collect_names_expr(c, set);
            }
            for s in body {
                collect_names_stmt(s, set);
            }
        }
        MirStmt::ForIn { item, iter, body } => {
            set.insert(item.clone());
            collect_names_expr(iter, set);
            for s in body {
                collect_names_stmt(s, set);
            }
        }
        MirStmt::MatchTagged {
            scrutinee,
            ok_name,
            ok_body,
            err_name,
            err_body,
        } => {
            collect_names_expr(scrutinee, set);
            if let Some(n) = ok_name {
                set.insert(n.clone());
            }
            if let Some(n) = err_name {
                set.insert(n.clone());
            }
            for s in ok_body {
                collect_names_stmt(s, set);
            }
            for s in err_body {
                collect_names_stmt(s, set);
            }
        }
        MirStmt::TaskSpawn { bind, .. } | MirStmt::TaskSpawnFn { bind, .. } => {
            if let Some(n) = bind {
                set.insert(n.clone());
            }
        }
        MirStmt::TaskJoin { handle, bind, .. } => {
            if let Some(h) = handle {
                collect_names_expr(h, set);
            }
            if let Some(n) = bind {
                set.insert(n.clone());
            }
        }
        MirStmt::ScopeEnter { .. }
        | MirStmt::ScopeExit { .. }
        | MirStmt::ReturnNone
        | MirStmt::Break
        | MirStmt::Continue => {}
    }
}

fn collect_names_expr(e: &MirExpr, set: &mut HashSet<String>) {
    match e {
        MirExpr::Name(n) => {
            set.insert(n.clone());
        }
        MirExpr::Binary { left, right, .. } => {
            collect_names_expr(left, set);
            collect_names_expr(right, set);
        }
        MirExpr::Unary { expr, .. }
        | MirExpr::Cast { expr, .. }
        | MirExpr::BoxValue { value: expr, .. }
        | MirExpr::UnboxValue { value: expr, .. }
        | MirExpr::StructTypeIs { value: expr, .. } => collect_names_expr(expr, set),
        MirExpr::FieldGet { base, .. } => collect_names_expr(base, set),
        MirExpr::Index { base, index } => {
            collect_names_expr(base, set);
            collect_names_expr(index, set);
        }
        MirExpr::Call { args, .. } | MirExpr::PrimCall { args, .. } => {
            for a in args {
                collect_names_expr(a, set);
            }
        }
        MirExpr::ListLit(xs) => {
            for x in xs {
                collect_names_expr(x, set);
            }
        }
        MirExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_names_expr(v, set);
            }
        }
        MirExpr::StringInterp { parts }
        | MirExpr::LocatorInterp { parts }
        | MirExpr::BytesInterp { parts } => {
            for p in parts {
                if let StrPart::Name(n) = p {
                    set.insert(n.clone());
                }
            }
        }
        MirExpr::Range { start, end } => {
            collect_names_expr(start, set);
            collect_names_expr(end, set);
        }
        MirExpr::FnValue { .. }
        | MirExpr::ConstI64(_)
        | MirExpr::ConstI32(_)
        | MirExpr::ConstInt { .. }
        | MirExpr::ConstBool(_)
        | MirExpr::ConstF64(_)
        | MirExpr::ConstF32(_)
        | MirExpr::ConstDuration(_)
        | MirExpr::StringLit { .. }
        | MirExpr::BytesLit { .. }
        | MirExpr::LocatorLit { .. } => {}
    }
}

fn exit_scopes_to_loop(ctx: &Ctx) -> Vec<MirStmt> {
    let mut v = Vec::new();
    let Some(&loop_sid) = ctx.loop_scopes.last() else {
        return v;
    };
    for &id in ctx.open.iter().rev() {
        v.push(MirStmt::ScopeExit { id });
        if id == loop_sid {
            break;
        }
    }
    v
}

enum ReturnKind {
    Ok(MirExpr, Option<echo_source::Span>),
    Err(MirExpr),
    None,
}

fn exit_then_return(ctx: &mut Ctx, ret: Option<ReturnKind>) -> Vec<MirStmt> {
    let mut v = Vec::new();
    // Always evaluate the return expression **before** scope exits.
    // Scalars that only *use* managed names (e.g. `xs[0] + xs[2]`) are not
    // themselves `expr_is_managed`, but still read the heap — materializing
    // first prevents use-after-free when root/open scopes free on exit.
    let ret = match ret {
        Some(ReturnKind::Ok(e, span)) => {
            let managed = expr_is_managed(&e);
            let e = materialize_once(&mut v, e, &mut ctx.next_id, span);
            if managed {
                v.push(MirStmt::ScopeDisown { value: e.clone() });
            }
            Some(ReturnKind::Ok(e, span))
        }
        Some(ReturnKind::Err(e)) => {
            let managed = expr_is_managed(&e);
            let e = materialize_once(&mut v, e, &mut ctx.next_id, None);
            if managed {
                v.push(MirStmt::ScopeDisown { value: e.clone() });
            }
            Some(ReturnKind::Err(e))
        }
        other => other,
    };
    for &id in ctx.open.iter().rev() {
        v.push(MirStmt::ScopeExit { id });
    }
    match ret {
        Some(ReturnKind::Ok(e, span)) => v.push(MirStmt::ReturnOk(e, span)),
        Some(ReturnKind::Err(e)) => v.push(MirStmt::ReturnErr(e)),
        Some(ReturnKind::None) => v.push(MirStmt::ReturnNone),
        None => {}
    }
    v
}

fn materialize_once(
    v: &mut Vec<MirStmt>,
    e: MirExpr,
    next_id: &mut u32,
    span: Option<echo_source::Span>,
) -> MirExpr {
    if let MirExpr::Name(_) = &e {
        return e;
    }
    let n = format!("__ret_{}", *next_id);
    *next_id += 1;
    let is_fresh = expr_is_fresh_alloc(&e);
    v.push(MirStmt::Set {
        name: n.clone(),
        value: e,
        span,
    });
    if is_fresh {
        v.push(MirStmt::ScopeRegister {
            value: MirExpr::Name(n.clone()),
        });
    }
    MirExpr::Name(n)
}

/// True for expressions that create a **new** heap allocation (register once).
#[must_use]
pub fn expr_is_fresh_alloc(e: &MirExpr) -> bool {
    match e {
        MirExpr::ListLit(_)
        | MirExpr::StringLit { .. }
        | MirExpr::BytesLit { .. }
        | MirExpr::LocatorLit { .. }
        | MirExpr::StructLit { .. }
        | MirExpr::Range { .. }
        | MirExpr::FnValue { .. }
        | MirExpr::StringInterp { .. }
        | MirExpr::LocatorInterp { .. }
        | MirExpr::BytesInterp { .. }
        | MirExpr::Call { .. }
        | MirExpr::BoxValue { .. } => true,
        _ => false,
    }
}

/// Conservative: treat heap-bearing surface forms as managed (for promote/disown).
#[must_use]
pub fn expr_is_managed(e: &MirExpr) -> bool {
    if expr_is_fresh_alloc(e) {
        return true;
    }
    match e {
        MirExpr::Name(_) => true,
        MirExpr::PrimCall { prim, .. } => matches!(prim, crate::MirPrim::ListGetChecked),
        MirExpr::FieldGet { .. } | MirExpr::Index { .. } => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{structured_to_cfg, MirRetShape};

    fn walk_promotes(stmts: &[MirStmt], out: &mut Vec<(String, u32)>) {
        for s in stmts {
            match s {
                MirStmt::ScopePromote { value, target } => {
                    let n = match value {
                        MirExpr::Name(n) => n.clone(),
                        _ => "?".into(),
                    };
                    out.push((n, *target));
                }
                MirStmt::If { arms, else_body } => {
                    for (_, b) in arms {
                        walk_promotes(b, out);
                    }
                    if let Some(b) = else_body {
                        walk_promotes(b, out);
                    }
                }
                MirStmt::Loop { body, .. } | MirStmt::ForIn { body, .. } => {
                    walk_promotes(body, out);
                }
                MirStmt::MatchTagged {
                    ok_body, err_body, ..
                } => {
                    walk_promotes(ok_body, out);
                    walk_promotes(err_body, out);
                }
                _ => {}
            }
        }
    }

    fn has_inward_demote(out: &[MirStmt], name: &str) -> bool {
        let mut promotes = Vec::new();
        walk_promotes(out, &mut promotes);
        promotes.iter().any(|(n, t)| n == name && *t != ROOT_SCOPE)
    }

    /// Structural escape-surface table: each shape must refuse demotion of the
    /// nested/escaped name when the container or alias peer is used after.
    /// New MirExpr embeds must extend `managed_names_in_expr` or this table fails.
    #[test]
    fn escape_surface_refuses_demote_when_container_used_after() {
        // Common tail: if { use xs }; return holder (or peer).
        let if_use_xs = MirStmt::If {
            arms: vec![(
                MirExpr::ConstI64(1),
                vec![MirStmt::Eval(MirExpr::Name("xs".into()))],
            )],
            else_body: None,
        };
        let ret_holder = MirStmt::ReturnOk(MirExpr::Name("holder".into()), None);

        let cases: Vec<(&str, Vec<MirStmt>, &str)> = vec![
            (
                "alias_assign",
                vec![
                    MirStmt::Set {
                        name: "holder".into(),
                        value: MirExpr::ListLit(vec![MirExpr::ConstI64(1)]),
                        span: None,
                    },
                    MirStmt::Set {
                        name: "xs".into(),
                        value: MirExpr::Name("holder".into()),
                        span: None,
                    },
                    if_use_xs.clone(),
                    ret_holder.clone(),
                ],
                "xs",
            ),
            (
                "list_push",
                vec![
                    MirStmt::Set {
                        name: "holder".into(),
                        value: MirExpr::ListLit(vec![]),
                        span: None,
                    },
                    MirStmt::Set {
                        name: "xs".into(),
                        value: MirExpr::ListLit(vec![MirExpr::ConstI64(7)]),
                        span: None,
                    },
                    MirStmt::ListPush {
                        base: MirExpr::Name("holder".into()),
                        value: MirExpr::Name("xs".into()),
                    },
                    if_use_xs.clone(),
                    ret_holder.clone(),
                ],
                "xs",
            ),
            (
                "field_set",
                vec![
                    MirStmt::Set {
                        name: "holder".into(),
                        value: MirExpr::StructLit {
                            type_name: String::new(),
                            fields: vec![],
                        },
                        span: None,
                    },
                    MirStmt::Set {
                        name: "xs".into(),
                        value: MirExpr::ListLit(vec![]),
                        span: None,
                    },
                    MirStmt::FieldSet {
                        base: MirExpr::Name("holder".into()),
                        field: "f".into(),
                        value: MirExpr::Name("xs".into()),
                    },
                    if_use_xs.clone(),
                    ret_holder.clone(),
                ],
                "xs",
            ),
            (
                "index_set",
                vec![
                    MirStmt::Set {
                        name: "holder".into(),
                        value: MirExpr::ListLit(vec![MirExpr::ConstI64(0)]),
                        span: None,
                    },
                    MirStmt::Set {
                        name: "xs".into(),
                        value: MirExpr::ListLit(vec![MirExpr::ConstI64(7)]),
                        span: None,
                    },
                    MirStmt::IndexSet {
                        base: MirExpr::Name("holder".into()),
                        index: MirExpr::ConstI64(0),
                        value: MirExpr::Name("xs".into()),
                    },
                    if_use_xs.clone(),
                    ret_holder.clone(),
                ],
                "xs",
            ),
            (
                "listlit_nest",
                vec![
                    MirStmt::Set {
                        name: "xs".into(),
                        value: MirExpr::ListLit(vec![MirExpr::ConstI64(7)]),
                        span: None,
                    },
                    MirStmt::Set {
                        name: "holder".into(),
                        value: MirExpr::ListLit(vec![MirExpr::Name("xs".into())]),
                        span: None,
                    },
                    if_use_xs.clone(),
                    ret_holder.clone(),
                ],
                "xs",
            ),
            (
                "structlit_nest",
                vec![
                    MirStmt::Set {
                        name: "xs".into(),
                        value: MirExpr::ListLit(vec![MirExpr::ConstI64(7)]),
                        span: None,
                    },
                    MirStmt::Set {
                        name: "holder".into(),
                        value: MirExpr::StructLit {
                            type_name: String::new(),
                            fields: vec![("f".into(), MirExpr::Name("xs".into()))],
                        },
                        span: None,
                    },
                    if_use_xs.clone(),
                    ret_holder.clone(),
                ],
                "xs",
            ),
            (
                "listlit_nested_list",
                vec![
                    MirStmt::Set {
                        name: "xs".into(),
                        value: MirExpr::ListLit(vec![MirExpr::ConstI64(7)]),
                        span: None,
                    },
                    MirStmt::Set {
                        name: "holder".into(),
                        value: MirExpr::ListLit(vec![MirExpr::ListLit(vec![MirExpr::Name(
                            "xs".into(),
                        )])]),
                        span: None,
                    },
                    if_use_xs.clone(),
                    ret_holder.clone(),
                ],
                "xs",
            ),
        ];

        for (label, body, escaped) in cases {
            let out = inject_lifetime(body);
            assert!(
                !has_inward_demote(&out, escaped),
                "escape shape {label}: must not demote {escaped} when holder used after\n{out:?}"
            );
        }
    }

    #[test]
    fn managed_names_in_expr_walks_listlit_and_structlit() {
        let e = MirExpr::ListLit(vec![
            MirExpr::Name("a".into()),
            MirExpr::StructLit {
                type_name: String::new(),
                fields: vec![("f".into(), MirExpr::Name("b".into()))],
            },
        ]);
        let names = managed_names_in_expr(&e);
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }

    #[test]
    fn inject_preserves_set_and_return_spans() {
        use echo_source::{BytePos, SourceId, Span};
        let span = Span::new(SourceId::from_u32(0), BytePos(8), BytePos(12));
        let out = inject_lifetime(vec![
            MirStmt::Set {
                name: "n".into(),
                value: MirExpr::ConstI64(1),
                span: Some(span),
            },
            MirStmt::ReturnOk(MirExpr::Name("n".into()), Some(span)),
        ]);
        assert!(
            out.iter().any(|s| matches!(
                s,
                MirStmt::Set { name, span: Some(sp), .. } if name == "n" && *sp == span
            )),
            "Set span must survive inject_lifetime: {out:?}"
        );
        assert!(
            out.iter()
                .any(|s| matches!(s, MirStmt::ReturnOk(_, Some(sp)) if *sp == span)),
            "ReturnOk span must survive inject_lifetime: {out:?}"
        );
    }

    #[test]
    fn no_demote_when_body_nests_into_outer_holder() {
        // xs unique at root; if body does holder = [xs]; holder used after.
        // Pre-scan must see the in-body nest and refuse demote of xs (else
        // demote→if + exit frees xs while holder at root still holds it).
        let body = vec![
            MirStmt::Set {
                name: "xs".into(),
                value: MirExpr::ListLit(vec![MirExpr::ConstI64(7)]),
                span: None,
            },
            MirStmt::Set {
                name: "holder".into(),
                value: MirExpr::ListLit(vec![]),
                span: None,
            },
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![MirStmt::Set {
                        name: "holder".into(),
                        value: MirExpr::ListLit(vec![MirExpr::Name("xs".into())]),
                        span: None,
                    }],
                )],
                else_body: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("holder".into()), None),
        ];
        let out = inject_lifetime(body);
        assert!(
            !has_inward_demote(&out, "xs"),
            "in-body ListLit nest into outer holder must refuse demote of xs\n{out:?}"
        );
        // Belt: inject must promote xs toward holder/root on the nest assign path.
        let mut promotes = Vec::new();
        walk_promotes(&out, &mut promotes);
        assert!(
            promotes.iter().any(|(n, t)| n == "xs" && *t == ROOT_SCOPE),
            "nest assign must ScopePromote xs to holder owner (root): {promotes:?}"
        );
    }

    #[test]
    fn no_demote_when_body_list_push_into_outer_holder() {
        let body = vec![
            MirStmt::Set {
                name: "xs".into(),
                value: MirExpr::ListLit(vec![MirExpr::ConstI64(7)]),
                span: None,
            },
            MirStmt::Set {
                name: "holder".into(),
                value: MirExpr::ListLit(vec![]),
                span: None,
            },
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![MirStmt::ListPush {
                        base: MirExpr::Name("holder".into()),
                        value: MirExpr::Name("xs".into()),
                    }],
                )],
                else_body: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("holder".into()), None),
        ];
        let out = inject_lifetime(body);
        assert!(
            !has_inward_demote(&out, "xs"),
            "in-body ListPush into outer holder must refuse demote of xs\n{out:?}"
        );
    }

    #[test]
    fn escape_surface_includes_in_body_nest_shapes() {
        // Table extension: nest/store happens *inside* demote region.
        let cases: Vec<(&str, Vec<MirStmt>)> = vec![
            (
                "in_body_listlit_nest",
                vec![
                    MirStmt::Set {
                        name: "xs".into(),
                        value: MirExpr::ListLit(vec![MirExpr::ConstI64(1)]),
                        span: None,
                    },
                    MirStmt::Set {
                        name: "holder".into(),
                        value: MirExpr::ListLit(vec![]),
                        span: None,
                    },
                    MirStmt::If {
                        arms: vec![(
                            MirExpr::ConstI64(1),
                            vec![MirStmt::Set {
                                name: "holder".into(),
                                value: MirExpr::ListLit(vec![MirExpr::Name("xs".into())]),
                                span: None,
                            }],
                        )],
                        else_body: None,
                    },
                    MirStmt::ReturnOk(MirExpr::Name("holder".into()), None),
                ],
            ),
            (
                "in_body_structlit_nest",
                vec![
                    MirStmt::Set {
                        name: "xs".into(),
                        value: MirExpr::ListLit(vec![MirExpr::ConstI64(1)]),
                        span: None,
                    },
                    MirStmt::Set {
                        name: "holder".into(),
                        value: MirExpr::StructLit {
                            type_name: String::new(),
                            fields: vec![],
                        },
                        span: None,
                    },
                    MirStmt::If {
                        arms: vec![(
                            MirExpr::ConstI64(1),
                            vec![MirStmt::Set {
                                name: "holder".into(),
                                value: MirExpr::StructLit {
                                    type_name: String::new(),
                                    fields: vec![("f".into(), MirExpr::Name("xs".into()))],
                                },
                                span: None,
                            }],
                        )],
                        else_body: None,
                    },
                    MirStmt::ReturnOk(MirExpr::Name("holder".into()), None),
                ],
            ),
            (
                "in_body_field_set",
                vec![
                    MirStmt::Set {
                        name: "xs".into(),
                        value: MirExpr::ListLit(vec![]),
                        span: None,
                    },
                    MirStmt::Set {
                        name: "holder".into(),
                        value: MirExpr::StructLit {
                            type_name: String::new(),
                            fields: vec![],
                        },
                        span: None,
                    },
                    MirStmt::If {
                        arms: vec![(
                            MirExpr::ConstI64(1),
                            vec![MirStmt::FieldSet {
                                base: MirExpr::Name("holder".into()),
                                field: "f".into(),
                                value: MirExpr::Name("xs".into()),
                            }],
                        )],
                        else_body: None,
                    },
                    MirStmt::ReturnOk(MirExpr::Name("holder".into()), None),
                ],
            ),
            (
                "in_body_index_set",
                vec![
                    MirStmt::Set {
                        name: "xs".into(),
                        value: MirExpr::ListLit(vec![MirExpr::ConstI64(9)]),
                        span: None,
                    },
                    MirStmt::Set {
                        name: "holder".into(),
                        value: MirExpr::ListLit(vec![MirExpr::ConstI64(0)]),
                        span: None,
                    },
                    MirStmt::If {
                        arms: vec![(
                            MirExpr::ConstI64(1),
                            vec![MirStmt::IndexSet {
                                base: MirExpr::Name("holder".into()),
                                index: MirExpr::ConstI64(0),
                                value: MirExpr::Name("xs".into()),
                            }],
                        )],
                        else_body: None,
                    },
                    MirStmt::ReturnOk(MirExpr::Name("holder".into()), None),
                ],
            ),
        ];
        for (label, body) in cases {
            let out = inject_lifetime(body);
            assert!(
                !has_inward_demote(&out, "xs"),
                "in-body escape {label}: must not demote xs\n{out:?}"
            );
        }
    }

    #[test]
    fn precise_promote_to_outer_bind_not_always_root() {
        // holder @ root; nested if assigns holder ← xs. Promote target is root (holder owner).
        let body = vec![
            MirStmt::Set {
                name: "holder".into(),
                value: MirExpr::ListLit(vec![]),
                span: None,
            },
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![
                        MirStmt::Set {
                            name: "xs".into(),
                            value: MirExpr::ListLit(vec![MirExpr::ConstI64(1)]),
                            span: None,
                        },
                        MirStmt::Set {
                            name: "holder".into(),
                            value: MirExpr::Name("xs".into()),
                            span: None,
                        },
                    ],
                )],
                else_body: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("holder".into()), None),
        ];
        let out = inject_lifetime(body);
        let mut promotes = Vec::new();
        walk_promotes(&out, &mut promotes);
        assert!(
            promotes
                .iter()
                .any(|(n, t)| n == "holder" && *t == ROOT_SCOPE),
            "holder reassign must promote to root: {promotes:?}\n{out:?}"
        );
    }

    #[test]
    fn nested_local_stays_registered_without_root_promote() {
        let body = vec![
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![MirStmt::Set {
                        name: "ys".into(),
                        value: MirExpr::ListLit(vec![]),
                        span: None,
                    }],
                )],
                else_body: None,
            },
            MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
        ];
        let out = inject_lifetime(body);
        let mut promotes = Vec::new();
        walk_promotes(&out, &mut promotes);
        assert!(
            promotes.iter().all(|(n, _)| n != "ys"),
            "local ys must not be promoted: {promotes:?}"
        );
    }

    #[test]
    fn demote_into_loop_disabled_under_immediate_free() {
        // Loop demote was disabled with if-arm demote: name-keyed inward
        // promote is unsound under immediate free; graph promote handles escape.
        let body = vec![
            MirStmt::Set {
                name: "xs".into(),
                value: MirExpr::ListLit(vec![MirExpr::ConstI64(1)]),
                span: None,
            },
            MirStmt::Loop {
                cond: Some(MirExpr::ConstI64(0)),
                body: vec![MirStmt::Eval(MirExpr::Name("xs".into()))],
            },
            MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
        ];
        let out = inject_lifetime(body);
        let mut promotes = Vec::new();
        walk_promotes(&out, &mut promotes);
        assert!(
            !promotes.iter().any(|(n, t)| n == "xs" && *t != ROOT_SCOPE),
            "loop demote disabled: unexpected promote of xs: {promotes:?}\n{out:?}"
        );
        assert!(
            out.iter()
                .any(|s| matches!(s, MirStmt::ScopeEnter { id: 1 })),
            "loop still wrapped in once-scope: {out:?}"
        );
    }

    #[test]
    fn no_demote_when_used_after_loop() {
        let body = vec![
            MirStmt::Set {
                name: "xs".into(),
                value: MirExpr::ListLit(vec![MirExpr::ConstI64(1)]),
                span: None,
            },
            MirStmt::Loop {
                cond: Some(MirExpr::ConstI64(0)),
                body: vec![MirStmt::Eval(MirExpr::Name("xs".into()))],
            },
            MirStmt::ReturnOk(MirExpr::Name("xs".into()), None),
        ];
        let out = inject_lifetime(body);
        let mut promotes = Vec::new();
        walk_promotes(&out, &mut promotes);
        assert!(
            !promotes.iter().any(|(n, t)| n == "xs" && *t != ROOT_SCOPE),
            "must not demote xs when used after: {promotes:?}"
        );
    }

    #[test]
    fn no_demote_when_aliased_peer_used_after() {
        // $ a = [10, 20]; ~ b = a; if { use b }; use a after
        // Demoting b would free a's handle under immediate free → UAF.
        let body = vec![
            MirStmt::Set {
                name: "a".into(),
                value: MirExpr::ListLit(vec![MirExpr::ConstI64(10), MirExpr::ConstI64(20)]),
                span: None,
            },
            MirStmt::Set {
                name: "b".into(),
                value: MirExpr::Name("a".into()),
                span: None,
            },
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![MirStmt::Eval(MirExpr::Name("b".into()))],
                )],
                else_body: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("a".into()), None),
        ];
        let out = inject_lifetime(body);
        let mut promotes = Vec::new();
        walk_promotes(&out, &mut promotes);
        assert!(
            !promotes
                .iter()
                .any(|(n, t)| (n == "a" || n == "b") && *t != ROOT_SCOPE),
            "must not demote aliased a/b when a used after: {promotes:?}\n{out:?}"
        );
    }

    #[test]
    fn no_demote_after_list_push_into_holder() {
        // holder=[]; xs=[7]; ListPush(holder, xs); if { use xs }; return holder
        // Demoting xs would free the element while holder still holds it.
        let body = vec![
            MirStmt::Set {
                name: "holder".into(),
                value: MirExpr::ListLit(vec![]),
                span: None,
            },
            MirStmt::Set {
                name: "xs".into(),
                value: MirExpr::ListLit(vec![MirExpr::ConstI64(7)]),
                span: None,
            },
            MirStmt::ListPush {
                base: MirExpr::Name("holder".into()),
                value: MirExpr::Name("xs".into()),
            },
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![MirStmt::Eval(MirExpr::Name("xs".into()))],
                )],
                else_body: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("holder".into()), None),
        ];
        let out = inject_lifetime(body);
        let mut promotes = Vec::new();
        walk_promotes(&out, &mut promotes);
        assert!(
            !promotes.iter().any(|(n, t)| n == "xs" && *t != ROOT_SCOPE),
            "must not demote xs after ListPush into holder: {promotes:?}\n{out:?}"
        );
    }

    #[test]
    fn return_scalar_from_index_materializes_before_scope_exit() {
        // `$ xs = [7, 8, 9]; ^ xs[0] + xs[2]` — Binary is not managed, but must
        // still evaluate while `xs` is live (before root ScopeExit).
        let body = vec![
            MirStmt::Set {
                name: "xs".into(),
                value: MirExpr::ListLit(vec![
                    MirExpr::ConstI64(7),
                    MirExpr::ConstI64(8),
                    MirExpr::ConstI64(9),
                ]),
                span: None,
            },
            MirStmt::ReturnOk(
                MirExpr::Binary {
                    op: echo_ast::BinaryOp::Add,
                    left: Box::new(MirExpr::Index {
                        base: Box::new(MirExpr::Name("xs".into())),
                        index: Box::new(MirExpr::ConstI64(0)),
                    }),
                    right: Box::new(MirExpr::Index {
                        base: Box::new(MirExpr::Name("xs".into())),
                        index: Box::new(MirExpr::ConstI64(2)),
                    }),
                },
                None,
            ),
        ];
        let out = inject_lifetime(body);
        // Find root ScopeExit then ReturnOk — a Set of the return temp must
        // appear before that exit.
        let mut saw_ret_set = false;
        let mut exit_before_eval = false;
        for s in &out {
            match s {
                MirStmt::Set { name, value, .. }
                    if name.starts_with("__ret_") && matches!(value, MirExpr::Binary { .. }) =>
                {
                    saw_ret_set = true;
                }
                MirStmt::ScopeExit { id } if *id == ROOT_SCOPE => {
                    if !saw_ret_set {
                        exit_before_eval = true;
                    }
                }
                MirStmt::ReturnOk(..) => {}
                _ => {}
            }
        }
        assert!(
            saw_ret_set && !exit_before_eval,
            "return binary must Set temp before root ScopeExit\n{out:?}"
        );
    }

    #[test]
    fn no_demote_after_field_set_escape() {
        // s = struct; xs = []; FieldSet(s, f, xs); if { use xs }; return s
        let body = vec![
            MirStmt::Set {
                name: "s".into(),
                value: MirExpr::StructLit {
                    type_name: String::new(),
                    fields: vec![],
                },
                span: None,
            },
            MirStmt::Set {
                name: "xs".into(),
                value: MirExpr::ListLit(vec![]),
                span: None,
            },
            MirStmt::FieldSet {
                base: MirExpr::Name("s".into()),
                field: "f".into(),
                value: MirExpr::Name("xs".into()),
            },
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![MirStmt::Eval(MirExpr::Name("xs".into()))],
                )],
                else_body: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("s".into()), None),
        ];
        let out = inject_lifetime(body);
        let mut promotes = Vec::new();
        walk_promotes(&out, &mut promotes);
        assert!(
            !promotes.iter().any(|(n, t)| n == "xs" && *t != ROOT_SCOPE),
            "must not demote xs after FieldSet into s: {promotes:?}\n{out:?}"
        );
    }

    #[test]
    fn no_demote_alias_even_if_peer_unused_after_name_only() {
        // b used in if, a never used after — still unsafe to demote only by name-use
        // of b without unique ownership (a remains a live name holding the handle).
        // We refuse demotion of any non-unique name.
        let body = vec![
            MirStmt::Set {
                name: "a".into(),
                value: MirExpr::ListLit(vec![MirExpr::ConstI64(1)]),
                span: None,
            },
            MirStmt::Set {
                name: "b".into(),
                value: MirExpr::Name("a".into()),
                span: None,
            },
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![MirStmt::Eval(MirExpr::Name("b".into()))],
                )],
                else_body: None,
            },
            MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
        ];
        let out = inject_lifetime(body);
        let mut promotes = Vec::new();
        walk_promotes(&out, &mut promotes);
        assert!(
            !promotes
                .iter()
                .any(|(n, t)| (n == "a" || n == "b") && *t != ROOT_SCOPE),
            "aliased names must not demote without unique ownership: {promotes:?}"
        );
    }

    #[test]
    fn return_err_exits_all_open_scopes() {
        let body = vec![MirStmt::If {
            arms: vec![(
                MirExpr::ConstI64(1),
                vec![MirStmt::ReturnErr(MirExpr::StringLit {
                    bytes: b"e".to_vec(),
                })],
            )],
            else_body: None,
        }];
        let out = inject_lifetime(body);
        let arm = match &out[1] {
            MirStmt::If { arms, .. } => &arms[0].1,
            o => panic!("expected If, got {o:?}"),
        };
        let exits: Vec<u32> = arm
            .iter()
            .filter_map(|s| match s {
                MirStmt::ScopeExit { id } => Some(*id),
                _ => None,
            })
            .collect();
        assert!(
            exits.len() >= 2,
            "return err must exit if + root: arm={arm:?}"
        );
        assert!(exits.contains(&ROOT_SCOPE));
        assert!(arm.iter().any(|s| matches!(s, MirStmt::ReturnErr(_))));
    }

    #[test]
    fn break_exits_if_and_loop_scopes_in_cfg() {
        let body = vec![
            MirStmt::Loop {
                cond: None,
                body: vec![MirStmt::If {
                    arms: vec![(MirExpr::ConstI64(1), vec![MirStmt::Break])],
                    else_body: None,
                }],
            },
            MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
        ];
        let out = inject_lifetime(body);
        let arm = {
            fn find_break_arm(stmts: &[MirStmt]) -> Option<&[MirStmt]> {
                for s in stmts {
                    match s {
                        MirStmt::If { arms, .. } => {
                            for (_, b) in arms {
                                if b.iter().any(|x| matches!(x, MirStmt::Break)) {
                                    return Some(b.as_slice());
                                }
                                if let Some(a) = find_break_arm(b) {
                                    return Some(a);
                                }
                            }
                        }
                        MirStmt::Loop { body, .. } | MirStmt::ForIn { body, .. } => {
                            if let Some(a) = find_break_arm(body) {
                                return Some(a);
                            }
                        }
                        _ => {}
                    }
                }
                None
            }
            find_break_arm(&out).expect("break arm")
        };
        assert!(
            arm.iter().any(|s| matches!(s, MirStmt::ScopeExit { .. })),
            "arm missing ScopeExit before break: {arm:?}"
        );
        assert!(arm.iter().any(|s| matches!(s, MirStmt::Break)));

        let cfg = structured_to_cfg(&out, MirRetShape::Plain);
        // Any block that enters a non-root scope and breaks must also exit that scope.
        let mut ok = false;
        for b in &cfg.blocks {
            let enters: Vec<u32> = b
                .ops
                .iter()
                .filter_map(|op| match op {
                    crate::MirOp::ScopeEnter { id } if *id != 0 => Some(*id),
                    _ => None,
                })
                .collect();
            for id in enters {
                let has_exit = b
                    .ops
                    .iter()
                    .any(|op| matches!(op, crate::MirOp::ScopeExit { id: e } if *e == id));
                if has_exit {
                    ok = true;
                }
            }
        }
        assert!(ok, "expected enter+exit of nested scope on cfg path");

        let cfg2 = crate::construct_ssa(cfg, &[]);
        let (cfg2, reprs) = crate::analyze_reprs(cfg2, &[]);
        let (cfg2, _reprs) = crate::simplify_local(cfg2, reprs);
        let mut ok2 = false;
        for b in &cfg2.blocks {
            for op in &b.ops {
                if let crate::MirOp::ScopeEnter { id } = op {
                    if *id != 0
                        && b.ops
                            .iter()
                            .any(|o| matches!(o, crate::MirOp::ScopeExit { id: e } if e == id))
                    {
                        ok2 = true;
                    }
                }
            }
        }
        assert!(ok2, "ScopeExit stripped after SSA/simplify");
    }

    #[test]
    fn inject_nested_for_in_return_versions_indices() {
        use crate::cfg::structured_to_cfg;
        use crate::ssa::construct_ssa;
        use crate::{inject_lifetime, MirExpr, MirRetShape, MirStmt};
        let body = inject_lifetime(vec![
            MirStmt::ForIn {
                item: "chain".into(),
                iter: MirExpr::Name("buckets".into()),
                body: vec![MirStmt::ForIn {
                    item: "e".into(),
                    iter: MirExpr::Name("chain".into()),
                    body: vec![MirStmt::ReturnOk(MirExpr::ConstBool(true), None)],
                }],
            },
            MirStmt::ReturnOk(MirExpr::ConstBool(false), None),
        ]);
        let cfg = structured_to_cfg(&body, MirRetShape::Plain);
        let ssa = construct_ssa(cfg, &["buckets".into()]);
        fn walk_expr(e: &MirExpr, bad: &mut Vec<String>) {
            match e {
                MirExpr::Name(n) if n.starts_with("__i_") && !n.contains('@') => {
                    bad.push(n.clone())
                }
                MirExpr::Binary { left, right, .. } => {
                    walk_expr(left, bad);
                    walk_expr(right, bad);
                }
                MirExpr::PrimCall { args, .. } => {
                    for a in args {
                        walk_expr(a, bad);
                    }
                }
                MirExpr::Unary { expr, .. } | MirExpr::Cast { expr, .. } => walk_expr(expr, bad),
                _ => {}
            }
        }
        let mut bad = Vec::new();
        for b in &ssa.blocks {
            for op in &b.ops {
                match op {
                    crate::MirOp::Set { value, .. } => walk_expr(value, &mut bad),
                    crate::MirOp::ScopeRegister { value }
                    | crate::MirOp::ScopePromote { value, .. }
                    | crate::MirOp::ScopeDisown { value }
                    | crate::MirOp::ScopeRelease { value } => walk_expr(value, &mut bad),
                    crate::MirOp::Eval(e) => walk_expr(e, &mut bad),
                    _ => {}
                }
            }
            match &b.term {
                crate::Terminator::Branch { cond, .. } => walk_expr(cond, &mut bad),
                crate::Terminator::ReturnOk(e, _) | crate::Terminator::ReturnErr(e) => {
                    walk_expr(e, &mut bad)
                }
                _ => {}
            }
        }
        assert!(
            bad.is_empty(),
            "unversioned __i_* after SSA: {bad:?}\ncfg={ssa:#?}"
        );
    }

    #[test]
    fn inject_wraps_root_and_if() {
        let body = vec![
            MirStmt::Set {
                name: "xs".into(),
                value: MirExpr::ListLit(vec![]),
                span: None,
            },
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![MirStmt::Set {
                        name: "ys".into(),
                        value: MirExpr::ListLit(vec![]),
                        span: None,
                    }],
                )],
                else_body: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("xs".into()), None),
        ];
        let out = inject_lifetime(body);
        assert!(matches!(out.first(), Some(MirStmt::ScopeEnter { id: 0 })));
        assert!(out
            .iter()
            .any(|s| matches!(s, MirStmt::ScopeRegister { .. })));
        let has_arm_scope = out.iter().any(|s| match s {
            MirStmt::If { arms, .. } => arms
                .iter()
                .any(|(_, b)| b.iter().any(|x| matches!(x, MirStmt::ScopeEnter { id: 1 }))),
            _ => false,
        });
        assert!(has_arm_scope);
        assert!(out.iter().any(|s| matches!(s, MirStmt::ScopeDisown { .. })));
        assert!(out
            .iter()
            .any(|s| matches!(s, MirStmt::ScopeExit { id: 0 })));
    }

    #[test]
    fn task_spawn_bind_is_scope_registered() {
        use std::path::PathBuf;
        let body = vec![
            MirStmt::TaskSpawn {
                module_path: PathBuf::from("t.echo"),
                body_symbol: "job".into(),
                bind: Some("h".into()),
            },
            MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
        ];
        let out = inject_lifetime(body);
        assert!(
            out.iter().any(|s| matches!(
                s,
                MirStmt::ScopeRegister {
                    value: MirExpr::Name(n)
                } if n == "h"
            )),
            "task handle must be registered: {out:?}"
        );
    }

    #[test]
    fn call_args_are_not_promoted_as_nested_into_result() {
        use crate::{CallTarget, MirExpr, MirRetShape, MirStmt};
        use std::path::PathBuf;
        // `$ st = f(path)` must register `st` but must NOT ScopePromote `path`.
        // Promoting call args re-homed the caller's string into the callee and
        // freed it on ScopeExit (fs.create_dir_all(root) wiped root).
        let body = vec![
            MirStmt::Set {
                name: "path".into(),
                value: MirExpr::StringLit {
                    bytes: b"/tmp/x".to_vec(),
                },
                span: None,
            },
            MirStmt::Set {
                name: "st".into(),
                value: MirExpr::Call {
                    target: CallTarget::Runtime {
                        export: "fs_create_dir_all".into(),
                    },
                    args: vec![MirExpr::Name("path".into())],
                    ret: MirRetShape::Plain,
                },
                span: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("path".into()), None),
        ];
        let out = inject_lifetime(body);
        let promotes_path = out.iter().any(|s| match s {
            MirStmt::ScopePromote {
                value: MirExpr::Name(n),
                ..
            } if n == "path" => true,
            _ => false,
        });
        assert!(
            !promotes_path,
            "must not promote call-arg path into result ownership: {out:?}"
        );
        assert!(nested_owned_names_in_expr(&MirExpr::Call {
            target: CallTarget::Function {
                module_path: PathBuf::from("m"),
                name: "f".into(),
            },
            args: vec![MirExpr::Name("path".into())],
            ret: MirRetShape::Plain,
        })
        .is_empty());
    }

    #[test]
    fn create_dir_all_shape_does_not_promote_path() {
        use crate::{CallTarget, MirExpr, MirRetShape, MirStmt};
        use echo_ast::BinaryOp;
        // Mirrors std/fs create_dir_all:
        //   $ st = runtime.fs_create_dir_all(path)
        //   ? st < 0 { ! "..." }
        //   ^
        let body = vec![
            MirStmt::Set {
                name: "st".into(),
                value: MirExpr::Call {
                    target: CallTarget::Runtime {
                        export: "fs_create_dir_all".into(),
                    },
                    args: vec![MirExpr::Name("path".into())],
                    ret: MirRetShape::Plain,
                },
                span: None,
            },
            MirStmt::If {
                arms: vec![(
                    MirExpr::Binary {
                        op: BinaryOp::Lt,
                        left: Box::new(MirExpr::Name("st".into())),
                        right: Box::new(MirExpr::ConstI64(0)),
                    },
                    vec![MirStmt::ReturnErr(MirExpr::StringLit {
                        bytes: b"create_dir_all failed".to_vec(),
                    })],
                )],
                else_body: None,
            },
            MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
        ];
        let out = inject_lifetime(body);
        let promotes: Vec<_> = out
            .iter()
            .filter_map(|s| match s {
                MirStmt::ScopePromote {
                    value: MirExpr::Name(n),
                    target,
                } => Some((n.clone(), *target)),
                _ => None,
            })
            .collect();
        assert!(
            !promotes.iter().any(|(n, _)| n == "path"),
            "create_dir_all shape must not promote path: {promotes:?}\nout={out:?}"
        );
    }

    #[test]
    fn return_call_materializes_once() {
        use crate::{CallTarget, MirExpr, MirRetShape, MirStmt};
        use std::path::PathBuf;
        let body = vec![MirStmt::ReturnOk(
            MirExpr::Call {
                target: CallTarget::Function {
                    module_path: PathBuf::from("m"),
                    name: "keys".into(),
                },
                args: vec![MirExpr::Name("t".into())],
                ret: MirRetShape::Plain,
            },
            None,
        )];
        let out = inject_lifetime(body);
        let sets: Vec<_> = out
            .iter()
            .filter_map(|s| match s {
                MirStmt::Set { name, value, .. } => Some((name.clone(), value.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(sets.len(), 1, "expected one temp bind, got {out:?}");
        assert!(sets[0].0.starts_with("__ret_"), "{:?}", sets[0].0);
        let returns: Vec<_> = out
            .iter()
            .filter(|s| matches!(s, MirStmt::ReturnOk(..)))
            .collect();
        assert_eq!(returns.len(), 1);
        match &returns[0] {
            MirStmt::ReturnOk(MirExpr::Name(n), _) => assert_eq!(n, &sets[0].0),
            other => panic!("return should be name, got {other:?}"),
        }
        fn count_calls(e: &MirExpr) -> usize {
            match e {
                MirExpr::Call { args, .. } => 1 + args.iter().map(count_calls).sum::<usize>(),
                MirExpr::FieldGet { base, .. } => count_calls(base),
                _ => 0,
            }
        }
        let mut n_calls = 0;
        for s in &out {
            match s {
                MirStmt::Set { value, .. } => n_calls += count_calls(value),
                MirStmt::ScopeDisown { value } => n_calls += count_calls(value),
                MirStmt::ReturnOk(e, _) | MirStmt::ReturnErr(e) => n_calls += count_calls(e),
                _ => {}
            }
        }
        assert_eq!(n_calls, 1, "call must appear once, out={out:?}");
    }

    #[test]
    fn field_store_promotes_to_base_bind_scope() {
        let body = vec![
            MirStmt::Set {
                name: "holder".into(),
                value: MirExpr::ListLit(vec![]),
                span: None,
            },
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![MirStmt::ListPush {
                        base: MirExpr::Name("holder".into()),
                        value: MirExpr::ListLit(vec![MirExpr::ConstI64(9)]),
                    }],
                )],
                else_body: None,
            },
            MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
        ];
        let out = inject_lifetime(body);
        let mut promotes = Vec::new();
        walk_promotes(&out, &mut promotes);
        assert!(
            promotes.iter().any(|(_, t)| *t == ROOT_SCOPE),
            "list push into root holder must promote to root: {promotes:?}\n{out:?}"
        );
    }

    #[test]
    fn mid_scope_bind_promote_not_forced_to_root() {
        // Introduce holder inside outer if (scope 1); reassign from nested if (scope 2).
        // Promote target must be scope 1, not root.
        let body = vec![MirStmt::If {
            arms: vec![(
                MirExpr::ConstI64(1),
                vec![
                    MirStmt::Set {
                        name: "holder".into(),
                        value: MirExpr::ListLit(vec![]),
                        span: None,
                    },
                    MirStmt::If {
                        arms: vec![(
                            MirExpr::ConstI64(1),
                            vec![
                                MirStmt::Set {
                                    name: "xs".into(),
                                    value: MirExpr::ListLit(vec![MirExpr::ConstI64(1)]),
                                    span: None,
                                },
                                MirStmt::Set {
                                    name: "holder".into(),
                                    value: MirExpr::Name("xs".into()),
                                    span: None,
                                },
                            ],
                        )],
                        else_body: None,
                    },
                ],
            )],
            else_body: None,
        }];
        let out = inject_lifetime(body);
        let mut promotes = Vec::new();
        walk_promotes(&out, &mut promotes);
        // holder introduced at scope 1; promote of holder should target 1, not 0.
        let holder_targets: Vec<u32> = promotes
            .iter()
            .filter(|(n, _)| n == "holder")
            .map(|(_, t)| *t)
            .collect();
        assert!(
            holder_targets.iter().any(|t| *t == 1),
            "holder promote must target mid scope 1, got {holder_targets:?} all={promotes:?}"
        );
        assert!(
            !holder_targets.iter().any(|t| *t == ROOT_SCOPE),
            "holder must not force-promote to root when bind lives at scope 1: {holder_targets:?}"
        );
    }
}
