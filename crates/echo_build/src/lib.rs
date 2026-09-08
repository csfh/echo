//! Build planning over fingerprints and the artifact cache.
//!
//! See `docs/incremental.md`. v0: describe which phases to run; does not yet
//! execute the compiler pipeline (that stays in `xo` / shared hosts).

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use echo_fingerprint::{
    invalidated_phases, phase_and_downstream, ArtifactPhase, CacheMode, CompilerComponent,
};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// What kind of host build is requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    /// Typecheck / semantics only (through Check).
    Check,
    /// Emit IR / native / JIT (through Codegen).
    Execute,
    /// Full cascade including diagnostics artifacts.
    Full,
}

impl BuildMode {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Execute => "execute",
            Self::Full => "full",
        }
    }

    /// Phases required for this mode (in pipeline order).
    #[must_use]
    pub fn phases(self) -> &'static [ArtifactPhase] {
        match self {
            Self::Check => &[
                ArtifactPhase::Lex,
                ArtifactPhase::Parse,
                ArtifactPhase::Index,
                ArtifactPhase::Resolve,
                ArtifactPhase::Check,
            ],
            Self::Execute => &[
                ArtifactPhase::Lex,
                ArtifactPhase::Parse,
                ArtifactPhase::Index,
                ArtifactPhase::Resolve,
                ArtifactPhase::Check,
                ArtifactPhase::Lower,
                ArtifactPhase::Codegen,
            ],
            Self::Full => &ArtifactPhase::ALL,
        }
    }
}

/// One unit of work in a plan (one file × one phase).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildJob {
    pub path: PathBuf,
    pub phase: ArtifactPhase,
}

/// Planned jobs for a set of entry files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlan {
    pub mode: BuildMode,
    pub jobs: Vec<BuildJob>,
}

/// Build a plan: every path runs every phase in `mode` (no cache hits yet).
#[must_use]
pub fn plan_all_phases(mode: BuildMode, paths: &[PathBuf]) -> BuildPlan {
    let mut jobs = Vec::new();
    for path in paths {
        for phase in mode.phases() {
            jobs.push(BuildJob {
                path: path.clone(),
                phase: *phase,
            });
        }
    }
    BuildPlan { mode, jobs }
}

/// After a component change, which phases must re-run (for cache eviction).
#[must_use]
pub fn phases_to_invalidate(
    mode: CacheMode,
    changed: CompilerComponent,
) -> BTreeSet<ArtifactPhase> {
    invalidated_phases(mode, changed)
}

/// Cascade from a dirty phase (e.g. file content changed at Parse).
#[must_use]
pub fn cascade_from(dirty: ArtifactPhase) -> BTreeSet<ArtifactPhase> {
    phase_and_downstream(dirty).iter().copied().collect()
}

/// Filter a full plan to only jobs whose phase is in `needed`.
#[must_use]
pub fn filter_plan(plan: &BuildPlan, needed: &BTreeSet<ArtifactPhase>) -> BuildPlan {
    BuildPlan {
        mode: plan.mode,
        jobs: plan
            .jobs
            .iter()
            .filter(|j| needed.contains(&j.phase))
            .cloned()
            .collect(),
    }
}

/// Resolve project root for cache: nearest ancestor containing `Cargo.toml` or `.git`,
/// else parent of `entry`, else cwd.
#[must_use]
pub fn project_root_for(entry: &Path) -> PathBuf {
    let start = if entry.is_file() {
        entry.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        entry.to_path_buf()
    };
    let mut cur = start.canonicalize().unwrap_or(start);
    loop {
        if cur.join("Cargo.toml").is_file() || cur.join(".git").exists() {
            return cur;
        }
        if !cur.pop() {
            break;
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_plan_has_five_phases_per_file() {
        let plan = plan_all_phases(
            BuildMode::Check,
            &[PathBuf::from("a.echo"), PathBuf::from("b.echo")],
        );
        assert_eq!(plan.jobs.len(), 10);
        assert!(plan.jobs.iter().all(|j| j.phase <= ArtifactPhase::Check));
    }

    #[test]
    fn filter_to_cascade() {
        let plan = plan_all_phases(BuildMode::Full, &[PathBuf::from("a.echo")]);
        let needed = cascade_from(ArtifactPhase::Codegen);
        let filtered = filter_plan(&plan, &needed);
        assert!(filtered
            .jobs
            .iter()
            .all(|j| j.phase == ArtifactPhase::Codegen
                || j.phase == ArtifactPhase::Diagnostics));
    }

    #[test]
    fn invalidate_codegen_component() {
        let set = phases_to_invalidate(CacheMode::Phase, CompilerComponent::Codegen);
        assert!(set.contains(&ArtifactPhase::Codegen));
        assert!(!set.contains(&ArtifactPhase::Parse));
    }
}
