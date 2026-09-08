//! Artifact cache: layout under `.xo/` and a simple content-addressed store.
//!
//! See `docs/incremental.md`. This crate does **not** run the compiler; it only
//! stores and retrieves blobs keyed by phase fingerprints.

#![forbid(unsafe_code)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use echo_fingerprint::{ArtifactPhase, Fingerprint, PhaseFingerprint, phase_fingerprint};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// On-disk layout for a project (`.xo/` under the project root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheLayout {
    root: PathBuf,
}

impl CacheLayout {
    /// `project_root/.xo`
    #[must_use]
    pub fn for_project(project_root: impl Into<PathBuf>) -> Self {
        Self {
            root: project_root.into().join(".xo"),
        }
    }

    /// Use an existing `.xo` directory as root.
    #[must_use]
    pub fn from_xo_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    #[must_use]
    pub fn phase_dir(&self, phase: ArtifactPhase) -> PathBuf {
        self.cache_dir().join(phase.name())
    }

    /// Live blobs for `phase` sit under this compiler-only stamp.
    ///
    /// Stamp is [`phase_fingerprint`] with no extras (versions + format +
    /// phase name). Source-specific extras stay in the blob file name.
    #[must_use]
    pub fn generation_dir(&self, phase: ArtifactPhase) -> PathBuf {
        self.phase_dir(phase)
            .join(phase_fingerprint(phase, &[]).fingerprint.as_str())
    }

    #[must_use]
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    #[must_use]
    pub fn index_dir(&self) -> PathBuf {
        self.root.join("index")
    }

    /// Create directories required for store operations.
    pub fn ensure(&self) -> io::Result<()> {
        fs::create_dir_all(self.cache_dir())?;
        fs::create_dir_all(self.tmp_dir())?;
        fs::create_dir_all(self.index_dir())?;
        for phase in ArtifactPhase::ALL {
            fs::create_dir_all(self.phase_dir(phase))?;
        }
        Ok(())
    }

    /// Remove the entire `.xo` tree (if present).
    pub fn clean(&self) -> io::Result<()> {
        if self.root.exists() {
            fs::remove_dir_all(&self.root)?;
        }
        Ok(())
    }

    /// Whether the layout root exists.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.root.exists()
    }
}

/// Key for a phase artifact: source content + phase compiler fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseCacheKey {
    pub phase: ArtifactPhase,
    /// Hash of file contents (or graph inputs).
    pub source_fingerprint: Fingerprint,
    /// Compiler component mix for this phase.
    pub phase_fingerprint: PhaseFingerprint,
}

impl PhaseCacheKey {
    /// Build a key from raw source bytes and optional extra phase inputs.
    #[must_use]
    pub fn for_source(phase: ArtifactPhase, source_bytes: &[u8], extra: &[(&str, &str)]) -> Self {
        let source_fingerprint = Fingerprint::from_bytes(source_bytes);
        let src = source_fingerprint.as_str().to_string();
        let mut owned: Vec<(String, String)> = vec![("source".into(), src)];
        for (k, v) in extra {
            owned.push(((*k).into(), (*v).into()));
        }
        let borrowed: Vec<(&str, &str)> = owned
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        Self {
            phase,
            source_fingerprint,
            phase_fingerprint: phase_fingerprint(phase, &borrowed),
        }
    }

    /// Stable file name under the phase directory.
    #[must_use]
    pub fn blob_name(&self) -> String {
        format!(
            "{}_{}.bin",
            self.source_fingerprint.as_str(),
            self.phase_fingerprint.fingerprint.as_str()
        )
    }
}

/// Filesystem-backed artifact store.
#[derive(Debug, Clone)]
pub struct ArtifactStore {
    layout: CacheLayout,
}

impl ArtifactStore {
    #[must_use]
    pub fn new(layout: CacheLayout) -> Self {
        Self { layout }
    }

    #[must_use]
    pub fn layout(&self) -> &CacheLayout {
        &self.layout
    }

    /// Write bytes for `key`. Creates layout dirs as needed.
    pub fn put(&self, key: &PhaseCacheKey, bytes: &[u8]) -> io::Result<PathBuf> {
        self.layout.ensure()?;
        let path = self.blob_path(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Atomic-ish: write tmp then rename.
        let tmp = self.layout.tmp_dir().join(format!(
            "{}.tmp",
            key.phase_fingerprint.fingerprint.as_str()
        ));
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
        Ok(path)
    }

    /// Read bytes if present.
    pub fn get(&self, key: &PhaseCacheKey) -> io::Result<Option<Vec<u8>>> {
        let path = self.blob_path(key);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(fs::read(path)?))
    }

    /// True if a blob exists for `key`.
    #[must_use]
    pub fn contains(&self, key: &PhaseCacheKey) -> bool {
        self.blob_path(key).exists()
    }

    #[must_use]
    pub fn blob_path(&self, key: &PhaseCacheKey) -> PathBuf {
        self.layout.generation_dir(key.phase).join(key.blob_name())
    }

    /// Count **live** artifact files (current compiler stamp) per phase.
    pub fn phase_counts(&self) -> io::Result<Vec<(ArtifactPhase, usize)>> {
        let mut out = Vec::new();
        for phase in ArtifactPhase::ALL {
            let dir = self.layout.generation_dir(phase);
            let n = if dir.is_dir() {
                fs::read_dir(&dir)?
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_file())
                    .count()
            } else {
                0
            };
            out.push((phase, n));
        }
        Ok(out)
    }

    /// Delete unused cache files. Live blobs (current compiler stamp) stay.
    ///
    /// Unused:
    /// - leftover files in `.xo/tmp`
    /// - files in a phase directory root (pre-v2 flat layout)
    /// - whole generation dirs whose stamp is not the current compiler mix
    pub fn gc(&self) -> io::Result<GcReport> {
        let mut report = GcReport::default();
        if !self.layout.exists() {
            return Ok(report);
        }

        let tmp = self.layout.tmp_dir();
        if tmp.is_dir() {
            for entry in fs::read_dir(&tmp)? {
                let path = entry?.path();
                if path.is_file() {
                    remove_file_counted(&path, &mut report)?;
                }
            }
        }

        for phase in ArtifactPhase::ALL {
            let dir = self.layout.phase_dir(phase);
            if !dir.is_dir() {
                continue;
            }
            let keep = phase_fingerprint(phase, &[])
                .fingerprint
                .as_str()
                .to_string();
            for entry in fs::read_dir(&dir)? {
                let path = entry?.path();
                if path.is_file() {
                    remove_file_counted(&path, &mut report)?;
                    continue;
                }
                if !path.is_dir() {
                    continue;
                }
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if name == keep {
                    continue;
                }
                count_files_under(&path, &mut report)?;
                fs::remove_dir_all(&path)?;
                report.dirs_removed += 1;
            }
        }
        Ok(report)
    }
}

/// Result of [`ArtifactStore::gc`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GcReport {
    pub blobs_removed: usize,
    pub bytes_removed: u64,
    pub dirs_removed: usize,
}

fn remove_file_counted(path: &Path, report: &mut GcReport) -> io::Result<()> {
    let n = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    fs::remove_file(path)?;
    report.blobs_removed += 1;
    report.bytes_removed += n;
    Ok(())
}

fn count_files_under(dir: &Path, report: &mut GcReport) -> io::Result<()> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        for entry in fs::read_dir(&cur)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                let n = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                report.blobs_removed += 1;
                report.bytes_removed += n;
            }
        }
    }
    Ok(())
}

/// SHA-256 of bytes as [`Fingerprint`] (re-export convenience).
#[must_use]
pub fn hash_bytes(bytes: &[u8]) -> Fingerprint {
    Fingerprint::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_project() -> PathBuf {
        let mut p = std::env::temp_dir();
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("echo-cache-test-{t}"));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn put_get_roundtrip() {
        let root = temp_project();
        let layout = CacheLayout::for_project(&root);
        let store = ArtifactStore::new(layout);
        let key = PhaseCacheKey::for_source(ArtifactPhase::Parse, b"$ x = 1\n", &[]);
        assert!(!store.contains(&key));
        store.put(&key, b"ast-blob").unwrap();
        assert!(store.contains(&key));
        assert_eq!(store.get(&key).unwrap().unwrap(), b"ast-blob");
        store.layout().clean().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn different_source_different_key() {
        let a = PhaseCacheKey::for_source(ArtifactPhase::Lex, b"a", &[]);
        let b = PhaseCacheKey::for_source(ArtifactPhase::Lex, b"b", &[]);
        assert_ne!(a.blob_name(), b.blob_name());
    }

    #[test]
    fn opt_level_extra_distinguishes_codegen_keys() {
        let src = b"$ f = (a) { ^ a + 1 }\n^ f(1)\n";
        let k0 = PhaseCacheKey::for_source(
            ArtifactPhase::Codegen,
            src,
            &[("opt", "O0"), ("artifact", "llvm_ir")],
        );
        let k2 = PhaseCacheKey::for_source(
            ArtifactPhase::Codegen,
            src,
            &[("opt", "O2"), ("artifact", "llvm_ir")],
        );
        let kz = PhaseCacheKey::for_source(
            ArtifactPhase::Codegen,
            src,
            &[("opt", "Oz"), ("artifact", "llvm_ir")],
        );
        assert_ne!(k0.blob_name(), k2.blob_name());
        assert_ne!(k2.blob_name(), kz.blob_name());
        assert_ne!(k0.blob_name(), kz.blob_name());
    }

    #[test]
    fn aot_binary_extra_distinguishes_from_ir_blob() {
        let ir = b"define i64 @echo_entry() { ret i64 0 }\n";
        let ir_key = PhaseCacheKey::for_source(
            ArtifactPhase::Codegen,
            ir,
            &[("artifact", "llvm_ir"), ("opt", "O2")],
        );
        let aot_key = PhaseCacheKey::for_source(
            ArtifactPhase::Codegen,
            ir,
            &[("artifact", "aot_binary"), ("runtime_abi", "1")],
        );
        assert_ne!(ir_key.blob_name(), aot_key.blob_name());
    }

    #[test]
    fn gc_keeps_live_blob_and_drops_stale() {
        let root = temp_project();
        let layout = CacheLayout::for_project(&root);
        let store = ArtifactStore::new(layout.clone());
        let key = PhaseCacheKey::for_source(ArtifactPhase::Parse, b"$ x = 1\n", &[]);
        store.put(&key, b"live").unwrap();

        let parse_dir = layout.phase_dir(ArtifactPhase::Parse);
        fs::create_dir_all(&parse_dir).unwrap();
        let flat = parse_dir.join("old_flat.bin");
        fs::write(&flat, b"flat").unwrap();
        let stale = parse_dir.join("deadstamp");
        fs::create_dir_all(&stale).unwrap();
        fs::write(stale.join("stale.bin"), b"stale-bytes").unwrap();

        layout.ensure().unwrap();
        let tmp = layout.tmp_dir().join("leftover.tmp");
        fs::write(&tmp, b"tmp").unwrap();

        let report = store.gc().unwrap();
        assert_eq!(report.blobs_removed, 3, "{report:?}");
        assert_eq!(report.dirs_removed, 1, "{report:?}");
        assert!(store.contains(&key));
        assert_eq!(store.get(&key).unwrap().unwrap(), b"live");
        assert!(!flat.exists());
        assert!(!stale.exists());
        assert!(!tmp.exists());

        let counts = store.phase_counts().unwrap();
        let parse_n = counts
            .iter()
            .find(|(p, _)| *p == ArtifactPhase::Parse)
            .map(|(_, n)| *n)
            .unwrap();
        assert_eq!(parse_n, 1);

        store.layout().clean().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gc_on_missing_layout_is_noop() {
        let root = temp_project();
        let layout = CacheLayout::for_project(&root);
        let store = ArtifactStore::new(layout);
        let report = store.gc().unwrap();
        assert_eq!(report, GcReport::default());
        let _ = fs::remove_dir_all(&root);
    }
}
