# xo.run

Static React site for Echo / `xo.run`, built with Vite, TypeScript, React, and
Tailwind CSS.

Public positioning, homepage outline, and nav rules: [`SITE.md`](SITE.md).

## Commands

```bash
npm install   # on a host with a system libvips: SHARP_IGNORE_GLOBAL_LIBVIPS=1 npm install
npm run dev
npm run lint
npm run format
npm run build
```

`/try` needs the compiler frontend wasm. From the repo root:

```bash
just wasm    # writes www/public/echo-wasm/
just try     # wasm + npm --prefix www run dev
```

`npm run dev` starts the local site. `npm run lint`, `npm run format`,
`npm run test`, and `npm run build` validate the site before publishing
`www/dist`. The docs-first homepage, primary nav, Documents catalog, and
discovery files live in `src/docs/site.ts`.

## Cloudflare Pages

The live host is [https://xo.run](https://xo.run). GitHub Pages is not the
product site; it only redirects to `xo.run`.

Same layout as the previous `xo.run` site:

| Setting        | Value           |
| -------------- | --------------- |
| Root directory | `www`           |
| Build command  | `npm run build` |
| Build output   | `dist`          |

`/try` loads `public/echo-wasm/` (from `just wasm`). Rebuild and commit those
bindings when the frontend or `std/**/*.echo` changes so Pages can ship the
playground without a Rust toolchain.

Content routes (Documents, Packages, Spec, Install, First program, Book, and
the rest of the docs tree) are written as `dist/<path>/index.html` so those
URLs are real pages. Unknown paths get the SPA shell with HTTP 200 from Pages'
built-in SPA fallback, which is on only while there is **no** top-level
`public/404.html` (a `/* /index.html 200` rule in `_redirects` is rejected by
Pages as an infinite loop, so there is none). Crawlers and `curl` see 200;
`npm run test` fails if either file is added. Existing static files still win.
Custom domain: `public/CNAME` → `xo.run`. Wasm bindings stay in `public/echo-wasm/`.

`/robots.txt` is a static file. `/sitemap.xml` is emitted at build from the
public catalog (`staticPages` and site chrome). Both use `https://xo.run`.
Privacy and Terms are listed only when those pages exist.

## Search

Docs search uses MiniSearch over content in `src/docs/`. Open the palette from
the top bar, or press `/` / `Ctrl+K` / `Cmd+K`.

Lexical search works for every build. Hybrid semantic ranking needs the local
embedding model under `public/models/xmlml6v2` and:

```bash
npm run build:semantic
```

Without the semantic index, the palette still works with lexical ranking and
shows Semantic as inactive.
