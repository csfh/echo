/**
 * Verifies user-facing docs prose in shipped content modules:
 * - no OBJECTIVE ban markers in narrative strings
 * - no residual Book/Docs twin slogans, keyword antithesis, rule-of-three CTAs
 * - SITE.md records spoken-doc voice + ban list
 * - major nav paths remain present
 *
 * Loads real TypeScript modules via Vite SSR (the same sources the site builds).
 */
import { createServer } from "vite";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** Token-level markers (apply to each prose unit). */
const BAN_PATTERNS = [
  { name: "em-dash", re: /—/ },
  { name: "instead-of", re: /\binstead of\b/i },
  { name: "rather-than", re: /\brather than\b/i },
  { name: "more-than-a", re: /\bmore than a\b/i },
  { name: "in-short", re: /\bIn short\b/ },
  { name: "simply-put", re: /\bSimply put\b/ },
  { name: "genuinely", re: /\bgenuinely\b/i },
  { name: "really", re: /\breally\b/i },
  { name: "truly", re: /\btruly\b/i },
  { name: "actually", re: /\bactually\b/i },
  { name: "leverage", re: /\bleverage\b/i },
  { name: "underscore-verb", re: /\bunderscore\b/i },
  { name: "not-just", re: /\bnot just\b/i },
  { name: "not-only", re: /\bnot only\b/i },
];

/**
 * Multi-word slogan / rhetorical templates that survived earlier greps.
 * Matched against joined paragraph units and chrome string literals.
 */
const SLOGAN_PATTERNS = [
  {
    name: "book-docs-motivation-forms",
    re: /Book supplies motivation|Docs pages hold exact forms|Bare forms live under Docs/i,
  },
  {
    name: "book-why-and-when",
    re: /Book[^.…]{0,80}why and when|why and when[^.…]{0,80}Book|matching Book chapter for why/i,
  },
  {
    name: "docs-book-twin-slogan",
    re: /Book covers why|covers design choices and motivation|form sheets without the surrounding reasoning live under Docs/i,
  },
  {
    name: "keywords-vs-leaders-antithesis",
    re: /Keywords reserve[\s\S]{0,120}Leaders leave/i,
  },
  {
    name: "rule-of-three-write-check-ship",
    re: /Write programs,\s*check them,\s*and ship|Write clear programs\.\s*Check them\.\s*Ship/i,
  },
  {
    name: "parallel-open-open",
    re: /Open a page for exact rules\.\s*Open the matching/i,
  },
  {
    name: "parallel-pages-cover-stack",
    re: /Language pages cover[\s\S]{0,80}Standard library pages document[\s\S]{0,80}Guides cover/i,
  },
  {
    name: "leaders-instead-of-keywords",
    re: /leaders instead of keywords/i,
  },
  {
    name: "reference-vs-book-colon-pair",
    re: /Form-by-form rules are the Reference[\s\S]{0,40}Narrative:\s*the Book/i,
  },
];

const REQUIRED_PATHS = [
  "/docs",
  "/docs/first-program",
  "/docs/leaders",
  "/book",
  "/e26",
  "/e26/spec",
  "/docs/std",
  "/docs/toolchain",
];

function partText(part) {
  return typeof part === "string" ? part : part.code;
}

/** Collect summaries + joined paragraph strings (one unit per paragraph block). */
function collectProse(page) {
  const out = [];
  out.push({ where: `${page.path} summary`, text: page.summary });
  for (const section of page.sections) {
    for (const block of section.blocks) {
      if (block.kind !== "paragraph") continue;
      const joined = block.text.map(partText).join("");
      if (joined.trim()) {
        out.push({ where: `${page.path}#${section.title}`, text: joined });
      }
    }
  }
  return out;
}

function checkPatterns(label, text, patterns, failures) {
  for (const ban of patterns) {
    if (ban.re.test(text)) {
      failures.push(`${label}: banned pattern "${ban.name}" in: ${text.slice(0, 140)}`);
    }
  }
}

const server = await createServer({
  root,
  logLevel: "error",
  server: { middlewareMode: true },
  appType: "custom",
});

try {
  const content = await server.ssrLoadModule("/src/docs/content.ts");
  const ref = await server.ssrLoadModule("/src/docs/std-reference.ts");
  const site = await server.ssrLoadModule("/src/docs/site.ts");

  const { docsPages, docsPageByPath } = content;
  const { stdModules } = ref;
  const { homePage, docsHubCatalog, footerBlurb, legalPages, tryPage } = site;
  const install = await server.ssrLoadModule("/src/docs/install-content.ts");
  const { installPage, inlineText } = install;

  const failures = [];
  const prose = [];

  for (const page of docsPages) {
    prose.push(...collectProse(page));
  }

  for (const m of stdModules) {
    prose.push({ where: `${m.path} summary`, text: m.summary });
    for (const e of m.exports) {
      prose.push({ where: `${m.path}.${e.name} description`, text: e.description });
      prose.push({ where: `${m.path}.${e.name} role`, text: e.role });
      if (e.params) {
        prose.push({ where: `${m.path}.${e.name} params`, text: e.params });
      }
    }
  }

  prose.push({ where: "home definition", text: homePage.definition });
  prose.push({ where: "home lead", text: homePage.lead });
  prose.push({ where: "home status", text: homePage.status });
  prose.push({ where: "footer blurb", text: footerBlurb });
  prose.push({ where: "try lead", text: tryPage.lead });
  prose.push({ where: "install lead", text: inlineText(installPage.lead) });
  for (const section of installPage.sections) {
    for (const paragraph of section.paragraphs) {
      prose.push({ where: `install ${section.title}`, text: inlineText(paragraph) });
    }
  }
  for (const link of homePage.links) {
    prose.push({ where: `home link ${link.title}`, text: link.description });
  }
  for (const group of docsHubCatalog) {
    prose.push({ where: `hub ${group.title}`, text: group.description });
    for (const entry of group.entries) {
      prose.push({ where: `hub ${group.title} ${entry.title}`, text: entry.description });
    }
  }
  for (const page of legalPages) {
    prose.push({ where: `${page.path} summary`, text: page.summary });
    for (const section of page.sections) {
      for (const paragraph of section.paragraphs) {
        prose.push({ where: `${page.path}#${section.title}`, text: paragraph });
      }
    }
  }

  for (const item of prose) {
    checkPatterns(item.where, item.text, BAN_PATTERNS, failures);
    checkPatterns(item.where, item.text, SLOGAN_PATTERNS, failures);
  }

  for (const p of REQUIRED_PATHS) {
    if (!docsPageByPath.get(p)) {
      failures.push(`missing required docs path: ${p}`);
    }
  }

  const siteMd = readFileSync(path.join(root, "SITE.md"), "utf8");
  for (const needle of [
    "Spoken-doc intent",
    "Prose ban list",
    "Antithesis",
    "Contrasting pairs",
    "Rule of three",
    "Em dashes in prose",
    "Filler intensifiers",
    "Corporate-register verbs",
    "Write public copy as calm programming-language documentation",
  ]) {
    if (!siteMd.includes(needle)) {
      failures.push(`SITE.md missing voice rule marker: ${needle}`);
    }
  }

  // High-visibility TSX chrome: em dashes + slogan templates inside string literals
  const tsxFiles = [
    "src/app.tsx",
    "src/install.tsx",
    "src/legal.tsx",
    "src/security.tsx",
    "src/router.tsx",
    "src/docs/site.ts",
    "src/docs/install-content.ts",
  ];
  const stringLitRe = /"(?:\\.|[^"\\])*"|`(?:\\.|[^`\\])*`/g;
  for (const rel of tsxFiles) {
    const src = readFileSync(path.join(root, rel), "utf8");
    let m;
    while ((m = stringLitRe.exec(src))) {
      const lit = m[0].slice(1, -1);
      if (lit.includes("—")) {
        failures.push(`${rel}: em dash in string literal: ${m[0].slice(0, 80)}`);
      }
      checkPatterns(`${rel} string`, lit, SLOGAN_PATTERNS, failures);
      checkPatterns(
        `${rel} string`,
        lit,
        BAN_PATTERNS.filter((b) => b.name !== "em-dash"),
        failures,
      );
    }
  }

  if (failures.length) {
    console.error(JSON.stringify({ ok: false, failures }, null, 2));
    process.exitCode = 1;
  } else {
    const report = {
      ok: true,
      pages: docsPages.length,
      proseUnits: prose.length,
      stdModules: stdModules.length,
      homeLinks: homePage.links.length,
      banPatterns: BAN_PATTERNS.length,
      sloganPatterns: SLOGAN_PATTERNS.length,
      requiredPaths: REQUIRED_PATHS,
    };
    console.log(JSON.stringify(report, null, 2));
  }
} finally {
  await server.close();
}
