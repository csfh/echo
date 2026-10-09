/**
 * Homepage snippets must be real files. Each `homePage.examples[].code` equals
 * its `source` file under the repository root, and `homePage.sample` equals
 * `samples/hero.echo`. Leading `;` comment lines in the files are not shown on
 * the page. When an `xo` binary exists (XO_BIN, or ../target/debug/xo), each
 * snippet also runs and its stdout must equal the output shown beside it.
 */
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { createServer } from "vite";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(root, "..");

function shown(text) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  while (lines.length && (lines[0].startsWith(";") || lines[0].trim() === "")) {
    lines.shift();
  }
  return lines.join("\n").trimEnd();
}

const server = await createServer({
  root,
  logLevel: "error",
  server: { middlewareMode: true },
  appType: "custom",
});

try {
  const { homePage } = await server.ssrLoadModule("/src/docs/site.ts");
  const failures = [];

  const xoBin =
    process.env.XO_BIN ??
    [path.join(repoRoot, "target/debug/xo"), path.join(repoRoot, "target/release/xo")].find(
      existsSync,
    );

  const cases = [
    {
      label: "hero",
      file: path.join(root, "samples/hero.echo"),
      code: homePage.sample,
      output: homePage.sampleOutput,
    },
    ...homePage.examples.map((example) => ({
      label: example.source,
      file: path.join(repoRoot, example.source),
      code: example.code,
      output: example.output,
    })),
  ];

  let executed = 0;
  for (const item of cases) {
    if (!existsSync(item.file)) {
      failures.push(`${item.label}: source file is missing`);
      continue;
    }
    if (shown(readFileSync(item.file, "utf8")) !== item.code.trimEnd()) {
      failures.push(
        `${item.label}: homepage code differs from ${path.relative(repoRoot, item.file)}`,
      );
    }
    if (xoBin) {
      const run = spawnSync(xoBin, ["run", item.file], { encoding: "utf8", cwd: repoRoot });
      executed += 1;
      if (run.status !== 0) {
        failures.push(`${item.label}: xo run exited ${run.status}: ${run.stderr.slice(0, 200)}`);
      } else if (run.stdout.trimEnd() !== item.output) {
        failures.push(
          `${item.label}: output ${JSON.stringify(run.stdout.trimEnd())} differs from page ${JSON.stringify(item.output)}`,
        );
      }
    }
  }

  if (failures.length) {
    console.error(JSON.stringify({ ok: false, failures }, null, 2));
    process.exitCode = 1;
  } else {
    console.log(
      JSON.stringify({
        ok: true,
        snippets: cases.length,
        executed,
        note: xoBin ? undefined : "xo binary not found; output was not re-run",
      }),
    );
  }
} finally {
  await server.close();
}
