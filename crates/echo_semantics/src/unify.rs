//! Unification of kinds.

use echo_diagnostics::{Diagnostic, Diagnostics};
use echo_source::Span;

use crate::types::{Subst, Type, VarId};

/// Unify `a` and `b` under `subst`. On failure, push `sem-type-mismatch` and
/// return `Type::Error` for recovery.
pub fn unify(subst: &mut Subst, a: &Type, b: &Type, span: Span, diags: &mut Diagnostics) -> Type {
    let a = subst.apply(a);
    let b = subst.apply(b);
    match (&a, &b) {
        (Type::Error, t) | (t, Type::Error) => t.clone(),
        (Type::Unknown, t) | (t, Type::Unknown) => t.clone(),
        // Universal ABI: accept any kind; stay `value` (do not freeze).
        // Do **not** write `value` into the other var — that rewrote the
        // caller's note on the same variable (`test.eq(a, b)` then `a < 0`
        // after `map.bucket_index`). A call must not change how we treat
        // the argument outside the callee (docs/semantics.md).
        (Type::Value, _) | (_, Type::Value) => Type::Value,
        (Type::Var(v), t) | (t, Type::Var(v)) => {
            if let Type::Var(w) = t {
                if v == w {
                    return Type::Var(*v);
                }
            }
            if occurs(*v, t, subst) {
                mismatch(diags, span, &a, &b);
                return Type::Error;
            }
            subst.insert(*v, t.clone());
            t.clone()
        }
        (Type::Int, Type::Int) => Type::Int,
        (Type::Int8, Type::Int8) => Type::Int8,
        (Type::Int16, Type::Int16) => Type::Int16,
        (Type::Int32, Type::Int32) => Type::Int32,
        (Type::UInt8, Type::UInt8) => Type::UInt8,
        (Type::UInt16, Type::UInt16) => Type::UInt16,
        (Type::UInt32, Type::UInt32) => Type::UInt32,
        (Type::UInt64, Type::UInt64) => Type::UInt64,
        // Default untagged/`<i64>` int yields to a more specific integer width
        // so untagged literals adopt the lane of `ui64` fields/ops.
        (Type::Int, t) if is_specific_int(t) => t.clone(),
        (t, Type::Int) if is_specific_int(t) => t.clone(),
        (Type::Float, Type::Float) => Type::Float,
        (Type::Float32, Type::Float32) => Type::Float32,
        (Type::Bool, Type::Bool) => Type::Bool,
        (Type::String, Type::String) => Type::String,
        (Type::Bytes, Type::Bytes) => Type::Bytes,
        (Type::Duration, Type::Duration) => Type::Duration,
        (Type::Range, Type::Range) => Type::Range,
        (Type::Module, Type::Module) => Type::Module,
        (Type::Named(x), Type::Named(y)) if x == y => Type::Named(x.clone()),
        // Named vs Named different → not unified here (return paths use Type::union_of).
        (Type::Union(xs), Type::Union(ys)) => {
            // Compatible if same members (order-insensitive).
            let mut a: Vec<_> = xs.iter().map(|t| subst.apply(t)).collect();
            let mut b: Vec<_> = ys.iter().map(|t| subst.apply(t)).collect();
            a.sort_by(|x, y| x.to_string().cmp(&y.to_string()));
            b.sort_by(|x, y| x.to_string().cmp(&y.to_string()));
            if a == b {
                Type::Union(a)
            } else {
                mismatch(
                    diags,
                    span,
                    &Type::Union(xs.clone()),
                    &Type::Union(ys.clone()),
                );
                Type::Error
            }
        }
        (Type::List(x), Type::List(y)) => Type::list(unify(subst, x, y, span, diags)),
        (Type::Option(x), Type::Option(y)) => Type::option(unify(subst, x, y, span, diags)),
        (
            Type::Result {
                ok: a_ok,
                err: a_err,
            },
            Type::Result {
                ok: b_ok,
                err: b_err,
            },
        ) => Type::result(
            unify(subst, a_ok, b_ok, span, diags),
            unify(subst, a_err, b_err, span, diags),
        ),
        (
            Type::Fn {
                params: ap,
                ret: ar,
                arity_known: aka,
            },
            Type::Fn {
                params: bp,
                ret: br,
                arity_known: akb,
            },
        ) => {
            if *aka && *akb && ap.len() != bp.len() {
                mismatch(diags, span, &a, &b);
                return Type::Error;
            }
            let (params_a, params_b, known) = if *aka && *akb {
                (ap, bp, true)
            } else if *aka {
                (ap, ap, true)
            } else if *akb {
                (bp, bp, true)
            } else {
                (ap, bp, false)
            };
            let mut params = Vec::new();
            for (x, y) in params_a.iter().zip(params_b.iter()) {
                params.push(unify(subst, x, y, span, diags));
            }
            Type::Fn {
                params,
                ret: Box::new(unify(subst, ar, br, span, diags)),
                arity_known: known,
            }
        }
        (Type::Anon(af), Type::Anon(bf)) => {
            // Structural: same field names (order-insensitive), unify each.
            if af.len() != bf.len() {
                mismatch(diags, span, &a, &b);
                return Type::Error;
            }
            let mut map_b: std::collections::HashMap<&str, &Type> =
                bf.iter().map(|(n, t)| (n.as_str(), t)).collect();
            let mut out = Vec::new();
            for (n, ta) in af {
                match map_b.remove(n.as_str()) {
                    Some(tb) => out.push((n.clone(), unify(subst, ta, tb, span, diags))),
                    None => {
                        mismatch(diags, span, &a, &b);
                        return Type::Error;
                    }
                }
            }
            if !map_b.is_empty() {
                mismatch(diags, span, &a, &b);
                return Type::Error;
            }
            Type::Anon(out)
        }
        _ => {
            mismatch(diags, span, &a, &b);
            Type::Error
        }
    }
}

/// Non-default integer widths (`i8`…`i32`, `ui*`). Default untagged/`i64` is [`Type::Int`].
fn is_specific_int(t: &Type) -> bool {
    matches!(
        t,
        Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::UInt8
            | Type::UInt16
            | Type::UInt32
            | Type::UInt64
    )
}

fn occurs(v: VarId, t: &Type, subst: &Subst) -> bool {
    let t = subst.apply(t);
    match t {
        Type::Var(w) => v == w,
        Type::List(e) | Type::Option(e) => occurs(v, &e, subst),
        Type::Result { ok, err } => occurs(v, &ok, subst) || occurs(v, &err, subst),
        Type::Fn { params, ret, .. } => {
            params.iter().any(|p| occurs(v, p, subst)) || occurs(v, &ret, subst)
        }
        Type::Anon(fields) => fields.iter().any(|(_, t)| occurs(v, t, subst)),
        Type::Union(xs) => xs.iter().any(|t| occurs(v, t, subst)),
        _ => false,
    }
}

fn mismatch(diags: &mut Diagnostics, span: Span, a: &Type, b: &Type) {
    diags.push(
        Diagnostic::error(format!("kind mismatch: expected `{a}`, found `{b}`"))
            .with_span(span)
            .with_code("sem-type-mismatch"),
    );
}
