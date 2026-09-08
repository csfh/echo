//! Embed `std/**/*.echo` so the browser check host has a closed std graph.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let std_dir = manifest
        .parent()
        .and_then(Path::parent)
        .expect("crates/echo_wasm")
        .join("std");
    println!("cargo:rerun-if-changed={}", std_dir.display());

    let mut files = Vec::new();
    collect_echo_files(&std_dir, &std_dir, &mut files);
    files.sort();

    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join("std_files.rs");
    let mut out = String::from("pub static STD_FILES: &[(&str, &str)] = &[\n");
    for rel in &files {
        let rel_unix = rel.to_string_lossy().replace('\\', "/");
        let abs = std_dir.join(rel);
        println!("cargo:rerun-if-changed={}", abs.display());
        out.push_str("    (\"");
        out.push_str(&rel_unix);
        out.push_str("\", include_str!(r#\"");
        out.push_str(&abs.display().to_string());
        out.push_str("\"#)),\n");
    }
    out.push_str("];\n");
    fs::write(dest, out).expect("write std_files.rs");
}

fn collect_echo_files(std_root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_echo_files(std_root, &path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("echo") {
            if let Ok(rel) = path.strip_prefix(std_root) {
                out.push(rel.to_path_buf());
            }
        }
    }
}
