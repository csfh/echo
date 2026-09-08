/**
 * Verifies shipped std package docs:
 * - every public export has full entry fields
 * - package pages use Introduction / Constants / Struct · / Functions outline
 * - callables render prose + example + parameters/returns
 * - documented struct methods appear as "struct · method" sections
 * - search indexes exports
 *
 * Loads real TypeScript modules via Vite SSR (the same sources the site builds).
 */
import { createServer } from "vite";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const server = await createServer({
  root,
  logLevel: "error",
  server: { middlewareMode: true },
  appType: "custom",
});

try {
  const ref = await server.ssrLoadModule("/src/docs/std-reference.ts");
  const content = await server.ssrLoadModule("/src/docs/content.ts");
  const search = await server.ssrLoadModule("/src/docs/search.ts");

  const { stdModules, stdExportCount, assertStdReferenceComplete, stdExportKind, stdMethodsFor } =
    ref;
  const { docsPageByPath, headingId } = content;
  const { buildDocsSearchRecords } = search;

  assertStdReferenceComplete();

  let fullCount = 0;
  let methodCount = 0;
  for (const m of stdModules) {
    for (const e of m.exports) {
      if (!e.call || !e.description || !e.params || !e.returns || !e.example) {
        throw new Error(`${m.path}.${e.name}: missing package entry fields after assert`);
      }
      fullCount += 1;
      methodCount += stdMethodsFor(m.path, e.name).length;
    }
  }

  for (const m of stdModules) {
    const page = docsPageByPath.get(m.docsPath);
    if (!page) {
      throw new Error(`missing docs page for ${m.docsPath}`);
    }

    const intro = page.sections.find((s) => s.title === "Introduction");
    if (!intro) {
      throw new Error(`${m.docsPath}: missing Introduction section`);
    }

    const consts = m.exports.filter((e) => stdExportKind(e) === "const");
    const funcs = m.exports.filter((e) => stdExportKind(e) === "func");

    if (consts.length > 0 && !page.sections.some((s) => s.title === "Constants")) {
      throw new Error(`${m.docsPath}: missing Constants group`);
    }
    if (funcs.length > 0 && !page.sections.some((s) => s.title === "Functions")) {
      throw new Error(`${m.docsPath}: missing Functions group`);
    }

    for (const e of m.exports) {
      const kind = stdExportKind(e);
      const title = kind === "struct" ? `Struct · ${e.name}` : e.name;
      const section = page.sections.find((s) => s.title === title);
      if (!section) {
        throw new Error(`${m.docsPath}: missing section for ${kind} ${e.name} (title ${title})`);
      }

      const text = section.blocks
        .filter((b) => b.kind === "paragraph")
        .map((b) => b.text.map((p) => (typeof p === "string" ? p : p.code)).join(""))
        .join(" ");
      if (!text.includes(e.description) && !text.includes("Call form")) {
        throw new Error(`${m.path}.${e.name}: section missing description`);
      }
      if (!text.includes(e.call)) {
        throw new Error(`${m.path}.${e.name}: call form not rendered`);
      }

      const hasParams = section.blocks.some(
        (b) =>
          b.kind === "paragraph" &&
          b.text.some((p) => typeof p === "string" && p.includes("Parameters:")),
      );
      const hasReturns = section.blocks.some(
        (b) =>
          b.kind === "paragraph" &&
          b.text.some((p) => typeof p === "string" && p.includes("Returns:")),
      );
      const hasExample = section.blocks.some((b) => b.kind === "code" && b.code === e.example);
      if (!hasParams || !hasReturns || !hasExample) {
        throw new Error(`${m.path}.${e.name}: entry missing params/returns/example in section`);
      }

      if (kind === "struct") {
        for (const method of stdMethodsFor(m.path, e.name)) {
          const mTitle = `${e.name} · ${method.name}`;
          const mSection = page.sections.find((s) => s.title === mTitle);
          if (!mSection) {
            throw new Error(`${m.docsPath}: missing method section ${mTitle}`);
          }
          const mText = mSection.blocks
            .filter((b) => b.kind === "paragraph")
            .map((b) => b.text.map((p) => (typeof p === "string" ? p : p.code)).join(""))
            .join(" ");
          if (!mText.includes(method.call)) {
            throw new Error(`${m.path}.${e.name}.${method.name}: method call not rendered`);
          }
          const mExample = mSection.blocks.some(
            (b) => b.kind === "code" && b.code === method.example,
          );
          if (!mExample) {
            throw new Error(`${m.path}.${e.name}.${method.name}: method example missing`);
          }
        }
      }
    }
  }

  const index = docsPageByPath.get("/docs/std/reference");
  if (!index) {
    throw new Error("missing /docs/std/reference");
  }
  if (!index.summary.includes(String(stdExportCount))) {
    throw new Error("API index summary missing export count");
  }

  const records = buildDocsSearchRecords();
  const sampleExports = [
    { path: "/docs/std/io", name: "print", call: "io.print" },
    { path: "/docs/std/str", name: "parse_int", call: "str.parse_int" },
    { path: "/docs/std/list", name: "sum_ints", call: "list.sum_ints" },
    { path: "/docs/std/math", name: "abs_i", call: "math.abs_i" },
    { path: "/docs/std/fs", name: "file · read", call: "file.read" },
  ];
  for (const sample of sampleExports) {
    const hit = records.find(
      (r) =>
        r.path === `${sample.path}#${headingId(sample.name)}` ||
        (r.path.startsWith(sample.path) && r.title === sample.name),
    );
    if (!hit) {
      throw new Error(`search missing section record for ${sample.path} ${sample.name}`);
    }
    const blob = `${hit.title} ${hit.body} ${hit.code} ${hit.tags} ${hit.aliases}`;
    if (!blob.includes(sample.name.split(" · ").pop()) && !blob.includes(sample.call)) {
      throw new Error(`search record for ${sample.name} does not include export name`);
    }
  }

  // Outline smoke: fs page has Struct · file and file · read
  const fsPage = docsPageByPath.get("/docs/std/fs");
  if (!fsPage?.sections.some((s) => s.title === "Struct · file")) {
    throw new Error("fs page missing Struct · file");
  }
  if (!fsPage?.sections.some((s) => s.title === "file · read")) {
    throw new Error("fs page missing file · read method");
  }
  if (!fsPage?.sections.some((s) => s.title === "Functions")) {
    throw new Error("fs page missing Functions group");
  }

  const report = {
    ok: true,
    modules: stdModules.length,
    exports: stdExportCount,
    fullEntries: fullCount,
    structMethods: methodCount,
    layout: "package → constants → struct(+methods) → functions; prose + example + params/returns",
    sampleAnchors: sampleExports.map((s) => `${s.path}#${headingId(s.name)}`),
    searchRecords: records.length,
  };
  console.log(JSON.stringify(report, null, 2));
} finally {
  await server.close();
}
