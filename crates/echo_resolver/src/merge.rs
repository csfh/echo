//! Merge `%` + `@` struct declarations across the graph.

use echo_diagnostics::{Diagnostic, Diagnostics};
use echo_index::{MemberFact, ModuleFacts, StructFact};
use echo_source::Span;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedMember {
    pub name: String,
    pub leader: echo_ast::BindLeader,
    pub span: Span,
    /// Module path that contributed this member (display).
    pub from: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedStruct {
    pub name: String,
    pub primary_span: Span,
    pub primary_from: String,
    pub members: Vec<MergedMember>,
}

/// Merge all struct facts from modules. Emits `res-*` diagnostics on errors.
pub fn merge_structs(
    modules: &[(String, &ModuleFacts)],
    diagnostics: &mut Diagnostics,
) -> HashMap<String, MergedStruct> {
    // name -> list of (path, fact)
    let mut by_name: HashMap<String, Vec<(String, StructFact)>> = HashMap::new();

    for (path, facts) in modules {
        for s in &facts.structs {
            by_name
                .entry(s.name.clone())
                .or_default()
                .push((path.clone(), s.clone()));
        }
    }

    let mut out = HashMap::new();

    for (name, decls) in by_name {
        let primaries: Vec<_> = decls.iter().filter(|(_, s)| s.is_primary).collect();
        let exts: Vec<_> = decls.iter().filter(|(_, s)| !s.is_primary).collect();

        if primaries.is_empty() {
            if let Some((path, s)) = exts.first() {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "`@ {name}` has no matching `% {name}` in the compilation graph"
                    ))
                    .with_span(s.span)
                    .with_code("res-struct-no-primary"),
                );
                let _ = path;
            }
            continue;
        }

        if primaries.len() > 1 {
            for (path, s) in &primaries {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "duplicate `% {name}` (one primary per graph); also in {path}"
                    ))
                    .with_span(s.span)
                    .with_code("res-struct-dup-primary"),
                );
            }
        }

        let (primary_from, primary) = primaries[0];
        let mut members: Vec<MergedMember> = Vec::new();
        let mut seen: HashMap<String, Span> = HashMap::new();

        let mut add_members = |from: &str, list: &[MemberFact]| {
            for m in list {
                if let Some(_prev) = seen.get(&m.name) {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "duplicate member `{name}.{}` (merged from multiple % / @)",
                            m.name
                        ))
                        .with_span(m.span)
                        .with_code("res-struct-dup-member"),
                    );
                } else {
                    seen.insert(m.name.clone(), m.span);
                    members.push(MergedMember {
                        name: m.name.clone(),
                        leader: m.leader,
                        span: m.span,
                        from: from.to_string(),
                    });
                }
            }
        };

        add_members(primary_from, &primary.members);
        for (from, s) in &exts {
            add_members(from, &s.members);
        }

        out.insert(
            name.clone(),
            MergedStruct {
                name,
                primary_span: primary.span,
                primary_from: primary_from.clone(),
                members,
            },
        );
    }

    out
}
