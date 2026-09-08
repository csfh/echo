//! Internal value kinds for inference (not a user-facing type language).
//!
//! There are **no keywords**. Names like `result` / `option` in diagnostic
//! dumps are internal kind labels for ok|err / some|none return *shapes*
//! produced by `^` / `!` — not types, structs, or reserved identifiers.
//! See `docs/semantics.md`.

use std::collections::HashMap;
use std::fmt;

/// Inference variable id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VarId(pub u32);

/// Runtime/check kind of a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// Untagged / `<i64>` integer (default int).
    Int,
    /// Explicit `<i8>`.
    Int8,
    /// Explicit `<i16>`.
    Int16,
    /// Explicit `<i32>`.
    Int32,
    /// Explicit `<ui8>` / `byte`.
    UInt8,
    /// Explicit `<ui16>`.
    UInt16,
    /// Explicit `<ui32>`.
    UInt32,
    /// Explicit `<ui64>`.
    UInt64,
    /// Untagged / f64 float.
    Float,
    /// Explicit `<f32>`.
    Float32,
    Bool,
    String,
    Bytes,
    Duration,
    /// Inclusive integer range `lo..hi`.
    Range,
    List(Box<Type>),
    /// `{ k: v, ... }` structural product.
    Anon(Vec<(String, Type)>),
    /// Named struct from `% name` (key is struct name, not module path).
    Named(String),
    /// Union of kinds (v0: mainly named structs from multi-path returns).
    /// Sorted by display for stability; no nested unions.
    Union(Vec<Type>),
    /// Function value.
    Fn {
        params: Vec<Type>,
        ret: Box<Type>,
        /// When false, `params` is empty and call-site arity is not checked
        /// (imported export whose param count is not yet known).
        arity_known: bool,
    },
    Option(Box<Type>),
    Result {
        ok: Box<Type>,
        err: Box<Type>,
    },
    Module,
    /// Empty list element or unconstrained.
    Unknown,
    /// Universal ABI slot (dynamic payload): unifies with any concrete kind
    /// **without freezing** to that kind. Used for unconstrained function
    /// params (after inference) and heterogeneous collection keys/values.
    /// Not a surface keyword — diagnostic label only. See `docs/semantics.md`.
    Value,
    /// After a failed unify (poison).
    Error,
    Var(VarId),
}

impl Type {
    #[must_use]
    pub fn list(elem: Type) -> Self {
        Type::List(Box::new(elem))
    }

    #[must_use]
    pub fn option(inner: Type) -> Self {
        Type::Option(Box::new(inner))
    }

    #[must_use]
    pub fn result(ok: Type, err: Type) -> Self {
        Type::Result {
            ok: Box::new(ok),
            err: Box::new(err),
        }
    }

    #[must_use]
    pub fn from_width(width: echo_ast::Width) -> Self {
        use echo_ast::Width;
        match width {
            Width::I8 => Type::Int8,
            Width::I16 => Type::Int16,
            Width::I32 => Type::Int32,
            Width::I64 => Type::Int,
            Width::Ui8 => Type::UInt8,
            Width::Ui16 => Type::UInt16,
            Width::Ui32 => Type::UInt32,
            Width::Ui64 => Type::UInt64,
            Width::F32 => Type::Float32,
            Width::F64 => Type::Float,
        }
    }

    /// Integers and floats — legal sources/targets of `<width> expr`.
    #[must_use]
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Type::Int
                | Type::Int8
                | Type::Int16
                | Type::Int32
                | Type::UInt8
                | Type::UInt16
                | Type::UInt32
                | Type::UInt64
                | Type::Float
                | Type::Float32
        )
    }

    #[must_use]
    pub fn func(params: Vec<Type>, ret: Type) -> Self {
        Type::Fn {
            params,
            ret: Box::new(ret),
            arity_known: true,
        }
    }

    /// Imported callable: `None` arity stays unchecked (runtime / missing facts).
    /// Known-arity params are [`Type::Value`] — exports carry count, not param
    /// kinds, so one call site must not freeze later mixed-kind calls.
    #[must_use]
    pub fn func_imported(arity: Option<usize>, ret: Type) -> Self {
        match arity {
            Some(n) => Type::func(vec![Type::Value; n], ret),
            None => Type::Fn {
                params: vec![],
                ret: Box::new(ret),
                arity_known: false,
            },
        }
    }

    /// Merge kinds into a flat union (dedup). Single member collapses.
    #[must_use]
    pub fn union_of(parts: impl IntoIterator<Item = Type>) -> Self {
        let mut out: Vec<Type> = Vec::new();
        for p in parts {
            match p {
                Type::Union(xs) => {
                    for x in xs {
                        if !out.contains(&x) {
                            out.push(x);
                        }
                    }
                }
                other => {
                    if !out.contains(&other) {
                        out.push(other);
                    }
                }
            }
        }
        out.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
        match out.len() {
            0 => Type::Unknown,
            1 => out.pop().unwrap(),
            _ => Type::Union(out),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => write!(f, "i64"),
            Type::Int8 => write!(f, "i8"),
            Type::Int16 => write!(f, "i16"),
            Type::Int32 => write!(f, "i32"),
            Type::UInt8 => write!(f, "ui8"),
            Type::UInt16 => write!(f, "ui16"),
            Type::UInt32 => write!(f, "ui32"),
            Type::UInt64 => write!(f, "ui64"),
            Type::Float => write!(f, "f64"),
            Type::Float32 => write!(f, "f32"),
            Type::Bool => write!(f, "bool"),
            Type::String => write!(f, "string"),
            Type::Bytes => write!(f, "bytes"),
            Type::Duration => write!(f, "duration"),
            Type::Range => write!(f, "range"),
            Type::List(t) => write!(f, "list[{t}]"),
            Type::Anon(fields) => {
                write!(f, "{{")?;
                for (i, (n, t)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{n}: {t}")?;
                }
                write!(f, "}}")
            }
            Type::Named(n) => write!(f, "{n}"),
            Type::Union(xs) => {
                for (i, t) in xs.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{t}")?;
                }
                Ok(())
            }
            Type::Fn {
                params,
                ret,
                arity_known,
            } => {
                write!(f, "fn(")?;
                if *arity_known {
                    for (i, p) in params.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{p}")?;
                    }
                } else {
                    write!(f, "...")?;
                }
                write!(f, ") -> {ret}")
            }
            Type::Option(t) => write!(f, "option[{t}]"),
            Type::Result { ok, err } => write!(f, "result[{ok}, {err}]"),
            Type::Module => write!(f, "module"),
            Type::Unknown => write!(f, "unknown"),
            Type::Value => write!(f, "value"),
            Type::Error => write!(f, "error"),
            Type::Var(VarId(id)) => write!(f, "?{id}"),
        }
    }
}

/// Substitution for type variables.
#[derive(Debug, Default, Clone)]
pub struct Subst {
    map: HashMap<VarId, Type>,
}

impl Subst {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, v: VarId, t: Type) {
        self.map.insert(v, t);
    }

    /// Apply substitution deeply.
    #[must_use]
    pub fn apply(&self, t: &Type) -> Type {
        match t {
            Type::Var(v) => {
                if let Some(u) = self.map.get(v) {
                    self.apply(u)
                } else {
                    Type::Var(*v)
                }
            }
            Type::List(e) => Type::list(self.apply(e)),
            Type::Anon(fields) => Type::Anon(
                fields
                    .iter()
                    .map(|(n, ty)| (n.clone(), self.apply(ty)))
                    .collect(),
            ),
            Type::Fn {
                params,
                ret,
                arity_known,
            } => Type::Fn {
                params: params.iter().map(|p| self.apply(p)).collect(),
                ret: Box::new(self.apply(ret)),
                arity_known: *arity_known,
            },
            Type::Option(i) => Type::option(self.apply(i)),
            Type::Result { ok, err } => Type::result(self.apply(ok), self.apply(err)),
            Type::Union(xs) => {
                Type::union_of(xs.iter().map(|x| self.apply(x)))
            }
            other => other.clone(),
        }
    }
}
