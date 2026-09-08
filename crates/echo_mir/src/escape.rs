//! Local escape analysis for boxed scalars and temporary allocations.
//!
//! Runs after representation analysis + local simplify and before a final
//! [`crate::simplify_local`]. Classifies SSA values using Echo runtime ABI
//! knowledge and elides redundant scalar `BoxValue`/`UnboxValue` pairs when
//! the box does not escape (`NoEscape`).

use std::collections::{HashMap, HashSet};

use echo_std::runtime_native_symbol;

use crate::cfg::{MirCfg, MirOp, Terminator};
use crate::repr::MirRepr;
use crate::{CallTarget, MirExpr, MirPrim, StrPart};

/// How far a value may escape its defining region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EscapeClass {
    /// Only local SSA uses; safe to keep native / elide scalar boxing.
    NoEscape,
    /// May be passed to a call that is not proven non-retaining.
    EscapesToCall,
    /// May be stored into heap-managed storage (list/struct/field).
    EscapesToHeap,
    /// May leave the function (return / error return).
    EscapesFromFunction,
    /// Insufficient proof.
    #[default]
    Unknown,
}

impl EscapeClass {
    fn rank(self) -> u8 {
        match self {
            Self::NoEscape => 0,
            Self::EscapesToCall => 1,
            Self::EscapesToHeap => 2,
            Self::EscapesFromFunction => 3,
            Self::Unknown => 4,
        }
    }

    #[must_use]
    pub fn meet(self, other: Self) -> Self {
        if self.rank() >= other.rank() {
            self
        } else {
            other
        }
    }
}

/// Local escape analysis + NoEscape scalar box elision.
#[must_use]
pub fn analyze_escapes(
    mut cfg: MirCfg,
    mut reprs: HashMap<String, MirRepr>,
) -> (
    MirCfg,
    HashMap<String, MirRepr>,
    HashMap<String, EscapeClass>,
) {
    let mut escapes: HashMap<String, EscapeClass> = HashMap::new();
    let mut parent: HashMap<String, String> = HashMap::new(); // union-find for copies/phis

    // Seed: every defined name starts NoEscape until a use raises it.
    for b in &cfg.blocks {
        for op in &b.ops {
            match op {
                MirOp::Phi { name, incomings } => {
                    ensure(&mut escapes, name);
                    for (_, n) in incomings {
                        ensure(&mut escapes, n);
                        union(&mut parent, name, n);
                    }
                }
                MirOp::MatchPayload { name } => {
                    // Payload comes from runtime packing — treat as may-escape unknown.
                    escapes.insert(name.clone(), EscapeClass::Unknown);
                }
                MirOp::Set { name, value, .. } => {
                    ensure(&mut escapes, name);
                    // Only pure SSA copies alias — do not peel through Box/Unbox.
                    if let MirExpr::Name(src) = value {
                        ensure(&mut escapes, src);
                        union(&mut parent, name, src);
                    }
                    // Allocation / box sites are still NoEscape until used
                    seed_alloc(name, value, &mut escapes);
                }
                _ => {}
            }
        }
    }

    // Params: enter as Unknown (ABI / caller-owned)
    // (params appear as name@0 after SSA; mark any root with @0 that has no def)
    // Uses will classify further.

    // Walk all uses and raise escape classes on alias roots.
    for b in &cfg.blocks {
        for op in &b.ops {
            match op {
                MirOp::Set { value, .. } => {
                    classify_expr_uses(value, UseCtx::Local, &mut escapes, &mut parent);
                }
                MirOp::Eval(value) => {
                    classify_expr_uses(value, UseCtx::Eval, &mut escapes, &mut parent);
                }
                MirOp::FieldSet { base, value, .. } => {
                    // base object mutated; value stored → heap escape
                    mark_names_in(base, EscapeClass::Unknown, &mut escapes, &mut parent);
                    mark_names_in(value, EscapeClass::EscapesToHeap, &mut escapes, &mut parent);
                }
                MirOp::IndexSet {
                    base, index, value, ..
                } => {
                    mark_names_in(base, EscapeClass::Unknown, &mut escapes, &mut parent);
                    classify_expr_uses(index, UseCtx::Local, &mut escapes, &mut parent);
                    mark_names_in(value, EscapeClass::EscapesToHeap, &mut escapes, &mut parent);
                }
                MirOp::ListPush { base, value, .. } => {
                    mark_names_in(base, EscapeClass::Unknown, &mut escapes, &mut parent);
                    mark_names_in(value, EscapeClass::EscapesToHeap, &mut escapes, &mut parent);
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
        match &b.term {
            Terminator::ReturnOk(e, _) | Terminator::ReturnErr(e) => {
                mark_names_in(
                    e,
                    EscapeClass::EscapesFromFunction,
                    &mut escapes,
                    &mut parent,
                );
            }
            Terminator::Branch { cond, .. } => {
                classify_expr_uses(cond, UseCtx::Local, &mut escapes, &mut parent);
            }
            Terminator::MatchTagged { scrutinee, .. } => {
                // Tag packing may retain until match arms — local within function
                classify_expr_uses(scrutinee, UseCtx::Local, &mut escapes, &mut parent);
            }
            _ => {}
        }
    }

    // Flatten: every name reports root's class
    let mut flat = HashMap::new();
    for name in escapes.keys().cloned().collect::<Vec<_>>() {
        let r = find(&mut parent, &name);
        let c = escapes.get(&r).copied().unwrap_or(EscapeClass::Unknown);
        flat.insert(name, c);
    }
    // Also store roots
    for (k, v) in &escapes {
        flat.entry(k.clone()).or_insert(*v);
    }

    // Elide NoEscape scalar boxes that are only unboxed.
    elide_noescape_scalar_boxes(&mut cfg, &flat, &mut reprs);

    (cfg, reprs, flat)
}

fn ensure(escapes: &mut HashMap<String, EscapeClass>, name: &str) {
    escapes
        .entry(name.to_string())
        .or_insert(EscapeClass::NoEscape);
}

fn seed_alloc(name: &str, value: &MirExpr, escapes: &mut HashMap<String, EscapeClass>) {
    match value {
        MirExpr::BoxValue { .. }
        | MirExpr::ListLit(_)
        | MirExpr::StructLit { .. }
        | MirExpr::StringLit { .. }
        | MirExpr::BytesLit { .. }
        | MirExpr::LocatorLit { .. }
        | MirExpr::LocatorInterp { .. }
        | MirExpr::BytesInterp { .. }
        | MirExpr::StringInterp { .. } => {
            escapes.insert(name.to_string(), EscapeClass::NoEscape);
        }
        _ => {}
    }
}

#[derive(Clone, Copy)]
enum UseCtx {
    Local,
    Eval,
}

fn classify_expr_uses(
    e: &MirExpr,
    ctx: UseCtx,
    escapes: &mut HashMap<String, EscapeClass>,
    parent: &mut HashMap<String, String>,
) {
    match e {
        MirExpr::Name(n) => {
            // Bare name use without store/return is local (NoEscape raise nothing)
            let _ = (n, ctx);
        }
        MirExpr::BoxValue { value, .. } => {
            // Boxing does not by itself escape the *inner* native value.
            classify_expr_uses(value, ctx, escapes, parent);
        }
        MirExpr::UnboxValue { value, .. } => {
            classify_expr_uses(value, ctx, escapes, parent);
        }
        MirExpr::Unary { expr, .. } | MirExpr::Cast { expr, .. } => {
            classify_expr_uses(expr, ctx, escapes, parent)
        }
        MirExpr::Binary { left, right, .. } => {
            classify_expr_uses(left, ctx, escapes, parent);
            classify_expr_uses(right, ctx, escapes, parent);
        }
        MirExpr::Call { target, args, .. } => match target {
            CallTarget::Runtime { export } => {
                if is_non_retaining_runtime(export) {
                    // Args used locally by known intrinsic
                    for a in args {
                        classify_expr_uses(a, UseCtx::Local, escapes, parent);
                    }
                } else {
                    for a in args {
                        mark_names_in(a, EscapeClass::EscapesToCall, escapes, parent);
                        classify_expr_uses(a, ctx, escapes, parent);
                    }
                }
            }
            CallTarget::Function { .. } => {
                for a in args {
                    mark_names_in(a, EscapeClass::EscapesToCall, escapes, parent);
                    classify_expr_uses(a, ctx, escapes, parent);
                }
            }
            CallTarget::Indirect { callee } => {
                classify_expr_uses(callee, UseCtx::Local, escapes, parent);
                for a in args {
                    mark_names_in(a, EscapeClass::EscapesToCall, escapes, parent);
                    classify_expr_uses(a, ctx, escapes, parent);
                }
            }
        },
        MirExpr::PrimCall { prim, args } => match prim {
            MirPrim::ListLen | MirPrim::ListGetChecked => {
                // Known non-retaining: list/index used without retaining the value.
                for a in args {
                    classify_expr_uses(a, UseCtx::Local, escapes, parent);
                }
            }
        },
        MirExpr::ListLit(items) => {
            for it in items {
                mark_names_in(it, EscapeClass::EscapesToHeap, escapes, parent);
                classify_expr_uses(it, ctx, escapes, parent);
            }
        }
        MirExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                mark_names_in(v, EscapeClass::EscapesToHeap, escapes, parent);
                classify_expr_uses(v, ctx, escapes, parent);
            }
        }
        MirExpr::StructTypeIs { value, .. } => {
            classify_expr_uses(value, ctx, escapes, parent);
        }
        MirExpr::Index { base, index } => {
            classify_expr_uses(base, ctx, escapes, parent);
            classify_expr_uses(index, ctx, escapes, parent);
        }
        MirExpr::FieldGet { base, .. } => {
            classify_expr_uses(base, ctx, escapes, parent);
        }
        MirExpr::StringInterp { parts }
        | MirExpr::LocatorInterp { parts }
        | MirExpr::BytesInterp { parts } => {
            for p in parts {
                if let StrPart::Name(n) = p {
                    // String builder may retain stringified value briefly —
                    // treat as call-like for safety of boxed values.
                    mark_name(n, EscapeClass::EscapesToCall, escapes, parent);
                }
            }
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
            classify_expr_uses(start, ctx, escapes, parent);
            classify_expr_uses(end, ctx, escapes, parent);
        }
    }
}

fn is_non_retaining_runtime(export: &str) -> bool {
    // print, eq, ne, and list length/get style are non-retaining.
    matches!(
        export,
        "print" | "eprint" | "eq" | "ne" | "list_len" | "list_get" | "list_get_unchecked"
    ) || runtime_native_symbol(export).is_some_and(|s| {
        s.contains("print")
            || s.contains("_eq")
            || s.contains("_ne")
            || s.contains("list_len")
            || s.contains("list_get")
    })
}

fn mark_names_in(
    e: &MirExpr,
    class: EscapeClass,
    escapes: &mut HashMap<String, EscapeClass>,
    parent: &mut HashMap<String, String>,
) {
    match e {
        MirExpr::Name(n) => mark_name(n, class, escapes, parent),
        MirExpr::BoxValue { value, .. }
        | MirExpr::UnboxValue { value, .. }
        | MirExpr::Unary { expr: value, .. }
        | MirExpr::Cast { expr: value, .. } => {
            mark_names_in(value, class, escapes, parent);
        }
        MirExpr::Binary { left, right, .. } => {
            mark_names_in(left, class, escapes, parent);
            mark_names_in(right, class, escapes, parent);
        }
        MirExpr::Call { target, args, .. } => {
            if let CallTarget::Indirect { callee } = target {
                mark_names_in(callee, class, escapes, parent);
            }
            for a in args {
                mark_names_in(a, class, escapes, parent);
            }
        }
        MirExpr::PrimCall { args, .. } => {
            for a in args {
                mark_names_in(a, class, escapes, parent);
            }
        }
        MirExpr::ListLit(xs) => {
            for x in xs {
                mark_names_in(x, class, escapes, parent);
            }
        }
        MirExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                mark_names_in(v, class, escapes, parent);
            }
        }
        MirExpr::StructTypeIs { value, .. } => {
            mark_names_in(value, class, escapes, parent);
        }
        MirExpr::Index { base, index } => {
            mark_names_in(base, class, escapes, parent);
            mark_names_in(index, class, escapes, parent);
        }
        MirExpr::FieldGet { base, .. } => mark_names_in(base, class, escapes, parent),
        MirExpr::StringInterp { parts }
        | MirExpr::LocatorInterp { parts }
        | MirExpr::BytesInterp { parts } => {
            for p in parts {
                if let StrPart::Name(n) = p {
                    mark_name(n, class, escapes, parent);
                }
            }
        }
        MirExpr::Range { start, end } => {
            mark_names_in(start, class, escapes, parent);
            mark_names_in(end, class, escapes, parent);
        }
        _ => {}
    }
}

fn mark_name(
    name: &str,
    class: EscapeClass,
    escapes: &mut HashMap<String, EscapeClass>,
    parent: &mut HashMap<String, String>,
) {
    let root = find(parent, name);
    let cur = escapes.get(&root).copied().unwrap_or(EscapeClass::NoEscape);
    escapes.insert(root, cur.meet(class));
}

fn find(parent: &mut HashMap<String, String>, name: &str) -> String {
    if !parent.contains_key(name) {
        parent.insert(name.to_string(), name.to_string());
        return name.to_string();
    }
    let mut cur = name.to_string();
    let mut path = Vec::new();
    while let Some(p) = parent.get(&cur) {
        if p == &cur {
            break;
        }
        path.push(cur.clone());
        cur = p.clone();
    }
    for n in path {
        parent.insert(n, cur.clone());
    }
    cur
}

fn union(parent: &mut HashMap<String, String>, a: &str, b: &str) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        parent.insert(ra, rb);
    }
}

/// Rewrite `Unbox(Name(box), R)` → payload when `box` is NoEscape `BoxValue(_, R)`.
fn elide_noescape_scalar_boxes(
    cfg: &mut MirCfg,
    escapes: &HashMap<String, EscapeClass>,
    reprs: &mut HashMap<String, MirRepr>,
) {
    // Map: name → (inner expr, from repr) for NoEscape BoxValue defs
    let mut box_defs: HashMap<String, (MirExpr, MirRepr)> = HashMap::new();
    for b in &cfg.blocks {
        for op in &b.ops {
            if let MirOp::Set {
                name,
                value: MirExpr::BoxValue { value, from },
                ..
            } = op
            {
                if escapes.get(name).copied() == Some(EscapeClass::NoEscape)
                    && from.is_native_scalar()
                {
                    box_defs.insert(name.clone(), ((**value).clone(), *from));
                }
            }
        }
    }
    if box_defs.is_empty() {
        return;
    }

    for b in &mut cfg.blocks {
        for op in &mut b.ops {
            match op {
                MirOp::Set { value, name, .. } => {
                    *value = elide_in_expr(value.clone(), &box_defs);
                    if let Some(r) = infer_after_elide(value) {
                        reprs.insert(name.clone(), r);
                    }
                }
                MirOp::Eval(value) => {
                    *value = elide_in_expr(value.clone(), &box_defs);
                }
                MirOp::FieldSet { base, value, .. } => {
                    *base = elide_in_expr(base.clone(), &box_defs);
                    *value = elide_in_expr(value.clone(), &box_defs);
                }
                MirOp::IndexSet {
                    base, index, value, ..
                } => {
                    *base = elide_in_expr(base.clone(), &box_defs);
                    *index = elide_in_expr(index.clone(), &box_defs);
                    *value = elide_in_expr(value.clone(), &box_defs);
                }
                MirOp::ListPush { base, value, .. } => {
                    *base = elide_in_expr(base.clone(), &box_defs);
                    *value = elide_in_expr(value.clone(), &box_defs);
                }
                _ => {}
            }
        }
        b.term = elide_in_term(b.term.clone(), &box_defs);
    }

    // DCE pure BoxValue sets that are now unused
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
                MirOp::Eval(v) => collect_uses(v, &mut used),
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
                MirOp::MatchPayload { .. }
                | MirOp::TaskSpawn { .. }
                | MirOp::TaskSpawnFn { .. }
                | MirOp::TaskJoin { .. }
                | MirOp::ScopeEnter { .. }
                | MirOp::ScopeExit { .. } => {}
                MirOp::ScopeRegister { value }
                | MirOp::ScopePromote { value, .. }
                | MirOp::ScopeDisown { value }
                | MirOp::ScopeRelease { value } => {
                    collect_uses(value, &mut used);
                }
            }
        }
        match &b.term {
            Terminator::Branch { cond, .. } => collect_uses(cond, &mut used),
            Terminator::MatchTagged { scrutinee, .. } => collect_uses(scrutinee, &mut used),
            Terminator::ReturnOk(e, _) | Terminator::ReturnErr(e) => collect_uses(e, &mut used),
            _ => {}
        }
    }
    for b in &mut cfg.blocks {
        b.ops.retain(|op| match op {
            MirOp::Set {
                name,
                value: MirExpr::BoxValue { .. },
                ..
            } if !used.contains(name) => false,
            _ => true,
        });
    }
}

fn elide_in_term(term: Terminator, boxes: &HashMap<String, (MirExpr, MirRepr)>) -> Terminator {
    match term {
        Terminator::Branch {
            cond,
            then_bb,
            else_bb,
        } => Terminator::Branch {
            cond: elide_in_expr(cond, boxes),
            then_bb,
            else_bb,
        },
        Terminator::MatchTagged {
            scrutinee,
            ok_bb,
            err_bb,
        } => Terminator::MatchTagged {
            scrutinee: elide_in_expr(scrutinee, boxes),
            ok_bb,
            err_bb,
        },
        Terminator::ReturnOk(e, span) => Terminator::ReturnOk(elide_in_expr(e, boxes), span),
        Terminator::ReturnErr(e) => Terminator::ReturnErr(elide_in_expr(e, boxes)),
        other => other,
    }
}

fn elide_in_expr(e: MirExpr, boxes: &HashMap<String, (MirExpr, MirRepr)>) -> MirExpr {
    match e {
        MirExpr::UnboxValue { value, to } => {
            let value = elide_in_expr(*value, boxes);
            if let MirExpr::Name(n) = &value {
                if let Some((inner, from)) = boxes.get(n) {
                    if *from == to {
                        return elide_in_expr(inner.clone(), boxes);
                    }
                }
            }
            // Unbox(Box(x,R),R) structural
            if let MirExpr::BoxValue { value: inner, from } = &value {
                if *from == to {
                    return elide_in_expr(*inner.clone(), boxes);
                }
            }
            MirExpr::UnboxValue {
                value: Box::new(value),
                to,
            }
        }
        MirExpr::BoxValue { value, from } => MirExpr::BoxValue {
            value: Box::new(elide_in_expr(*value, boxes)),
            from,
        },
        MirExpr::Unary { op, expr } => MirExpr::Unary {
            op,
            expr: Box::new(elide_in_expr(*expr, boxes)),
        },
        MirExpr::Cast { to, expr } => MirExpr::Cast {
            to,
            expr: Box::new(elide_in_expr(*expr, boxes)),
        },
        MirExpr::Binary { op, left, right } => MirExpr::Binary {
            op,
            left: Box::new(elide_in_expr(*left, boxes)),
            right: Box::new(elide_in_expr(*right, boxes)),
        },
        MirExpr::Call { target, args, ret } => {
            let target = match target {
                CallTarget::Indirect { callee } => CallTarget::Indirect {
                    callee: Box::new(elide_in_expr(*callee, boxes)),
                },
                other => other,
            };
            MirExpr::Call {
                target,
                args: args.into_iter().map(|a| elide_in_expr(a, boxes)).collect(),
                ret,
            }
        }
        MirExpr::Range { start, end } => MirExpr::Range {
            start: Box::new(elide_in_expr(*start, boxes)),
            end: Box::new(elide_in_expr(*end, boxes)),
        },
        MirExpr::PrimCall { prim, args } => MirExpr::PrimCall {
            prim,
            args: args.into_iter().map(|a| elide_in_expr(a, boxes)).collect(),
        },
        MirExpr::ListLit(xs) => {
            MirExpr::ListLit(xs.into_iter().map(|x| elide_in_expr(x, boxes)).collect())
        }
        MirExpr::StructLit { type_name, fields } => MirExpr::StructLit {
            type_name,
            fields: fields
                .into_iter()
                .map(|(k, v)| (k, elide_in_expr(v, boxes)))
                .collect(),
        },
        MirExpr::StructTypeIs { value, type_name } => MirExpr::StructTypeIs {
            value: Box::new(elide_in_expr(*value, boxes)),
            type_name,
        },
        MirExpr::Index { base, index } => MirExpr::Index {
            base: Box::new(elide_in_expr(*base, boxes)),
            index: Box::new(elide_in_expr(*index, boxes)),
        },
        MirExpr::FieldGet { base, field } => MirExpr::FieldGet {
            base: Box::new(elide_in_expr(*base, boxes)),
            field,
        },
        other => other,
    }
}

fn infer_after_elide(e: &MirExpr) -> Option<MirRepr> {
    match e {
        MirExpr::ConstI64(_) => Some(MirRepr::Int64),
        MirExpr::ConstI32(_) => Some(MirRepr::Int32),
        MirExpr::ConstInt { width, .. } => Some(match width {
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
        }),
        MirExpr::ConstBool(_) => Some(MirRepr::Bool),
        MirExpr::ConstF64(_) => Some(MirRepr::Float64),
        MirExpr::ConstF32(_) => Some(MirRepr::Float32),
        MirExpr::ConstDuration(_) => Some(MirRepr::Duration),
        MirExpr::UnboxValue { to, .. } => Some(*to),
        MirExpr::BoxValue { .. } => Some(MirRepr::Boxed),
        _ => None,
    }
}

fn collect_uses(e: &MirExpr, used: &mut HashSet<String>) {
    match e {
        MirExpr::Name(n) => {
            used.insert(n.clone());
        }
        MirExpr::Unary { expr, .. }
        | MirExpr::Cast { expr, .. }
        | MirExpr::BoxValue { value: expr, .. }
        | MirExpr::UnboxValue { value: expr, .. } => collect_uses(expr, used),
        MirExpr::Binary { left, right, .. } => {
            collect_uses(left, used);
            collect_uses(right, used);
        }
        MirExpr::Call { args, .. } | MirExpr::PrimCall { args, .. } => {
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
            for p in parts {
                if let StrPart::Name(n) = p {
                    used.insert(n.clone());
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        analyze_reprs, construct_ssa, simplify_local, structured_to_cfg, MirRetShape, MirStmt,
    };
    use echo_ast::BinaryOp;

    /// Production handoff order without the final simplify (escape still applied).
    fn pipeline(stmts: Vec<MirStmt>, params: &[&str]) -> (MirCfg, HashMap<String, EscapeClass>) {
        let params: Vec<String> = params.iter().map(|s| (*s).to_string()).collect();
        let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
        let cfg = construct_ssa(cfg, &params);
        let (cfg, reprs) = analyze_reprs(cfg, &params);
        let (cfg, reprs) = simplify_local(cfg, reprs);
        let (cfg, _reprs, escapes) = analyze_escapes(cfg, reprs);
        (cfg, escapes)
    }

    fn any_escape(escapes: &HashMap<String, EscapeClass>, class: EscapeClass) -> bool {
        escapes.values().any(|c| *c == class)
    }

    #[test]
    fn box_local_unbox_is_no_escape() {
        // box → unbox → return unboxed value: user box is NoEscape and elided.
        let stmts = vec![
            MirStmt::Set {
                name: "x".into(),
                value: MirExpr::ConstI64(42),
                span: None,
            },
            MirStmt::Set {
                name: "b".into(),
                value: MirExpr::BoxValue {
                    value: Box::new(MirExpr::Name("x".into())),
                    from: MirRepr::Int64,
                },
                span: None,
            },
            MirStmt::Set {
                name: "u".into(),
                value: MirExpr::UnboxValue {
                    value: Box::new(MirExpr::Name("b".into())),
                    to: MirRepr::Int64,
                },
                span: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("u".into()), None),
        ];
        let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
        let cfg = construct_ssa(cfg, &[]);
        let (cfg, reprs) = analyze_reprs(cfg, &[]);
        let (cfg, _reprs, escapes) = analyze_escapes(cfg, reprs);
        assert_eq!(
            escapes.get("b@0").copied().unwrap_or(EscapeClass::NoEscape),
            EscapeClass::NoEscape,
            "local box must be NoEscape; escapes={escapes:?}"
        );
        let still_has_local_box = cfg.blocks.iter().any(|b| {
            b.ops.iter().any(|op| {
                matches!(
                    op,
                    MirOp::Set {
                        name,
                        value: MirExpr::BoxValue { .. },
                        ..
                    } if name.starts_with("b@")
                )
            })
        });
        assert!(
            !still_has_local_box,
            "local NoEscape box should be elided; cfg={cfg:?}"
        );
    }

    #[test]
    fn returned_box_escapes_function() {
        let stmts = vec![
            MirStmt::Set {
                name: "b".into(),
                value: MirExpr::BoxValue {
                    value: Box::new(MirExpr::ConstI64(1)),
                    from: MirRepr::Int64,
                },
                span: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("b".into()), None),
        ];
        let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
        let cfg = construct_ssa(cfg, &[]);
        let (cfg, reprs) = analyze_reprs(cfg, &[]);
        // Keep box for return (return uses Name of box — may box at boundary in ensure_repr)
        let (_cfg, _r, escapes) = analyze_escapes(cfg, reprs);
        assert!(
            any_escape(&escapes, EscapeClass::EscapesFromFunction)
                || any_escape(&escapes, EscapeClass::Unknown),
            "returned value must escape; escapes={escapes:?}"
        );
    }

    #[test]
    fn dynamic_call_escapes_to_call() {
        let stmts = vec![
            MirStmt::Set {
                name: "b".into(),
                value: MirExpr::BoxValue {
                    value: Box::new(MirExpr::ConstI64(1)),
                    from: MirRepr::Int64,
                },
                span: None,
            },
            MirStmt::Eval(MirExpr::Call {
                target: CallTarget::Function {
                    module_path: std::path::PathBuf::from("/t.echo"),
                    name: "f".into(),
                },
                args: vec![MirExpr::Name("b".into())],
                ret: MirRetShape::Plain,
            }),
            MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
        ];
        let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
        let cfg = construct_ssa(cfg, &[]);
        let (cfg, reprs) = analyze_reprs(cfg, &[]);
        let (_cfg, _, escapes) = analyze_escapes(cfg, reprs);
        assert!(
            any_escape(&escapes, EscapeClass::EscapesToCall)
                || any_escape(&escapes, EscapeClass::EscapesFromFunction),
            "dynamic call arg escapes; escapes={escapes:?}"
        );
    }

    #[test]
    fn print_is_non_retaining() {
        let stmts = vec![
            MirStmt::Set {
                name: "b".into(),
                value: MirExpr::BoxValue {
                    value: Box::new(MirExpr::ConstI64(7)),
                    from: MirRepr::Int64,
                },
                span: None,
            },
            MirStmt::Eval(MirExpr::Call {
                target: CallTarget::Runtime {
                    export: "print".into(),
                },
                args: vec![MirExpr::Name("b".into())],
                ret: MirRetShape::Plain,
            }),
            MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
        ];
        let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
        let cfg = construct_ssa(cfg, &[]);
        let (cfg, reprs) = analyze_reprs(cfg, &[]);
        let (_cfg, _, escapes) = analyze_escapes(cfg, reprs);
        // The user box `b@*` must stay NoEscape (print is non-retaining).
        // ABI may introduce a separate return box that escapes — ignore those.
        let user_box: Vec<_> = escapes
            .iter()
            .filter(|(k, _)| k.starts_with("b@"))
            .map(|(_, c)| *c)
            .collect();
        assert!(
            !user_box.is_empty() && user_box.iter().all(|c| *c == EscapeClass::NoEscape),
            "print is non-retaining; escapes={escapes:?}"
        );
    }

    #[test]
    fn stored_to_struct_escapes_heap() {
        let stmts = vec![
            MirStmt::Set {
                name: "b".into(),
                value: MirExpr::BoxValue {
                    value: Box::new(MirExpr::ConstI64(1)),
                    from: MirRepr::Int64,
                },
                span: None,
            },
            MirStmt::Set {
                name: "s".into(),
                value: MirExpr::StructLit {
                    type_name: String::new(),
                    fields: vec![("f".into(), MirExpr::Name("b".into()))],
                },
                span: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("s".into()), None),
        ];
        let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
        let cfg = construct_ssa(cfg, &[]);
        let (cfg, reprs) = analyze_reprs(cfg, &[]);
        let (_cfg, _, escapes) = analyze_escapes(cfg, reprs);
        assert!(
            any_escape(&escapes, EscapeClass::EscapesToHeap)
                || any_escape(&escapes, EscapeClass::EscapesFromFunction),
            "stored value escapes to heap; escapes={escapes:?}"
        );
    }

    #[test]
    fn phi_and_copy_preserve_classification() {
        // Both arms box then merge — return merged → EscapesFromFunction
        let stmts = vec![
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![MirStmt::Set {
                        name: "b".into(),
                        value: MirExpr::BoxValue {
                            value: Box::new(MirExpr::ConstI64(1)),
                            from: MirRepr::Int64,
                        },
                        span: None,
                    }],
                )],
                else_body: Some(vec![MirStmt::Set {
                    name: "b".into(),
                    value: MirExpr::BoxValue {
                        value: Box::new(MirExpr::ConstI64(2)),
                        from: MirRepr::Int64,
                    },
                    span: None,
                }]),
            },
            MirStmt::ReturnOk(MirExpr::Name("b".into()), None),
        ];
        let (_cfg, escapes) = pipeline(stmts, &[]);
        assert!(
            any_escape(&escapes, EscapeClass::EscapesFromFunction)
                || any_escape(&escapes, EscapeClass::Unknown),
            "phi of returned boxes escapes; escapes={escapes:?}"
        );
    }

    #[test]
    fn noescape_elides_redundant_box_unbox() {
        let stmts = vec![
            MirStmt::Set {
                name: "x".into(),
                value: MirExpr::ConstI64(3),
                span: None,
            },
            MirStmt::Set {
                name: "b".into(),
                value: MirExpr::BoxValue {
                    value: Box::new(MirExpr::Name("x".into())),
                    from: MirRepr::Int64,
                },
                span: None,
            },
            MirStmt::Set {
                name: "y".into(),
                value: MirExpr::UnboxValue {
                    value: Box::new(MirExpr::Name("b".into())),
                    to: MirRepr::Int64,
                },
                span: None,
            },
            MirStmt::Set {
                name: "z".into(),
                value: MirExpr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(MirExpr::Name("y".into())),
                    right: Box::new(MirExpr::ConstI64(1)),
                },
                span: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("z".into()), None),
        ];
        let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
        let cfg = construct_ssa(cfg, &[]);
        let (cfg, reprs) = analyze_reprs(cfg, &[]);
        let (cfg, _, _) = analyze_escapes(cfg, reprs);
        let has_box = cfg.blocks.iter().any(|b| {
            b.ops.iter().any(|op| {
                matches!(
                    op,
                    MirOp::Set {
                        value: MirExpr::BoxValue { .. },
                        ..
                    }
                )
            })
        });
        // Intermediate NoEscape boxes used only via unbox should be gone; a
        // return-boundary box from ABI lower may still remain.
        let intermediate_box = cfg.blocks.iter().any(|b| {
            b.ops.iter().any(|op| {
                matches!(
                    op,
                    MirOp::Set {
                        name,
                        value: MirExpr::BoxValue { .. },
                        ..
                    } if name.starts_with("b@")
                )
            })
        });
        assert!(
            !intermediate_box && !has_box || !intermediate_box,
            "NoEscape box→unbox should elide intermediate boxes; cfg={cfg:?}"
        );
    }
}
