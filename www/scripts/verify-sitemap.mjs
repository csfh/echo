/**
 * Verifies public discovery files for xo.run:
 * - sitemap.xml lists the public catalog on https://xo.run
 * - robots.txt has User-agent rules and a Sitemap: line
 * - Privacy and Terms are listed only when those pages exist
 * - github.io is never listed
 *
 * Loads src/docs/site.ts, content.ts, and static-html.ts through Vite SSR.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const server = await createServer({
  root,
  logLevel: "error",
  server: { middlewareMode: true },
  appType: "custom",
});

try {
  const site = await server.ssrLoadModule("/src/docs/site.ts");
  const content = await server.ssrLoadModule("/src/docs/content.ts");
  const staticHtml = await server.ssrLoadModule("/src/docs/static-html.ts");

  const {
    collectPublicCatalogPaths,
    omittedCatalogPaths,
    publicCatalogUrl,
    publicChromePaths,
    publicSiteOrigin,
    publicSurfacePaths,
    renderRobotsTxt,
    renderSitemapXml,
  } = site;
  const { docsPages } = content;
  const { staticPages } = staticHtml;

  const failures = [];

  function fail(message) {
    failures.push(message);
  }

  if (publicSiteOrigin !== "https://xo.run") {
    fail(`publicSiteOrigin must be https://xo.run, got ${publicSiteOrigin}`);
  }

  const existingPagePaths = [
    ...new Set([...staticPages().map((page) => page.path), ...docsPages.map((page) => page.path)]),
  ];
  const existing = new Set(existingPagePaths);
  const catalogPaths = collectPublicCatalogPaths(existingPagePaths);
  const sitemap = renderSitemapXml(catalogPaths);
  const robots = renderRobotsTxt();
  const committedRobots = readFileSync(path.join(root, "public/robots.txt"), "utf8");

  if (!sitemap.startsWith('<?xml version="1.0" encoding="UTF-8"?>')) {
    fail("sitemap.xml must be a real XML document");
  }
  if (!sitemap.includes('<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">')) {
    fail("sitemap.xml must use the sitemaps.org urlset namespace");
  }

  const requiredPaths = [
    ...publicSurfacePaths,
    ...publicChromePaths(),
    "/docs",
    "/docs/std",
    "/docs/leaders",
    "/book",
    "/e26",
    "/e26/spec",
    "/try",
    "/install",
  ];
  for (const required of requiredPaths) {
    if (!catalogPaths.includes(required)) {
      fail(`public catalog missing ${required}`);
    }
    const loc = publicCatalogUrl(required);
    if (!sitemap.includes(`<loc>${loc}</loc>`)) {
      fail(`sitemap.xml missing ${loc}`);
    }
  }

  for (const page of docsPages) {
    if (!catalogPaths.includes(page.path)) {
      fail(`docs page ${page.path} missing from the public catalog`);
    }
  }

  for (const candidate of omittedCatalogPaths) {
    if (existing.has(candidate)) {
      if (!catalogPaths.includes(candidate)) {
        fail(`public catalog missing existing page ${candidate}`);
      }
      continue;
    }
    if (catalogPaths.includes(candidate)) {
      fail(`public catalog must not invent ${candidate}`);
    }
    if (sitemap.toLowerCase().includes(candidate)) {
      fail(`sitemap.xml must not list ${candidate} until that page exists`);
    }
  }

  if (/github\.io/i.test(sitemap) || /github\.io/i.test(robots)) {
    fail("discovery files must not list the GitHub Pages host");
  }
  if (!/https:\/\/xo\.run\//.test(sitemap) || /http:\/\/xo\.run/.test(sitemap)) {
    fail("sitemap.xml must use https://xo.run as the live host");
  }

  if (!/^User-agent:\s+\*/m.test(robots)) {
    fail("robots.txt must include a User-agent rule");
  }
  if (!/^Allow:\s+\//m.test(robots)) {
    fail("robots.txt must include an Allow rule");
  }
  if (!/^Sitemap:\s+https:\/\/xo\.run\/sitemap\.xml$/m.test(robots)) {
    fail("robots.txt must point Sitemap: at https://xo.run/sitemap.xml");
  }
  if (committedRobots !== robots) {
    fail("public/robots.txt must match renderRobotsTxt()");
  }

  if (failures.length) {
    console.error(JSON.stringify({ ok: false, failures }, null, 2));
    process.exitCode = 1;
  } else {
    console.log(
      JSON.stringify(
        {
          ok: true,
          origin: publicSiteOrigin,
          urls: catalogPaths.length,
          sitemapBytes: sitemap.length,
        },
        null,
        2,
      ),
    );
  }
} finally {
  await server.close();
}
