//! Local Echo ABI cleanup after representation analysis.
//!
//! Primary job: cancel redundant **box/unbox** pairs and related local copies so
//! codegen does not emit opaque barriers LLVM cannot see through. Light copy/φ
//! cleanup supports that — not a generic constprop/GVN/LICM mid-end (LLVM owns
//! residual scalar opts via `default<On>`).
//!
//! Does **not** remove conversions that cross a real ABI boundary.
//!
//! Note: MIR ops/exprs do not currently carry `Span`; when spans are added,
//! this pass must rewrite in place (not rebuild nodes without provenance).

use std::collections::{HashMap, HashSet};

use crate::cfg::{MirCfg, MirOp, Terminator};
use crate::repr::MirRepr;
use crate::{CallTarget, MirExpr, StrPart};

/// Local simplification + dead pure conversion elimination.
#[must_use]
pub fn simplify_local(
    mut cfg: MirCfg,
    mut reprs: HashMap<String, MirRepr>,
) -> (MirCfg, HashMap<String, MirRepr>) {
    for _ in 0..32 {
        let mut changed = false;
        changed |= fold_exprs(&mut cfg);
        changed |= propagate_and_collapse(&mut cfg, &mut reprs);
        changed |= dce_pure(&mut cfg, &reprs);
        if !changed {
            break;
        }
    }
    // Refresh facts after rewrites (names removed / aliases).
    re_infer_light(&cfg, &mut reprs);
    (cfg, reprs)
}

/// Fold Unbox∘Box / Box∘Unbox and identity conversions in every expression.
fn fold_exprs(cfg: &mut MirCfg) -> bool {
    let mut changed = false;
    for b in &mut cfg.blocks {
        for op in &mut b.ops {
            match op {
                MirOp::Set { value, .. } => {
                    let n = simplify_expr(value.clone());
                    if !expr_eq(&n, value) {
                        *value = n;
                        changed = true;
                    }
                }
                MirOp::Eval(value) => {
                    let n = simplify_expr(value.clone());
                    if !expr_eq(&n, value) {
                        *value = n;
                        changed = true;
                    }
                }
                MirOp::FieldSet { base, value, .. } => {
                    let nb = simplify_expr(base.clone());
                    let nv = simplify_expr(value.clone());
                    if !expr_eq(&nb, base) || !expr_eq(&nv, value) {
                        *base = nb;
                        *value = nv;
                        changed = true;
                    }
                }
                MirOp::IndexSet {
                    base, index, value, ..
                } => {
                    let nb = simplify_expr(base.clone());
                    let ni = simplify_expr(index.clone());
                    let nv = simplify_expr(value.clone());
                    if !expr_eq(&nb, base) || !expr_eq(&ni, index) || !expr_eq(&nv, value) {
                        *base = nb;
                        *index = ni;
                        *value = nv;
                        changed = true;
                    }
                }
                MirOp::ListPush { base, value, .. } => {
                    let nb = simplify_expr(base.clone());
                    let nv = simplify_expr(value.clone());
                    if !expr_eq(&nb, base) || !expr_eq(&nv, value) {
                        *base = nb;
                        *value = nv;
                        changed = true;
                    }
                }
                MirOp::Phi { .. }
                | MirOp::MatchPayload { .. }
                | MirOp::TaskSpawn { .. }
                | MirOp::TaskSpawnFn { .. }
                | MirOp::TaskJoin { .. }
                | MirOp::ScopeEnter { .. }
                | MirOp::ScopeExit { .. }
                | MirOp::ScopeRegister { .. }
                | MirOp::ScopePromote { .. }
                | MirOp::ScopeDisown { .. }
                | MirOp::ScopeRelease { .. } => {}
            }
        }
        let term = b.term.clone();
        let nt = simplify_term(term);
        if !term_eq(&nt, &b.term) {
            b.term = nt;
            changed = true;
        }
    }
    changed
}

fn simplify_term(term: Terminator) -> Terminator {
    match term {
        Terminator::Branch {
            cond,
            then_bb,
            else_bb,
        } => Terminator::Branch {
            cond: simplify_expr(cond),
            then_bb,
            else_bb,
        },
        Terminator::MatchTagged {
            scrutinee,
            ok_bb,
            err_bb,
        } => Terminator::MatchTagged {
            scrutinee: simplify_expr(scrutinee),
            ok_bb,
            err_bb,
        },
        Terminator::ReturnOk(e, span) => Terminator::ReturnOk(simplify_expr(e), span),
        Terminator::ReturnErr(e) => Terminator::ReturnErr(simplify_expr(e)),
        other => other,
    }
}

/// Recursive expression simplify (local, no global rewrites).
pub fn simplify_expr(e: MirExpr) -> MirExpr {
    match e {
        MirExpr::UnboxValue { value, to } => {
            let value = simplify_expr(*value);
            match value {
                // UnboxValue(BoxValue(x, R), R) → x
                MirExpr::BoxValue { value: inner, from } if from == to => simplify_expr(*inner),
                // Unbox of a value already at `to` is identity (handled when Name
                // is not available here; structural only for nested forms).
                other => MirExpr::UnboxValue {
                    value: Box::new(other),
                    to,
                },
            }
        }
        MirExpr::BoxValue { value, from } => {
            let value = simplify_expr(*value);
            match value {
                // BoxValue(UnboxValue(x, R), R) → x when x is already boxed-shaped.
                MirExpr::UnboxValue { value: inner, to }
                    if to == from && is_boxed_shaped(&inner) =>
                {
                    simplify_expr(*inner)
                }
                other => MirExpr::BoxValue {
                    value: Box::new(other),
                    from,
                },
            }
        }
        MirExpr::Unary { op, expr } => MirExpr::Unary {
            op,
            expr: Box::new(simplify_expr(*expr)),
        },
        MirExpr::Binary { op, left, right } => MirExpr::Binary {
            op,
            left: Box::new(simplify_expr(*left)),
            right: Box::new(simplify_expr(*right)),
        },
        MirExpr::Call { target, args, ret } => {
            let target = match target {
                CallTarget::Indirect { callee } => CallTarget::Indirect {
                    callee: Box::new(simplify_expr(*callee)),
                },
                other => other,
            };
            MirExpr::Call {
                target,
                args: args.into_iter().map(simplify_expr).collect(),
                ret,
            }
        }
        MirExpr::PrimCall { prim, args } => MirExpr::PrimCall {
            prim,
            args: args.into_iter().map(simplify_expr).collect(),
        },
        MirExpr::ListLit(xs) => MirExpr::ListLit(xs.into_iter().map(simplify_expr).collect()),
        MirExpr::StructLit { type_name, fields } => MirExpr::StructLit {
            type_name,
            fields: fields
                .into_iter()
                .map(|(k, v)| (k, simplify_expr(v)))
                .collect(),
        },
        MirExpr::StructTypeIs { value, type_name } => MirExpr::StructTypeIs {
            value: Box::new(simplify_expr(*value)),
            type_name,
        },
        MirExpr::Index { base, index } => MirExpr::Index {
            base: Box::new(simplify_expr(*base)),
            index: Box::new(simplify_expr(*index)),
        },
        MirExpr::FieldGet { base, field } => MirExpr::FieldGet {
            base: Box::new(simplify_expr(*base)),
            field,
        },
        MirExpr::StringInterp { parts } => MirExpr::StringInterp { parts },
        MirExpr::LocatorInterp { parts } => MirExpr::LocatorInterp { parts },
        MirExpr::BytesInterp { parts } => MirExpr::BytesInterp { parts },
        MirExpr::FnValue { .. } => e,
        MirExpr::Range { start, end } => MirExpr::Range {
            start: Box::new(simplify_expr(*start)),
            end: Box::new(simplify_expr(*end)),
        },
        other => other,
    }
}

/// Operand of Unbox that is already a universal/boxed Echo value.
fn is_boxed_shaped(e: &MirExpr) -> bool {
    match e {
        MirExpr::Name(_) => true, // resolved via reprs at call sites; structural OK
        MirExpr::BoxValue { .. } => true,
        MirExpr::Call { .. } => true,
        MirExpr::PrimCall { .. } => true,
        MirExpr::Index { .. } | MirExpr::FieldGet { .. } => true,
        MirExpr::ListLit(_)
        | MirExpr::StringLit { .. }
        | MirExpr::BytesLit { .. }
        | MirExpr::LocatorLit { .. }
        | MirExpr::LocatorInterp { .. }
        | MirExpr::BytesInterp { .. }
        | MirExpr::StringInterp { .. } => {
            // These produce refs; after unbox path they're not typically unbox sources.
            false
        }
        MirExpr::ConstI64(_)
        | MirExpr::ConstI32(_)
        | MirExpr::ConstInt { .. }
        | MirExpr::Cast { .. }
        | MirExpr::ConstBool(_)
        | MirExpr::ConstF64(_)
        | MirExpr::ConstF32(_)
        | MirExpr::ConstDuration(_) => false,
        MirExpr::UnboxValue { .. } => false,
        MirExpr::Unary { .. } | MirExpr::Binary { .. } => false,
        MirExpr::StructLit { .. } => false,
        MirExpr::StructTypeIs { .. } => true, // runtime type_is call → i64 bool
        MirExpr::FnValue { .. } => true,      // code pointer as i64 value
        MirExpr::Range { .. } => true,        // heap range handle
    }
}

/// Copy-propagate `Set x = y`, collapse same-incoming φ, rewrite uses.
fn propagate_and_collapse(cfg: &mut MirCfg, reprs: &mut HashMap<String, MirRepr>) -> bool {
    let mut changed = false;

    // Build copy aliases: dest → src (peel Name only).
    let mut alias: HashMap<String, String> = HashMap::new();
    for b in &cfg.blocks {
        for op in &b.ops {
            if let MirOp::Set {
                name,
                value: MirExpr::Name(src),
                ..
            } = op
            {
                let root = resolve_alias(&alias, src);
                if root != *name {
                    alias.insert(name.clone(), root);
                }
            }
            // Identity unbox/box folded to Name after fold_exprs may land here next iter.
            if let MirOp::Set {
                name,
                value: MirExpr::UnboxValue { value, to },
                ..
            } = op
            {
                if let MirExpr::Name(src) = value.as_ref() {
                    if reprs.get(src).copied() == Some(*to) {
                        // UnboxValue(x, R) when x already has R → copy
                        let root = resolve_alias(&alias, src);
                        if root != *name {
                            alias.insert(name.clone(), root);
                            changed = true;
                        }
                    }
                }
            }
            if let MirOp::Set {
                name,
                value: MirExpr::BoxValue { value, from: _ },
                ..
            } = op
            {
                if let MirExpr::Name(src) = value.as_ref() {
                    if reprs.get(src).copied().is_some_and(|r| r.is_universal()) {
                        // Box of already-universal is redundant copy
                        let root = resolve_alias(&alias, src);
                        if root != *name {
                            alias.insert(name.clone(), root);
                            changed = true;
                        }
                    }
                }
            }
        }
        // Trivial φ: all incomings equal
        for op in &b.ops {
            if let MirOp::Phi { name, incomings } = op {
                if incomings.is_empty() {
                    continue;
                }
                let roots: Vec<String> = incomings
                    .iter()
                    .map(|(_, n)| resolve_alias(&alias, n))
                    .collect();
                if roots.iter().all(|r| r == &roots[0]) {
                    let root = roots[0].clone();
                    if root != *name {
                        alias.insert(name.clone(), root);
                        changed = true;
                    }
                }
            }
        }
    }

    if alias.is_empty() && !changed {
        return false;
    }

    // Rewrite all uses through alias map; drop trivial φ and pure copy Sets.
    for b in &mut cfg.blocks {
        let mut new_ops = Vec::new();
        for op in std::mem::take(&mut b.ops) {
            match op {
                MirOp::Phi { name, incomings } => {
                    if let Some(root) = alias.get(&name) {
                        // Collapsed φ — drop (uses rewritten to root).
                        let _ = root;
                        changed = true;
                        continue;
                    }
                    let incomings: Vec<_> = incomings
                        .into_iter()
                        .map(|(p, n)| (p, resolve_alias(&alias, &n)))
                        .collect();
                    // Check again after resolve
                    if !incomings.is_empty() && incomings.iter().all(|(_, n)| n == &incomings[0].1)
                    {
                        alias.insert(name.clone(), incomings[0].1.clone());
                        changed = true;
                        continue;
                    }
                    new_ops.push(MirOp::Phi { name, incomings });
                }
                MirOp::Set { name, value, span } => {
                    if alias.contains_key(&name) {
                        // Pure copy / collapsed — drop definition.
                        changed = true;
                        continue;
                    }
                    let value = rewrite_names(value, &alias);
                    new_ops.push(MirOp::Set { name, value, span });
                }
                MirOp::Eval(value) => {
                    new_ops.push(MirOp::Eval(rewrite_names(value, &alias)));
                }
                MirOp::FieldSet { base, field, value } => {
                    new_ops.push(MirOp::FieldSet {
                        base: rewrite_names(base, &alias),
                        field,
                        value: rewrite_names(value, &alias),
                    });
                }
                MirOp::IndexSet { base, index, value } => {
                    new_ops.push(MirOp::IndexSet {
                        base: rewrite_names(base, &alias),
                        index: rewrite_names(index, &alias),
                        value: rewrite_names(value, &alias),
                    });
                }
                MirOp::ListPush { base, value } => {
                    new_ops.push(MirOp::ListPush {
                        base: rewrite_names(base, &alias),
                        value: rewrite_names(value, &alias),
                    });
                }
                MirOp::MatchPayload { name } => {
                    if alias.contains_key(&name) {
                        changed = true;
                        continue;
                    }
                    new_ops.push(MirOp::MatchPayload { name });
                }
                MirOp::TaskSpawn {
                    module_path,
                    body_symbol,
                    bind,
                } => {
                    new_ops.push(MirOp::TaskSpawn {
                        module_path,
                        body_symbol,
                        bind,
                    });
                }
                MirOp::TaskSpawnFn {
                    module_path,
                    fn_symbol,
                    args,
                    bind,
                } => {
                    new_ops.push(MirOp::TaskSpawnFn {
                        module_path,
                        fn_symbol,
                        args: args.into_iter().map(|a| rewrite_names(a, &alias)).collect(),
                        bind,
                    });
                }
                MirOp::TaskJoin {
                    module_path,
                    body_symbol,
                    handle,
                    bind,
                } => {
                    new_ops.push(MirOp::TaskJoin {
                        module_path,
                        body_symbol,
                        handle: handle.map(|h| rewrite_names(h, &alias)),
                        bind,
                    });
                }
                MirOp::ScopeEnter { id } => new_ops.push(MirOp::ScopeEnter { id }),
                MirOp::ScopeExit { id } => new_ops.push(MirOp::ScopeExit { id }),
                MirOp::ScopeRegister { value } => {
                    new_ops.push(MirOp::ScopeRegister {
                        value: rewrite_names(value, &alias),
                    });
                }
                MirOp::ScopePromote { value, target } => {
                    new_ops.push(MirOp::ScopePromote {
                        value: rewrite_names(value, &alias),
                        target,
                    });
                }
                MirOp::ScopeDisown { value } => {
                    new_ops.push(MirOp::ScopeDisown {
                        value: rewrite_names(value, &alias),
                    });
                }
                MirOp::ScopeRelease { value } => {
                    new_ops.push(MirOp::ScopeRelease {
                        value: rewrite_names(value, &alias),
                    });
                }
            }
        }
        b.ops = new_ops;
        b.term = rewrite_term_names(b.term.clone(), &alias);
    }

    // Transfer reprs for aliases
    for (from, to) in &alias {
        if let Some(r) = reprs.get(to).copied() {
            reprs.insert(from.clone(), r);
        }
        // Remove dead name fact optional — keep for safety
    }

    !alias.is_empty() || changed
}

fn alias_interp_parts(parts: Vec<StrPart>, alias: &HashMap<String, String>) -> Vec<StrPart> {
    parts
        .into_iter()
        .map(|p| match p {
            StrPart::Lit(b) => StrPart::Lit(b),
            StrPart::Name(n) => StrPart::Name(resolve_alias(alias, &n)),
        })
        .collect()
}

fn collect_interp_uses(parts: &[StrPart], used: &mut HashSet<String>) {
    for p in parts {
        if let StrPart::Name(n) = p {
            used.insert(n.clone());
        }
    }
}

fn resolve_alias(alias: &HashMap<String, String>, name: &str) -> String {
    let mut cur = name.to_string();
    let mut seen = HashSet::new();
    while let Some(next) = alias.get(&cur) {
        if !seen.insert(cur.clone()) {
            break;
        }
        cur = next.clone();
    }
    cur
}

fn rewrite_names(e: MirExpr, alias: &HashMap<String, String>) -> MirExpr {
    match e {
        MirExpr::Name(n) => MirExpr::Name(resolve_alias(alias, &n)),
        MirExpr::UnboxValue { value, to } => MirExpr::UnboxValue {
            value: Box::new(rewrite_names(*value, alias)),
            to,
        },
        MirExpr::BoxValue { value, from } => MirExpr::BoxValue {
            value: Box::new(rewrite_names(*value, alias)),
            from,
        },
        MirExpr::Unary { op, expr } => MirExpr::Unary {
            op,
            expr: Box::new(rewrite_names(*expr, alias)),
        },
        MirExpr::Binary { op, left, right } => MirExpr::Binary {
            op,
            left: Box::new(rewrite_names(*left, alias)),
            right: Box::new(rewrite_names(*right, alias)),
        },
        MirExpr::Call { target, args, ret } => {
            let target = match target {
                CallTarget::Indirect { callee } => CallTarget::Indirect {
                    callee: Box::new(rewrite_names(*callee, alias)),
                },
                other => other,
            };
            MirExpr::Call {
                target,
                args: args.into_iter().map(|a| rewrite_names(a, alias)).collect(),
                ret,
            }
        }
        MirExpr::PrimCall { prim, args } => MirExpr::PrimCall {
            prim,
            args: args.into_iter().map(|a| rewrite_names(a, alias)).collect(),
        },
        MirExpr::Range { start, end } => MirExpr::Range {
            start: Box::new(rewrite_names(*start, alias)),
            end: Box::new(rewrite_names(*end, alias)),
        },
        MirExpr::ListLit(xs) => {
            MirExpr::ListLit(xs.into_iter().map(|x| rewrite_names(x, alias)).collect())
        }
        MirExpr::StructLit { type_name, fields } => MirExpr::StructLit {
            type_name,
            fields: fields
                .into_iter()
                .map(|(k, v)| (k, rewrite_names(v, alias)))
                .collect(),
        },
        MirExpr::StructTypeIs { value, type_name } => MirExpr::StructTypeIs {
            value: Box::new(rewrite_names(*value, alias)),
            type_name,
        },
        MirExpr::Index { base, index } => MirExpr::Index {
            base: Box::new(rewrite_names(*base, alias)),
            index: Box::new(rewrite_names(*index, alias)),
        },
        MirExpr::FieldGet { base, field } => MirExpr::FieldGet {
            base: Box::new(rewrite_names(*base, alias)),
            field,
        },
        MirExpr::StringInterp { parts } => MirExpr::StringInterp {
            parts: alias_interp_parts(parts, alias),
        },
        MirExpr::LocatorInterp { parts } => MirExpr::LocatorInterp {
            parts: alias_interp_parts(parts, alias),
        },
        MirExpr::BytesInterp { parts } => MirExpr::BytesInterp {
            parts: alias_interp_parts(parts, alias),
        },
        other => other,
    }
}

fn rewrite_term_names(term: Terminator, alias: &HashMap<String, String>) -> Terminator {
    match term {
        Terminator::Branch {
            cond,
            then_bb,
            else_bb,
        } => Terminator::Branch {
            cond: rewrite_names(cond, alias),
            then_bb,
            else_bb,
        },
        Terminator::MatchTagged {
            scrutinee,
            ok_bb,
            err_bb,
        } => Terminator::MatchTagged {
            scrutinee: rewrite_names(scrutinee, alias),
            ok_bb,
            err_bb,
        },
        Terminator::ReturnOk(e, span) => Terminator::ReturnOk(rewrite_names(e, alias), span),
        Terminator::ReturnErr(e) => Terminator::ReturnErr(rewrite_names(e, alias)),
        other => other,
    }
}

/// Remove unused pure box/unbox and pure copy-like sets.
fn dce_pure(cfg: &mut MirCfg, _reprs: &HashMap<String, MirRepr>) -> bool {
    let mut used = HashSet::new();
    for b in &cfg.blocks {
        for op in &b.ops {
            match op {
                MirOp::Phi { incomings, .. } => {
                    for (_, n) in incomings {
                        used.insert(n.clone());
                    }
                }
                MirOp::Set { value, .. } => collect_uses(value, &mut used),
                MirOp::Eval(value) => collect_uses(value, &mut used),
                MirOp::FieldSet { base, value, .. } => {
                    collect_uses(base, &mut used);
                    collect_uses(value, &mut used);
                }
                MirOp::IndexSet {
                    base, index, value, ..
                } => {
                    collect_uses(base, &mut used);
                    collect_uses(index, &mut used);
                    collect_uses(value, &mut used);
                }
                MirOp::ListPush { base, value, .. } => {
                    collect_uses(base, &mut used);
                    collect_uses(value, &mut used);
                }
                MirOp::MatchPayload { .. } | MirOp::TaskSpawn { .. } => {}
                MirOp::TaskSpawnFn { args, .. } => {
                    for a in args {
                        collect_uses(a, &mut used);
                    }
                }
                MirOp::TaskJoin { handle, .. } => {
                    if let Some(h) = handle {
                        collect_uses(h, &mut used);
                    }
                }
                MirOp::ScopeEnter { .. } | MirOp::ScopeExit { .. } => {}
                MirOp::ScopeRegister { value }
                | MirOp::ScopePromote { value, .. }
                | MirOp::ScopeDisown { value }
                | MirOp::ScopeRelease { value } => {
                    collect_uses(value, &mut used);
                }
            }
        }
        collect_term_uses(&b.term, &mut used);
    }

    let mut changed = false;
    for b in &mut cfg.blocks {
        let before = b.ops.len();
        b.ops.retain(|op| match op {
            MirOp::Set { name, value, .. } if is_pure_conversion_or_copy(value) => {
                if used.contains(name) {
                    true
                } else {
                    changed = true;
                    false
                }
            }
            _ => true,
        });
        if b.ops.len() != before {
            changed = true;
        }
    }
    changed
}

fn is_pure_conversion_or_copy(e: &MirExpr) -> bool {
    match e {
        MirExpr::Name(_)
        | MirExpr::ConstI64(_)
        | MirExpr::ConstI32(_)
        | MirExpr::ConstInt { .. }
        | MirExpr::Cast { .. }
        | MirExpr::ConstBool(_)
        | MirExpr::ConstF64(_)
        | MirExpr::ConstF32(_)
        | MirExpr::ConstDuration(_)
        | MirExpr::BoxValue { .. }
        | MirExpr::UnboxValue { .. } => true,
        _ => false,
    }
}

fn collect_uses(e: &MirExpr, used: &mut HashSet<String>) {
    match e {
        MirExpr::Name(n) => {
            used.insert(n.clone());
        }
        MirExpr::UnboxValue { value, .. } | MirExpr::BoxValue { value, .. } => {
            collect_uses(value, used);
        }
        MirExpr::Unary { expr, .. } | MirExpr::Cast { expr, .. } => collect_uses(expr, used),
        MirExpr::Binary { left, right, .. } => {
            collect_uses(left, used);
            collect_uses(right, used);
        }
        MirExpr::Call { target, args, .. } => {
            if let CallTarget::Indirect { callee } = target {
                collect_uses(callee, used);
            }
            for a in args {
                collect_uses(a, used);
            }
        }
        MirExpr::PrimCall { args, .. } => {
            for a in args {
                collect_uses(a, used);
            }
        }
        MirExpr::ListLit(xs) => {
            for x in xs {
                collect_uses(x, used);
            }
        }
        MirExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_uses(v, used);
            }
        }
        MirExpr::StructTypeIs { value, .. } => collect_uses(value, used),
        MirExpr::Index { base, index } => {
            collect_uses(base, used);
            collect_uses(index, used);
        }
        MirExpr::FieldGet { base, .. } => collect_uses(base, used),
        MirExpr::StringInterp { parts }
        | MirExpr::LocatorInterp { parts }
        | MirExpr::BytesInterp { parts } => {
            collect_interp_uses(parts, used);
        }
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
        MirExpr::Range { start, end } => {
            collect_uses(start, used);
            collect_uses(end, used);
        }
    }
}

fn collect_term_uses(term: &Terminator, used: &mut HashSet<String>) {
    match term {
        Terminator::Branch { cond, .. } => collect_uses(cond, used),
        Terminator::MatchTagged { scrutinee, .. } => collect_uses(scrutinee, used),
        Terminator::ReturnOk(e, _) | Terminator::ReturnErr(e) => collect_uses(e, used),
        Terminator::Goto(_) | Terminator::ReturnNone | Terminator::Unreachable => {}
    }
}

fn re_infer_light(cfg: &MirCfg, reprs: &mut HashMap<String, MirRepr>) {
    for b in &cfg.blocks {
        for op in &b.ops {
            match op {
                MirOp::Set { name, value, .. } => {
                    let r = light_infer(value, reprs);
                    reprs.insert(name.clone(), r);
                }
                MirOp::Phi { name, incomings } => {
                    let mut r = MirRepr::Unknown;
                    for (_, n) in incomings {
                        r = r.merge_phi(reprs.get(n).copied().unwrap_or(MirRepr::Unknown));
                    }
                    reprs.insert(name.clone(), r);
                }
                MirOp::MatchPayload { name } => {
                    reprs.insert(name.clone(), MirRepr::Boxed);
                }
                _ => {}
            }
        }
    }
}

fn light_infer(e: &MirExpr, reprs: &HashMap<String, MirRepr>) -> MirRepr {
    match e {
        MirExpr::ConstI64(_) => MirRepr::Int64,
        MirExpr::ConstI32(_) => MirRepr::Int32,
        MirExpr::ConstInt { width, .. } => match width {
            echo_ast::Width::I8 => MirRepr::Int8,
            echo_ast::Width::I16 => MirRepr::Int16,
            echo_ast::Width::I32 => MirRepr::Int32,
            echo_ast::Width::I64 => MirRepr::Int64,
            echo_ast::Width::Ui8 => MirRepr::UInt8,
            echo_ast::Width::Ui16 => MirRepr::UInt16,
            echo_ast::Width::Ui32 => MirRepr::UInt32,
            echo_ast::Width::Ui64 => MirRepr::UInt64,
            echo_ast::Width::F32 => MirRepr::Float32,
            echo_ast::Width::F64 => MirRepr::Float64,
        },
        MirExpr::Cast { to, expr } => {
            let _ = light_infer(expr, reprs);
            match to {
                echo_ast::Width::I8 => MirRepr::Int8,
                echo_ast::Width::I16 => MirRepr::Int16,
                echo_ast::Width::I32 => MirRepr::Int32,
                echo_ast::Width::I64 => MirRepr::Int64,
                echo_ast::Width::Ui8 => MirRepr::UInt8,
                echo_ast::Width::Ui16 => MirRepr::UInt16,
                echo_ast::Width::Ui32 => MirRepr::UInt32,
                echo_ast::Width::Ui64 => MirRepr::UInt64,
                echo_ast::Width::F32 => MirRepr::Float32,
                echo_ast::Width::F64 => MirRepr::Float64,
            }
        }
        MirExpr::ConstBool(_) => MirRepr::Bool,
        MirExpr::ConstF64(_) => MirRepr::Float64,
        MirExpr::ConstF32(_) => MirRepr::Float32,
        MirExpr::ConstDuration(_) => MirRepr::Duration,
        MirExpr::Name(n) => reprs.get(n).copied().unwrap_or(MirRepr::Unknown),
        MirExpr::BoxValue { .. } => MirRepr::Boxed,
        MirExpr::UnboxValue { to, .. } => *to,
        MirExpr::Binary { op, left, right } => {
            let lr = light_infer(left, reprs);
            let rr = light_infer(right, reprs);
            use echo_ast::BinaryOp::*;
            match op {
                Add | Sub | Mul | Div | Rem if lr == rr && lr.is_native_int() => lr,
                Add | Sub | Mul | Div | Rem if lr == MirRepr::Float32 && rr == MirRepr::Float32 => {
                    MirRepr::Float32
                }
                BitAnd | BitOr | BitXor | Shl | Shr if lr == rr && lr.is_native_int() => lr,
                Add | Sub if lr == MirRepr::Duration && rr == MirRepr::Duration => {
                    MirRepr::Duration
                }
                Eq | NotEq | EqEqEq | NotEqEq | Lt | Gt | LtEq | GtEq => MirRepr::Bool,
                And | Or => MirRepr::Bool,
                _ => MirRepr::Unknown,
            }
        }
        MirExpr::Call { .. } => MirRepr::Boxed,
        MirExpr::ListLit(_) => MirRepr::ListRef,
        MirExpr::StringLit { .. } | MirExpr::StringInterp { .. } => MirRepr::StringRef,
        MirExpr::BytesLit { .. } | MirExpr::BytesInterp { .. } => MirRepr::BytesRef,
        MirExpr::LocatorLit { .. } | MirExpr::LocatorInterp { .. } => MirRepr::LocatorRef,
        MirExpr::StructLit { .. } => MirRepr::ObjectRef,
        _ => MirRepr::Unknown,
    }
}

fn expr_eq(a: &MirExpr, b: &MirExpr) -> bool {
    // Cheap structural: Debug format is fine for local simplify fixpoint.
    format!("{a:?}") == format!("{b:?}")
}

fn term_eq(a: &Terminator, b: &Terminator) -> bool {
    format!("{a:?}") == format!("{b:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        analyze_reprs, construct_ssa, structured_to_cfg, CallTarget, MirRetShape, MirStmt,
    };
    use echo_ast::BinaryOp;

    fn pipeline(stmts: Vec<MirStmt>, params: &[&str]) -> (MirCfg, HashMap<String, MirRepr>) {
        let params: Vec<String> = params.iter().map(|s| (*s).to_string()).collect();
        let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
        let cfg = construct_ssa(cfg, &params);
        let (cfg, reprs) = analyze_reprs(cfg, &params);
        simplify_local(cfg, reprs)
    }

    fn count_boxes(cfg: &MirCfg) -> usize {
        let mut n = 0;
        for b in &cfg.blocks {
            for op in &b.ops {
                match op {
                    MirOp::Set { value, .. } | MirOp::Eval(value) => {
                        n += count_boxes_expr(value);
                    }
                    MirOp::FieldSet { base, value, .. } => {
                        n += count_boxes_expr(base);
                        n += count_boxes_expr(value);
                    }
                    MirOp::IndexSet {
                        base, index, value, ..
                    } => {
                        n += count_boxes_expr(base);
                        n += count_boxes_expr(index);
                        n += count_boxes_expr(value);
                    }
                    MirOp::ListPush { base, value, .. } => {
                        n += count_boxes_expr(base);
                        n += count_boxes_expr(value);
                    }
                    _ => {}
                }
            }
            match &b.term {
                Terminator::ReturnOk(e, _) | Terminator::ReturnErr(e) => {
                    n += count_boxes_expr(e);
                }
                Terminator::Branch { cond, .. } => n += count_boxes_expr(cond),
                _ => {}
            }
        }
        n
    }

    fn count_boxes_expr(e: &MirExpr) -> usize {
        match e {
            MirExpr::BoxValue { value, .. } => 1 + count_boxes_expr(value),
            MirExpr::UnboxValue { value, .. } => count_boxes_expr(value),
            MirExpr::Binary { left, right, .. } => count_boxes_expr(left) + count_boxes_expr(right),
            MirExpr::Unary { expr, .. } => count_boxes_expr(expr),
            MirExpr::Call { args, .. } | MirExpr::PrimCall { args, .. } => {
                args.iter().map(count_boxes_expr).sum()
            }
            _ => 0,
        }
    }

    fn count_unboxes(cfg: &MirCfg) -> usize {
        let mut n = 0;
        for b in &cfg.blocks {
            for op in &b.ops {
                if let MirOp::Set { value, .. } = op {
                    n += count_unboxes_expr(value);
                }
                if let MirOp::Eval(value) = op {
                    n += count_unboxes_expr(value);
                }
            }
        }
        n
    }

    fn count_unboxes_expr(e: &MirExpr) -> usize {
        match e {
            MirExpr::UnboxValue { value, .. } => 1 + count_unboxes_expr(value),
            MirExpr::BoxValue { value, .. } => count_unboxes_expr(value),
            MirExpr::Binary { left, right, .. } => {
                count_unboxes_expr(left) + count_unboxes_expr(right)
            }
            MirExpr::Unary { expr, .. } => count_unboxes_expr(expr),
            MirExpr::Call { args, .. } | MirExpr::PrimCall { args, .. } => {
                args.iter().map(count_unboxes_expr).sum()
            }
            _ => 0,
        }
    }

    #[test]
    fn unbox_box_cancels() {
        // Manually craft: after SSA+repr, inject Unbox(Box(x))
        let stmts = vec![
            MirStmt::Set {
                name: "x".into(),
                value: MirExpr::ConstI64(1),
                span: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("x".into()), None),
        ];
        let (mut cfg, mut reprs) = {
            let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
            let cfg = construct_ssa(cfg, &[]);
            analyze_reprs(cfg, &[])
        };
        // Append redundant unbox(box(x@0))
        let x = cfg.blocks[0]
            .ops
            .iter()
            .find_map(|op| match op {
                MirOp::Set { name, .. } if name.starts_with("x@") => Some(name.clone()),
                _ => None,
            })
            .expect("x");
        cfg.blocks[0].ops.push(MirOp::Set {
            name: "y@0".into(),
            value: MirExpr::UnboxValue {
                value: Box::new(MirExpr::BoxValue {
                    value: Box::new(MirExpr::Name(x.clone())),
                    from: MirRepr::Int64,
                }),
                to: MirRepr::Int64,
            },
            span: None,
        });
        cfg.blocks[0].term = Terminator::ReturnOk(MirExpr::Name("y@0".into()), None);
        reprs.insert("y@0".into(), MirRepr::Int64);

        let (cfg, _) = simplify_local(cfg, reprs);
        // y should be copy of x and collapsed/DCE'd or return uses x
        let ret_is_x = matches!(
            &cfg.blocks[0].term,
            Terminator::ReturnOk(MirExpr::Name(n), None) if n == &x || n.starts_with("x@")
        );
        let y_is_copy = cfg.blocks[0].ops.iter().any(|op| {
            matches!(
                op,
                MirOp::Set {
                    name,
                    value: MirExpr::Name(src), .. } if name.starts_with("y") && src == &x
            )
        });
        // After full simplify, return should be x without nested unbox/box
        assert!(
            ret_is_x || y_is_copy || count_unboxes(&cfg) == 0,
            "expected cancel; cfg={cfg:?}"
        );
        assert_eq!(count_boxes(&cfg), 0, "no leftover boxes; cfg={cfg:?}");
        assert_eq!(count_unboxes(&cfg), 0, "no leftover unboxes; cfg={cfg:?}");
    }

    #[test]
    fn native_arith_then_print_one_box() {
        // Return an already-boxed param so the only new box is print(c).
        let stmts = vec![
            MirStmt::Set {
                name: "c".into(),
                value: MirExpr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(MirExpr::Name("a".into())),
                    right: Box::new(MirExpr::Name("b".into())),
                },
                span: None,
            },
            MirStmt::Eval(MirExpr::Call {
                target: CallTarget::Runtime {
                    export: "print".into(),
                },
                args: vec![MirExpr::Name("c".into())],
                ret: MirRetShape::Plain,
            }),
            MirStmt::ReturnOk(MirExpr::Name("a".into()), None),
        ];
        let (cfg, _) = pipeline(stmts, &["a", "b"]);
        let boxes = count_boxes(&cfg);
        assert_eq!(
            boxes, 1,
            "expected exactly one box at print boundary; boxes={boxes} cfg={cfg:?}"
        );
    }

    #[test]
    fn same_value_phi_collapses() {
        // Same SSA name on every φ incoming → collapse to that name.
        let cfg2 = MirCfg {
            entry: crate::BlockId(0),
            blocks: vec![
                crate::MirBlock {
                    id: crate::BlockId(0),
                    ops: vec![],
                    term: Terminator::Branch {
                        cond: MirExpr::ConstI64(1),
                        then_bb: crate::BlockId(1),
                        else_bb: crate::BlockId(2),
                    },
                },
                crate::MirBlock {
                    id: crate::BlockId(1),
                    ops: vec![],
                    term: Terminator::Goto(crate::BlockId(3)),
                },
                crate::MirBlock {
                    id: crate::BlockId(2),
                    ops: vec![],
                    term: Terminator::Goto(crate::BlockId(3)),
                },
                crate::MirBlock {
                    id: crate::BlockId(3),
                    ops: vec![MirOp::Phi {
                        name: "p@0".into(),
                        incomings: vec![
                            (crate::BlockId(1), "v@0".into()),
                            (crate::BlockId(2), "v@0".into()),
                        ],
                    }],
                    term: Terminator::ReturnOk(MirExpr::Name("p@0".into()), None),
                },
            ],
        };
        let mut reprs = HashMap::new();
        reprs.insert("v@0".into(), MirRepr::Int64);
        reprs.insert("p@0".into(), MirRepr::Int64);
        let (cfg2, _) = simplify_local(cfg2, reprs);
        let has_phi = cfg2
            .blocks
            .iter()
            .any(|b| b.ops.iter().any(|op| matches!(op, MirOp::Phi { .. })));
        assert!(!has_phi, "same-incoming phi must collapse; cfg={cfg2:?}");
        assert!(
            matches!(
                &cfg2.blocks[3].term,
                Terminator::ReturnOk(MirExpr::Name(n), None) if n == "v@0"
            ),
            "return should use v@0; cfg={cfg2:?}"
        );
    }

    #[test]
    fn incompatible_unbox_box_remains() {
        // Unbox to Bool then Box as Int64 must NOT cancel
        let e = MirExpr::BoxValue {
            value: Box::new(MirExpr::UnboxValue {
                value: Box::new(MirExpr::Name("b@0".into())),
                to: MirRepr::Bool,
            }),
            from: MirRepr::Int64, // different from Bool
        };
        let s = simplify_expr(e.clone());
        assert!(
            matches!(s, MirExpr::BoxValue { .. }),
            "incompatible R must remain; got {s:?}"
        );
    }

    #[test]
    fn abi_boundary_still_boxed() {
        let stmts = vec![
            MirStmt::Eval(MirExpr::Call {
                target: CallTarget::Runtime {
                    export: "print".into(),
                },
                args: vec![MirExpr::Name("n".into())],
                ret: MirRetShape::Plain,
            }),
            MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
        ];
        // n is param boxed already — may be zero boxes if already ABI-ready
        let (cfg, _) = pipeline(stmts, &["n"]);
        // Call args must not be native-only without box when source is native.
        // Param is already Boxed → zero boxes is correct for ABI.
        // Add native then print:
        let stmts = vec![
            MirStmt::Set {
                name: "x".into(),
                value: MirExpr::ConstI64(1),
                span: None,
            },
            MirStmt::Eval(MirExpr::Call {
                target: CallTarget::Runtime {
                    export: "print".into(),
                },
                args: vec![MirExpr::Name("x".into())],
                ret: MirRetShape::Plain,
            }),
            MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
        ];
        let (cfg2, _) = pipeline(stmts, &[]);
        assert!(
            count_boxes(&cfg2) >= 1,
            "print of native must keep a box; cfg={cfg2:?}"
        );
        let _ = cfg;
    }

    #[test]
    fn box_unbox_roundtrip_to_native_cancels() {
        let e = MirExpr::UnboxValue {
            value: Box::new(MirExpr::BoxValue {
                value: Box::new(MirExpr::Name("x@0".into())),
                from: MirRepr::Int64,
            }),
            to: MirRepr::Int64,
        };
        let s = simplify_expr(e);
        assert!(
            matches!(&s, MirExpr::Name(n) if n == "x@0"),
            "Unbox(Box(x,R),R) → x; got {s:?}"
        );
    }

    #[test]
    fn box_of_unbox_cancels_when_already_boxed() {
        let e = MirExpr::BoxValue {
            value: Box::new(MirExpr::UnboxValue {
                value: Box::new(MirExpr::Name("boxed@0".into())),
                to: MirRepr::Int64,
            }),
            from: MirRepr::Int64,
        };
        let s = simplify_expr(e);
        assert!(
            matches!(&s, MirExpr::Name(n) if n == "boxed@0"),
            "Box(Unbox(x,R),R) → x when x boxed-shaped; got {s:?}"
        );
    }
}
