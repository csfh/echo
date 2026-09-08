/**
 * Verifies the /try playground contract:
 * - default Sum sample is the homepage sum.echo figure
 * - page copy names host limits and does not claim `xo run` or native LLVM
 * - shipped www/public/echo-wasm bindings check the sample and capture io.print
 *
 * Loads site/playground modules through Vite SSR. Instantiates the committed
 * wasm bindings in-process so Pages can ship without a Rust toolchain.
 */
import { createServer } from "vite";
import { existsSync, readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const wasmDir = path.join(root, "public/echo-wasm");
const trySourcePath = path.join(root, "src/try.tsx");

const server = await createServer({
  root,
  logLevel: "error",
  server: { middlewareMode: true },
  appType: "custom",
});

const failures = [];

function fail(message) {
  failures.push(message);
}

try {
  const site = await server.ssrLoadModule("/src/docs/site.ts");
  const playground = await server.ssrLoadModule("/src/lib/playground.ts");
  const tryPage = await server.ssrLoadModule("/src/try.tsx");

  const homepage = site.homePage.sample.trim();
  const sum = playground.playgroundSumSource().trim();
  if (sum !== homepage) {
    fail("playgroundSumSource must match homePage.sample");
  }

  const samples = tryPage.PLAYGROUND_SAMPLES;
  const sumSample = samples?.find((sample) => sample.id === "sum");
  if (sumSample == null) {
    fail("PLAYGROUND_SAMPLES must include a sum sample");
  } else if (sumSample.source.trim() !== homepage) {
    fail("PLAYGROUND_SAMPLES sum source must match homePage.sample");
  }

  const limits = playground.PLAYGROUND_HOST_LIMITS;
  for (const limit of ["filesystem", "net", "process", "tasks"]) {
    if (!limits?.includes(limit)) {
      fail(`PLAYGROUND_HOST_LIMITS missing ${limit}`);
    }
  }

  const trySource = `${readFileSync(trySourcePath, "utf8")}\n${site.tryPage.lead}`;
  if (/\bxo run\b/.test(trySource)) {
    fail("/try must not describe itself as xo run");
  }
  if (/native LLVM/i.test(trySource)) {
    fail("/try must not claim native LLVM in the browser");
  }
  for (const limit of ["filesystem", "net", "process", "tasks"]) {
    if (!trySource.toLowerCase().includes(limit)) {
      fail(`/try page must name host limit ${limit}`);
    }
  }
  if (!trySource.includes("io.print")) {
    fail("/try page must mention captured io.print");
  }

  const requiredWasm = ["echo_wasm.js", "echo_wasm_bg.wasm", "echo_wasm.d.ts"];
  for (const name of requiredWasm) {
    if (!existsSync(path.join(wasmDir, name))) {
      fail(`missing shipped binding www/public/echo-wasm/${name}`);
    }
  }

  if (failures.length === 0) {
    const jsUrl = pathToFileURL(path.join(wasmDir, "echo_wasm.js")).href;
    const wasmBytes = await readFile(path.join(wasmDir, "echo_wasm_bg.wasm"));
    const wasm = await import(jsUrl);
    await wasm.default({ module_or_path: wasmBytes });

    if (typeof wasm.check !== "function" || typeof wasm.playgroundRun !== "function") {
      fail("echo_wasm bindings must export check and playgroundRun");
    } else {
      const checked = JSON.parse(wasm.check(site.homePage.sample));
      if (!checked.ok) {
        fail(`homepage sample must check: ${JSON.stringify(checked)}`);
      }
      const ran = JSON.parse(wasm.playgroundRun(site.homePage.sample));
      if (!ran.ok || ran.printed !== "sum=6\n") {
        fail(`homepage sample must print sum=6, got ${JSON.stringify(ran)}`);
      }
    }
  }
} catch (error) {
  fail(error instanceof Error ? error.message : String(error));
} finally {
  await server.close();
}

if (failures.length) {
  console.error(JSON.stringify({ ok: false, failures }, null, 2));
  process.exitCode = 1;
} else {
  console.log(
    JSON.stringify({
      ok: true,
      sample: "sum.echo",
      printed: "sum=6",
      wasm: "www/public/echo-wasm",
    }),
  );
}
