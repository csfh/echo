/**
 * Verifies Cloudflare Pages serves unknown paths as the SPA shell with HTTP 200.
 *
 * Pages has no rewrite rule for this: with no top-level 404.html it falls back
 * to SPA mode and serves /index.html with 200. A `/* /index.html 200` line in
 * _redirects is rejected by Pages as an infinite loop, so it must not exist,
 * and a public/404.html turns the fallback off (real 404 for unknown paths).
 */
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const failures = [];

function fail(message) {
  failures.push(message);
}

if (existsSync(path.join(root, "public", "404.html"))) {
  fail("public/404.html must not exist: Pages would 404 unknown paths instead of serving the SPA");
}

if (existsSync(path.join(root, "public", "_redirects"))) {
  fail("public/_redirects must not exist: the SPA fallback is Pages' built-in mode, not a rule");
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.error(failure);
  }
  process.exit(1);
}

console.log("verify-spa-redirects: ok");
