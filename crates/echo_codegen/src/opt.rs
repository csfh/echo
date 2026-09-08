//! LLVM optimization levels and in-process pass pipeline.
//!
//! Same `OptLevel` drives AOT (optimized IR → clang) and JIT (optimized IR →
//! MCJIT). Passes run via inkwell's new pass manager (`default<On>`).

use inkwell::OptimizationLevel;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetMachine};

/// User-facing optimization level (`xo -O0` … `-Oz`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OptLevel {
    /// No mid-end passes; IR as emitted. Default for `xo run` / `xo ir`.
    #[default]
    O0,
    O1,
    O2,
    O3,
    /// Size-oriented pipeline (`default<Oz>`).
    Oz,
}

impl OptLevel {
    /// CLI / cache token (`O0`, `O1`, …).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::O0 => "O0",
            Self::O1 => "O1",
            Self::O2 => "O2",
            Self::O3 => "O3",
            Self::Oz => "Oz",
        }
    }

    /// Parse `0`/`O0`/`o0`, `1`/`O1`, …, `z`/`Oz` (case-insensitive on letter).
    pub fn parse(s: &str) -> Result<Self, String> {
        let t = s.trim();
        let norm = if let Some(rest) = t.strip_prefix(['O', 'o']) {
            rest
        } else {
            t
        };
        match norm {
            "0" => Ok(Self::O0),
            "1" => Ok(Self::O1),
            "2" => Ok(Self::O2),
            "3" => Ok(Self::O3),
            "z" | "Z" | "s" | "S" => Ok(Self::Oz),
            _ => Err(format!(
                "unknown opt level `{s}` (expected O0, O1, O2, O3, or Oz)"
            )),
        }
    }

    /// New-PM pipeline string, or `None` for O0 (skip `run_passes`).
    #[must_use]
    pub fn pass_pipeline(self) -> Option<&'static str> {
        match self {
            Self::O0 => None,
            Self::O1 => Some("default<O1>"),
            Self::O2 => Some("default<O2>"),
            Self::O3 => Some("default<O3>"),
            Self::Oz => Some("default<Oz>"),
        }
    }

    /// Codegen opt for target machine construction.
    #[must_use]
    pub fn codegen_level(self) -> OptimizationLevel {
        match self {
            Self::O0 => OptimizationLevel::None,
            Self::O1 => OptimizationLevel::Less,
            Self::O2 | Self::Oz => OptimizationLevel::Default,
            Self::O3 => OptimizationLevel::Aggressive,
        }
    }

    /// Clang `-O*` for object emission. After in-process `run_passes`, AOT uses
    /// `-O0` so clang does not re-run the mid-end; this is only for completeness
    /// when linking raw O0 IR.
    #[must_use]
    pub fn clang_opt_flag(self) -> &'static str {
        match self {
            Self::O0 => "-O0",
            Self::O1 => "-O1",
            Self::O2 => "-O2",
            Self::O3 => "-O3",
            Self::Oz => "-Oz",
        }
    }
}

impl std::fmt::Display for OptLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Host target machine for `Module::run_passes`.
pub fn host_target_machine(opt: OptLevel) -> Result<TargetMachine, String> {
    Target::initialize_native(&InitializationConfig::default())
        .map_err(|e| format!("init native target: {e}"))?;
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| format!("target from triple: {e}"))?;
    let cpu = TargetMachine::get_host_cpu_name().to_string();
    let features = TargetMachine::get_host_cpu_features().to_string();
    target
        .create_target_machine(
            &triple,
            &cpu,
            &features,
            opt.codegen_level(),
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| "create_target_machine failed".to_string())
}

/// Set module triple/data layout and run the new-PM pipeline for `opt`.
///
/// True no-op when `opt` is [`OptLevel::O0`] (module text unchanged).
pub fn optimize_module(module: &Module<'_>, opt: OptLevel) -> Result<(), String> {
    let Some(pipeline) = opt.pass_pipeline() else {
        return Ok(());
    };

    let machine = host_target_machine(opt)?;
    let triple = TargetMachine::get_default_triple();
    module.set_triple(&triple);
    module.set_data_layout(&machine.get_target_data().get_data_layout());

    let options = PassBuilderOptions::create();
    options.set_verify_each(false);
    module
        .run_passes(pipeline, &machine, options)
        .map_err(|e| format!("LLVM run_passes({pipeline}): {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_all_supported_levels() {
        assert_eq!(OptLevel::parse("0").unwrap(), OptLevel::O0);
        assert_eq!(OptLevel::parse("O0").unwrap(), OptLevel::O0);
        assert_eq!(OptLevel::parse("o0").unwrap(), OptLevel::O0);
        assert_eq!(OptLevel::parse("1").unwrap(), OptLevel::O1);
        assert_eq!(OptLevel::parse("O1").unwrap(), OptLevel::O1);
        assert_eq!(OptLevel::parse("2").unwrap(), OptLevel::O2);
        assert_eq!(OptLevel::parse("O2").unwrap(), OptLevel::O2);
        assert_eq!(OptLevel::parse("3").unwrap(), OptLevel::O3);
        assert_eq!(OptLevel::parse("O3").unwrap(), OptLevel::O3);
        assert_eq!(OptLevel::parse("z").unwrap(), OptLevel::Oz);
        assert_eq!(OptLevel::parse("Oz").unwrap(), OptLevel::Oz);
        assert_eq!(OptLevel::parse("oz").unwrap(), OptLevel::Oz);
        assert_eq!(OptLevel::parse("s").unwrap(), OptLevel::Oz);
    }

    #[test]
    fn parse_rejects_invalid_levels() {
        for bad in ["O9", "4", "fast", "", "O", "Oo", "Ozz"] {
            assert!(OptLevel::parse(bad).is_err(), "expected error for `{bad}`");
        }
        let err = OptLevel::parse("O9").unwrap_err();
        assert!(err.contains("unknown opt level"), "{err}");
        assert!(err.contains("O0"), "{err}");
    }

    #[test]
    fn default_is_o0() {
        assert_eq!(OptLevel::default(), OptLevel::O0);
    }

    #[test]
    fn o0_skips_pipeline_oz_is_size() {
        assert!(OptLevel::O0.pass_pipeline().is_none());
        assert_eq!(OptLevel::O1.pass_pipeline(), Some("default<O1>"));
        assert_eq!(OptLevel::O2.pass_pipeline(), Some("default<O2>"));
        assert_eq!(OptLevel::O3.pass_pipeline(), Some("default<O3>"));
        assert_eq!(OptLevel::Oz.pass_pipeline(), Some("default<Oz>"));
        // Oz must not alias a speed level in the pass pipeline string.
        assert_ne!(OptLevel::Oz.pass_pipeline(), OptLevel::O2.pass_pipeline());
        assert_ne!(OptLevel::Oz.pass_pipeline(), OptLevel::O3.pass_pipeline());
    }

    #[test]
    fn as_str_tokens_are_stable_cache_keys() {
        assert_eq!(OptLevel::O0.as_str(), "O0");
        assert_eq!(OptLevel::O1.as_str(), "O1");
        assert_eq!(OptLevel::O2.as_str(), "O2");
        assert_eq!(OptLevel::O3.as_str(), "O3");
        assert_eq!(OptLevel::Oz.as_str(), "Oz");
        // Distinct cache tokens for every level (incl. O2 vs Oz).
        let tokens: std::collections::HashSet<_> = [
            OptLevel::O0,
            OptLevel::O1,
            OptLevel::O2,
            OptLevel::O3,
            OptLevel::Oz,
        ]
        .into_iter()
        .map(OptLevel::as_str)
        .collect();
        assert_eq!(tokens.len(), 5);
    }
}
