/**
 * Static HTML snapshots for public routes. The build writes one index.html
 * per path so catalog and footer links are real documents, not SPA-only hops.
 */
import { docsPages, type DocsBlock, type DocsPage, type DocsTextPart } from "./content";
import { installPage, inlineText, isLinkPart, type InlinePart } from "./install-content";
import { currentPrereleaseAssets } from "../lib/current-release";
import {
  homePage,
  legalPages,
  renderStaticHomeAndHub,
  securityContact,
  securityPage,
  tryPage,
  type LegalPageContent,
} from "./site";

export type StaticPage = {
  path: string;
  title: string;
  description: string;
  body: string;
};

export function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function renderInline(parts: readonly DocsTextPart[] | readonly InlinePart[]): string {
  return parts
    .map((part) => {
      if (typeof part === "string") {
        return escapeHtml(part);
      }
      if (isLinkPart(part)) {
        return `<a href="${escapeHtml(part.href)}">${escapeHtml(part.label)}</a>`;
      }
      return `<code>${escapeHtml(part.code)}</code>`;
    })
    .join("");
}

function renderBlock(block: DocsBlock): string {
  if (block.kind === "paragraph") {
    return `<p>${renderInline(block.text)}</p>`;
  }
  if (block.kind === "catalog") {
    const items = block.entries
      .map(
        (entry) =>
          `<li><a href="${escapeHtml(entry.to)}">${escapeHtml(entry.title)}</a> ${escapeHtml(entry.description)}</li>`,
      )
      .join("");
    return `<ul>${items}</ul>`;
  }
  return `<pre>${escapeHtml(block.code)}</pre>`;
}

export function renderStaticDocsPage(page: DocsPage): string {
  const sections = page.sections
    .map((section) => {
      const blocks = section.blocks.map(renderBlock).join("");
      return `<section><h2>${escapeHtml(section.title)}</h2>${blocks}</section>`;
    })
    .join("");

  return [
    `<main>`,
    `<p>${escapeHtml(page.category)}</p>`,
    `<h1>${escapeHtml(page.title)}</h1>`,
    `<p>${escapeHtml(page.summary)}</p>`,
    sections,
    `</main>`,
  ].join("");
}

export function renderStaticInstall(): string {
  const sections = installPage.sections
    .map((section) => {
      const paragraphs = section.paragraphs.map((text) => `<p>${renderInline(text)}</p>`).join("");
      const assets = section.assets
        ? `<ul>${currentPrereleaseAssets
            .map(
              (asset) =>
                `<li><code>${escapeHtml(asset.archive)}</code> · ${escapeHtml(asset.host)}</li>`,
            )
            .join("")}</ul>`
        : "";
      const code = section.code ? `<pre>${escapeHtml(section.code)}</pre>` : "";
      return `<section><h2>${escapeHtml(section.title)}</h2>${paragraphs}${assets}${code}</section>`;
    })
    .join("");
  const next = installPage.nextSteps
    .map(
      (step, index) =>
        `<li><a href="${escapeHtml(step.to)}">${index + 1}. ${escapeHtml(step.title)}</a>: ${escapeHtml(step.text)}</li>`,
    )
    .join("");

  return [
    `<main>`,
    `<h1>${escapeHtml(installPage.title)}</h1>`,
    `<p>${renderInline(installPage.lead)}</p>`,
    sections,
    `<section>`,
    `<h2>${escapeHtml(installPage.nextTitle)}</h2>`,
    `<ol>${next}</ol>`,
    `<p>${escapeHtml(installPage.projectLead)}<a href="${escapeHtml(installPage.projectTo)}">${escapeHtml(installPage.projectLabel)}</a>.</p>`,
    `</section>`,
    `</main>`,
  ].join("");
}

function linkLegalMail(html: string): string {
  return html.replace(/([a-z.]+@christofferhallas\.com)/g, '<a href="mailto:$1">$1</a>');
}

export function renderStaticLegal(page: LegalPageContent): string {
  const sections = page.sections
    .map((section) => {
      const paragraphs = section.paragraphs
        .map((text) => `<p>${linkLegalMail(escapeHtml(text))}</p>`)
        .join("");
      return `<section><h2>${escapeHtml(section.title)}</h2>${paragraphs}</section>`;
    })
    .join("");

  return [
    `<main>`,
    `<h1>${escapeHtml(page.title)}</h1>`,
    `<p>${escapeHtml(page.summary)}</p>`,
    sections,
    `<p><a href="/privacy">Privacy</a> <a href="/terms">Terms</a></p>`,
    `</main>`,
  ].join("");
}

export function renderStaticTry(): string {
  return [
    `<main>`,
    `<h1>${escapeHtml(tryPage.title)}</h1>`,
    `<p>${escapeHtml(tryPage.lead)}</p>`,
    `<p>The playground needs JavaScript. Install xo to compile through LLVM.</p>`,
    `<p><a href="/install">Install xo</a> <a href="/docs/first-program">First program</a></p>`,
    `</main>`,
  ].join("");
}

export function renderStaticSecurity(): string {
  return [
    `<main>`,
    `<h1>${escapeHtml(securityPage.title)}</h1>`,
    `<p>${escapeHtml(securityPage.lead)}</p>`,
    `<p>Email <a href="${escapeHtml(securityContact.mailto)}">${escapeHtml(securityContact.email)}</a> with a description of the issue and its impact, steps to reproduce or a proof of concept if available, and affected versions, commits, or platforms if known.</p>`,
    `<p>The repository policy is <a href="${escapeHtml(securityContact.policyUrl)}">SECURITY.md</a>.</p>`,
    `</main>`,
  ].join("");
}

function documentTitle(pageTitle: string): string {
  return `${pageTitle} · Echo`;
}

export function staticPages(): StaticPage[] {
  const pages: StaticPage[] = [
    {
      path: "/",
      title: "Echo Programming Language",
      description: homePage.lead,
      body: renderStaticHomeAndHub(),
    },
    {
      path: "/install",
      title: documentTitle(installPage.title),
      description: inlineText(installPage.lead),
      body: renderStaticInstall(),
    },
    {
      path: "/try",
      title: documentTitle(tryPage.title),
      description: tryPage.lead,
      body: renderStaticTry(),
    },
    {
      path: securityContact.path,
      title: documentTitle(securityPage.title),
      description: securityPage.lead,
      body: renderStaticSecurity(),
    },
  ];

  for (const page of legalPages) {
    pages.push({
      path: page.path,
      title: documentTitle(page.title),
      description: page.summary,
      body: renderStaticLegal(page),
    });
  }

  for (const page of docsPages) {
    pages.push({
      path: page.path,
      title: documentTitle(page.title),
      description: page.summary,
      body: renderStaticDocsPage(page),
    });
  }

  return pages;
}

export function staticPageByPath(): Map<string, StaticPage> {
  return new Map(staticPages().map((page) => [page.path, page]));
}

export function distFileForPath(path: string): string {
  if (path === "/") {
    return "index.html";
  }
  return `${path.replace(/^\//, "")}/index.html`;
}
