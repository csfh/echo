/**
 * Install page copy shared by the React page and the static HTML snapshot.
 * Prerelease tag and archives come from current-release.ts.
 */

import {
  currentPrereleaseAssets,
  currentPrereleaseTag,
  currentPrereleaseUrl,
  releasesIndexUrl,
} from "../lib/current-release";

export type CodePart = { code: string };
export type LinkPart = { href: string; label: string };
export type InlinePart = string | CodePart | LinkPart;

export type InstallSection = {
  title: string;
  paragraphs: InlinePart[][];
  assets?: boolean;
  code?: string;
};

export type InstallNextStep = {
  title: string;
  text: string;
  to: string;
  label: string;
  variant?: "primary" | "secondary";
};

export const PREBUILT_INSTALL = `# Newest published prerelease for this machine (linux-x86_64 / macos-arm64 / windows-x86_64)
curl -fsSL https://raw.githubusercontent.com/csfh/echo/main/scripts/install.sh \\
  | bash -s -- from-release

# Pin this tag
# … | bash -s -- from-release ${currentPrereleaseTag}

xo --help
xo doctor 2>/dev/null || true`;

export const CLONE_BUILD = `git clone https://github.com/csfh/echo.git
cd echo
cargo build -p xo
./target/debug/xo --help`;

export const USER_INSTALL = `# From the checkout: release build + XDG install + ~/.local/bin/xo
./scripts/install.sh
./scripts/install.sh doctor

# Newest published prerelease, or pin a tag
./scripts/install.sh from-release
./scripts/install.sh from-release ${currentPrereleaseTag}

# Upgrade (keeps prior toolchain dirs)
./scripts/install.sh upgrade

# Uninstall toolchain (add --purge to also clear $XO_HOME packages)
./scripts/uninstall.sh`;

export const FIRST_RUN = `# After from-release / install.sh, xo is on PATH
xo run examples/misc/hello.echo

# Or from a debug build in a checkout
./target/debug/xo run examples/misc/hello.echo
./target/debug/xo run examples/misc/sum_list.echo`;

export function isLinkPart(part: object | string): part is LinkPart {
  return typeof part === "object" && "href" in part;
}

export function isCodePart(part: object | string): part is CodePart {
  return typeof part === "object" && "code" in part && !("href" in part);
}

function archiveParts(): InlinePart[] {
  const parts: InlinePart[] = [];
  for (const [index, asset] of currentPrereleaseAssets.entries()) {
    if (index > 0) {
      parts.push(" and ");
    }
    parts.push({ code: asset.archive });
  }
  return parts;
}

export const installPage = {
  title: "Install Echo",
  lead: [
    "The public CLI is ",
    { code: "xo" },
    ". Published builds are prereleases. The current tag is ",
    { href: currentPrereleaseUrl, label: currentPrereleaseTag },
    ". Take a prebuilt when you only need to run programs. Build from source when you edit the compiler.",
  ] satisfies InlinePart[],
  sections: [
    {
      title: "Prebuilt (recommended)",
      paragraphs: [
        [
          { code: currentPrereleaseTag },
          " ships ",
          ...archiveParts(),
          ". The script downloads ",
          { code: "xo" },
          " (",
          { code: "xo.exe" },
          " on Windows), the runtime staticlib, and ",
          { code: "std/" },
          " for that host, then links ",
          { code: "~/.local/bin/xo" },
          ".",
        ],
        [
          { code: "from-release" },
          " with no tag installs the newest published prerelease. Pass a tag to pin. GitHub ",
          { code: "/releases/latest" },
          " only resolves a non-prerelease and 404s today. See the ",
          { href: releasesIndexUrl, label: "releases list" },
          ".",
        ],
      ],
      assets: true,
      code: PREBUILT_INSTALL,
    },
    {
      title: "Requirements",
      paragraphs: [
        [
          "To run programs you need ",
          { code: "clang" },
          " on ",
          { code: "PATH" },
          " for AOT link. Building this repository also expects Rust (edition 2024), ",
          { code: "mold" },
          ", and ",
          { code: "sccache" },
          ". The workspace Cargo config selects those tools directly. Compiling the toolchain needs LLVM as well.",
        ],
      ],
    },
    {
      title: "Build from source",
      paragraphs: [
        ["Clone the repository and build the CLI package when you develop Echo itself."],
      ],
      code: CLONE_BUILD,
    },
    {
      title: "Install to PATH (XDG)",
      paragraphs: [
        [
          "From a checkout, install co-located ",
          { code: "xo" },
          " and ",
          { code: "std" },
          " under XDG data, link ",
          { code: "~/.local/bin/xo" },
          ", and create package cache and state dirs. Upgrade flips ",
          { code: "current" },
          " without wiping packages. The checkout file ",
          { code: "docs/install.md" },
          " records install layout details.",
        ],
      ],
      code: USER_INSTALL,
    },
    {
      title: "Run an example",
      paragraphs: [
        [
          "Point ",
          { code: "xo" },
          " at a sample program from a checkout, or at any path after install.",
        ],
      ],
      code: FIRST_RUN,
    },
  ] satisfies InstallSection[],
  nextTitle: "From here",
  nextSteps: [
    {
      title: "First program",
      text: "the minimal shape and the same commands you just ran.",
      to: "/docs/first-program",
      label: "First program",
    },
    {
      title: "Reference",
      text: "form sheets for leaders, Result, structs, and the rest of Echo 2026.",
      to: "/docs",
      label: "Reference",
      variant: "secondary",
    },
    {
      title: "Language Spec",
      text: "edition TOC that maps Reference pages to the suite.",
      to: "/e26/spec",
      label: "Language Spec",
      variant: "secondary",
    },
  ] satisfies InstallNextStep[],
  projectLead: "Longer project shape and workflow notes live under ",
  projectLabel: "Project setup",
  projectTo: "/docs/project",
};

export function inlineText(parts: readonly InlinePart[]): string {
  return parts
    .map((part) => {
      if (typeof part === "string") {
        return part;
      }
      if (isLinkPart(part)) {
        return part.label;
      }
      return part.code;
    })
    .join("");
}
