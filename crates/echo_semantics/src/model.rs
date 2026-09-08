//! Semantic model facts attached to an analyzed module.
//!
//! These facts are the **only** source of language meaning for executable
//! lowering. MIR must not invent struct types or import status.

use std::collections::HashMap;

use echo_source::Span;

use crate::BindingKind;

/// Stable binding identity within one analyzed module (analysis session).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindId(pub u32);

/// Coarse value kind for lowering (not a full industrial type system).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueKind {
    Unknown,
    Int,
    Bool,
    String,
    List,
    /// Instance of a named `%` struct.
    Struct {
        name: String,
    },
    /// Import module object (`/ path` last segment).
    Module,
}

impl ValueKind {
    /// DWARF / diagnostic label (not a user-written type name).
    #[must_use]
    pub fn as_di_label(&self) -> &str {
        match self {
            Self::Unknown => "unknown",
            Self::Int => "i64",
            Self::Bool => "bool",
            Self::String => "string",
            Self::List => "list",
            Self::Struct { name } => name.as_str(),
            Self::Module => "module",
        }
    }

    #[must_use]
    pub fn struct_name(&self) -> Option<&str> {
        match self {
            Self::Struct { name } => Some(name.as_str()),
            _ => None,
        }
    }
}

/// One introduced name in the module (top-level or nested, name-keyed for v1).
#[derive(Debug, Clone)]
pub struct BindFact {
    pub id: BindId,
    pub name: String,
    pub binding: BindingKind,
    pub value_kind: ValueKind,
    pub span: Span,
    /// Owning lexical scope id for lifetime (0 = function/module root of the walk).
    /// Slice-2 ownership facts — consumable by analysis product / MIR inject.
    pub owning_scope: u32,
}

/// Analysis facts for one module — consumed by MIR, never re-derived ad hoc.
#[derive(Debug, Clone, Default)]
pub struct SemanticModel {
    next_id: u32,
    /// Local / param / import name → fact.
    pub binds: HashMap<String, BindFact>,
    /// Locals known to hold a struct instance → struct type name.
    pub value_struct: HashMap<String, String>,
    /// `(struct_name, method_name)` → method returns the receiver (`.`).
    pub returns_receiver: HashMap<(String, String), bool>,
    /// Next free scope id for ownership walks (root is 0).
    pub next_scope_id: u32,
}

impl SemanticModel {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn alloc_id(&mut self) -> BindId {
        let id = BindId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Record or refresh a bind fact at the **current** ownership scope (`owning_scope` 0 = root).
    pub fn introduce(
        &mut self,
        name: impl Into<String>,
        binding: BindingKind,
        value_kind: ValueKind,
        span: Span,
    ) {
        self.introduce_in_scope(name, binding, value_kind, span, 0);
    }

    /// Record or refresh a bind fact with an explicit owning scope id.
    pub fn introduce_in_scope(
        &mut self,
        name: impl Into<String>,
        binding: BindingKind,
        value_kind: ValueKind,
        span: Span,
        owning_scope: u32,
    ) {
        let name = name.into();
        if let ValueKind::Struct { name: ref st } = value_kind {
            self.value_struct.insert(name.clone(), st.clone());
        } else if !matches!(value_kind, ValueKind::Unknown) {
            // Non-struct concrete kinds clear prior struct typing.
            self.value_struct.remove(&name);
        }
        let id = self
            .binds
            .get(&name)
            .map(|b| b.id)
            .unwrap_or_else(|| self.alloc_id());
        // Keep introduction scope on reassignment (same name, no shadowing).
        let owning_scope = self
            .binds
            .get(&name)
            .map(|b| b.owning_scope)
            .unwrap_or(owning_scope);
        self.binds.insert(
            name.clone(),
            BindFact {
                id,
                name,
                binding,
                value_kind,
                span,
                owning_scope,
            },
        );
    }

    /// Allocate a nested scope id for ownership walks.
    pub fn alloc_scope(&mut self) -> u32 {
        self.next_scope_id += 1;
        self.next_scope_id
    }

    /// Owning scope of a bind, if recorded.
    #[must_use]
    pub fn owning_scope_of(&self, name: &str) -> Option<u32> {
        self.binds.get(name).map(|b| b.owning_scope)
    }

    /// True when `value_kind` is a managed heap-bearing kind (v0).
    #[must_use]
    pub fn is_managed_kind(kind: &ValueKind) -> bool {
        matches!(
            kind,
            ValueKind::List | ValueKind::String | ValueKind::Struct { .. }
        )
    }

    /// Copy struct typing from one name to another (assignment of handles).
    pub fn copy_struct_type(&mut self, from: &str, to: &str) {
        if let Some(st) = self.value_struct.get(from).cloned() {
            self.value_struct.insert(to.to_string(), st);
        }
    }

    /// Struct type name for a local, if known.
    #[must_use]
    pub fn struct_of(&self, name: &str) -> Option<&str> {
        self.value_struct.get(name).map(String::as_str)
    }

    #[must_use]
    pub fn is_module_import(&self, name: &str) -> bool {
        matches!(
            self.binds.get(name).map(|b| &b.value_kind),
            Some(ValueKind::Module)
        )
    }

    /// Record whether a method's value returns are only the receiver.
    pub fn set_method_returns_receiver(
        &mut self,
        struct_name: impl Into<String>,
        method: impl Into<String>,
        yes: bool,
    ) {
        self.returns_receiver
            .insert((struct_name.into(), method.into()), yes);
    }

    /// True when `receiver`'s static type has a self-returning `method`.
    #[must_use]
    pub fn method_returns_receiver(&self, receiver: &str, method: &str) -> bool {
        let Some(st) = self.struct_of(receiver) else {
            return false;
        };
        self.returns_receiver
            .get(&(st.to_string(), method.to_string()))
            .copied()
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use echo_source::{BytePos, SourceId};

    fn sp() -> Span {
        Span::new(SourceId::from_u32(0), BytePos(0), BytePos(1))
    }

    #[test]
    fn struct_typing_and_copy() {
        let mut m = SemanticModel::new();
        m.introduce(
            "c",
            BindingKind::Immutable,
            ValueKind::Struct {
                name: "counter".into(),
            },
            sp(),
        );
        assert_eq!(m.struct_of("c"), Some("counter"));
        m.copy_struct_type("c", "d");
        assert_eq!(m.struct_of("d"), Some("counter"));
    }

    #[test]
    fn owning_scope_preserved_on_reassign() {
        let mut m = SemanticModel::new();
        m.introduce_in_scope("xs", BindingKind::Mutable, ValueKind::List, sp(), 0);
        let nested = m.alloc_scope();
        assert_eq!(nested, 1);
        m.introduce_in_scope("xs", BindingKind::Mutable, ValueKind::List, sp(), nested);
        assert_eq!(m.owning_scope_of("xs"), Some(0));
        assert!(SemanticModel::is_managed_kind(&ValueKind::List));
        assert!(!SemanticModel::is_managed_kind(&ValueKind::Int));
    }
}
