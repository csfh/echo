/**
 * Public site chrome: homepage, primary nav, footer, legal pages, Documents
 * hub catalog, and the discovery files (`/sitemap.xml`, `/robots.txt`). Pages
 * and tests load this module; do not duplicate the lists in UI or fixtures.
 */

/** Live public host. Do not emit github.io URLs in discovery files. */
export const publicSiteOrigin = "https://xo.run";

/** Top-level routes that are not docs pages. */
export const publicSurfacePaths = ["/", "/install", "/try"] as const;

/**
 * Legal routes stay out of the sitemap until the pages exist.
 * Do not add these paths here when inventing placeholder URLs.
 */
export const omittedCatalogPaths = ["/privacy", "/terms"] as const;

export type SiteNavItem = {
  label: string;
  to: string;
};

export type SiteLink = {
  title: string;
  to: string;
  description: string;
};

export type DocsCatalogGroup = {
  title: string;
  description: string;
  entries: SiteLink[];
};

export type HomePageContent = {
  definition: string;
  lead: string;
  status: string;
  sampleCaption: string;
  sample: string;
  links: SiteLink[];
};

/** Top-bar links. The logo goes home. Install stays the solid CTA. */
export const primaryNav: SiteNavItem[] = [
  { label: "Documents", to: "/docs" },
  { label: "Packages", to: "/docs/std" },
  { label: "Echo 2026", to: "/e26" },
  { label: "Try", to: "/try" },
];

export const installCta: SiteNavItem = { label: "Install", to: "/install" };

/**
 * Public vulnerability reporting. Mailbox is oss@christofferhallas.com.
 * Discord stays omitted from the footer until there is a public invite.
 */
export const securityContact = {
  email: "oss@christofferhallas.com",
  mailto: "mailto:oss@christofferhallas.com",
  path: "/security",
  policyUrl: "https://github.com/csfh/echo/blob/main/SECURITY.md",
} as const;

export const securityPage = {
  title: "Security",
  lead: "Report vulnerabilities by email. Do not open a public GitHub issue for a security report.",
};

export type FooterLink = {
  label: string;
  href: string;
  disabled?: boolean;
};

export type FooterLinkGroup = {
  title: string;
  links: FooterLink[];
};

/**
 * Footer chrome. Discord stays omitted until there is a public invite URL.
 * Public project mail on linked pages is oss@christofferhallas.com.
 */
export const footerLinkGroups: FooterLinkGroup[] = [
  {
    title: "Learn",
    links: [
      { label: "Install", href: "/install" },
      { label: "Try Echo", href: "/try" },
      { label: "First program", href: "/docs/first-program" },
      { label: "Documents", href: "/docs" },
      { label: "Book", href: "/book" },
      { label: "Echo 2026", href: "/e26" },
    ],
  },
  {
    title: "Community",
    links: [
      {
        label: "GitHub",
        href: "https://github.com/csfh/echo",
      },
    ],
  },
  {
    title: "About",
    links: [
      {
        label: "Christoffer Hallas",
        href: "https://christofferhallas.com",
      },
      { label: "Privacy", href: "/privacy" },
      { label: "Terms", href: "/terms" },
      { label: "Security", href: securityContact.path },
      {
        label: "MIT License",
        href: "https://github.com/csfh/echo/blob/main/LICENSE",
      },
    ],
  },
];

export const footerBlurb =
  "Echo is a compiled language. xo checks a program and emits a native binary from the same LLVM pipeline. Echo 2026 is the current edition, published as prerelease tags and licensed under MIT.";

export const tryPage = {
  title: "Try Echo",
  lead: "This page checks with the shared compiler frontend. A playground run then executes the checked program and captures io.print. Filesystem, net, process, and tasks stay unavailable here. Install xo to compile through LLVM.",
};

/**
 * Internal homepage, nav, Documents hub, and footer destinations.
 * Each path must ship as a real HTML file, or the link must come down.
 */
export function publicChromePaths(): string[] {
  const paths = new Set<string>();

  for (const item of primaryNav) {
    paths.add(item.to);
  }
  paths.add(installCta.to);
  for (const link of homePage.links) {
    paths.add(link.to);
  }
  for (const group of docsHubCatalog) {
    for (const entry of group.entries) {
      paths.add(entry.to);
    }
  }
  for (const group of footerLinkGroups) {
    for (const link of group.links) {
      if (!link.disabled && link.href.startsWith("/")) {
        paths.add(link.href);
      }
    }
  }

  return [...paths].sort();
}

/** Public addresses already used in-repo. Keep the site on this domain. */
export const publicMailAddresses = ["oss@christofferhallas.com"] as const;

export type LegalSection = {
  title: string;
  paragraphs: string[];
};

export type LegalPageContent = {
  path: string;
  title: string;
  summary: string;
  sections: LegalSection[];
};

export const privacyPage: LegalPageContent = {
  path: "/privacy",
  title: "Privacy",
  summary:
    "This page describes how the Echo project site and related project mail handle information. Echo is an open-source compiled language. xo.run is its public documentation site.",
  sections: [
    {
      title: "The site",
      paragraphs: [
        "xo.run publishes language documentation, the Echo 2026 spec, install instructions, and an in-browser checker. Christoffer Hallas maintains the site. These pages have no sign-in.",
      ],
    },
    {
      title: "Hosting logs",
      paragraphs: [
        "The site is a static site served from a CDN. The host may record ordinary request data such as IP address, user agent, requested URL, and time. This site does not run a first-party analytics product.",
      ],
    },
    {
      title: "Mail",
      paragraphs: [
        "If you write to oss@christofferhallas.com, we receive the address and message you send so we can reply.",
      ],
    },
    {
      title: "GitHub",
      paragraphs: [
        "Issues, pull requests, and other activity on the public Echo repository are public on GitHub and follow GitHub's terms.",
      ],
    },
    {
      title: "Try Echo",
      paragraphs: [
        "The Try page runs the compiler frontend in your browser. Source you type there stays in that browser session. Playground source is not uploaded to a project server.",
      ],
    },
    {
      title: "The compiler",
      paragraphs: [
        "xo is a local toolchain. It reads source on the machine where you run it. This site is not a hosted compile service.",
      ],
    },
    {
      title: "Contact",
      paragraphs: [
        "Questions about this page go to oss@christofferhallas.com. Report security issues to the same address.",
      ],
    },
  ],
};

export const termsPage: LegalPageContent = {
  path: "/terms",
  title: "Terms",
  summary:
    "These terms cover the xo.run website and the Echo software published by Christoffer Hallas.",
  sections: [
    {
      title: "Software",
      paragraphs: [
        "Echo is released under the MIT License. The license text lives in the project LICENSE file. You may use, copy, modify, and distribute the software under that license.",
      ],
    },
    {
      title: "Website",
      paragraphs: [
        "The pages on xo.run describe Echo and how to use xo. We may change or remove pages as the language changes. The site is provided as is.",
      ],
    },
    {
      title: "Warranty",
      paragraphs: [
        "The software and this site come with no warranty. The MIT License states the full disclaimer.",
      ],
    },
    {
      title: "Contributions",
      paragraphs: [
        "Contributions to the public repository are governed by the Contributor License Agreement in CLA.md. Submitting a contribution accepts that agreement.",
      ],
    },
    {
      title: "Conduct",
      paragraphs: [
        "Project participation follows the community note in the repository README. Report conduct issues to oss@christofferhallas.com.",
      ],
    },
    {
      title: "Security",
      paragraphs: [
        "Report vulnerabilities to oss@christofferhallas.com. Use a private report for security issues.",
      ],
    },
    {
      title: "Contact",
      paragraphs: ["Other questions go to oss@christofferhallas.com."],
    },
  ],
};

export const legalPages: readonly LegalPageContent[] = [privacyPage, termsPage];

/**
 * Documents stays active on language and guide pages. Packages owns /docs/std.
 */
export function primaryNavItemIsActive(to: string, pathname: string): boolean {
  if (to === "/docs") {
    return (
      pathname === "/docs" || (pathname.startsWith("/docs/") && !pathname.startsWith("/docs/std"))
    );
  }
  if (pathname === to) {
    return true;
  }
  return to !== "/" && pathname.startsWith(`${to}/`);
}

export const homePage: HomePageContent = {
  definition: "Echo is a compiled language.",
  lead: "Statement leaders mark control and binding. The rest of each line is an ordinary expression. xo checks a program and emits a native binary from the same LLVM pipeline.",
  status:
    "Echo 2026 is the public edition. A Rust toolchain ships as prerelease tags on GitHub. The repository is MIT licensed.",
  sampleCaption: "sum.echo",
  sample: `/ std/io

$ xs = [1, 2, 3]
~ sum = 0
* x : xs {
    ~ sum = sum + x
}
io.print("sum={sum}")`,
  links: [
    {
      title: "Documents",
      to: "/docs",
      description: "Language forms, the first program, and the toolchain.",
    },
    {
      title: "Packages",
      to: "/docs/std",
      description: "Shipped std modules and the API index.",
    },
    {
      title: "Spec",
      to: "/e26",
      description: "Echo 2026 language law and the conformance suite.",
    },
  ],
};

/** Criterion-4 language pages. The Documents hub Language group uses this list. */
export const languageFeatureEntries: SiteLink[] = [
  {
    title: "Leaders",
    to: "/docs/leaders",
    description: "Statement leader forms. A glyph marks the role of each statement.",
  },
  {
    title: "Binds and functions",
    to: "/docs/binds",
    description: "Immutable, mutable, and const binds. Functions are values.",
  },
  {
    title: "Values and operators",
    to: "/docs/values",
    description: "Literal forms, operator precedence, equality, and copy.",
  },
  {
    title: "Collections and ranges",
    to: "/docs/collections",
    description: "Lists, products, indexing, and inclusive ranges.",
  },
  {
    title: "Control",
    to: "/docs/control",
    description: "If, loops, match, break, and continue.",
  },
  {
    title: "Result and option",
    to: "/docs/result-option",
    description: "Produce and match result and option shapes.",
  },
  {
    title: "Strings",
    to: "/docs/strings",
    description: "Pure and rich string forms.",
  },
  {
    title: "Modules and std",
    to: "/docs/modules",
    description: "Imports, exports, and the std/ import policy.",
  },
  {
    title: "Structs",
    to: "/docs/structs",
    description: "% shape, @ members, and the method receiver.",
  },
  {
    title: "Tasks",
    to: "/docs/tasks",
    description: "Spawn work, capture values, and join task handles.",
  },
];

export const docsHubCatalog: DocsCatalogGroup[] = [
  {
    title: "Start",
    description: "Build xo and write a first program.",
    entries: [
      {
        title: "Install",
        to: "/install",
        description: "Install the current xo prerelease, or build from a checkout.",
      },
      {
        title: "First program",
        to: "/docs/first-program",
        description: "Minimal runnable shape and the xo check, run, and build commands.",
      },
      {
        title: "Project setup",
        to: "/docs/project",
        description: "Create an entry file and a local workflow.",
      },
    ],
  },
  {
    title: "Language",
    description: "Statement forms for the Echo 2026 edition.",
    entries: languageFeatureEntries,
  },
  {
    title: "Packages",
    description: "Userland modules imported as std/ paths.",
    entries: [
      {
        title: "Standard library",
        to: "/docs/std",
        description:
          "Userland modules for I/O, strings, files, process, JSON, encoding, crypto, collections, and networking.",
      },
      {
        title: "API reference",
        to: "/docs/std/reference",
        description: "Index of public exports across shipped std modules.",
      },
    ],
  },
  {
    title: "Spec",
    description: "Public language law and the machine-checked suite.",
    entries: [
      {
        title: "Echo 2026",
        to: "/e26",
        description: "Edition home for the current public language.",
      },
      {
        title: "Language Spec",
        to: "/e26/spec",
        description: "Spec table of contents mapped to the Reference and the suite.",
      },
    ],
  },
];

/**
 * Public catalog paths for `/sitemap.xml`.
 * `existingPagePaths` is the shipped HTML catalog (`staticPages` / `docsPages`).
 * Privacy and Terms are included only when those pages already exist.
 */
export function collectPublicCatalogPaths(existingPagePaths: readonly string[]): string[] {
  const existing = new Set(existingPagePaths);
  const paths = new Set<string>([
    ...publicSurfacePaths,
    ...publicChromePaths(),
    ...existingPagePaths,
  ]);

  for (const omitted of omittedCatalogPaths) {
    if (!existing.has(omitted)) {
      paths.delete(omitted);
    }
  }

  return [...paths].sort((left, right) => left.localeCompare(right));
}

export function publicCatalogUrl(path: string): string {
  if (!path.startsWith("/")) {
    throw new Error(`catalog path must be absolute, got ${path}`);
  }
  if (path === "/") {
    return `${publicSiteOrigin}/`;
  }
  return `${publicSiteOrigin}${path}`;
}

export function renderSitemapXml(paths: readonly string[]): string {
  const urls = paths
    .map((path) => `  <url>\n    <loc>${escapeHtml(publicCatalogUrl(path))}</loc>\n  </url>`)
    .join("\n");

  return [
    `<?xml version="1.0" encoding="UTF-8"?>`,
    `<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">`,
    urls,
    `</urlset>`,
    ``,
  ].join("\n");
}

export function renderRobotsTxt(): string {
  return ["User-agent: *", "Allow: /", "", `Sitemap: ${publicSiteOrigin}/sitemap.xml`, ""].join(
    "\n",
  );
}

function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function renderLinkList(links: readonly SiteLink[]): string {
  return links
    .map(
      (link) =>
        `<li><a href="${escapeHtml(link.to)}">${escapeHtml(link.title)}</a> ${escapeHtml(link.description)}</li>`,
    )
    .join("");
}

/**
 * No-JS snapshot of the homepage and Documents hub, built from the same
 * objects the React pages render. The build injects this into index.html.
 */
export function renderStaticHomeAndHub(): string {
  const surfaceLinks = renderLinkList(homePage.links);
  const catalog = docsHubCatalog
    .map((group) => {
      return [
        `<section>`,
        `<h2>${escapeHtml(group.title)}</h2>`,
        `<p>${escapeHtml(group.description)}</p>`,
        `<ul>${renderLinkList(group.entries)}</ul>`,
        `</section>`,
      ].join("");
    })
    .join("");

  return [
    `<main>`,
    `<h1>${escapeHtml(homePage.definition)}</h1>`,
    `<p>${escapeHtml(homePage.lead)}</p>`,
    `<p>${escapeHtml(homePage.status)}</p>`,
    `<pre>${escapeHtml(homePage.sample)}</pre>`,
    `<nav aria-label="Language surfaces"><ul>${surfaceLinks}</ul></nav>`,
    catalog,
    `<aside>`,
    `<h2>Security</h2>`,
    `<p>Report vulnerabilities to <a href="${escapeHtml(securityContact.mailto)}">${escapeHtml(securityContact.email)}</a>.`,
    ` Policy: <a href="${escapeHtml(securityContact.policyUrl)}">SECURITY.md</a>.</p>`,
    `</aside>`,
    `</main>`,
  ].join("");
}
