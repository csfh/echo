import MiniSearch, { type Options } from "minisearch";
import {
  docsPages,
  headingId,
  textPartText,
  type DocsBlock,
  type DocsPage,
  type DocsSection,
} from "./content";

export type DocsSearchKind = "page" | "section" | "code";

export type DocsSearchRecord = {
  id: string;
  path: string;
  title: string;
  category: string;
  kind: DocsSearchKind;
  summary: string;
  body: string;
  code: string;
  tags: string;
  aliases: string;
  excerpt: string;
  signature?: string;
};

export type DocsSearchAsset = {
  checksum: string;
  records: DocsSearchRecord[];
  miniSearchIndex: ReturnType<MiniSearch<DocsSearchRecord>["toJSON"]>;
};

export type DocsSemanticRecord = {
  id: string;
  embedding: number[];
};

export type DocsSemanticAsset = {
  checksum: string;
  model: "xmlml6v2";
  dimensions: 384;
  records: DocsSemanticRecord[];
};

export const docsSearchOptions: Options<DocsSearchRecord> = {
  fields: ["title", "summary", "body", "code", "tags", "aliases"],
  idField: "id",
  storeFields: ["id", "path", "title", "category", "kind", "excerpt", "signature"],
  searchOptions: {
    boost: {
      title: 8,
      aliases: 6,
      tags: 5,
      summary: 3,
      body: 1,
      code: 1,
    },
    fuzzy: 0.2,
    prefix: true,
  },
};

export function createDocsMiniSearch() {
  return new MiniSearch<DocsSearchRecord>(docsSearchOptions);
}

export function buildDocsSearchAsset(): DocsSearchAsset {
  const records = buildDocsSearchRecords();
  const miniSearch = createDocsMiniSearch();
  miniSearch.addAll(records);

  return {
    checksum: "",
    records,
    miniSearchIndex: miniSearch.toJSON(),
  };
}

export function buildDocsSearchRecords(): DocsSearchRecord[] {
  return docsPages.flatMap(pageRecords);
}

export function loadDocsMiniSearch(asset: DocsSearchAsset) {
  return MiniSearch.loadJSON<DocsSearchRecord>(
    JSON.stringify(asset.miniSearchIndex),
    docsSearchOptions,
  );
}

export function cosineSimilarity(a: number[], b: number[]) {
  let dot = 0;
  let aMagnitude = 0;
  let bMagnitude = 0;

  for (let index = 0; index < a.length; index += 1) {
    dot += a[index]! * b[index]!;
    aMagnitude += a[index]! * a[index]!;
    bMagnitude += b[index]! * b[index]!;
  }

  if (aMagnitude === 0 || bMagnitude === 0) {
    return 0;
  }

  return dot / (Math.sqrt(aMagnitude) * Math.sqrt(bMagnitude));
}

function pageRecords(page: DocsPage): DocsSearchRecord[] {
  return [
    {
      id: `page:${page.id}`,
      path: page.path,
      title: page.title,
      category: page.category,
      kind: "page",
      summary: page.summary,
      body: page.sections.map(sectionText).join(" "),
      code: page.sections.map(sectionCode).join("\n\n"),
      tags: joinTerms(page.tags),
      aliases: joinTerms(page.aliases),
      excerpt: page.summary,
    },
    ...page.sections.flatMap((section) => sectionRecords(page, section)),
  ];
}

function sectionRecords(page: DocsPage, section: DocsSection): DocsSearchRecord[] {
  const path = `${page.path}#${headingId(section.title)}`;
  const text = sectionText(section);
  const code = sectionCode(section);

  return [
    {
      id: `section:${page.id}:${headingId(section.title)}`,
      path,
      title: section.title,
      category: page.category,
      kind: "section",
      summary: page.summary,
      body: text,
      code,
      tags: joinTerms([...(page.tags ?? []), ...(section.tags ?? [])]),
      aliases: joinTerms([...(page.aliases ?? []), ...(section.aliases ?? [])]),
      excerpt: firstSentence(text) || page.summary,
    },
    ...section.blocks
      .filter((block): block is Extract<DocsBlock, { kind: "code" }> => block.kind === "code")
      .map((block, index) => ({
        id: `code:${page.id}:${headingId(section.title)}:${index}`,
        path,
        title: `${section.title} example`,
        category: page.category,
        kind: "code" as const,
        summary: page.summary,
        body: text,
        code: block.code,
        tags: joinTerms([...(page.tags ?? []), ...(section.tags ?? [])]),
        aliases: joinTerms(section.aliases),
        excerpt: block.code.split("\n")[0] ?? block.code,
      })),
  ];
}

function sectionText(section: DocsSection) {
  return section.blocks
    .flatMap((block: DocsBlock) => {
      if (block.kind === "paragraph") {
        return [block.text.map(textPartText).join("")];
      }
      if (block.kind === "catalog") {
        return block.entries.map((entry) => `${entry.title} ${entry.description} ${entry.to}`);
      }
      return [];
    })
    .join(" ");
}

function sectionCode(section: DocsSection) {
  return section.blocks
    .filter((block): block is Extract<DocsBlock, { kind: "code" }> => block.kind === "code")
    .map((block) => block.code)
    .join("\n\n");
}

function joinTerms(terms: readonly string[] | undefined) {
  return terms?.filter(Boolean).join(" ") ?? "";
}

function firstSentence(text: string) {
  return text.match(/[^.?!]+[.?!]/)?.[0] ?? text;
}
