//! Language-level value class (docs/semantics.md § Value vs reference).
//!
//! Distinct from [`crate::MirRepr`] (storage/ABI). This classifies **pass
//! semantics**: RefValue = copy the reference; StaticValue = copy the value.

use echo_hir::{HirExpr, HirExprKind};
use echo_semantics::ValueKind;

/// Locked userland pass class (struct|list → ref; everything else → value).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueClass {
    /// Struct or list — pass copies the reference (shared object identity).
    RefValue,
    /// Int, float, bool, string, bytes, locator, duration, range, fn, unit, …
    StaticValue,
}

impl ValueClass {
    /// Classify a semantic value kind.
    #[must_use]
    pub fn from_value_kind(k: &ValueKind) -> Self {
        match k {
            ValueKind::Struct { .. } | ValueKind::List => Self::RefValue,
            ValueKind::Unknown
            | ValueKind::Int
            | ValueKind::Bool
            | ValueKind::String
            | ValueKind::Module => Self::StaticValue,
        }
    }

    /// Classify from a MIR storage repr (best-effort; Boxed/Unknown → Static).
    #[must_use]
    pub fn from_mir_repr(r: crate::MirRepr) -> Self {
        use crate::MirRepr;
        match r {
            MirRepr::ObjectRef | MirRepr::ListRef => Self::RefValue,
            MirRepr::Unknown
            | MirRepr::Boxed
            | MirRepr::Int64
            | MirRepr::Int8
            | MirRepr::Int16
            | MirRepr::Int32
            | MirRepr::UInt8
            | MirRepr::UInt16
            | MirRepr::UInt32
            | MirRepr::UInt64
            | MirRepr::Float64
            | MirRepr::Float32
            | MirRepr::Duration
            | MirRepr::Bool
            | MirRepr::StringRef
            | MirRepr::BytesRef
            | MirRepr::LocatorRef => Self::StaticValue,
        }
    }

    /// Best-effort class from a HIR expression (struct lit / list lit / name lookup via `struct_of`).
    #[must_use]
    pub fn from_hir_expr(
        e: &HirExpr,
        name_is_struct: &dyn Fn(&str) -> bool,
        name_is_list: &dyn Fn(&str) -> bool,
    ) -> Self {
        match &e.kind {
            HirExprKind::StructLit { .. } => Self::RefValue,
            HirExprKind::List(_) => Self::RefValue,
            HirExprKind::Name(n) if name_is_struct(n) || name_is_list(n) => Self::RefValue,
            HirExprKind::Group(inner) => {
                Self::from_hir_expr(inner, name_is_struct, name_is_list)
            }
            _ => Self::StaticValue,
        }
    }

    #[must_use]
    pub fn is_ref(self) -> bool {
        matches!(self, Self::RefValue)
    }

    #[must_use]
    pub fn is_static(self) -> bool {
        matches!(self, Self::StaticValue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MirRepr;
    use echo_source::{BytePos, SourceId, Span};

    #[test]
    fn value_kind_struct_and_list_are_ref() {
        assert_eq!(
            ValueClass::from_value_kind(&ValueKind::Struct {
                name: "conn".into()
            }),
            ValueClass::RefValue
        );
        assert_eq!(
            ValueClass::from_value_kind(&ValueKind::List),
            ValueClass::RefValue
        );
        assert_eq!(
            ValueClass::from_value_kind(&ValueKind::Int),
            ValueClass::StaticValue
        );
        assert_eq!(
            ValueClass::from_value_kind(&ValueKind::String),
            ValueClass::StaticValue
        );
    }

    #[test]
    fn mir_repr_object_list_are_ref_strings_static() {
        // Language model: strings are StaticValue (pass by value), even if ABI is a handle.
        assert_eq!(
            ValueClass::from_mir_repr(MirRepr::ObjectRef),
            ValueClass::RefValue
        );
        assert_eq!(
            ValueClass::from_mir_repr(MirRepr::ListRef),
            ValueClass::RefValue
        );
        assert_eq!(
            ValueClass::from_mir_repr(MirRepr::StringRef),
            ValueClass::StaticValue
        );
        assert_eq!(
            ValueClass::from_mir_repr(MirRepr::Int64),
            ValueClass::StaticValue
        );
    }

    #[test]
    fn hir_lit_classification() {
        let span = Span::new(SourceId::from_u32(0), BytePos(0), BytePos(1));
        let sl = HirExpr {
            span,
            kind: HirExprKind::StructLit {
                name: "box".into(),
                fields: vec![],
            },
        };
        let ll = HirExpr {
            span,
            kind: HirExprKind::List(vec![]),
        };
        let n = HirExpr {
            span,
            kind: HirExprKind::Int {
                value: 1,
                width: None,
            },
        };
        let none = |_: &str| false;
        assert_eq!(
            ValueClass::from_hir_expr(&sl, &none, &none),
            ValueClass::RefValue
        );
        assert_eq!(
            ValueClass::from_hir_expr(&ll, &none, &none),
            ValueClass::RefValue
        );
        assert_eq!(
            ValueClass::from_hir_expr(&n, &none, &none),
            ValueClass::StaticValue
        );
        assert_eq!(
            ValueClass::from_hir_expr(
                &HirExpr {
                    span,
                    kind: HirExprKind::Name("c".into()),
                },
                &|n| n == "c",
                &none
            ),
            ValueClass::RefValue
        );
    }
}
