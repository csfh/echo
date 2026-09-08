//! SSA value representation analysis.
//!
//! Proves when an SSA name can stay a native LLVM scalar/ref instead of the
//! universal runtime Echo value (`i64` bits / heap handle ABI). Inserts
//! explicit [`MirExpr::BoxValue`] / [`MirExpr::UnboxValue`] at boundaries.

use std::collections::HashMap;

use echo_ast::{BinaryOp, UnaryOp};

use crate::cfg::{BlockId, MirCfg, MirOp, Terminator};
use crate::{MirExpr, MirPrim, StrPart};

/// Proven storage representation of an SSA value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MirRepr {
    /// No fact yet / not inferred.
    #[default]
    Unknown,
    /// Runtime Echo value (`i64` payload or heap handle bits).
    Boxed,
    Int64,
    Int8,
    Int16,
    /// Native signed 32-bit int from `<i32>` literals / i32 ops.
    Int32,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Float64,
    /// Native 32-bit float from `<f32>` literals / f32 ops.
    Float32,
    /// Duration as i64 nanoseconds (from `5s`, `10ms`, …).
    Duration,
    Bool,
    StringRef,
    /// Heap bytes handle from `b'…'` / `b"…"`.
    BytesRef,
    /// Heap locator handle from `p'…'` / `p"…"`.
    LocatorRef,
    ObjectRef,
    ListRef,
}

impl MirRepr {
    #[must_use]
    pub fn is_native_scalar(self) -> bool {
        matches!(
            self,
            Self::Int64
                | Self::Int8
                | Self::Int16
                | Self::Int32
                | Self::UInt8
                | Self::UInt16
                | Self::UInt32
                | Self::UInt64
                | Self::Float64
                | Self::Float32
                | Self::Duration
                | Self::Bool
        )
    }

    #[must_use]
    pub fn is_ref(self) -> bool {
        matches!(
            self,
            Self::StringRef | Self::BytesRef | Self::LocatorRef | Self::ObjectRef | Self::ListRef
        )
    }

    /// Whether this is the universal runtime value ABI.
    #[must_use]
    pub fn is_universal(self) -> bool {
        matches!(self, Self::Unknown | Self::Boxed)
    }

    /// φ merge: same rep preserved; disagreement → Boxed (never silent coerce).
    #[must_use]
    pub fn merge_phi(self, other: Self) -> Self {
        if self == other {
            return self;
        }
        // Unknown absorbs until both known
        if self == Self::Unknown {
            return other;
        }
        if other == Self::Unknown {
            return self;
        }
        // Distinct concrete reps must not coerce
        Self::Boxed
    }
}

/// Infer representations and insert box/unbox at φ and ABI boundaries.
#[must_use]
pub fn analyze_reprs(mut cfg: MirCfg, params: &[String]) -> (MirCfg, HashMap<String, MirRepr>) {
    let mut reprs: HashMap<String, MirRepr> = HashMap::new();
    for p in params {
        // Function ABI is the universal Echo value (`i64`).
        reprs.insert(format!("{p}@0"), MirRepr::Boxed);
        reprs.insert(p.clone(), MirRepr::Boxed);
    }

    // Fixed-point over RPO (φ back-edges).
    for _ in 0..64 {
        let before = reprs.clone();
        infer_pass(&cfg, &mut reprs);
        if reprs == before {
            break;
        }
    }

    cfg = insert_phi_boxes(cfg, &mut reprs);
    cfg = insert_abi_boxes(cfg, &mut reprs);

    // Re-infer after rewrites
    for _ in 0..16 {
        let before = reprs.clone();
        infer_pass(&cfg, &mut reprs);
        if reprs == before {
            break;
        }
    }

    (cfg, reprs)
}

fn infer_pass(cfg: &MirCfg, reprs: &mut HashMap<String, MirRepr>) {
    let order = rpo(cfg);
    for bid in order {
        let b = cfg.block(bid);
        for op in &b.ops {
            match op {
                MirOp::Phi { name, incomings } => {
                    let mut r = MirRepr::Unknown;
                    for (_, n) in incomings {
                        let ir = reprs.get(n).copied().unwrap_or(MirRepr::Unknown);
                        r = r.merge_phi(ir);
                    }
                    // If still Unknown with no incomings, leave
                    if r == MirRepr::Unknown && !incomings.is_empty() {
                        // all unknown → Unknown
                    }
                    reprs.insert(name.clone(), r);
                }
                MirOp::MatchPayload { name } => {
                    // Tagged payload is the universal i64 ABI payload.
                    reprs.insert(name.clone(), MirRepr::Boxed);
                }
                MirOp::Set { name, value, .. } => {
                    let r = infer_expr(value, reprs);
                    reprs.insert(name.clone(), r);
                }
                MirOp::Eval(_)
                | MirOp::FieldSet { .. }
                | MirOp::IndexSet { .. }
                | MirOp::ListPush { .. }
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
    }
}

fn infer_expr(e: &MirExpr, reprs: &HashMap<String, MirRepr>) -> MirRepr {
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
            let _ = infer_expr(expr, reprs);
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
        MirExpr::Unary { op, expr } => {
            let er = infer_expr(expr, reprs);
            match op {
                UnaryOp::Neg if er == MirRepr::Int64 => MirRepr::Int64,
                UnaryOp::Neg if er == MirRepr::Int32 => MirRepr::Int32,
                UnaryOp::Neg if er == MirRepr::Float64 => MirRepr::Float64,
                UnaryOp::Neg if er == MirRepr::Float32 => MirRepr::Float32,
                UnaryOp::Neg if er == MirRepr::Duration => MirRepr::Duration,
                UnaryOp::Not if er == MirRepr::Bool => MirRepr::Bool,
                UnaryOp::Not if er == MirRepr::Int64 || er == MirRepr::Int32 => MirRepr::Bool,
                UnaryOp::BitNot if er.is_native_int() => er,
                _ => MirRepr::Unknown,
            }
        }
        MirExpr::Binary { op, left, right } => {
            let lr = infer_expr(left, reprs);
            let rr = infer_expr(right, reprs);
            infer_binary(*op, lr, rr)
        }
        MirExpr::Call { ret, .. } => {
            if ret.is_tagged() {
                // Packed tag+payload is not a native scalar; treat as boxed carrier.
                MirRepr::Boxed
            } else {
                MirRepr::Boxed
            }
        }
        MirExpr::PrimCall { prim, .. } => match prim {
            MirPrim::ListLen => MirRepr::Int64,
            MirPrim::ListGetChecked => MirRepr::Boxed,
        },
        MirExpr::ListLit(_) => MirRepr::ListRef,
        MirExpr::StringLit { .. } | MirExpr::StringInterp { .. } => MirRepr::StringRef,
        MirExpr::BytesLit { .. } | MirExpr::BytesInterp { .. } => MirRepr::BytesRef,
        MirExpr::LocatorLit { .. } | MirExpr::LocatorInterp { .. } => MirRepr::LocatorRef,
        MirExpr::StructLit { .. } => MirRepr::ObjectRef,
        MirExpr::StructTypeIs { value, .. } => {
            let _ = infer_expr(value, reprs);
            MirRepr::Bool
        }
        MirExpr::Index { .. } => MirRepr::Boxed,
        MirExpr::FieldGet { .. } => MirRepr::Boxed,
        MirExpr::BoxValue { .. } => MirRepr::Boxed,
        MirExpr::UnboxValue { to, .. } => *to,
        MirExpr::FnValue { .. } => MirRepr::Boxed,
        MirExpr::Range { start, end } => {
            let _ = (infer_expr(start, reprs), infer_expr(end, reprs));
            MirRepr::Boxed
        }
    }
}

fn is_same_int_pair(left: MirRepr, right: MirRepr) -> Option<MirRepr> {
    if left == right && left.is_native_int() {
        return Some(left);
    }
    // Default i64 / universal ABI yields to a more specific integer width.
    // Free-fn params are Boxed at the ABI; `<ui64> C ^ k` must stay unsigned so
    // `>>` lowers to logical shift (SipHash / rotl patterns).
    match (left, right) {
        (MirRepr::Int64, r) if r.is_native_int() && r != MirRepr::Int64 => Some(r),
        (l, MirRepr::Int64) if l.is_native_int() && l != MirRepr::Int64 => Some(l),
        (l, r) if l.is_unsigned_int() && r.is_universal() => Some(l),
        (l, r) if r.is_unsigned_int() && l.is_universal() => Some(r),
        _ => None,
    }
}

impl MirRepr {
    #[must_use]
    pub fn is_native_int(self) -> bool {
        matches!(
            self,
            Self::Int8
                | Self::Int16
                | Self::Int32
                | Self::Int64
                | Self::UInt8
                | Self::UInt16
                | Self::UInt32
                | Self::UInt64
        )
    }

    #[must_use]
    pub fn is_unsigned_int(self) -> bool {
        matches!(
            self,
            Self::UInt8 | Self::UInt16 | Self::UInt32 | Self::UInt64
        )
    }
}

fn infer_binary(op: BinaryOp, left: MirRepr, right: MirRepr) -> MirRepr {
    use BinaryOp::*;
    match op {
        Add | Sub if left == MirRepr::Duration && right == MirRepr::Duration => MirRepr::Duration,
        Add | Sub | Mul | Div | Rem => {
            if let Some(r) = is_same_int_pair(left, right) {
                r
            } else if left == MirRepr::Float64 && right == MirRepr::Float64 {
                MirRepr::Float64
            } else if left == MirRepr::Float32 && right == MirRepr::Float32 {
                MirRepr::Float32
            } else if (left == MirRepr::Float64 || right == MirRepr::Float64)
                && (left == MirRepr::Float64
                    || right == MirRepr::Float64
                    || left.is_universal()
                    || right.is_universal())
            {
                // Boxed float handle + float literal / peer (codegen unboxes via float_to_f64).
                MirRepr::Float64
            } else if (left == MirRepr::Float32 || right == MirRepr::Float32)
                && (left == MirRepr::Float32
                    || right == MirRepr::Float32
                    || left.is_universal()
                    || right.is_universal())
            {
                MirRepr::Float32
            } else {
                MirRepr::Unknown
            }
        }
        BitAnd | BitOr | BitXor | Shl | Shr => {
            is_same_int_pair(left, right).unwrap_or(MirRepr::Unknown)
        }
        Eq | NotEq | EqEqEq | NotEqEq | Lt | Gt | LtEq | GtEq => {
            // Comparisons yield Bool when operands are proven compatible scalars
            // or both universal (runtime eq still returns bool-as-i64; we treat as Bool).
            if left == right && (left.is_native_scalar() || left.is_ref()) {
                MirRepr::Bool
            } else if left.is_universal() || right.is_universal() {
                // runtime eq path
                MirRepr::Bool
            } else if left == MirRepr::Int64 && right == MirRepr::Int64 {
                MirRepr::Bool
            } else {
                MirRepr::Bool // still a boolean result, even if via runtime
            }
        }
        And | Or => {
            if left == MirRepr::Bool && right == MirRepr::Bool {
                MirRepr::Bool
            } else if left == MirRepr::Int64 && right == MirRepr::Int64 {
                // truthiness on ints → Bool result
                MirRepr::Bool
            } else {
                MirRepr::Unknown
            }
        }
    }
}

/// When φ incomings disagree, box natives in the predecessor so the φ is Boxed.
fn insert_phi_boxes(mut cfg: MirCfg, reprs: &mut HashMap<String, MirRepr>) -> MirCfg {
    // Collect rewrites: (phi_block, phi_op_index, pred, old_name) → new boxed name
    let mut rewrites: Vec<(BlockId, usize, BlockId, String, String, MirRepr)> = Vec::new();

    for b in &cfg.blocks {
        for (pi, op) in b.ops.iter().enumerate() {
            let MirOp::Phi { name, incomings } = op else {
                break;
            };
            let dest_r = reprs.get(name).copied().unwrap_or(MirRepr::Unknown);
            if !dest_r.is_universal() {
                continue; // homogeneous native φ
            }
            for (pred, in_name) in incomings {
                let ir = reprs.get(in_name).copied().unwrap_or(MirRepr::Unknown);
                if ir.is_native_scalar() || ir.is_ref() {
                    let boxed_name = format!("{}__box", in_name);
                    rewrites.push((*pred, pi, b.id, in_name.clone(), boxed_name, ir));
                }
            }
        }
    }

    for (pred, _phi_idx, phi_bb, old, boxed_name, from) in &rewrites {
        // Insert Set boxed = BoxValue(old) before terminator in pred
        let block = &mut cfg.blocks[pred.0 as usize];
        if block
            .ops
            .iter()
            .any(|op| matches!(op, MirOp::Set { name, .. } if name == boxed_name))
        {
            // already inserted
        } else {
            block.ops.push(MirOp::Set {
                name: boxed_name.clone(),
                value: MirExpr::BoxValue {
                    value: Box::new(MirExpr::Name(old.clone())),
                    from: *from,
                },
                span: None,
            });
            reprs.insert(boxed_name.clone(), MirRepr::Boxed);
        }
        // Rewrite φ incoming
        for op in &mut cfg.blocks[phi_bb.0 as usize].ops {
            if let MirOp::Phi { incomings, .. } = op {
                for (p, n) in incomings.iter_mut() {
                    if *p == *pred && n == old {
                        *n = boxed_name.clone();
                    }
                }
            } else {
                break;
            }
        }
    }

    // Ensure φ dest marked Boxed when we boxed incomings
    for b in &cfg.blocks {
        for op in &b.ops {
            if let MirOp::Phi { name, incomings } = op {
                let mut r = MirRepr::Unknown;
                for (_, n) in incomings {
                    r = r.merge_phi(reprs.get(n).copied().unwrap_or(MirRepr::Unknown));
                }
                reprs.insert(name.clone(), r);
            } else {
                break;
            }
        }
    }

    cfg
}

/// Box/unbox at known ABI call boundaries (runtime Echo value args).
fn insert_abi_boxes(mut cfg: MirCfg, reprs: &mut HashMap<String, MirRepr>) -> MirCfg {
    let mut next_tmp = 0u32;
    let mut fresh = |prefix: &str| {
        let n = format!("__{prefix}{next_tmp}");
        next_tmp += 1;
        n
    };

    for bi in 0..cfg.blocks.len() {
        let mut new_ops: Vec<MirOp> = Vec::new();
        let ops = cfg.blocks[bi].ops.clone();
        for op in ops {
            match op {
                MirOp::Set { name, value, span } => {
                    let (pre, value) = rewrite_expr_abi(value, reprs, &mut fresh);
                    for p in pre {
                        if let MirOp::Set { name: ref n, .. } = p {
                            // provisional
                            let _ = n;
                        }
                        // track reprs for temps
                        if let MirOp::Set {
                            name: ref tn,
                            value: ref tv,
                            ..
                        } = p
                        {
                            reprs.insert(tn.clone(), infer_expr(tv, reprs));
                        }
                        new_ops.push(p);
                    }
                    let r = infer_expr(&value, reprs);
                    reprs.insert(name.clone(), r);
                    new_ops.push(MirOp::Set { name, value, span });
                }
                MirOp::Eval(value) => {
                    let (pre, value) = rewrite_expr_abi(value, reprs, &mut fresh);
                    for p in pre {
                        if let MirOp::Set {
                            name: ref tn,
                            value: ref tv,
                            span: None,
                        } = p
                        {
                            reprs.insert(tn.clone(), infer_expr(tv, reprs));
                        }
                        new_ops.push(p);
                    }
                    new_ops.push(MirOp::Eval(value));
                }
                MirOp::FieldSet { base, field, value } => {
                    let (pre1, base) = ensure_repr(base, MirRepr::ObjectRef, reprs, &mut fresh);
                    let (pre2, value) = ensure_repr(value, MirRepr::Boxed, reprs, &mut fresh);
                    for p in pre1.into_iter().chain(pre2) {
                        if let MirOp::Set {
                            name: ref tn,
                            value: ref tv,
                            span: None,
                        } = p
                        {
                            reprs.insert(tn.clone(), infer_expr(tv, reprs));
                        }
                        new_ops.push(p);
                    }
                    new_ops.push(MirOp::FieldSet { base, field, value });
                }
                MirOp::IndexSet { base, index, value } => {
                    let (pre1, base) = ensure_repr(base, MirRepr::ListRef, reprs, &mut fresh);
                    let (pre2, index) = ensure_repr(index, MirRepr::Int64, reprs, &mut fresh);
                    let (pre3, value) = ensure_repr(value, MirRepr::Boxed, reprs, &mut fresh);
                    for p in pre1.into_iter().chain(pre2).chain(pre3) {
                        if let MirOp::Set {
                            name: ref tn,
                            value: ref tv,
                            span: None,
                        } = p
                        {
                            reprs.insert(tn.clone(), infer_expr(tv, reprs));
                        }
                        new_ops.push(p);
                    }
                    new_ops.push(MirOp::IndexSet { base, index, value });
                }
                MirOp::ListPush { base, value } => {
                    let (pre1, base) = ensure_repr(base, MirRepr::ListRef, reprs, &mut fresh);
                    let (pre2, value) = ensure_repr(value, MirRepr::Boxed, reprs, &mut fresh);
                    for p in pre1.into_iter().chain(pre2) {
                        if let MirOp::Set {
                            name: ref tn,
                            value: ref tv,
                            span: None,
                        } = p
                        {
                            reprs.insert(tn.clone(), infer_expr(tv, reprs));
                        }
                        new_ops.push(p);
                    }
                    new_ops.push(MirOp::ListPush { base, value });
                }
                other => new_ops.push(other),
            }
        }

        // Terminators: return / branch cond
        let term = cfg.blocks[bi].term.clone();
        let (term_pre, term) = rewrite_term_abi(term, reprs, &mut fresh);
        for p in term_pre {
            if let MirOp::Set {
                name: ref tn,
                value: ref tv,
                span: None,
            } = p
            {
                reprs.insert(tn.clone(), infer_expr(tv, reprs));
            }
            new_ops.push(p);
        }
        cfg.blocks[bi].ops = new_ops;
        cfg.blocks[bi].term = term;
    }
    cfg
}

fn rewrite_term_abi(
    term: Terminator,
    reprs: &HashMap<String, MirRepr>,
    fresh: &mut impl FnMut(&str) -> String,
) -> (Vec<MirOp>, Terminator) {
    match term {
        Terminator::ReturnOk(e, span) => {
            // Plain return ABI is i64 Echo value.
            let (pre, e) = ensure_repr(e, MirRepr::Boxed, reprs, fresh);
            (pre, Terminator::ReturnOk(e, span))
        }
        Terminator::ReturnErr(e) => {
            let (pre, e) = ensure_repr(e, MirRepr::Boxed, reprs, fresh);
            (pre, Terminator::ReturnErr(e))
        }
        Terminator::Branch {
            cond,
            then_bb,
            else_bb,
        } => {
            // Prefer Bool; allow Int64 truthiness without boxing.
            let cr = infer_expr(&cond, reprs);
            if cr == MirRepr::Bool || cr == MirRepr::Int64 {
                (
                    vec![],
                    Terminator::Branch {
                        cond,
                        then_bb,
                        else_bb,
                    },
                )
            } else {
                let (pre, cond) = ensure_repr(cond, MirRepr::Bool, reprs, fresh);
                (
                    pre,
                    Terminator::Branch {
                        cond,
                        then_bb,
                        else_bb,
                    },
                )
            }
        }
        other => (vec![], other),
    }
}

fn rewrite_expr_abi(
    e: MirExpr,
    reprs: &HashMap<String, MirRepr>,
    fresh: &mut impl FnMut(&str) -> String,
) -> (Vec<MirOp>, MirExpr) {
    match e {
        MirExpr::Call { target, args, ret } => {
            let mut pre = Vec::new();
            let mut new_args = Vec::new();
            for a in args {
                // All Echo calls / runtime take universal values today.
                let (p, a) = ensure_repr(a, MirRepr::Boxed, reprs, fresh);
                pre.extend(p);
                new_args.push(a);
            }
            (
                pre,
                MirExpr::Call {
                    target,
                    args: new_args,
                    ret,
                },
            )
        }
        MirExpr::PrimCall { prim, args } => {
            let mut pre = Vec::new();
            let mut new_args = Vec::new();
            match prim {
                MirPrim::ListLen => {
                    let (p, a) = ensure_list_handle(args.into_iter().next(), reprs, fresh);
                    pre.extend(p);
                    new_args.push(a);
                }
                MirPrim::ListGetChecked => {
                    let mut it = args.into_iter();
                    let (p1, list) = ensure_list_handle(it.next(), reprs, fresh);
                    let (p2, idx) = ensure_repr(
                        it.next().unwrap_or(MirExpr::ConstI64(0)),
                        MirRepr::Int64,
                        reprs,
                        fresh,
                    );
                    pre.extend(p1);
                    pre.extend(p2);
                    new_args.push(list);
                    new_args.push(idx);
                }
            }
            (
                pre,
                MirExpr::PrimCall {
                    prim,
                    args: new_args,
                },
            )
        }
        MirExpr::Binary { op, left, right } => {
            // Keep native arithmetic native; only rewrite children structurally.
            let (p1, left) = rewrite_expr_abi(*left, reprs, fresh);
            let (p2, right) = rewrite_expr_abi(*right, reprs, fresh);
            let mut pre = p1;
            pre.extend(p2);
            // For int arith, ensure Int64 operands. Do **not** force Int64 when
            // either side is float — that unboxed heap floats as integers and
            // broke `x + 1.0` / `a < 4.0` (codegen then saw f64 bits as i64).
            let lr = infer_expr(&left, reprs);
            let rr = infer_expr(&right, reprs);
            let either_float = lr == MirRepr::Float64
                || rr == MirRepr::Float64
                || lr == MirRepr::Float32
                || rr == MirRepr::Float32;
            let needs_int = !either_float
                && matches!(
                    op,
                    BinaryOp::Add
                        | BinaryOp::Sub
                        | BinaryOp::Mul
                        | BinaryOp::Div
                        | BinaryOp::Rem
                        | BinaryOp::Lt
                        | BinaryOp::Gt
                        | BinaryOp::LtEq
                        | BinaryOp::GtEq
                );
            if needs_int {
                let (left, extra1) = if lr == MirRepr::Int64 {
                    (left, vec![])
                } else if lr.is_universal() {
                    let (p, e) = ensure_repr(left, MirRepr::Int64, reprs, fresh);
                    (e, p)
                } else {
                    (left, vec![])
                };
                let (right, extra2) = if rr == MirRepr::Int64 {
                    (right, vec![])
                } else if rr.is_universal() {
                    let (p, e) = ensure_repr(right, MirRepr::Int64, reprs, fresh);
                    (e, p)
                } else {
                    (right, vec![])
                };
                pre.extend(extra1);
                pre.extend(extra2);
                (
                    pre,
                    MirExpr::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                )
            } else {
                (
                    pre,
                    MirExpr::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                )
            }
        }
        MirExpr::Unary { op, expr } => {
            let (pre, expr) = rewrite_expr_abi(*expr, reprs, fresh);
            (
                pre,
                MirExpr::Unary {
                    op,
                    expr: Box::new(expr),
                },
            )
        }
        MirExpr::ListLit(items) => {
            let mut pre = Vec::new();
            let mut new_items = Vec::new();
            for it in items {
                let (p, it) = ensure_repr(it, MirRepr::Boxed, reprs, fresh);
                pre.extend(p);
                new_items.push(it);
            }
            (pre, MirExpr::ListLit(new_items))
        }
        MirExpr::StructLit { type_name, fields } => {
            let mut pre = Vec::new();
            let mut new_fields = Vec::new();
            for (k, v) in fields {
                let (p, v) = ensure_repr(v, MirRepr::Boxed, reprs, fresh);
                pre.extend(p);
                new_fields.push((k, v));
            }
            (
                pre,
                MirExpr::StructLit {
                    type_name,
                    fields: new_fields,
                },
            )
        }
        MirExpr::StructTypeIs { value, type_name } => {
            let (p, v) = ensure_repr(*value, MirRepr::Boxed, reprs, fresh);
            (
                p,
                MirExpr::StructTypeIs {
                    value: Box::new(v),
                    type_name,
                },
            )
        }
        MirExpr::StringInterp { parts } => {
            let (pre, parts) = rewrite_interp_parts(parts, reprs, fresh);
            (pre, MirExpr::StringInterp { parts })
        }
        MirExpr::LocatorInterp { parts } => {
            let (pre, parts) = rewrite_interp_parts(parts, reprs, fresh);
            (pre, MirExpr::LocatorInterp { parts })
        }
        MirExpr::BytesInterp { parts } => {
            let (pre, parts) = rewrite_interp_parts(parts, reprs, fresh);
            (pre, MirExpr::BytesInterp { parts })
        }
        other => (vec![], other),
    }
}

fn rewrite_interp_parts(
    parts: Vec<StrPart>,
    reprs: &HashMap<String, MirRepr>,
    fresh: &mut impl FnMut(&str) -> String,
) -> (Vec<MirOp>, Vec<StrPart>) {
    // Names in interp are pushed as universal values.
    let mut pre = Vec::new();
    let mut new_parts = Vec::new();
    for part in parts {
        match part {
            StrPart::Lit(b) => new_parts.push(StrPart::Lit(b)),
            StrPart::Name(n) => {
                let r = reprs.get(&n).copied().unwrap_or(MirRepr::Unknown);
                if r.is_native_scalar() || r.is_ref() {
                    let tmp = fresh("box");
                    pre.push(MirOp::Set {
                        name: tmp.clone(),
                        value: MirExpr::BoxValue {
                            value: Box::new(MirExpr::Name(n)),
                            from: r,
                        },
                        span: None,
                    });
                    new_parts.push(StrPart::Name(tmp));
                } else {
                    new_parts.push(StrPart::Name(n));
                }
            }
        }
    }
    (pre, new_parts)
}

fn ensure_list_handle(
    e: Option<MirExpr>,
    reprs: &HashMap<String, MirRepr>,
    fresh: &mut impl FnMut(&str) -> String,
) -> (Vec<MirOp>, MirExpr) {
    let e = e.unwrap_or(MirExpr::ConstI64(0));
    let r = infer_expr(&e, reprs);
    if r == MirRepr::ListRef || r.is_universal() {
        // ListRef is ptr-like; ABI still i64 handle — box ListRef to universal.
        if r == MirRepr::ListRef {
            ensure_repr(e, MirRepr::Boxed, reprs, fresh)
        } else {
            (vec![], e)
        }
    } else {
        ensure_repr(e, MirRepr::Boxed, reprs, fresh)
    }
}

/// Ensure `e` has representation `want`, inserting Box/Unbox SSA temps as needed.
fn ensure_repr(
    e: MirExpr,
    want: MirRepr,
    reprs: &HashMap<String, MirRepr>,
    fresh: &mut impl FnMut(&str) -> String,
) -> (Vec<MirOp>, MirExpr) {
    let have = infer_expr(&e, reprs);
    if have == want {
        return (vec![], e);
    }
    // Already universal and want Boxed/Unknown
    if want.is_universal() && have.is_universal() {
        return (vec![], e);
    }
    if want.is_universal() {
        // native/ref → box
        if have.is_native_scalar() || have.is_ref() {
            let tmp = fresh("box");
            let op = MirOp::Set {
                name: tmp.clone(),
                value: MirExpr::BoxValue {
                    value: Box::new(e),
                    from: have,
                },
                span: None,
            };
            return (vec![op], MirExpr::Name(tmp));
        }
        return (vec![], e);
    }
    // want native, have universal → unbox
    if have.is_universal() && (want.is_native_scalar() || want.is_ref()) {
        let tmp = fresh("unbox");
        let op = MirOp::Set {
            name: tmp.clone(),
            value: MirExpr::UnboxValue {
                value: Box::new(e),
                to: want,
            },
            span: None,
        };
        return (vec![op], MirExpr::Name(tmp));
    }
    // want native, have different native → refuse silent coerce; box then unbox would be wrong.
    // Leave as-is; codegen may error or treat as unknown.
    (vec![], e)
}

fn rpo(cfg: &MirCfg) -> Vec<BlockId> {
    let n = cfg.blocks.len();
    let mut seen = vec![false; n];
    let mut post = Vec::new();
    fn dfs(cfg: &MirCfg, id: BlockId, seen: &mut [bool], post: &mut Vec<BlockId>) {
        if seen[id.0 as usize] {
            return;
        }
        seen[id.0 as usize] = true;
        for s in cfg.successors(id) {
            dfs(cfg, s, seen, post);
        }
        post.push(id);
    }
    dfs(cfg, cfg.entry, &mut seen, &mut post);
    for b in &cfg.blocks {
        dfs(cfg, b.id, seen.as_mut(), &mut post);
    }
    post.reverse();
    post
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{construct_ssa, structured_to_cfg, MirRetShape, MirStmt};

    fn ssa_of(stmts: Vec<MirStmt>) -> (MirCfg, HashMap<String, MirRepr>) {
        let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
        let cfg = construct_ssa(cfg, &[]);
        analyze_reprs(cfg, &[])
    }

    #[test]
    fn ui64_xor_boxed_param_stays_unsigned() {
        // Free-fn params are Boxed; paper constants are UInt64.
        assert_eq!(
            is_same_int_pair(MirRepr::UInt64, MirRepr::Boxed),
            Some(MirRepr::UInt64)
        );
        assert_eq!(
            is_same_int_pair(MirRepr::Boxed, MirRepr::UInt64),
            Some(MirRepr::UInt64)
        );
        assert_eq!(
            infer_binary(BinaryOp::BitXor, MirRepr::UInt64, MirRepr::Boxed),
            MirRepr::UInt64
        );
        assert_eq!(
            infer_binary(BinaryOp::Shr, MirRepr::UInt64, MirRepr::Int64),
            MirRepr::UInt64
        );
    }

    #[test]
    fn int_arith_is_int64() {
        let stmts = vec![
            MirStmt::Set {
                name: "a".into(),
                value: MirExpr::ConstI64(1),
                span: None,
            },
            MirStmt::Set {
                name: "b".into(),
                value: MirExpr::ConstI64(2),
                span: None,
            },
            MirStmt::Set {
                name: "c".into(),
                value: MirExpr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(MirExpr::Name("a".into())),
                    right: Box::new(MirExpr::Name("b".into())),
                },
                span: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("c".into()), None),
        ];
        let (_cfg, reprs) = ssa_of(stmts);
        let c = reprs
            .iter()
            .find(|(k, _)| k.starts_with("c@"))
            .map(|(_, r)| *r);
        // c@0 = 1+2 style — last c version
        let c_reps: Vec<_> = reprs
            .iter()
            .filter(|(k, _)| k.starts_with("c@"))
            .map(|(_, r)| *r)
            .collect();
        assert!(
            c_reps.iter().any(|r| *r == MirRepr::Int64),
            "c_reps={c_reps:?} all={reprs:?} c={c:?}"
        );
    }

    #[test]
    fn comparison_is_bool() {
        let stmts = vec![
            MirStmt::Set {
                name: "t".into(),
                value: MirExpr::Binary {
                    op: BinaryOp::Lt,
                    left: Box::new(MirExpr::ConstI64(1)),
                    right: Box::new(MirExpr::ConstI64(2)),
                },
                span: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("t".into()), None),
        ];
        let (_cfg, reprs) = ssa_of(stmts);
        let t_reps: Vec<_> = reprs
            .iter()
            .filter(|(k, _)| k.starts_with("t@"))
            .map(|(_, r)| *r)
            .collect();
        assert!(t_reps.contains(&MirRepr::Bool), "{t_reps:?}");
    }

    #[test]
    fn same_type_phi_stays_int64() {
        let stmts = vec![
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![MirStmt::Set {
                        name: "x".into(),
                        value: MirExpr::ConstI64(1),
                        span: None,
                    }],
                )],
                else_body: Some(vec![MirStmt::Set {
                    name: "x".into(),
                    value: MirExpr::ConstI64(2),
                    span: None,
                }]),
            },
            MirStmt::ReturnOk(MirExpr::Name("x".into()), None),
        ];
        let (cfg, reprs) = ssa_of(stmts);
        let phi = cfg.blocks.iter().find_map(|b| {
            b.ops.iter().find_map(|op| match op {
                MirOp::Phi { name, .. } => Some(name.clone()),
                _ => None,
            })
        });
        let Some(phi_name) = phi else {
            // might fold differently — at least some x@ is Int64
            assert!(reprs.values().any(|r| *r == MirRepr::Int64));
            return;
        };
        assert_eq!(
            reprs.get(&phi_name).copied().unwrap_or(MirRepr::Unknown),
            MirRepr::Int64,
            "phi {phi_name} reprs={reprs:?}"
        );
    }

    #[test]
    fn mixed_phi_boxes() {
        // x = 1 in then (Int64), x = true-as-bool path: use ConstBool in else
        let stmts = vec![
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![MirStmt::Set {
                        name: "x".into(),
                        value: MirExpr::ConstI64(1),
                        span: None,
                    }],
                )],
                else_body: Some(vec![MirStmt::Set {
                    name: "x".into(),
                    value: MirExpr::ConstBool(true),
                    span: None,
                }]),
            },
            MirStmt::ReturnOk(MirExpr::Name("x".into()), None),
        ];
        let (cfg, reprs) = ssa_of(stmts);
        let phi = cfg.blocks.iter().find_map(|b| {
            b.ops.iter().find_map(|op| match op {
                MirOp::Phi { name, incomings } => Some((name.clone(), incomings.clone())),
                _ => None,
            })
        });
        let Some((phi_name, incs)) = phi else {
            panic!("expected phi");
        };
        assert!(
            matches!(
                reprs.get(&phi_name).copied(),
                Some(MirRepr::Boxed) | Some(MirRepr::Unknown)
            ),
            "phi should not stay a single native; got {:?} reprs={reprs:?}",
            reprs.get(&phi_name)
        );
        // Incomings should be boxed names or box ops exist
        let has_box_op = cfg.blocks.iter().any(|b| {
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
        assert!(
            has_box_op || incs.iter().any(|(_, n)| n.contains("__box")),
            "expected box conversion; incs={incs:?}"
        );
    }

    #[test]
    fn call_args_get_boxed() {
        let stmts = vec![
            MirStmt::Set {
                name: "n".into(),
                value: MirExpr::ConstI64(42),
                span: None,
            },
            MirStmt::Eval(MirExpr::Call {
                target: crate::CallTarget::Runtime {
                    export: "print".into(),
                },
                args: vec![MirExpr::Name("n".into())],
                ret: MirRetShape::Plain,
            }),
            MirStmt::ReturnOk(MirExpr::ConstI64(0), None),
        ];
        let (cfg, _reprs) = ssa_of(stmts);
        let has_box = cfg.blocks.iter().any(|b| {
            b.ops.iter().any(|op| {
                matches!(
                    op,
                    MirOp::Set {
                        value: MirExpr::BoxValue {
                            from: MirRepr::Int64,
                            ..
                        },
                        ..
                    }
                )
            })
        });
        assert!(has_box, "print arg should box Int64; cfg={cfg:?}");
    }
}
