//! User package cache under `$XO_HOME` (ADR 0014).
//!
//! Layout: `$XO_HOME/packages/<encoded-id>/<version>/…`
//! Downloads always go here — never into `{project}/.xo/`.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use echo_index::PathSeg;

/// Environment override for the user `.xo` root.
pub const XO_HOME_ENV: &str = "XO_HOME";

/// Version directory name for local `--path` installs when no `@version` is given.
/// (There is **no** global `default` alias for git packages.)
pub const LOCAL_VERSION: &str = "local";

thread_local! {
    /// Test-only override (avoids `set_var` under `forbid(unsafe_code)`).
    static XO_HOME_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Run `f` with a temporary `$XO_HOME` for tests.
pub fn with_xo_home_for_test<R>(home: PathBuf, f: impl FnOnce() -> R) -> R {
    XO_HOME_OVERRIDE.with(|c| {
        *c.borrow_mut() = Some(home);
    });
    let out = f();
    XO_HOME_OVERRIDE.with(|c| {
        *c.borrow_mut() = None;
    });
    out
}

/// Resolve the user `.xo` root:
/// test override → `$XO_HOME` → `$XDG_CACHE_HOME/.xo` → `~/.cache/.xo`.
#[must_use]
pub fn xo_home() -> PathBuf {
    if let Some(p) = XO_HOME_OVERRIDE.with(|c| c.borrow().clone()) {
        return p;
    }
    if let Ok(h) = std::env::var(XO_HOME_ENV) {
        let p = PathBuf::from(h);
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        let p = PathBuf::from(xdg);
        if !p.as_os_str().is_empty() {
            return p.join(".xo");
        }
    }
    dirs_home()
        .map(|h| h.join(".cache").join(".xo"))
        .unwrap_or_else(|| PathBuf::from(".xo"))
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// `$XO_HOME/packages`.
#[must_use]
pub fn packages_root() -> PathBuf {
    xo_home().join("packages")
}

/// Encode a package id (`github.com/csfh/echo-pkg`) as a single path segment.
#[must_use]
pub fn encode_package_id(package_id: &str) -> String {
    let mut out = String::with_capacity(package_id.len() * 2);
    for b in package_id.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                out.push(b as char);
            }
            b'/' => out.push_str("%2F"),
            other => {
                out.push('%');
                out.push(nibble(other >> 4));
                out.push(nibble(other & 0xf));
            }
        }
    }
    out
}

fn nibble(n: u8) -> char {
    char::from(if n < 10 { b'0' + n } else { b'A' + (n - 10) })
}

/// Directory for one installed package version.
#[must_use]
pub fn package_version_dir(package_id: &str, version: &str) -> PathBuf {
    packages_root()
        .join(encode_package_id(package_id))
        .join(version)
}

/// Collapse lexer `name . name` runs into dotted names (`github` `.` `com` → `github.com`).
///
/// Import paths are tokenized with `.` as its own segment, so host paths need this.
#[must_use]
pub fn coalesce_import_names(segments: &[PathSeg]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < segments.len() {
        match &segments[i] {
            PathSeg::Name(n) => {
                let mut s = n.clone();
                while i + 2 < segments.len()
                    && matches!(segments[i + 1], PathSeg::Dot)
                    && matches!(segments[i + 2], PathSeg::Name(_))
                {
                    let PathSeg::Name(next) = &segments[i + 2] else {
                        unreachable!()
                    };
                    s.push('.');
                    s.push_str(next);
                    i += 2;
                }
                out.push(s);
                i += 1;
            }
            PathSeg::Dot | PathSeg::DotDot => {
                // Leading `./` / `../` or stray dots — skip; relative imports never use this helper first.
                i += 1;
            }
        }
    }
    out
}

/// Whether this import path is host/URL-shaped (not relative, not bare `std`/`runtime`).
#[must_use]
pub fn is_host_path(segments: &[PathSeg]) -> bool {
    // Relative imports start with `.` or `..`
    if matches!(segments.first(), Some(PathSeg::Dot | PathSeg::DotDot)) {
        return false;
    }
    let names = coalesce_import_names(segments);
    match names.first().map(|s| s.as_str()) {
        Some("std") | Some("runtime") | None => false,
        Some(n) => n.contains('.') || n.contains(':'),
    }
}

/// Split host import into `(package_id, module_subpath)`.
///
/// Rule: if the first segment looks like a host (`github.com`, …) and there are
/// at least 3 segments, package id is `host/owner/repo` and the rest is the
/// module path inside the package. Otherwise the whole path is the package id
/// (module at package root).
#[must_use]
pub fn split_host_import(segments: &[PathSeg]) -> Option<(String, Vec<String>)> {
    if !is_host_path(segments) {
        return None;
    }
    let names = coalesce_import_names(segments);
    if names.is_empty() {
        return None;
    }
    // github.com / gitlab.com / bitbucket.org style: host/owner/repo[/module…]
    let host = names[0].as_str();
    if (host == "github.com" || host == "gitlab.com" || host == "bitbucket.org") && names.len() >= 3
    {
        let pkg = names[..3].join("/");
        let module: Vec<String> = names[3..].to_vec();
        return Some((pkg, module));
    }
    // Other host paths: first segment host, rest path — package = host + first path
    // component when len >= 2; else whole host as package.
    if names.len() >= 2 {
        let pkg = names[..2].join("/");
        let module = names[2..].to_vec();
        Some((pkg, module))
    } else {
        Some((names[0].clone(), vec![]))
    }
}

/// List installed version directory names under a package id.
#[must_use]
pub fn list_versions(package_id: &str) -> Vec<String> {
    let root = packages_root().join(encode_package_id(package_id));
    let Ok(rd) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut vers: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    vers.sort();
    vers
}

/// Result of choosing among installed package versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionPick {
    /// Use this installed directory name.
    Use(String),
    /// No versions installed.
    NoneInstalled,
    /// Several versions, no `default` — caller must pin (do not guess).
    Ambiguous(Vec<String>),
}

/// Match a pin to an installed version dir: exact, or pin is a prefix of a hash dir
/// (so `abc1234` matches full SHA in the cache).
#[must_use]
pub fn match_installed_version(versions: &[String], pin: &str) -> Option<String> {
    if let Some(v) = versions.iter().find(|v| v.as_str() == pin) {
        return Some(v.clone());
    }
    // Short hash pin → unique full-hash install
    let prefix_hits: Vec<&String> = versions
        .iter()
        .filter(|v| v.starts_with(pin) && v.len() > pin.len() && pin.len() >= 7)
        .collect();
    if prefix_hits.len() == 1 {
        return Some(prefix_hits[0].clone());
    }
    None
}

/// Pick a version when **no** `xo.toml` pin: sole install only; never invent a default alias.
#[must_use]
pub fn select_version(versions: &[String]) -> VersionPick {
    if versions.is_empty() {
        return VersionPick::NoneInstalled;
    }
    if versions.len() == 1 {
        return VersionPick::Use(versions[0].clone());
    }
    VersionPick::Ambiguous(versions.to_vec())
}

/// Prefer `base.echo` file, else `base/` directory that contains `*.echo`.
pub fn resolve_file_or_dir_module(base: &Path) -> Result<PathBuf, String> {
    let file = base.with_extension("echo");
    if file.is_file() {
        return file.canonicalize().map_err(|e| e.to_string());
    }
    if base.is_dir() {
        let echoes = list_echo_files(base);
        if !echoes.is_empty() {
            return base.canonicalize().map_err(|e| e.to_string());
        }
        return Err(format!("directory {} has no .echo sources", base.display()));
    }
    Err(format!(
        "no module at {}.echo or {}/",
        base.display(),
        base.display()
    ))
}

/// Sorted `*.echo` files directly in `dir` (not recursive).
#[must_use]
pub fn list_echo_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("echo"))
        .collect();
    files.sort();
    files
}

/// Resolve a host import to a module root (file or directory) under the package cache.
///
/// When `declared_deps` contains this package id and nothing is installed yet,
/// auto-`install_git` with the pinned version (entry `xo.toml` only).
pub fn resolve_host_import(
    segments: &[PathSeg],
    declared_deps: &std::collections::HashMap<String, String>,
) -> Result<PathBuf, String> {
    let Some((package_id, module)) = split_host_import(segments) else {
        return Err("not a host import path".into());
    };
    // Declared pin wins: use installed pin (exact or hash-prefix match), else auto-get.
    let version = if let Some(pin) = declared_deps.get(&package_id) {
        let versions = list_versions(&package_id);
        if let Some(matched) = match_installed_version(&versions, pin) {
            matched
        } else {
            install_git(&package_id, Some(pin.as_str()))
                .map_err(|e| format!("auto-get `{package_id}@{pin}` (from xo.toml): {e}"))?;
            // After install, dir name may be the pin string (tag) or we installed as pin.
            let versions = list_versions(&package_id);
            match_installed_version(&versions, pin).unwrap_or_else(|| pin.clone())
        }
    } else {
        let versions = list_versions(&package_id);
        match select_version(&versions) {
            VersionPick::Use(v) => v,
            VersionPick::NoneInstalled => {
                return Err(format!(
                    "package `{package_id}` is not installed in {} (run `xo get {package_id}`, or list it in xo.toml [dependencies])",
                    packages_root().display()
                ));
            }
            VersionPick::Ambiguous(vs) => {
                return Err(format!(
                    "package `{package_id}` has multiple installed versions ({}) and no `default`; pin in xo.toml [dependencies] or install a single version",
                    vs.join(", ")
                ));
            }
        }
    };
    let root = package_version_dir(&package_id, &version);
    if !root.is_dir() {
        return Err(format!(
            "package `{package_id}@{version}` missing at {}",
            root.display()
        ));
    }

    if module.is_empty() {
        // Package root: try name.echo / main.echo / … else the package root dir if it has sources.
        let last = package_id.rsplit('/').next().unwrap_or("main");
        for name in [last, "main", "mod", "lib"] {
            let p = root.join(format!("{name}.echo"));
            if p.is_file() {
                return p.canonicalize().map_err(|e| e.to_string());
            }
        }
        if !list_echo_files(&root).is_empty() {
            return root.canonicalize().map_err(|e| e.to_string());
        }
        return Err(format!(
            "package `{package_id}@{version}` has no root module at {}",
            root.display()
        ));
    }

    let mut base = root.clone();
    for part in &module {
        base.push(part);
    }
    resolve_file_or_dir_module(&base).map_err(|e| {
        format!(
            "module `{}` in package `{package_id}@{version}`: {e}",
            module.join("/")
        )
    })
}

/// Parse `package[@version]` install specs.
///
/// `version` is `None` when the user omitted `@…` — git installs then **pin to
/// commit hash**; local `--path` installs use [`LOCAL_VERSION`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSpec {
    pub package_id: String,
    /// Tag, branch, or commit; `None` = unspecified (git → hash pin).
    pub version: Option<String>,
}

impl PackageSpec {
    /// `github.com/foo/bar`, `github.com/foo/bar@v1.2.3`, `github.com/foo/bar@abc1234`.
    #[must_use]
    pub fn parse(spec: &str) -> Option<Self> {
        let spec = spec.trim();
        if spec.is_empty() {
            return None;
        }
        let (id, ver) = match spec.rsplit_once('@') {
            Some((id, ver)) if !id.is_empty() && !ver.is_empty() && !id.contains("://") => {
                // Avoid splitting user@host; only @ref when id has a path slash.
                if id.contains('/') {
                    (id, Some(ver.to_string()))
                } else {
                    (spec, None)
                }
            }
            _ => (spec, None),
        };
        // Strip optional scheme
        let id = id
            .strip_prefix("https://")
            .or_else(|| id.strip_prefix("http://"))
            .unwrap_or(id)
            .trim_end_matches('/')
            .to_string();
        if id.is_empty() {
            return None;
        }
        Some(Self {
            package_id: id,
            version: ver,
        })
    }
}

/// Minimal `xo.toml`: dependency pins only.
///
/// Package identity is **where it is hosted / how it is required** (e.g.
/// `github.com/acme/lib` in an import or `xo get` spec) — not a name field.
/// Modules are paths on disk — not listed in TOML.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XoToml {
    pub dependencies: Vec<(String, String)>,
}

impl XoToml {
    /// Parse a subset of TOML without a heavy dependency (hand-rolled tables).
    ///
    /// Only **`[dependencies]`** is read. Other sections (e.g. `[package]`,
    /// `[modules]`) and unknown keys are **ignored**.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut out = Self::default();
        let mut section = String::new();
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].trim().to_string();
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                // Ignore non key=value lines outside our concern.
                continue;
            };
            if section != "dependencies" {
                continue;
            }
            let key = unquote(k.trim())?;
            let val = unquote(v.trim())?;
            out.dependencies.push((key, val));
        }
        Ok(out)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }
}

fn unquote(s: &str) -> Result<String, String> {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        if s.len() < 2 {
            return Err("xo.toml: empty quotes".into());
        }
        return Ok(s[1..s.len() - 1].to_string());
    }
    Ok(s.to_string())
}

/// Install a local directory tree into the package cache (for tests and path packages).
pub fn install_local_dir(package_id: &str, version: &str, src: &Path) -> Result<PathBuf, String> {
    if !src.is_dir() {
        return Err(format!("not a directory: {}", src.display()));
    }
    let dest = package_version_dir(package_id, version);
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| format!("remove {}: {e}", dest.display()))?;
    }
    fs::create_dir_all(dest.parent().unwrap_or(Path::new(".")))
        .map_err(|e| format!("create parent: {e}"))?;
    copy_dir_all(src, &dest).map_err(|e| format!("copy to {}: {e}", dest.display()))?;
    Ok(dest)
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

/// Git clone `https://{package_id}` into the package cache.
///
/// - `Some(ref)` — tag/branch/commit; version dir = that string. Skip clone if dir exists.
/// - `None` — resolve **HEAD** hash via `git ls-remote`, pin to full hash; skip if that hash already cached.
pub fn install_git(package_id: &str, version: Option<&str>) -> Result<PathBuf, String> {
    let url = if package_id.starts_with("http://") || package_id.starts_with("https://") {
        package_id.to_string()
    } else {
        format!("https://{package_id}")
    };

    let version_name = if let Some(ver) = version {
        ver.to_string()
    } else {
        // Unspecified: pin to current remote HEAD hash (no `default` alias).
        let out = std::process::Command::new("git")
            .args(["ls-remote", &url, "HEAD"])
            .output()
            .map_err(|e| format!("git ls-remote: {e}"))?;
        if !out.status.success() {
            return Err(format!("git ls-remote {url} HEAD failed"));
        }
        let line = String::from_utf8_lossy(&out.stdout);
        let hash = line
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if hash.len() < 7 {
            return Err(format!("git ls-remote {url} HEAD: empty hash"));
        }
        hash
    };

    let dest = package_version_dir(package_id, &version_name);
    if dest.is_dir() {
        // Already cached (same tag or same HEAD hash) — no-op.
        return Ok(dest);
    }

    let parent = packages_root().join(encode_package_id(package_id));
    fs::create_dir_all(&parent).map_err(|e| format!("create {}: {e}", parent.display()))?;

    let tmp = parent.join(format!(".tmp-{}", std::process::id()));
    if tmp.exists() {
        fs::remove_dir_all(&tmp).map_err(|e| format!("remove {}: {e}", tmp.display()))?;
    }
    let tmp_s = tmp.to_string_lossy().into_owned();

    let mut cmd = std::process::Command::new("git");
    cmd.args(["clone", "--depth", "1"]);
    if let Some(ver) = version {
        cmd.args(["--branch", ver]);
    }
    cmd.arg(&url).arg(&tmp_s);
    let status = cmd
        .status()
        .map_err(|e| format!("git clone failed to start: {e}"))?;
    if !status.success() {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!(
            "git clone {url} @ {version_name} failed (exit {status})"
        ));
    }

    // When pinning by hash without --branch, verify HEAD matches expected hash.
    if version.is_none() {
        let out = std::process::Command::new("git")
            .args(["-C", &tmp_s, "rev-parse", "HEAD"])
            .output()
            .map_err(|e| format!("git rev-parse: {e}"))?;
        let got = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if got != version_name {
            let _ = fs::remove_dir_all(&tmp);
            return Err(format!(
                "HEAD moved during get: expected {version_name}, got {got}"
            ));
        }
    }

    fs::rename(&tmp, &dest).map_err(|e| {
        let _ = fs::remove_dir_all(&tmp);
        format!("move clone to {}: {e}", dest.display())
    })?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_package_id_flat() {
        assert_eq!(
            encode_package_id("github.com/csfh/echo-pkg"),
            "github.com%2Fcsfh%2Fecho-pkg"
        );
    }

    #[test]
    fn package_spec_parse() {
        let s = PackageSpec::parse("github.com/foo/bar@v1.2.3").unwrap();
        assert_eq!(s.package_id, "github.com/foo/bar");
        assert_eq!(s.version.as_deref(), Some("v1.2.3"));
        let s = PackageSpec::parse("https://github.com/foo/bar").unwrap();
        assert_eq!(s.package_id, "github.com/foo/bar");
        assert_eq!(s.version, None); // pin to hash at install time
    }

    #[test]
    fn select_version_ambiguous() {
        assert_eq!(
            select_version(&["v1".into(), "v2".into()]),
            VersionPick::Ambiguous(vec!["v1".into(), "v2".into()])
        );
        assert_eq!(
            select_version(&["v1".into()]),
            VersionPick::Use("v1".into())
        );
    }

    #[test]
    fn xo_toml_parse() {
        let t = XoToml::parse(
            r#"
[dependencies]
"github.com/other/lib" = "v1.2.3"
"#,
        )
        .unwrap();
        assert_eq!(
            t.dependencies,
            vec![("github.com/other/lib".into(), "v1.2.3".into())]
        );
    }

    #[test]
    fn xo_toml_ignores_unknown_sections() {
        let t = XoToml::parse(
            r#"
[package]
name = "ignored"

[modules]
http = "http"

[dependencies]
"github.com/other/lib" = "v1.2.3"
"#,
        )
        .unwrap();
        assert_eq!(
            t.dependencies,
            vec![("github.com/other/lib".into(), "v1.2.3".into())]
        );
    }

    #[test]
    fn install_and_resolve_host_module() {
        let mut root = std::env::temp_dir();
        root.push(format!("xo-home-{}-{}", std::process::id(), line!()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        with_xo_home_for_test(root.clone(), || {
            let pkg = root.join("src-pkg");
            fs::create_dir_all(&pkg).unwrap();
            fs::write(pkg.join("http.echo"), "\\ ok\n$ ok = 1\n").unwrap();

            install_local_dir("github.com/csfh/echo-pkg", "v1", &pkg).unwrap();

            let segs = vec![
                PathSeg::Name("github.com".into()),
                PathSeg::Name("csfh".into()),
                PathSeg::Name("echo-pkg".into()),
                PathSeg::Name("http".into()),
            ];
            let empty = std::collections::HashMap::new();
            let path = resolve_host_import(&segs, &empty).unwrap();
            assert!(path.ends_with("http.echo"));
            assert!(path.is_file());
        });
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn split_github_import() {
        // Lexer shape: github . com / a / b / http
        let segs = vec![
            PathSeg::Name("github".into()),
            PathSeg::Dot,
            PathSeg::Name("com".into()),
            PathSeg::Name("a".into()),
            PathSeg::Name("b".into()),
            PathSeg::Name("http".into()),
        ];
        let (pkg, module) = split_host_import(&segs).unwrap();
        assert_eq!(pkg, "github.com/a/b");
        assert_eq!(module, vec!["http".to_string()]);
    }

    #[test]
    fn coalesce_github_dot() {
        let segs = vec![
            PathSeg::Name("github".into()),
            PathSeg::Dot,
            PathSeg::Name("com".into()),
            PathSeg::Name("x".into()),
        ];
        assert_eq!(
            coalesce_import_names(&segs),
            vec!["github.com".to_string(), "x".to_string()]
        );
    }
}
