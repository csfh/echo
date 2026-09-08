/**
 * Copy web-tree-sitter runtime wasm + generated Echo language assets into public/.
 *
 * Echo grammar: regenerate with
 *   cargo build -p xo && ./target/debug/xo tools grammar tree-sitter -o grammars/tree-sitter-echo
 *   (cd grammars/tree-sitter-echo && tree-sitter generate && tree-sitter build --wasm -o tree-sitter-echo.wasm .)
 * then re-run this script (or npm install / npm run sync:tree-sitter).
 */
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const wwwRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = join(wwwRoot, "..");
const outDir = join(wwwRoot, "public", "tree-sitter");
mkdirSync(outDir, { recursive: true });

const runtimeWasm = join(wwwRoot, "node_modules", "web-tree-sitter", "web-tree-sitter.wasm");
const echoWasm = join(repoRoot, "grammars", "tree-sitter-echo", "tree-sitter-echo.wasm");
const highlights = join(repoRoot, "grammars", "tree-sitter-echo", "queries", "highlights.scm");

function requireFile(path, label) {
  if (!existsSync(path)) {
    console.warn(`sync-tree-sitter: missing ${label}: ${path}`);
    return false;
  }
  return true;
}

if (requireFile(runtimeWasm, "web-tree-sitter.wasm")) {
  copyFileSync(runtimeWasm, join(outDir, "web-tree-sitter.wasm"));
  copyFileSync(runtimeWasm, join(outDir, "tree-sitter.wasm"));
}

if (requireFile(echoWasm, "tree-sitter-echo.wasm")) {
  copyFileSync(echoWasm, join(outDir, "tree-sitter-echo.wasm"));
}

if (requireFile(highlights, "highlights.scm")) {
  copyFileSync(highlights, join(outDir, "highlights.scm"));
}

console.log("sync-tree-sitter: public/tree-sitter ready");
