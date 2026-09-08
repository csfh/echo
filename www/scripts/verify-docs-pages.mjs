/**
 * Verifies the shipped docs-first site model:
 * - homepage definition, leader-bearing sample, docs/std/spec links
 * - public facts: compiled language, MIT, Rust, LLVM, xo, Echo 2026, prerelease
 * - no production-ready, crates.io, Windows, or Discord claims
 * - primary nav labels and paths
 * - footer About links Privacy, Terms, and MIT; Discord stays hidden
 * - legal pages use oss@christofferhallas.com mail only
 * - Documents hub catalog groups
 * - security mailbox, SECURITY.md, and footer /security pointer
 * - every language-feature catalog entry is a real page with a summary
 *   and at least one Echo code block
 * - catalog, footer, and nav destinations have static HTML snapshots
 *
 * Loads src/docs/site.ts, src/docs/content.ts, and src/lib/current-release.ts
 * through Vite SSR. Also checks that /install, README, and docs/install.md
 * name the current prerelease and its real assets.
 */
import { createServer } from "vite";
import { readFileSync } from "node:fs";
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
  const site = await server.ssrLoadModule("/src/docs/site.ts");
  const content = await server.ssrLoadModule("/src/docs/content.ts");
  const release = await server.ssrLoadModule("/src/lib/current-release.ts");

  const {
    docsHubCatalog,
    footerBlurb,
    footerLinkGroups,
    homePage,
    installCta,
    languageFeatureEntries,
    legalPages,
    primaryNav,
    primaryNavItemIsActive,
    privacyPage,
    publicChromePaths,
    publicMailAddresses,
    renderStaticHomeAndHub,
    securityContact,
    termsPage,
  } = site;
  const { docsPageByPath } = content;
  const staticHtml = await server.ssrLoadModule("/src/docs/static-html.ts");
  const { distFileForPath, staticPageByPath } = staticHtml;

  const failures = [];

  function fail(message) {
    failures.push(message);
  }

  if (!/compiled language/i.test(homePage.definition)) {
    fail("homePage.definition must name Echo as a compiled language");
  }
  if (!/^\$ /m.test(homePage.sample) || !/^~ /m.test(homePage.sample)) {
    fail("homePage.sample must show $ and ~ statement leaders");
  }

  const publicChrome = [homePage.definition, homePage.lead, homePage.status, footerBlurb].join(
    "\n",
  );
  const requiredFacts = [
    "compiled language",
    "MIT",
    "Rust",
    "LLVM",
    "xo",
    "Echo 2026",
    "prerelease",
  ];
  for (const needle of requiredFacts) {
    if (!new RegExp(needle, "i").test(publicChrome)) {
      fail(`public chrome must mention ${needle}`);
    }
  }
  for (const banned of [
    "production-ready",
    "production ready",
    "crates.io",
    "Windows",
    "Discord",
  ]) {
    if (publicChrome.toLowerCase().includes(banned.toLowerCase())) {
      fail(`public chrome must not mention ${banned}`);
    }
  }

  const homeTargets = new Set(homePage.links.map((link) => link.to));
  for (const required of ["/docs", "/docs/std", "/e26"]) {
    if (!homeTargets.has(required)) {
      fail(`homePage.links missing ${required}`);
    }
  }

  const navByLabel = new Map(primaryNav.map((item) => [item.label, item.to]));
  const expectedNav = [
    ["Documents", "/docs"],
    ["Packages", "/docs/std"],
    ["Echo 2026", "/e26"],
    ["Try", "/try"],
  ];
  for (const [label, to] of expectedNav) {
    if (navByLabel.get(label) !== to) {
      fail(`primaryNav ${label} should be ${to}, got ${navByLabel.get(label)}`);
    }
  }
  if (installCta.label !== "Install" || installCta.to !== "/install") {
    fail("installCta must be Install → /install");
  }

  const footerLinks = footerLinkGroups.flatMap((group) => group.links);
  const footerByLabel = new Map(footerLinks.map((link) => [link.label, link.href]));
  if (footerByLabel.get("Privacy") !== "/privacy") {
    fail(`footer Privacy should be /privacy, got ${footerByLabel.get("Privacy")}`);
  }
  if (footerByLabel.get("Terms") !== "/terms") {
    fail(`footer Terms should be /terms, got ${footerByLabel.get("Terms")}`);
  }
  if (footerByLabel.get("Security") !== securityContact.path) {
    fail(`footer Security should be ${securityContact.path}, got ${footerByLabel.get("Security")}`);
  }
  if (footerByLabel.get("Christoffer Hallas") !== "https://christofferhallas.com") {
    fail("footer About must keep the Christoffer Hallas link");
  }
  if (footerByLabel.get("MIT License") !== "https://github.com/csfh/echo/blob/main/LICENSE") {
    fail("footer About must link the MIT License");
  }
  if (footerByLabel.has("Discord") || footerLinks.some((link) => /discord/i.test(link.label))) {
    fail("footer must hide Discord until there is a public invite");
  }

  const expectedMail = ["oss@christofferhallas.com"];
  if (JSON.stringify([...publicMailAddresses]) !== JSON.stringify(expectedMail)) {
    fail(`publicMailAddresses must be ${expectedMail.join(", ")}`);
  }

  if (privacyPage.path !== "/privacy" || termsPage.path !== "/terms") {
    fail("legal pages must live at /privacy and /terms");
  }
  if (legalPages.length !== 2) {
    fail("legalPages should be Privacy and Terms only");
  }

  const legalText = legalPages
    .flatMap((page) => [page.summary, ...page.sections.flatMap((section) => section.paragraphs)])
    .join("\n");
  for (const address of expectedMail) {
    if (!legalText.includes(address)) {
      fail(`legal pages missing ${address}`);
    }
  }
  if (/modoterra/i.test(legalText)) {
    fail("legal pages must not mention Modoterra");
  }
  if (legalText.includes("github.io")) {
    fail("legal pages must not list github.io");
  }
  const publishedMail = legalText.match(/[a-z0-9._%+-]+@[a-z0-9-]+(?:\.[a-z0-9-]+)+/gi) ?? [];
  for (const address of publishedMail) {
    if (!address.endsWith("@christofferhallas.com")) {
      fail(`legal pages published unexpected mail: ${address}`);
    }
  }

  if (securityContact.email !== "oss@christofferhallas.com") {
    fail(`security mailbox must be oss@christofferhallas.com, got ${securityContact.email}`);
  }
  if (securityContact.mailto !== `mailto:${securityContact.email}`) {
    fail("securityContact.mailto must match the public mailbox");
  }
  if (securityContact.path !== "/security") {
    fail("securityContact.path must be /security");
  }
  if (!securityContact.policyUrl.includes("SECURITY.md")) {
    fail("securityContact.policyUrl must point at SECURITY.md");
  }
  if (!securityContact.policyUrl.includes("github.com/csfh/echo")) {
    fail("securityContact.policyUrl must point at this repository");
  }

  if (!primaryNavItemIsActive("/docs", "/docs/leaders")) {
    fail("Documents nav should be active on /docs/leaders");
  }
  if (primaryNavItemIsActive("/docs", "/docs/std/io")) {
    fail("Documents nav should stay inactive on package pages");
  }
  if (!primaryNavItemIsActive("/docs/std", "/docs/std/io")) {
    fail("Packages nav should be active on /docs/std/io");
  }
  if (!primaryNavItemIsActive("/e26", "/e26/spec")) {
    fail("Echo 2026 nav should be active on /e26/spec");
  }

  const groups = new Map(docsHubCatalog.map((group) => [group.title, group]));
  for (const title of ["Start", "Language", "Packages", "Spec"]) {
    if (!groups.has(title)) {
      fail(`docsHubCatalog missing group ${title}`);
    }
  }

  const startPaths = new Set(groups.get("Start")?.entries.map((entry) => entry.to) ?? []);
  if (!startPaths.has("/install") || !startPaths.has("/docs/first-program")) {
    fail("Start catalog must link to /install and /docs/first-program");
  }

  const languageGroup = groups.get("Language");
  if (languageGroup && languageGroup.entries !== languageFeatureEntries) {
    fail("Language catalog entries must be the languageFeatureEntries array");
  }

  const packagePaths = new Set(groups.get("Packages")?.entries.map((entry) => entry.to) ?? []);
  if (!packagePaths.has("/docs/std")) {
    fail("Packages catalog must link to /docs/std");
  }

  const specPaths = new Set(groups.get("Spec")?.entries.map((entry) => entry.to) ?? []);
  if (!specPaths.has("/e26") && !specPaths.has("/e26/spec")) {
    fail("Spec catalog must link to /e26 or /e26/spec");
  }

  for (const entry of languageFeatureEntries) {
    if (!entry.title.trim() || !entry.description.trim()) {
      fail(`${entry.to}: catalog entry needs a title and description`);
      continue;
    }
    const page = docsPageByPath.get(entry.to);
    if (!page) {
      fail(`${entry.to}: missing from docsPageByPath`);
      continue;
    }
    if (!page.summary.trim()) {
      fail(`${entry.to}: empty summary`);
    }
    const hasEchoCode = page.sections.some((section) =>
      section.blocks.some(
        (block) => block.kind === "code" && (block.language ?? "echo") === "echo",
      ),
    );
    if (!hasEchoCode) {
      fail(`${entry.to}: needs at least one Echo code block`);
    }
  }

  const { currentPrereleaseTag, currentPrereleaseAssets, currentPrereleaseUrl, releasesIndexUrl } =
    release;
  if (currentPrereleaseTag !== "v0.0.1-alpha.rc.1") {
    fail(`currentPrereleaseTag should be v0.0.1-alpha.rc.1, got ${currentPrereleaseTag}`);
  }
  const artifactIds = currentPrereleaseAssets.map((asset) => asset.artifact);
  if (artifactIds.join(",") !== "linux-x86_64,macos-arm64,windows-x86_64") {
    fail(
      `current prerelease assets should be linux-x86_64, macos-arm64, and windows-x86_64, got ${artifactIds}`,
    );
  }
  if (!currentPrereleaseAssets.some((asset) => asset.artifact === "windows-x86_64")) {
    fail("current prerelease must list the Windows tarball attached to this tag");
  }
  if (!currentPrereleaseUrl.endsWith(`/releases/tag/${currentPrereleaseTag}`)) {
    fail("currentPrereleaseUrl must point at the current tag, not /releases/latest");
  }
  if (releasesIndexUrl.endsWith("/releases/latest")) {
    fail("releasesIndexUrl must not be /releases/latest");
  }

  const repoRoot = path.resolve(root, "..");
  const installPage = readFileSync(path.join(root, "src/install.tsx"), "utf8");
  const installContent = readFileSync(path.join(root, "src/docs/install-content.ts"), "utf8");
  const installSources = `${installPage}\n${installContent}`;
  const readme = readFileSync(path.join(repoRoot, "README.md"), "utf8");
  const installDoc = readFileSync(path.join(repoRoot, "docs/install.md"), "utf8");
  const installSh = readFileSync(path.join(repoRoot, "scripts/install.sh"), "utf8");
  if (
    !installSources.includes("current-release") ||
    !installSources.includes("currentPrereleaseTag")
  ) {
    fail("install page must render the current prerelease from current-release.ts");
  }
  if (
    !installSources.includes("from-release") ||
    !installSources.includes("currentPrereleaseTag")
  ) {
    fail("install page must show from-release and how to pin the current tag");
  }
  if (!/windows-x86_64/.test(installSources)) {
    fail("install page must name the Windows tarball on this tag");
  }
  if (!/prerelease/i.test(installSources) || !/prerelease/i.test(readme)) {
    fail("install page and README must say published builds are prereleases");
  }
  for (const [label, text] of [
    ["README.md", readme],
    ["docs/install.md", installDoc],
    ["scripts/install.sh", installSh],
  ]) {
    if (!text.includes(currentPrereleaseTag)) {
      fail(`${label} must name ${currentPrereleaseTag}`);
    }
    if (
      !text.includes("xo-linux-x86_64") ||
      !text.includes("xo-macos-arm64") ||
      !text.includes("xo-windows-x86_64")
    ) {
      fail(`${label} must list xo-linux-x86_64, xo-macos-arm64, and xo-windows-x86_64`);
    }
    if (/latest GitHub release/i.test(text)) {
      fail(`${label} must not present a GitHub latest release`);
    }
    if (/releases\/latest/.test(text) && !/404/.test(text)) {
      fail(`${label} must not present /releases/latest as working`);
    }
  }
  if (/latest GitHub release/i.test(installSources)) {
    fail("install page must not present a GitHub latest release");
  }
  if (/releases\/latest/.test(installSources) && !/404/.test(installSources)) {
    fail("install page must not present /releases/latest as working");
  }
  if (!installSh.includes("releases?per_page=")) {
    fail("install.sh from-release must list published releases, including prereleases");
  }

  const snapshot = renderStaticHomeAndHub();
  if (/modoterra/i.test(snapshot)) {
    fail("static homepage must not mention Modoterra");
  }
  const siteMd = readFileSync(path.join(root, "SITE.md"), "utf8");
  for (const needle of ["Public facts", "prerelease tags", "MIT license", "v0.0.1-alpha.rc.1"]) {
    if (!siteMd.includes(needle)) {
      fail(`SITE.md missing public-fact marker: ${needle}`);
    }
  }

  for (const needle of [
    homePage.definition,
    homePage.status,
    homePage.sample.trim().split("\n")[0],
    "/docs",
    "/docs/std",
    "/e26",
    "/docs/leaders",
    "/install",
    securityContact.email,
    securityContact.mailto,
    "SECURITY.md",
  ]) {
    if (!snapshot.includes(needle)) {
      fail(`renderStaticHomeAndHub missing ${needle}`);
    }
  }

  const pages = staticPageByPath();
  const requiredChrome = [
    ["/docs", "Documents"],
    ["/docs/std", "Standard library"],
    ["/e26", "Echo 2026"],
    ["/install", "Install Echo"],
    ["/security", "Security"],
    ["/docs/first-program", "First program"],
    ["/book", "Introduction"],
    ["/privacy", "Privacy"],
    ["/terms", "Terms"],
  ];
  for (const [path, heading] of requiredChrome) {
    const page = pages.get(path);
    if (!page) {
      fail(`${path}: missing static page for catalog/footer link`);
      continue;
    }
    if (!page.body.includes(`<h1>${heading}</h1>`)) {
      fail(`${path}: static page must include <h1>${heading}</h1>`);
    }
    if (!page.title.trim() || !page.description.trim()) {
      fail(`${path}: static page needs a title and description`);
    }
    if (distFileForPath(path) !== `${path.slice(1)}/index.html`) {
      fail(`${path}: expected dist file ${path.slice(1)}/index.html`);
    }
  }

  for (const path of publicChromePaths()) {
    const page = pages.get(path);
    if (!page) {
      fail(`${path}: catalog/nav/footer link has no static page`);
      continue;
    }
    if (!page.body.includes("<h1>")) {
      fail(`${path}: static page is missing an h1`);
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
          definition: homePage.definition,
          nav: primaryNav.map((item) => item.label),
          footerAbout: footerLinkGroups
            .find((group) => group.title === "About")
            ?.links.map((link) => link.label),
          languagePages: languageFeatureEntries.map((entry) => entry.to),
          hubGroups: docsHubCatalog.map((group) => group.title),
          chromePaths: publicChromePaths(),
        },
        null,
        2,
      ),
    );
  }
} finally {
  await server.close();
}
