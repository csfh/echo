/**
 * Verifies Cloudflare Pages serves SPA routes as HTTP 200:
 * - public/_redirects rewrites /* to /index.html with status 200
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const redirectsPath = path.join(root, "public", "_redirects");
const source = readFileSync(redirectsPath, "utf8");

const failures = [];

function fail(message) {
  failures.push(message);
}

const rewrite = source
  .split("\n")
  .map((line) => line.trim())
  .filter((line) => line && !line.startsWith("#"))
  .find((line) => {
    const parts = line.split(/\s+/);
    return parts[0] === "/*" && parts[1] === "/index.html" && parts[2] === "200";
  });

if (!rewrite) {
  fail("public/_redirects must rewrite /* to /index.html with status 200");
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.error(failure);
  }
  process.exit(1);
}

console.log("verify-spa-redirects: ok");
