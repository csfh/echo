# Echo public site (www)

Positioning and structure for xo.run. In-repo design notes live under `docs/`.
Edition / Spec: Echo 2026.

## Voice

Write public copy as calm programming-language documentation. Speak the way a
clear engineer would explain a form at a whiteboard: facts, forms, and examples
first. Vary sentence length. Prefer plain verbs. Keep the register spoken, not
performed.

Zig clarity + TypeScript pedagogy + Elixir polish still apply as product taste:

- Short sentences, concrete claims
- Glyphs as first-class characters in copy
- Show code, then result
- Honest early-stage status; no fake logos or unearned social proof
- Calm white space; code is the hero medium

## Editorial craft

Public docs aim for Laravel-class clarity (example-first, skimmable headings,
explicit call forms) without Laravel chapter inventory or framework framing.
Language law stays Echo 2026 Spec + Reference + suite.

### Std package pages (Laravel-style)

Each `/docs/std/…` page is a **package** page. Outline matches product docs such
as Laravel’s rate-limiting chapter: introduce the package, then group the
surface, with an example next to each callable.

```text
Package (Introduction + import)
Constants          ← KIND_* and other constant exports
Struct · name      ← shape export
  name · method    ← receiver methods (when documented)
  name · method
Functions
  free_func
  free_func
```

#### Entry body (each const, method, or function)

Spoken prose first (what it does, inline call form). **Example next.** Then one
short line for parameters and return shape:

1. **Description** + call form in the same paragraph
2. **Example** — Echo snippet with the package import
3. **Parameters / Returns** — structured field notes, not a second essay

Keep description and returns in spoken prose. Keep params structured
(`name: meaning.`). Do not add marketing cadence, antithesis, or em dashes.

#### Data

- Free exports live on `stdModules[].exports` with inferred kind
  (`const` / `struct` / `func`).
- Struct methods live in `stdStructMethods["std/path.export"]` so free exports
  stay the public module surface while methods expand the Struct section.

### Spoken-doc intent

Body copy teaches. Lead with what a form does, how you write it, and what
`xo` or std does next. Let the example carry weight. Avoid marketing cadence.
Avoid throat-clearing openers ("In this section…", "It's important to…"). Avoid
landing sentences that restate the paragraph in a punchline. Avoid setup/payoff
pairs that withhold the form until a final flourish.

### Prose ban list (OBJECTIVE)

Do not use these rhetorical patterns in user-facing narrative under `www/`:

| Pattern                                       | What to avoid                                            |
| --------------------------------------------- | -------------------------------------------------------- |
| Antithesis                                    | X-vs-Y framing as flourish                               |
| Corrective negation                           | "not X, but Y" as a device                               |
| Paragraph pinning                             | Closing every block with a moral                         |
| Parataxis                                     | Stacked fragments that only list tone                    |
| Summary beats                                 | "In short…", "The key takeaway…"                         |
| Rhetorical crutches                           | "Simply put", "Needless to say"                          |
| Negative parallelisms                         | "No A. No B. No C." as rhythm                            |
| Negative anaphoras                            | Repeated "not … / not …" openings                        |
| Contrasting pairs                             | Book/Docs, old/new, cost/gain as twin slogans            |
| Rule of three                                 | Forced triple cadence for style                          |
| Em dashes in prose                            | Use periods, commas, or parentheses                      |
| Throat-clearing openers                       | Warm-up clauses before the fact                          |
| Landing sentences                             | Final "that's why" punchlines                            |
| Setup/payoff constructions                    | Withhold the rule for a reveal                           |
| Parallel sentence structures in one paragraph | Same skeleton repeated for effect                        |
| Stacked noun phrases                          | Dense modifier piles                                     |
| Filler intensifiers                           | genuinely, really, truly, actually                       |
| Corporate-register verbs                      | leverage, underscore, reflect (as prose verbs)           |
| Nominalization                                | Turning clear verbs into heavy abstract nouns            |
| Hedging qualifiers                            | somewhat, relatively, in many ways (unless a real limit) |

Language-law statements may still forbid a form ("trailing commas are rejected",
"this shape is invalid"). State the rule plainly. Do not dress the forbid as
antithesis or corrective flourish.

Code samples, shell snippets, call forms, and structured param fields are
non-prose. Param fields may use a fixed shape such as `name: meaning.` Keep
them free of banned flourish.

### Good shape

```text
Leaders start statements. A space follows the glyph (except bare < and >).
The body opens with { on the same line as the introducer.
```

```text
xo check resolves and type-checks the module graph.
xo run compiles and executes through the shared LLVM path.
xo build emits a native executable from that same pipeline.
```

## Positioning

**H1 (definition):** Echo is a compiled language.

**Lead:** Statement leaders mark control and binding. The rest of each line is
an ordinary expression. `xo` checks a program and emits a native binary from
the same LLVM pipeline.

**Status:** Echo 2026 is the public edition. A Rust toolchain ships as
prerelease tags on GitHub. The repository is MIT licensed.

Keep product claims factual. Do not invent APIs, partner logos, or unearned
maturity.

## Public facts

Public chrome (homepage, footer, install) states what this repository is:

- Compiled language
- MIT license
- Implemented in Rust
- LLVM pipeline
- CLI is `xo`
- Current edition is Echo 2026
- GitHub releases are prerelease tags

Do not call the language production-ready. Do not imply a crates.io package.
Name a host OS only when that platform has an asset on the current release
(v0.0.1-alpha.rc.1 ships `linux-x86_64`, `macos-arm64`, and `windows-x86_64`). Do not list Discord
while there is no live invite. Public project mail is `oss@christofferhallas.com`.

## Pillars

1. **Leaders** — `$` `~` `?` `*` `^` (and the rest of the glyph set) mark statement roles
2. **Errors as values** — `!` / match; optionals follow the same idea
3. **Small loop, native output** — `xo check` / `run` / `build`; I/O from `std`

## Primary nav

The logo is the only Home control. Book stays at `/book` and in the footer.
Security lives at `/security` and in the footer About group.
Catalog, footer, and nav destinations are real HTML files at those paths.

| Item              | Path        | Notes                                         |
| ----------------- | ----------- | --------------------------------------------- |
| Documents         | `/docs`     | Language reference hub                        |
| Packages          | `/docs/std` | Standard library                              |
| Echo 2026         | `/e26`      | Edition + Spec TOC + suite                    |
| Try               | `/try`      | In-browser check + playground run (wasm host) |
| **Install** (CTA) | `/install`  | Solid button; get `xo`                        |
| Security          | `/security` | Vulnerability mailbox + `SECURITY.md`         |

## Docs left rail

The left docs TOC uses **one** slate track + gradient train (same as **On this
page**). Nested Standard library group children indent under that rail. Do not
add a second rail or nested train.

## Homepage sections

The home page is a language-docs front door. Copy and links live in
`src/docs/site.ts` (`homePage`, `primaryNav`, `docsHubCatalog`, `footerLinkGroups`).

1. Language definition (`Echo is a compiled language`) plus the lead
2. Status line: Echo 2026, Rust, prerelease tags, MIT
3. Representative sample that shows statement leaders and binds
4. First-class links: Documents (`/docs`), Packages (`/docs/std`), Spec (`/e26`)
5. Footer (compiled language, `xo`, LLVM, Echo 2026, prerelease, MIT)

Each of those links, plus Install, First program, Book, Try, Privacy, Terms,
and Security, is a real page (`path/index.html`) with that page title and
body. A destination without a document is removed from the catalog or footer.

Homepage trust stays factual. Rust, LLVM, the public edition, MIT, prerelease
tags, and the machine-checked suite are implementation facts. The footer lists
GitHub. It does not list Discord.

## Security

`/security` is the public reporting page. It points to
`oss@christofferhallas.com` and the repository `SECURITY.md`. The footer About
group links there. Public mail is `oss@christofferhallas.com`. Discord stays
omitted from the footer until there is a real invite.

Copy and URLs live in `src/docs/site.ts` (`securityContact`, `footerLinkGroups`).

## Documents hub

`/docs` is a short catalog. Groups: Start (install, first program, project),
Language (the Echo 2026 form pages), Packages (std + API index), Spec
(Echo 2026 + Language Spec). Each entry has a title, one-line description,
and a working path that the build writes as HTML.

## Echo 2026 section

| Path            | Role                                      |
| --------------- | ----------------------------------------- |
| `/e26`          | Edition overview                          |
| `/e26/spec`     | Language Spec TOC (Reference + suite map) |
| `/e26/run`      | Suite runner                              |
| `/e26/layout`   | Fixture layout                            |
| `/e26/protocol` | Candidate binary protocol                 |

Cross-links: Reference ↔ Spec ↔ suite pages keep the triangle explicit.

## Static pages

`npm run build` writes `index.html` for every content route: the homepage,
`/install`, `/try`, `/privacy`, `/terms`, `/security`, and each page in
`docsPages` (Documents, Packages, Spec, Book, First program, and the rest of
the Reference / std / suite pages). Each file keeps the SPA shell and a
noscript body from the same modules the React app renders. Unknown paths
still use the `404.html` bounce.

Wasm bindings stay in `www/public/echo-wasm/`.

## Playground

`/try` runs the shared compiler frontend in WebAssembly (`just wasm`). It
checks source the same way `xo check` does, including bundled `std`. A
playground run then executes the checked MIR and captures `io.print`.
Filesystem, net, process, and tasks fail with a playground-host error.
Compile and native run stay on `xo` (LLVM).

## Footer

Learn, Community, and About. Copy lives in `src/docs/site.ts` (`footerLinkGroups`).

| Group     | Links                                                                                                            |
| --------- | ---------------------------------------------------------------------------------------------------------------- |
| Learn     | Install, Try Echo, First program, Documents, Book, Echo 2026                                                     |
| Community | GitHub. Omit Discord until a public invite URL exists.                                                           |
| About     | Christoffer Hallas (`https://christofferhallas.com`), Privacy (`/privacy`), Terms (`/terms`), Security (`/security`), MIT License |

Public project mail on the site is `oss@christofferhallas.com`.

Privacy and Terms are short compiler/OSS pages for this documentation site.
They are not a consumer-app policy.

## Discovery

`/sitemap.xml` lists the public catalog on `https://xo.run`: home, Install,
Try, Security, catalog and footer routes, and every shipped docs, Book,
Echo 2026, and std page. `/robots.txt` allows crawlers and points at that
sitemap. Privacy and Terms are listed only when those pages exist. Do not
list the GitHub Pages host.

## Out of scope (later)

Richer download tabs, `/e26` URL rename to `/echo-2026`. Discord footer link
when a real invite exists.
