//! MSVC link flags for official LLVM Windows tarballs.
//!
//! LLVM 19+ x64 Windows releases vendor rpmalloc into LLVMSupport as
//! `malloc` / `free` / `realloc` (`LLVM_ENABLE_RPMALLOC`, static CRT). rustc
//! on MSVC uses the UCRT (`/MD`, `/defaultlib:msvcrt`). Statically linking
//! llvm-sys then fails with LNK2005 / LNK1169 (v0.0.1-alpha.11 run
//! 32506671329). llvm-sys cannot `prefer-dynamic` on MSVC.
//!
//! Keep rpmalloc's definitions and drop LLVM's `libcmt` defaultlib. See
//! [`docs/ci.md`](../../docs/ci.md).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let Ok(env) = std::env::var("CARGO_CFG_TARGET_ENV") else {
        return;
    };
    if env != "msvc" {
        return;
    }
    println!("cargo:rustc-link-arg=/FORCE:MULTIPLE");
    println!("cargo:rustc-link-arg=/NODEFAULTLIB:libcmt");
}
