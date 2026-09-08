/**
 * Echo syntax highlighting via web-tree-sitter + generated highlights.scm.
 *
 * Grammar authority: `xo tools grammar tree-sitter` → grammars/tree-sitter-echo.
 * Runtime WASM: bundled from `web-tree-sitter` (correct Vite MIME / URL).
 * Language WASM + queries: `/tree-sitter/tree-sitter-echo.wasm`, `highlights.scm`.
 */

import { Parser, Language, Query, type QueryCapture } from "web-tree-sitter";
// Vite resolves this to a real asset URL with application/wasm (avoids SPA HTML MIME).
import treeSitterWasmUrl from "web-tree-sitter/web-tree-sitter.wasm?url";

export type HighlightSpan = {
  start: number;
  end: number;
  /** Capture name without leading `@` (e.g. `keyword`, `string`). */
  className: string;
};

type Highlighter = {
  parser: Parser;
  query: Query;
};

let initPromise: Promise<Highlighter> | null = null;

function publicAssetUrl(name: string) {
  const base = import.meta.env.BASE_URL ?? "/";
  const root = base.endsWith("/") ? base : `${base}/`;
  return `${root}tree-sitter/${name}`;
}

async function loadHighlighter(): Promise<Highlighter> {
  await Parser.init({
    locateFile(scriptName: string, scriptDirectory?: string) {
      // Always load the runtime wasm from the Vite-bundled asset URL.
      if (
        scriptName.endsWith("web-tree-sitter.wasm") ||
        scriptName.endsWith("tree-sitter.wasm") ||
        (scriptName.endsWith(".wasm") && !scriptName.includes("echo"))
      ) {
        return treeSitterWasmUrl;
      }
      // Absolute / already-resolved
      if (scriptName.startsWith("http") || scriptName.startsWith("/")) {
        return scriptName;
      }
      return `${scriptDirectory ?? ""}${scriptName}`;
    },
  });

  const [langWasm, highlightsSource] = await Promise.all([
    fetch(publicAssetUrl("tree-sitter-echo.wasm")).then(async (r) => {
      if (!r.ok) {
        throw new Error(`tree-sitter-echo.wasm: ${r.status}`);
      }
      const buf = await r.arrayBuffer();
      // Guard against SPA fallback HTML
      const head = new Uint8Array(buf.slice(0, 4));
      const isWasm = head[0] === 0x00 && head[1] === 0x61 && head[2] === 0x73 && head[3] === 0x6d;
      if (!isWasm) {
        throw new Error("tree-sitter-echo.wasm response is not a wasm module");
      }
      return buf;
    }),
    fetch(publicAssetUrl("highlights.scm")).then((r) => {
      if (!r.ok) {
        throw new Error(`highlights.scm: ${r.status}`);
      }
      return r.text();
    }),
  ]);

  const language = await Language.load(new Uint8Array(langWasm));
  const parser = new Parser();
  parser.setLanguage(language);
  const query = new Query(language, highlightsSource);
  return { parser, query };
}

function getHighlighter() {
  initPromise ??= loadHighlighter().catch((err) => {
    initPromise = null;
    throw err;
  });
  return initPromise;
}

/**
 * Build non-overlapping highlight spans. Later query captures win on overlaps
 * (tree-sitter editors often last-match-wins for the same range).
 */
export function capturesToSpans(source: string, captures: QueryCapture[]): HighlightSpan[] {
  if (captures.length === 0) {
    return [];
  }

  const classes: (string | null)[] = Array.from({ length: source.length }, () => null);

  for (const cap of captures) {
    const name = cap.name;
    if (!name) {
      continue;
    }
    const className = name.replace(/\./g, "-");
    const start = Math.max(0, Math.min(source.length, cap.node.startIndex));
    const end = Math.max(start, Math.min(source.length, cap.node.endIndex));
    for (let i = start; i < end; i++) {
      classes[i] = className;
    }
  }

  const spans: HighlightSpan[] = [];
  let i = 0;
  while (i < source.length) {
    const c = classes[i];
    if (c == null) {
      i++;
      continue;
    }
    let j = i + 1;
    while (j < source.length && classes[j] === c) {
      j++;
    }
    spans.push({ start: i, end: j, className: c });
    i = j;
  }
  return spans;
}

/** Parse + query; returns spans for the full source. */
export async function highlightEcho(source: string): Promise<HighlightSpan[]> {
  const { parser, query } = await getHighlighter();
  const tree = parser.parse(source);
  if (!tree) {
    return [];
  }
  try {
    const captures = query.captures(tree.rootNode);
    return capturesToSpans(source, captures);
  } finally {
    tree.delete();
  }
}

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/** Render source to HTML with `<span class="tok-…">` wrappers. */
export function spansToHtml(source: string, spans: HighlightSpan[]): string {
  if (spans.length === 0) {
    return escapeHtml(source);
  }
  let html = "";
  let cursor = 0;
  for (const span of spans) {
    if (span.start > cursor) {
      html += escapeHtml(source.slice(cursor, span.start));
    }
    html += `<span class="tok-${span.className}">${escapeHtml(
      source.slice(span.start, span.end),
    )}</span>`;
    cursor = span.end;
  }
  if (cursor < source.length) {
    html += escapeHtml(source.slice(cursor));
  }
  return html;
}

export async function highlightEchoHtml(source: string): Promise<string> {
  const spans = await highlightEcho(source);
  return spansToHtml(source, spans);
}
