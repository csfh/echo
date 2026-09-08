/**
 * After vite build, every catalog / footer / nav destination must exist as a
 * real HTML file with that page's title and body. SPA-only copies of index.html
 * are not enough.
 */
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dist = path.join(root, "dist");

const server = await createServer({
  root,
  logLevel: "error",
  server: { middlewareMode: true },
  appType: "custom",
});

try {
  const site = await server.ssrLoadModule("/src/docs/site.ts");
  const staticHtml = await server.ssrLoadModule("/src/docs/static-html.ts");
  const { publicChromePaths } = site;
  const { distFileForPath, staticPageByPath } = staticHtml;
  const pages = staticPageByPath();
  const failures = [];

  function fail(message) {
    failures.push(message);
  }

  if (!existsSync(path.join(dist, "index.html"))) {
    fail("dist/index.html is missing; run this script after vite build");
  }

  for (const route of publicChromePaths()) {
    const page = pages.get(route);
    if (!page) {
      fail(`${route}: renderer has no static page`);
      continue;
    }

    const file = path.join(dist, distFileForPath(route));
    if (!existsSync(file)) {
      fail(`${route}: missing ${path.relative(root, file)}`);
      continue;
    }

    const html = readFileSync(file, "utf8");
    if (!html.includes(`<title>${page.title}</title>`)) {
      fail(`${route}: ${path.relative(root, file)} is missing title ${page.title}`);
    }
    if (!html.includes(page.body)) {
      fail(`${route}: ${path.relative(root, file)} is missing the page body`);
    }
    if (route !== "/" && html.includes(`<h1>${site.homePage.definition}</h1>`)) {
      fail(`${route}: ${path.relative(root, file)} still has the homepage snapshot`);
    }
  }

  if (failures.length) {
    console.error(JSON.stringify({ ok: false, failures }, null, 2));
    process.exitCode = 1;
  } else {
    console.log(
      JSON.stringify(
        {
          ok: true,
          files: publicChromePaths().map((route) => distFileForPath(route)),
        },
        null,
        2,
      ),
    );
  }
} finally {
  await server.close();
}
