import {
  RiArrowRightLine,
  RiBookOpenLine,
  RiCloseLine,
  RiCodeBoxLine,
  RiFileSearchLine,
  RiFileTextLine,
  RiRocketLine,
  RiSearchLine,
  RiTerminalBoxLine,
  type RemixiconComponentType,
} from "@remixicon/react";
import { Link, useNavigate } from "@tanstack/react-router";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { docsSearchIndexUrl, docsSemanticIndexUrl } from "virtual:docs-search-indices";
import {
  cosineSimilarity,
  loadDocsMiniSearch,
  type DocsSearchAsset,
  type DocsSearchRecord,
  type DocsSemanticAsset,
} from "../docs/search";

type DocsSearchResult = Pick<
  DocsSearchRecord,
  "id" | "path" | "title" | "category" | "kind" | "excerpt" | "signature"
> & {
  score: number;
  lexicalScore?: number;
  semanticScore?: number;
};

const docsSearchResultLimit = 8;
const docsSearchLexicalCandidateLimit = 24;
const docsSearchSemanticCandidateLimit = 24;
const docsSearchLexicalWeight = 0.6;
const docsSearchSemanticWeight = 0.4;
const instantSearchHashScroll = {
  behavior: "auto",
  block: "start",
} as const satisfies ScrollIntoViewOptions;

let docsSearchAssetPromise: Promise<DocsSearchAsset> | null = null;
let docsSemanticAssetPromise: Promise<DocsSemanticAsset> | null = null;
let queryEmbedderPromise: Promise<{
  (
    query: string,
    options: { pooling: "mean"; normalize: true },
  ): Promise<{
    data: ArrayLike<number>;
  }>;
}> | null = null;

function mergeHybridSearchResults({
  lexicalResults,
  recordById,
  semanticResults,
}: {
  lexicalResults: DocsSearchResult[];
  recordById: Map<string, DocsSearchRecord>;
  semanticResults: { id: string; score: number }[];
}) {
  const maxLexicalScore = Math.max(1, ...lexicalResults.map((result) => result.score));
  const maxSemanticScore = Math.max(0.0001, ...semanticResults.map((result) => result.score));
  const merged = new Map<string, DocsSearchResult>();

  for (const result of lexicalResults) {
    merged.set(result.id, {
      ...result,
      lexicalScore: result.score / maxLexicalScore,
      score: (result.score / maxLexicalScore) * docsSearchLexicalWeight,
    });
  }

  for (const semanticResult of semanticResults) {
    const normalizedSemanticScore = semanticResult.score / maxSemanticScore;
    const existing = merged.get(semanticResult.id);

    if (existing) {
      existing.semanticScore = normalizedSemanticScore;
      existing.score += normalizedSemanticScore * docsSearchSemanticWeight;
      continue;
    }

    const record = recordById.get(semanticResult.id);
    if (!record) {
      continue;
    }

    merged.set(record.id, {
      id: record.id,
      path: record.path,
      title: record.title,
      category: record.category,
      kind: record.kind,
      excerpt: record.excerpt,
      score: normalizedSemanticScore * docsSearchSemanticWeight,
      semanticScore: normalizedSemanticScore,
    });
  }

  return Array.from(merged.values())
    .sort((left, right) => right.score - left.score)
    .slice(0, docsSearchResultLimit);
}

function loadDocsSearchAsset() {
  docsSearchAssetPromise ??= fetchDocsIndex<DocsSearchAsset>(docsSearchIndexUrl);
  return docsSearchAssetPromise;
}

function loadDocsSemanticAsset() {
  docsSemanticAssetPromise ??= fetchDocsIndex<DocsSemanticAsset>(docsSemanticIndexUrl);
  return docsSemanticAssetPromise;
}

async function fetchDocsIndex<T>(path: string) {
  const response = await fetch(path, { cache: "force-cache" });
  const contentType = response.headers.get("Content-Type") ?? "";

  if (!response.ok || !contentType.includes("application/json")) {
    throw new Error(`Docs index is not available: ${path}`);
  }

  return (await response.json()) as T;
}

function preloadSearchEmbedder() {
  queryEmbedderPromise ??= import("@huggingface/transformers").then(async ({ env, pipeline }) => {
    env.localModelPath = "/models/";
    env.allowLocalModels = true;
    env.allowRemoteModels = false;

    return pipeline("feature-extraction", "xmlml6v2", {
      dtype: "q8",
    }) as unknown as {
      (
        query: string,
        options: { pooling: "mean"; normalize: true },
      ): Promise<{
        data: ArrayLike<number>;
      }>;
    };
  });

  return queryEmbedderPromise;
}

async function embedSearchQuery(query: string) {
  const embedder = await preloadSearchEmbedder();
  const output = await embedder(query, { pooling: "mean", normalize: true });
  return Array.from(output.data);
}

function scrollElementIntoContainerView(
  container: HTMLElement,
  element: HTMLElement,
  behavior: ScrollBehavior,
) {
  const containerRect = container.getBoundingClientRect();
  const elementRect = element.getBoundingClientRect();
  const topOverflow = elementRect.top - containerRect.top;
  const bottomOverflow = elementRect.bottom - containerRect.bottom;

  if (topOverflow < 0) {
    container.scrollBy({ behavior, top: topOverflow - 8 });
    return;
  }

  if (bottomOverflow > 0) {
    container.scrollBy({ behavior, top: bottomOverflow + 8 });
  }
}

function SearchResultIcon({ result }: { result: DocsSearchResult }) {
  const Icon = searchResultIcon(result);
  return <Icon size={20} />;
}

function searchResultMeta(result: DocsSearchResult) {
  return `${result.category} · ${result.kind}`;
}

function searchResultIcon(result: DocsSearchResult): RemixiconComponentType {
  if (result.kind === "code") {
    return RiCodeBoxLine;
  }

  switch (result.category) {
    case "Getting Started":
      return RiRocketLine;
    case "Tooling":
      return RiTerminalBoxLine;
    case "Language":
      return RiBookOpenLine;
    default:
      return result.kind === "section" ? RiFileSearchLine : RiFileTextLine;
  }
}

export function DocsSearch({
  fullWidth = false,
  iconOnly = false,
  onNavigate,
}: {
  fullWidth?: boolean;
  iconOnly?: boolean;
  onNavigate?: () => void;
} = {}) {
  const navigate = useNavigate();
  const [isOpen, setIsOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeResultIndex, setActiveResultIndex] = useState(0);
  const [asset, setAsset] = useState<DocsSearchAsset | null>(null);
  const [semanticAsset, setSemanticAsset] = useState<DocsSemanticAsset | null>(null);
  const [queryEmbedding, setQueryEmbedding] = useState<number[] | null>(null);
  const [isLoadingIndex, setIsLoadingIndex] = useState(false);
  const [isSemanticModelReady, setIsSemanticModelReady] = useState(false);
  const [semanticUnavailable, setSemanticUnavailable] = useState(false);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const searchResultsRef = useRef<HTMLDivElement | null>(null);
  const searchResultRefs = useRef<Record<string, HTMLLIElement | null>>({});
  const miniSearch = useMemo(() => (asset ? loadDocsMiniSearch(asset) : null), [asset]);
  const results = useMemo(() => {
    const trimmedQuery = query.trim();

    if (!miniSearch || !trimmedQuery) {
      return [];
    }

    const lexicalResults = miniSearch.search(trimmedQuery) as unknown as DocsSearchResult[];

    if (!asset || !semanticAsset || !queryEmbedding) {
      return lexicalResults.slice(0, docsSearchResultLimit);
    }

    const recordById = new Map(asset.records.map((record) => [record.id, record]));
    const semanticResults = semanticAsset.records
      .map((record) => ({
        id: record.id,
        score: cosineSimilarity(queryEmbedding, record.embedding),
      }))
      .sort((left, right) => right.score - left.score)
      .slice(0, docsSearchSemanticCandidateLimit);

    return mergeHybridSearchResults({
      lexicalResults: lexicalResults.slice(0, docsSearchLexicalCandidateLimit),
      recordById,
      semanticResults,
    });
  }, [asset, miniSearch, query, queryEmbedding, semanticAsset]);
  const activeResult = results[activeResultIndex];

  useEffect(() => {
    if (!isOpen || asset) {
      return;
    }

    let active = true;
    setIsLoadingIndex(true);

    void loadDocsSearchAsset()
      .then((loadedAsset) => {
        if (active) {
          setAsset(loadedAsset);
        }
      })
      .finally(() => {
        if (active) {
          setIsLoadingIndex(false);
        }
      });

    return () => {
      active = false;
    };
  }, [asset, isOpen]);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    window.setTimeout(() => searchInputRef.current?.focus(), 0);
  }, [isOpen]);

  useEffect(() => {
    setActiveResultIndex(0);
  }, [query]);

  useLayoutEffect(() => {
    if (!isOpen || !activeResult) {
      return;
    }

    const container = searchResultsRef.current;
    const item = searchResultRefs.current[activeResult.id];

    if (!container || !item) {
      return;
    }

    scrollElementIntoContainerView(container, item, "smooth");
  }, [activeResult, isOpen]);

  useEffect(() => {
    if (!isOpen || semanticAsset || semanticUnavailable) {
      return;
    }

    let active = true;

    void loadDocsSemanticAsset()
      .then((loadedSemanticAsset) => {
        if (active) {
          setSemanticAsset(loadedSemanticAsset);
        }
      })
      .catch(() => {
        if (active) {
          setSemanticUnavailable(true);
        }
      });

    return () => {
      active = false;
    };
  }, [isOpen, semanticAsset, semanticUnavailable]);

  useEffect(() => {
    if (!isOpen || !semanticAsset || isSemanticModelReady || semanticUnavailable) {
      return;
    }

    let active = true;

    void preloadSearchEmbedder()
      .then(() => {
        if (active) {
          setIsSemanticModelReady(true);
        }
      })
      .catch(() => {
        if (active) {
          setSemanticUnavailable(true);
        }
      });

    return () => {
      active = false;
    };
  }, [isOpen, isSemanticModelReady, semanticAsset, semanticUnavailable]);

  useEffect(() => {
    const trimmedQuery = query.trim();

    if (!semanticAsset || !trimmedQuery || !isSemanticModelReady) {
      setQueryEmbedding(null);
      return;
    }

    let active = true;

    void embedSearchQuery(trimmedQuery).then((embedding) => {
      if (active) {
        setQueryEmbedding(embedding);
      }
    });

    return () => {
      active = false;
    };
  }, [isSemanticModelReady, query, semanticAsset]);

  useEffect(() => {
    function handleGlobalSearchKey(event: KeyboardEvent) {
      const target = event.target;
      const isEditableTarget =
        target instanceof HTMLElement &&
        (target.isContentEditable || target.matches("input, textarea, select"));

      if (isEditableTarget) {
        return;
      }

      if (
        event.key === "/" ||
        ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k")
      ) {
        event.preventDefault();
        setIsOpen(true);
      }
    }

    window.addEventListener("keydown", handleGlobalSearchKey);
    return () => {
      window.removeEventListener("keydown", handleGlobalSearchKey);
    };
  }, []);

  useEffect(() => {
    function handlePaletteKey(event: KeyboardEvent) {
      if (event.key === "Escape") {
        closeSearch();
        return;
      }

      if (event.key === "ArrowDown") {
        event.preventDefault();
        setActiveResultIndex((index) => Math.min(index + 1, Math.max(results.length - 1, 0)));
        return;
      }

      if (event.key === "ArrowUp") {
        event.preventDefault();
        setActiveResultIndex((index) => Math.max(index - 1, 0));
        return;
      }

      if (event.key === "Enter" && activeResult) {
        void navigate({
          hashScrollIntoView: instantSearchHashScroll,
          to: activeResult.path,
          viewTransition: false,
        });
        closeSearch();
        onNavigate?.();
      }
    }

    if (isOpen) {
      window.addEventListener("keydown", handlePaletteKey);
    }

    return () => {
      window.removeEventListener("keydown", handlePaletteKey);
    };
  }, [activeResult, isOpen, navigate, onNavigate, results.length]);

  function closeSearch() {
    setIsOpen(false);
    setQuery("");
    setActiveResultIndex(0);
    setQueryEmbedding(null);
    searchResultsRef.current?.scrollTo({ top: 0 });
  }

  return (
    <>
      <button
        aria-expanded={isOpen}
        aria-label="Search documentation"
        className={
          iconOnly
            ? "inline-flex size-10 items-center justify-center rounded-md border border-slate-200 bg-white text-slate-500 transition hover:border-slate-300 hover:text-slate-950"
            : fullWidth
              ? "inline-flex h-11 w-full items-center gap-3 rounded-md border border-slate-200 bg-white px-4 text-sm font-semibold text-slate-500 transition hover:border-slate-300 hover:text-slate-950"
              : "inline-flex h-9 items-center gap-2 rounded-md border border-slate-200 bg-white px-3 text-sm font-semibold text-slate-500 transition hover:border-slate-300 hover:text-slate-950"
        }
        onClick={() => setIsOpen(true)}
        type="button"
      >
        <RiSearchLine size={16} />
        {iconOnly ? null : (
          <>
            <span className={fullWidth ? "inline" : "hidden sm:inline"}>
              {fullWidth ? "Search docs" : "Search"}
            </span>
            <span className="hidden rounded border border-slate-200 px-1.5 py-0.5 text-xs text-slate-400 lg:inline">
              /
            </span>
          </>
        )}
      </button>
      {createPortal(
        <AnimatePresence>
          {isOpen ? (
            <motion.div
              animate={{ opacity: 1 }}
              className="fixed inset-0 z-50 flex items-start justify-center bg-slate-950/05 px-4 pt-28 backdrop-blur-xs"
              exit={{ opacity: 0 }}
              initial={{ opacity: 0 }}
              onMouseDown={closeSearch}
              transition={{ duration: 0.16, ease: "easeOut" }}
            >
              <motion.div
                animate={{ opacity: 1, scale: 1, y: 0 }}
                className="w-full max-w-2xl overflow-hidden rounded-lg border border-slate-200 bg-white shadow-2xl"
                exit={{ opacity: 0, scale: 0.98, y: -6 }}
                initial={{ opacity: 0, scale: 0.98, y: -6 }}
                onMouseDown={(event) => event.stopPropagation()}
                role="dialog"
                aria-modal="true"
                aria-label="Search documentation"
                transition={{ duration: 0.18, ease: "easeOut" }}
              >
                <div className="flex h-16 items-center gap-3 border-b border-slate-200 px-5">
                  <RiSearchLine className="shrink-0 text-slate-400" size={22} />
                  <input
                    className="h-full min-w-0 flex-1 bg-transparent text-lg text-slate-950 outline-none placeholder:text-slate-400"
                    onChange={(event) => setQuery(event.target.value)}
                    placeholder="Search docs"
                    ref={searchInputRef}
                    value={query}
                  />
                  {query ? (
                    <button
                      aria-label="Clear search"
                      className="inline-flex size-8 items-center justify-center rounded-md text-slate-400 transition hover:bg-slate-100 hover:text-slate-950"
                      onClick={() => setQuery("")}
                      type="button"
                    >
                      <RiCloseLine size={20} />
                    </button>
                  ) : null}
                  <button
                    aria-label="Close search"
                    className="hidden rounded-md border border-slate-200 bg-white px-2 py-1 text-xs font-semibold text-slate-400 transition hover:border-slate-300 hover:bg-slate-50 hover:text-slate-950 sm:inline"
                    onClick={closeSearch}
                    type="button"
                  >
                    Esc
                  </button>
                </div>
                <div className="flex min-h-10 items-center justify-between border-b border-slate-100 px-5 py-2 text-xs font-semibold text-slate-400">
                  <div className="flex items-center gap-2">
                    {isLoadingIndex ? <span>Loading index…</span> : null}
                  </div>
                  <span className="hidden text-slate-300 sm:inline">
                    <span className="font-mono">↑↓</span> Select{" "}
                    <span className="font-mono">Enter</span> Open
                  </span>
                </div>
                <div
                  className="scrollbar-nice max-h-[28rem] overflow-auto p-3"
                  ref={searchResultsRef}
                >
                  {!query.trim() ? (
                    <p className="px-3 py-10 text-center text-sm text-slate-500">
                      Search Echo docs, sections, commands, and examples.
                    </p>
                  ) : null}
                  {query.trim() && !isLoadingIndex && results.length === 0 ? (
                    <p className="px-3 py-10 text-center text-sm text-slate-500">
                      No results found.
                    </p>
                  ) : null}
                  <ul className="flex flex-col gap-1">
                    {results.map((result, index) => {
                      const isActive = index === activeResultIndex;

                      return (
                        <li
                          key={result.id}
                          ref={(element) => {
                            searchResultRefs.current[result.id] = element;
                          }}
                        >
                          <Link
                            className={
                              isActive
                                ? "grid grid-cols-[2rem_minmax(0,1fr)_1.25rem] gap-3 rounded-md bg-slate-100 px-3 py-3 text-slate-950"
                                : "grid grid-cols-[2rem_minmax(0,1fr)_1.25rem] gap-3 rounded-md px-3 py-3 text-slate-600 transition hover:bg-slate-50 hover:text-slate-950"
                            }
                            onClick={() => {
                              closeSearch();
                              onNavigate?.();
                            }}
                            onMouseEnter={() => setActiveResultIndex(index)}
                            hashScrollIntoView={instantSearchHashScroll}
                            to={result.path}
                            viewTransition={false}
                          >
                            <span className="mt-1 text-slate-400">
                              <SearchResultIcon result={result} />
                            </span>
                            <span className="min-w-0">
                              <span className="block truncate text-sm font-semibold">
                                {result.title}
                              </span>
                              <span className="mt-1 block text-xs font-semibold text-slate-400">
                                {searchResultMeta(result)}
                              </span>
                              {result.signature ? (
                                <span className="mt-2 block truncate font-mono text-sm text-slate-600">
                                  {result.signature}
                                </span>
                              ) : null}
                              <span className="mt-1 line-clamp-2 block text-sm leading-6 text-slate-500">
                                {result.excerpt}
                              </span>
                            </span>
                            <span className="flex size-5 items-center justify-center self-center text-slate-400">
                              {isActive ? <RiArrowRightLine size={18} /> : null}
                            </span>
                          </Link>
                        </li>
                      );
                    })}
                  </ul>
                </div>
              </motion.div>
            </motion.div>
          ) : null}
        </AnimatePresence>,
        document.body,
      )}
    </>
  );
}
