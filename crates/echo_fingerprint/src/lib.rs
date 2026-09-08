//! Compiler component fingerprints for phase cache invalidation.
//!
//! See `docs/incremental.md`. Language-agnostic: no PHP / Composer concepts.
//! Bump a component's version constant when that component's **artifact shape**
//! or meaning changes so cached blobs for dependent phases are discarded.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;

use sha2::{Digest, Sha256};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// On-disk cache format version (layout / key encoding).
pub const CACHE_FORMAT_VERSION: u32 = 2; // phase blobs live under compiler-stamp dirs

// --- Per-component schema versions (bump when that layer's cacheable output changes) ---

pub const LEXER_VERSION: u32 = 3; // lex-escape on unknown rich escapes
pub const PARSER_VERSION: u32 = 8; // `/ ../name` parent-relative import segments
pub const AST_SCHEMA_VERSION: u32 = 6; // ImportPathSeg::DotDot
pub const INDEX_VERSION: u32 = 3; // PathSeg::DotDot
pub const INDEX_SCHEMA_VERSION: u32 = 3; // PathSeg::DotDot
pub const RESOLVER_VERSION: u32 = 5; // `/ ../name` from importer directory
pub const RESOLVE_SCHEMA_VERSION: u32 = 1;
pub const SEMANTICS_VERSION: u32 = 20; // # const duration/bytes/locator lits
pub const HIR_LOWERER_VERSION: u32 = 17; // free-fn returns_structs via local name ^ m
pub const HIR_SCHEMA_VERSION: u32 = 4; // HirExprKind::Range
/// Bumped when MIR handoff meaning changes (CFG/SSA/for-in, method fallthrough, …).
pub const MIR_LOWERER_VERSION: u32 = 33; // # const duration/bytes/locator fold
pub const MIR_SCHEMA_VERSION: u32 = 7; // MirExpr::BytesInterp
/// Bumped when LLVM emission / opt / cache-key participation changes.
pub const CODEGEN_VERSION: u32 = 23; // store + git pack natives
pub const CODEGEN_SCHEMA_VERSION: u32 = 1;
/// Bumped when runtime deep eq / identity eq / locator heap changes.
pub const RUNTIME_ABI_VERSION: u32 = 60; // store + git pack natives
pub const STDLIB_VERSION: u32 = 41; // streamed HTTP + std/store + std/git
pub const DIAGNOSTICS_VERSION: u32 = 1;
pub const TARGET_OPTIONS_VERSION: u32 = 1;
pub const PROJECT_METADATA_VERSION: u32 = 1;

/// Pipeline phases that produce cacheable artifacts.
///
/// Order is the normal dependency order for invalidation cascades.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArtifactPhase {
    Lex,
    Parse,
    /// Project facts (`echo_index`).
    Index,
    /// Import graph, `%`/`@` merge (`echo_resolver`).
    Resolve,
    /// Local semantics / kinds (`echo_semantics`).
    Check,
    /// HIR + MIR lower.
    Lower,
    Codegen,
    Diagnostics,
}

impl ArtifactPhase {
    pub const ALL: [Self; 8] = [
        Self::Lex,
        Self::Parse,
        Self::Index,
        Self::Resolve,
        Self::Check,
        Self::Lower,
        Self::Codegen,
        Self::Diagnostics,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Lex => "lex",
            Self::Parse => "parse",
            Self::Index => "index",
            Self::Resolve => "resolve",
            Self::Check => "check",
            Self::Lower => "lower",
            Self::Codegen => "codegen",
            Self::Diagnostics => "diagnostics",
        }
    }

    /// Parse a phase name (CLI / tests).
    #[must_use]
    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == s)
    }
}

/// Compiler pieces whose version contributes to phase fingerprints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompilerComponent {
    Lexer,
    Parser,
    AstSchema,
    Index,
    IndexSchema,
    Resolver,
    ResolveSchema,
    Semantics,
    HirLowerer,
    HirSchema,
    MirLowerer,
    MirSchema,
    Codegen,
    CodegenSchema,
    RuntimeAbi,
    Stdlib,
    Diagnostics,
    TargetOptions,
    ProjectMetadata,
}

impl CompilerComponent {
    pub const ALL: [Self; 19] = [
        Self::Lexer,
        Self::Parser,
        Self::AstSchema,
        Self::Index,
        Self::IndexSchema,
        Self::Resolver,
        Self::ResolveSchema,
        Self::Semantics,
        Self::HirLowerer,
        Self::HirSchema,
        Self::MirLowerer,
        Self::MirSchema,
        Self::Codegen,
        Self::CodegenSchema,
        Self::RuntimeAbi,
        Self::Stdlib,
        Self::Diagnostics,
        Self::TargetOptions,
        Self::ProjectMetadata,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Lexer => "lexer",
            Self::Parser => "parser",
            Self::AstSchema => "ast_schema",
            Self::Index => "index",
            Self::IndexSchema => "index_schema",
            Self::Resolver => "resolver",
            Self::ResolveSchema => "resolve_schema",
            Self::Semantics => "semantics",
            Self::HirLowerer => "hir_lowerer",
            Self::HirSchema => "hir_schema",
            Self::MirLowerer => "mir_lowerer",
            Self::MirSchema => "mir_schema",
            Self::Codegen => "codegen",
            Self::CodegenSchema => "codegen_schema",
            Self::RuntimeAbi => "runtime_abi",
            Self::Stdlib => "stdlib",
            Self::Diagnostics => "diagnostics",
            Self::TargetOptions => "target_options",
            Self::ProjectMetadata => "project_metadata",
        }
    }

    #[must_use]
    pub const fn version(self) -> u32 {
        match self {
            Self::Lexer => LEXER_VERSION,
            Self::Parser => PARSER_VERSION,
            Self::AstSchema => AST_SCHEMA_VERSION,
            Self::Index => INDEX_VERSION,
            Self::IndexSchema => INDEX_SCHEMA_VERSION,
            Self::Resolver => RESOLVER_VERSION,
            Self::ResolveSchema => RESOLVE_SCHEMA_VERSION,
            Self::Semantics => SEMANTICS_VERSION,
            Self::HirLowerer => HIR_LOWERER_VERSION,
            Self::HirSchema => HIR_SCHEMA_VERSION,
            Self::MirLowerer => MIR_LOWERER_VERSION,
            Self::MirSchema => MIR_SCHEMA_VERSION,
            Self::Codegen => CODEGEN_VERSION,
            Self::CodegenSchema => CODEGEN_SCHEMA_VERSION,
            Self::RuntimeAbi => RUNTIME_ABI_VERSION,
            Self::Stdlib => STDLIB_VERSION,
            Self::Diagnostics => DIAGNOSTICS_VERSION,
            Self::TargetOptions => TARGET_OPTIONS_VERSION,
            Self::ProjectMetadata => PROJECT_METADATA_VERSION,
        }
    }
}

/// How aggressively to invalidate when a component changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMode {
    /// Any component change invalidates every phase (simple / debugging).
    Safe,
    /// Only phases that depend on the component (and downstream).
    Phase,
}

impl CacheMode {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Phase => "phase",
        }
    }
}

/// Stable hex digest over ordered `(name, value)` pairs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fingerprint {
    digest: String,
}

impl Fingerprint {
    #[must_use]
    pub fn new(parts: &[(&str, &str)]) -> Self {
        let mut hasher = Sha256::new();
        for (name, value) in parts {
            hasher.update((name.len() as u64).to_le_bytes());
            hasher.update(name.as_bytes());
            hasher.update((value.len() as u64).to_le_bytes());
            hasher.update(value.as_bytes());
        }
        Self {
            digest: hex_lower(&hasher.finalize()),
        }
    }

    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Self {
            digest: hex_lower(&hasher.finalize()),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.digest
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.digest)
    }
}

/// Phase identity for cache keys: phase name + component version mix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseFingerprint {
    pub phase: ArtifactPhase,
    pub fingerprint: Fingerprint,
}

/// Fingerprint for a phase, optionally with extra inputs (source hash, target, …).
#[must_use]
pub fn phase_fingerprint(phase: ArtifactPhase, extra_inputs: &[(&str, &str)]) -> PhaseFingerprint {
    let mut owned = vec![
        ("cache_format".to_string(), CACHE_FORMAT_VERSION.to_string()),
        ("phase".to_string(), phase.name().to_string()),
    ];
    for component in phase_components(phase) {
        owned.push((
            component.name().to_string(),
            component.version().to_string(),
        ));
    }
    for (name, value) in extra_inputs {
        owned.push(((*name).to_string(), (*value).to_string()));
    }
    let borrowed: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    PhaseFingerprint {
        phase,
        fingerprint: Fingerprint::new(&borrowed),
    }
}

/// Components whose versions feed this phase's fingerprint.
///
/// Each phase lists **every** component that can change that phase's cacheable
/// output without changing source bytes. That includes the full upstream stack
/// (not only the crates that “own” the phase), so a MIR/SSA fix invalidates
/// codegen IR keys even if a caller forgets to thread `lower_fp` extras.
///
/// Keep lists in pipeline order; duplicates across phases are intentional.
#[must_use]
pub fn phase_components(phase: ArtifactPhase) -> &'static [CompilerComponent] {
    match phase {
        ArtifactPhase::Lex => &[CompilerComponent::Lexer],
        ArtifactPhase::Parse => &[
            CompilerComponent::Lexer,
            CompilerComponent::Parser,
            CompilerComponent::AstSchema,
        ],
        ArtifactPhase::Index => &[
            CompilerComponent::Lexer,
            CompilerComponent::Parser,
            CompilerComponent::AstSchema,
            CompilerComponent::Index,
            CompilerComponent::IndexSchema,
            CompilerComponent::ProjectMetadata,
        ],
        ArtifactPhase::Resolve => &[
            CompilerComponent::Lexer,
            CompilerComponent::Parser,
            CompilerComponent::AstSchema,
            CompilerComponent::Index,
            CompilerComponent::IndexSchema,
            CompilerComponent::Resolver,
            CompilerComponent::ResolveSchema,
            CompilerComponent::Stdlib,
            CompilerComponent::ProjectMetadata,
        ],
        ArtifactPhase::Check => &[
            CompilerComponent::Lexer,
            CompilerComponent::Parser,
            CompilerComponent::AstSchema,
            CompilerComponent::Index,
            CompilerComponent::IndexSchema,
            CompilerComponent::Resolver,
            CompilerComponent::ResolveSchema,
            CompilerComponent::Semantics,
            CompilerComponent::Diagnostics,
            CompilerComponent::Stdlib,
            CompilerComponent::ProjectMetadata,
        ],
        ArtifactPhase::Lower => &[
            CompilerComponent::Lexer,
            CompilerComponent::Parser,
            CompilerComponent::AstSchema,
            CompilerComponent::Index,
            CompilerComponent::IndexSchema,
            CompilerComponent::Resolver,
            CompilerComponent::ResolveSchema,
            CompilerComponent::Semantics,
            CompilerComponent::Diagnostics,
            CompilerComponent::Stdlib,
            CompilerComponent::ProjectMetadata,
            CompilerComponent::HirLowerer,
            CompilerComponent::HirSchema,
            CompilerComponent::MirLowerer,
            CompilerComponent::MirSchema,
            CompilerComponent::TargetOptions,
        ],
        ArtifactPhase::Codegen => &[
            CompilerComponent::Lexer,
            CompilerComponent::Parser,
            CompilerComponent::AstSchema,
            CompilerComponent::Index,
            CompilerComponent::IndexSchema,
            CompilerComponent::Resolver,
            CompilerComponent::ResolveSchema,
            CompilerComponent::Semantics,
            CompilerComponent::Diagnostics,
            CompilerComponent::Stdlib,
            CompilerComponent::ProjectMetadata,
            CompilerComponent::HirLowerer,
            CompilerComponent::HirSchema,
            CompilerComponent::MirLowerer,
            CompilerComponent::MirSchema,
            CompilerComponent::Codegen,
            CompilerComponent::CodegenSchema,
            CompilerComponent::RuntimeAbi,
            CompilerComponent::TargetOptions,
        ],
        ArtifactPhase::Diagnostics => &[CompilerComponent::Diagnostics],
    }
}

/// Phases that must re-run when `changed` is updated (phase mode).
#[must_use]
pub fn phases_invalidated_by_component(changed: CompilerComponent) -> BTreeSet<ArtifactPhase> {
    ArtifactPhase::ALL
        .into_iter()
        .filter(|phase| phase_components(*phase).contains(&changed))
        .flat_map(|phase| phase_and_downstream(phase).iter().copied())
        .collect()
}

/// Invalidate according to [`CacheMode`].
#[must_use]
pub fn invalidated_phases(mode: CacheMode, changed: CompilerComponent) -> BTreeSet<ArtifactPhase> {
    match mode {
        CacheMode::Safe => ArtifactPhase::ALL.into_iter().collect(),
        CacheMode::Phase => phases_invalidated_by_component(changed),
    }
}

/// `phase` and every phase that depends on it (cascade).
#[must_use]
pub const fn phase_and_downstream(phase: ArtifactPhase) -> &'static [ArtifactPhase] {
    match phase {
        ArtifactPhase::Lex => &ArtifactPhase::ALL,
        ArtifactPhase::Parse => &[
            ArtifactPhase::Parse,
            ArtifactPhase::Index,
            ArtifactPhase::Resolve,
            ArtifactPhase::Check,
            ArtifactPhase::Lower,
            ArtifactPhase::Codegen,
            ArtifactPhase::Diagnostics,
        ],
        ArtifactPhase::Index => &[
            ArtifactPhase::Index,
            ArtifactPhase::Resolve,
            ArtifactPhase::Check,
            ArtifactPhase::Lower,
            ArtifactPhase::Codegen,
            ArtifactPhase::Diagnostics,
        ],
        ArtifactPhase::Resolve => &[
            ArtifactPhase::Resolve,
            ArtifactPhase::Check,
            ArtifactPhase::Lower,
            ArtifactPhase::Codegen,
            ArtifactPhase::Diagnostics,
        ],
        ArtifactPhase::Check => &[
            ArtifactPhase::Check,
            ArtifactPhase::Lower,
            ArtifactPhase::Codegen,
            ArtifactPhase::Diagnostics,
        ],
        ArtifactPhase::Lower => &[
            ArtifactPhase::Lower,
            ArtifactPhase::Codegen,
            ArtifactPhase::Diagnostics,
        ],
        ArtifactPhase::Codegen => &[ArtifactPhase::Codegen, ArtifactPhase::Diagnostics],
        ArtifactPhase::Diagnostics => &[ArtifactPhase::Diagnostics],
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_fingerprint_stable() {
        let a = phase_fingerprint(ArtifactPhase::Parse, &[("source", "abc")]);
        let b = phase_fingerprint(ArtifactPhase::Parse, &[("source", "abc")]);
        assert_eq!(a.fingerprint, b.fingerprint);
        let c = phase_fingerprint(ArtifactPhase::Parse, &[("source", "abd")]);
        assert_ne!(a.fingerprint, c.fingerprint);
    }

    #[test]
    fn lexer_change_invalidates_from_lex_down() {
        let set = invalidated_phases(CacheMode::Phase, CompilerComponent::Lexer);
        assert!(set.contains(&ArtifactPhase::Lex));
        assert!(set.contains(&ArtifactPhase::Parse));
        assert!(set.contains(&ArtifactPhase::Codegen));
    }

    #[test]
    fn codegen_change_does_not_invalidate_parse() {
        let set = invalidated_phases(CacheMode::Phase, CompilerComponent::Codegen);
        assert!(set.contains(&ArtifactPhase::Codegen));
        assert!(!set.contains(&ArtifactPhase::Parse));
        assert!(!set.contains(&ArtifactPhase::Lex));
    }

    #[test]
    fn safe_mode_invalidates_all() {
        let set = invalidated_phases(CacheMode::Safe, CompilerComponent::Codegen);
        assert_eq!(set.len(), ArtifactPhase::ALL.len());
    }

    #[test]
    fn fingerprint_from_bytes() {
        let f = Fingerprint::from_bytes(b"hello");
        assert_eq!(f.as_str().len(), 64);
    }

    #[test]
    fn codegen_fingerprint_includes_mir_and_hir() {
        // IR cache keys use ArtifactPhase::Codegen; MIR/SSA/HIR changes must
        // participate without relying on pipeline extras alone.
        let comps = phase_components(ArtifactPhase::Codegen);
        assert!(
            comps.contains(&CompilerComponent::MirLowerer),
            "codegen phase must fingerprint mir_lowerer"
        );
        assert!(
            comps.contains(&CompilerComponent::HirLowerer),
            "codegen phase must fingerprint hir_lowerer"
        );
        assert!(
            comps.contains(&CompilerComponent::Semantics),
            "codegen phase must fingerprint semantics"
        );
        assert!(
            comps.contains(&CompilerComponent::Parser),
            "codegen phase must fingerprint parser (same source, new parse meaning)"
        );
    }

    #[test]
    fn check_fingerprint_includes_frontend_stack() {
        let comps = phase_components(ArtifactPhase::Check);
        assert!(comps.contains(&CompilerComponent::Semantics));
        assert!(comps.contains(&CompilerComponent::Parser));
        assert!(comps.contains(&CompilerComponent::Resolver));
    }

    #[test]
    fn mir_lowerer_change_invalidates_codegen() {
        let set = invalidated_phases(CacheMode::Phase, CompilerComponent::MirLowerer);
        assert!(
            set.contains(&ArtifactPhase::Codegen),
            "MIR lowerer bump must invalidate codegen IR cache phase"
        );
        assert!(set.contains(&ArtifactPhase::Lower));
    }

    #[test]
    fn codegen_fp_stable_and_tracks_extra_inputs() {
        let a = phase_fingerprint(ArtifactPhase::Codegen, &[("opt", "O0")]);
        let b = phase_fingerprint(ArtifactPhase::Codegen, &[("opt", "O0")]);
        assert_eq!(a.fingerprint, b.fingerprint);
        let c = phase_fingerprint(ArtifactPhase::Codegen, &[("opt", "O2")]);
        assert_ne!(a.fingerprint, c.fingerprint);
        // Component mix is non-empty and includes mir version in the digest input set.
        assert!(
            phase_components(ArtifactPhase::Codegen)
                .iter()
                .any(|c| c.version() > 0)
        );
    }
}
