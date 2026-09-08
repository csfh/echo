//! Locate `libecho_runtime` for AOT clang link.
//!
//! Policy: [`docs/llvm.md`](../../../docs/llvm.md) — in each search root
//! (install dir or cargo profile + `deps/`), take the newest matching
//! archive. Do not compare the archive to `xo`'s mtime: Cargo writes `xo`
//! after the staticlib, and a later `xo`-only rebuild must still link.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Resolve the staticlib used by [`crate::link_aot`].
///
/// `ECHO_RUNTIME_LIB` wins when set (explicit override).
pub fn find_runtime_staticlib() -> Result<PathBuf, String> {
    find_runtime_staticlib_for(crate::LinkMode::Dynamic)
}

/// Prefer a musl archive when linking a scratch/static image.
pub fn find_runtime_staticlib_for(mode: crate::LinkMode) -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("ECHO_RUNTIME_LIB") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!("ECHO_RUNTIME_LIB not a file: {}", path.display()));
    }

    let exe = std::env::current_exe().ok();
    let mut roots = Vec::new();
    if mode == crate::LinkMode::Static {
        if let Ok(cwd) = std::env::current_dir() {
            roots.push(cwd.join("target/x86_64-unknown-linux-musl/debug"));
            roots.push(cwd.join("target/x86_64-unknown-linux-musl/release"));
        }
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let root = PathBuf::from(manifest_dir);
            roots.push(root.join("../../target/x86_64-unknown-linux-musl/debug"));
            roots.push(root.join("../../target/x86_64-unknown-linux-musl/release"));
        }
    }
    if let Some(exe) = exe.as_ref() {
        if let Some(profile) = infer_profile_dir(exe) {
            roots.push(profile);
        }
    }
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let root = PathBuf::from(manifest_dir);
        roots.push(root.join("../../target/debug"));
        roots.push(root.join("../../target/release"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join("target/debug"));
        roots.push(cwd.join("target/release"));
    }

    let mut tried = Vec::new();
    for profile in &roots {
        tried.push(profile.clone());
        if let Some(p) = pick_runtime_in_profile(profile) {
            return Ok(p);
        }
    }

    Err(format!(
        "could not find libecho_runtime.a (set ECHO_RUNTIME_LIB or `cargo build -p echo_runtime`). tried: {}",
        tried
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

#[must_use]
pub(crate) fn is_runtime_archive_name(name: &str) -> bool {
    (name.starts_with("libecho_runtime") && name.ends_with(".a"))
        || (name.starts_with("echo_runtime") && name.ends_with(".lib"))
}

#[must_use]
pub(crate) fn infer_profile_dir(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?.to_path_buf();
    if dir.file_name().and_then(|s| s.to_str()) == Some("deps") {
        return dir.parent().map(Path::to_path_buf);
    }
    Some(dir)
}

#[must_use]
pub(crate) fn collect_runtime_archives(profile_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in [profile_dir.to_path_buf(), profile_dir.join("deps")] {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            if p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_runtime_archive_name)
            {
                out.push(p);
            }
        }
    }
    out
}

#[must_use]
pub(crate) fn newest_archive(paths: &[PathBuf]) -> Option<PathBuf> {
    paths
        .iter()
        .max_by_key(|p| mtime(p).unwrap_or(SystemTime::UNIX_EPOCH))
        .cloned()
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn pick_runtime_in_profile(profile_dir: &Path) -> Option<PathBuf> {
    let found = collect_runtime_archives(profile_dir);
    let lib = newest_archive(&found)?;
    Some(lib.canonicalize().unwrap_or(lib))
}

/// Extra text when clang cannot resolve `echo_runtime_*`.
#[must_use]
pub fn missing_runtime_symbol_hint(clang_stderr: &str) -> Option<&'static str> {
    if clang_stderr.contains("echo_runtime_")
        && (clang_stderr.contains("undefined reference")
            || clang_stderr.contains("unresolved external")
            || clang_stderr.contains("ld: symbol(s) not found"))
    {
        Some(
            "Runtime archive is missing a symbol this `xo` needs. Rebuild it (`cargo build -p echo_runtime`) or set ECHO_RUNTIME_LIB to a current libecho_runtime.a.",
        )
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::time::{Duration, SystemTime};

    fn temp_dir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "echo-rtlib-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(p.join("deps")).unwrap();
        p
    }

    fn touch(path: &Path, age: Duration) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        File::create(path).unwrap();
        let t = SystemTime::now() - age;
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    #[test]
    fn name_filter_accepts_hashed_a_not_rlib() {
        assert!(is_runtime_archive_name("libecho_runtime.a"));
        assert!(is_runtime_archive_name("libecho_runtime-abc123.a"));
        assert!(is_runtime_archive_name("echo_runtime.lib"));
        assert!(!is_runtime_archive_name("libecho_runtime.rlib"));
        assert!(!is_runtime_archive_name("libecho_runtime.d"));
    }

    #[test]
    fn infer_profile_from_deps_test_binary() {
        let p = PathBuf::from("/tmp/target/debug/deps/echo_codegen-deadbeef");
        assert_eq!(
            infer_profile_dir(&p).as_deref(),
            Some(Path::new("/tmp/target/debug"))
        );
        let p = PathBuf::from("/tmp/target/debug/xo");
        assert_eq!(
            infer_profile_dir(&p).as_deref(),
            Some(Path::new("/tmp/target/debug"))
        );
    }

    #[test]
    fn newest_hashed_wins_over_stale_unhashed() {
        let dir = temp_dir();
        touch(&dir.join("libecho_runtime.a"), Duration::from_secs(3600));
        let hashed = dir.join("deps").join("libecho_runtime-newer.a");
        touch(&hashed, Duration::from_secs(1));
        let got = newest_archive(&collect_runtime_archives(&dir)).unwrap();
        assert_eq!(got, hashed);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_layout_picks_sibling_archive() {
        let dir = std::env::temp_dir().join(format!(
            "echo-rtlib-install-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        touch(&dir.join("libecho_runtime.a"), Duration::from_secs(3600));
        touch(&dir.join("xo"), Duration::from_secs(1));
        let got = pick_runtime_in_profile(&dir).unwrap();
        assert!(got.ends_with("libecho_runtime.a"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hint_on_undefined_runtime_symbol() {
        let err = "ld: /tmp/x.o: undefined reference to `echo_runtime_locator_class'";
        assert!(missing_runtime_symbol_hint(err).is_some());
        assert!(missing_runtime_symbol_hint("clang: unused argument").is_none());
    }
}
