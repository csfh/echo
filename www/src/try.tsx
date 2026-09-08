import { useEffect, useMemo, useRef, useState } from "react";
import { CtaLink } from "./components/cta-link";
import { tryPage } from "./docs/site";
import {
  EchoWasmMissingError,
  loadEchoCheck,
  type CheckDiagnostic,
  type CheckResult,
  type EchoCheckApi,
  type RunResult,
} from "./lib/echo-check";
import { playgroundSumSource } from "./lib/playground";

type Sample = {
  id: string;
  label: string;
  source: string;
};

export const PLAYGROUND_SAMPLES: Sample[] = [
  {
    id: "sum",
    label: "Sum",
    source: playgroundSumSource(),
  },
  {
    id: "result",
    label: "Result",
    source: `/ std/io
/ std/str

$ checked = (x) {
    ? x < 0 {
        ! 99
    }
    ^ x
}

| checked(7) {
    $ v {
        io.print(str.from_int(v))
    }
    ! e {
        io.print(str.from_int(e))
    }
}
`,
  },
  {
    id: "struct",
    label: "Struct",
    source: `/ std/io
/ std/str

% point {
    ~ x
    ~ y
}

$ p = point { x: 3, y: 4 }
io.print(str.from_int(p.x))
~ p.x = p.x + 10
io.print(str.from_int(p.x))
`,
  },
  {
    id: "reject",
    label: "Reject",
    source: `; ! is a Result err return from a function.
; At file scope the checker reports sem-error-return.

! 1
`,
  },
];

function selectAt(textarea: HTMLTextAreaElement, line: number, column: number) {
  if (line < 1) {
    return;
  }
  const lines = textarea.value.split("\n");
  let offset = 0;
  for (let i = 0; i < line - 1 && i < lines.length; i += 1) {
    offset += lines[i].length + 1;
  }
  offset += Math.max(0, column - 1);
  const max = textarea.value.length;
  const pos = Math.min(offset, max);
  textarea.focus();
  textarea.setSelectionRange(pos, pos);
}

function safePlaygroundRun(api: EchoCheckApi, source: string): RunResult {
  try {
    return api.run(source);
  } catch (error) {
    console.error("playground run failed", error);
    return {
      ok: false,
      host_error:
        "playground-host: the in-browser runner failed to start. Hard-refresh, or rebuild with `just wasm`.",
      diagnostics: [],
    };
  }
}

function diagnosticLabel(diag: CheckDiagnostic) {
  const where =
    diag.line > 0
      ? `${diag.path || "playground.echo"}:${diag.line}:${diag.column}`
      : diag.path || "playground.echo";
  const code = diag.code ? ` ${diag.code}` : "";
  return `${diag.severity}${code}  ${where}`;
}

/**
 * In-browser check plus a playground run that captures io.print.
 * Native compile stays on an xo install.
 */
export function TryPage() {
  const [source, setSource] = useState(PLAYGROUND_SAMPLES[0].source);
  const [activeSample, setActiveSample] = useState(PLAYGROUND_SAMPLES[0].id);
  const [api, setApi] = useState<EchoCheckApi | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [result, setResult] = useState<CheckResult | null>(null);
  const [runResult, setRunResult] = useState<RunResult | null>(null);
  const [busy, setBusy] = useState(false);
  const editorRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    document.title = "Try Echo";
    return () => {
      document.title = "Echo";
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    loadEchoCheck()
      .then((next) => {
        if (!cancelled) {
          setApi(next);
        }
      })
      .catch((error: unknown) => {
        if (cancelled) {
          return;
        }
        if (error instanceof EchoWasmMissingError) {
          setLoadError(error.message);
        } else {
          setLoadError("The in-browser checker failed to start.");
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!api) {
      return;
    }
    const handle = window.setTimeout(() => {
      const next = api.check(source);
      setResult(next);
      if (next.ok) {
        setRunResult(safePlaygroundRun(api, source));
      } else {
        setRunResult(null);
      }
    }, 280);
    return () => window.clearTimeout(handle);
  }, [api, source]);

  const errorCount = useMemo(
    () => result?.diagnostics.filter((d) => d.severity === "error").length ?? 0,
    [result],
  );

  function applySample(sample: Sample) {
    setActiveSample(sample.id);
    setSource(sample.source);
  }

  function runCheck() {
    if (!api) {
      return;
    }
    const next = api.check(source);
    setResult(next);
    if (next.ok) {
      setRunResult(safePlaygroundRun(api, source));
    } else {
      setRunResult(null);
    }
  }

  function runPlayground() {
    if (!api) {
      return;
    }
    setRunResult(safePlaygroundRun(api, source));
    setResult(api.check(source));
  }

  function runFormat() {
    if (!api) {
      return;
    }
    setBusy(true);
    const formatted = api.format(source);
    if (formatted.ok && formatted.text != null) {
      setSource(formatted.text);
      setResult(api.check(formatted.text));
    } else {
      setResult({
        ok: false,
        diagnostics: formatted.diagnostics,
      });
    }
    setBusy(false);
  }

  return (
    <main className="min-h-screen bg-white px-6 pb-24 pt-32 text-slate-950 sm:pt-36">
      <div className="mx-auto w-full max-w-6xl">
        <p className="text-sm font-semibold uppercase tracking-[0.18em] text-slate-500">
          Playground
        </p>
        <h1 className="mt-4 text-3xl font-semibold tracking-normal text-slate-950 sm:text-4xl">
          {tryPage.title}
        </h1>
        <p className="mt-4 max-w-3xl text-pretty text-lg leading-8 text-slate-600">
          {tryPage.lead}
        </p>
        <div className="mt-6 flex flex-wrap gap-3">
          <CtaLink to="/install">Install xo</CtaLink>
          <CtaLink to="/docs/first-program" variant="secondary">
            First program
          </CtaLink>
        </div>

        {loadError ? (
          <div
            className="mt-10 rounded-lg border border-amber-200 bg-amber-50 px-5 py-4 text-sm leading-6 text-amber-950"
            role="status"
          >
            <p>{loadError}</p>
            <p className="mt-2 font-mono text-amber-900">just wasm</p>
          </div>
        ) : null}

        <div className="mt-10 grid gap-6 lg:grid-cols-[minmax(0,1.4fr)_minmax(280px,0.8fr)]">
          <section className="min-w-0">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex flex-wrap gap-2" role="tablist" aria-label="Sample programs">
                {PLAYGROUND_SAMPLES.map((sample) => {
                  const selected = sample.id === activeSample;
                  return (
                    <button
                      key={sample.id}
                      aria-selected={selected}
                      className={
                        selected
                          ? "rounded-md bg-slate-950 px-3 py-1.5 text-sm font-semibold text-white"
                          : "rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-semibold text-slate-600 transition hover:border-violet-300 hover:text-violet-700"
                      }
                      onClick={() => applySample(sample)}
                      role="tab"
                      type="button"
                    >
                      {sample.label}
                    </button>
                  );
                })}
              </div>
              <div className="flex flex-wrap gap-2">
                <button
                  className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-semibold text-slate-700 transition hover:border-violet-300 hover:text-violet-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-300 disabled:cursor-not-allowed disabled:opacity-50"
                  disabled={!api || busy}
                  onClick={runFormat}
                  type="button"
                >
                  Format
                </button>
                <button
                  className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-semibold text-slate-700 transition hover:border-violet-300 hover:text-violet-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-300 disabled:cursor-not-allowed disabled:opacity-50"
                  disabled={!api}
                  onClick={runCheck}
                  type="button"
                >
                  Check
                </button>
                <button
                  className="rounded-md bg-violet-600 px-3 py-1.5 text-sm font-semibold text-white transition hover:bg-violet-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-not-allowed disabled:opacity-50"
                  disabled={!api}
                  onClick={runPlayground}
                  type="button"
                >
                  Playground run
                </button>
              </div>
            </div>

            <label className="mt-4 block">
              <span className="sr-only">Echo source</span>
              <textarea
                ref={editorRef}
                className="scrollbar-nice mt-0 min-h-[28rem] w-full resize-y rounded-lg border border-slate-200 bg-slate-50 px-4 py-4 font-mono text-sm leading-7 text-slate-900 outline-none focus-visible:border-violet-300 focus-visible:ring-2 focus-visible:ring-violet-200"
                onChange={(event) => {
                  setActiveSample("");
                  setSource(event.target.value);
                }}
                onKeyDown={(event) => {
                  if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
                    event.preventDefault();
                    runPlayground();
                  }
                }}
                spellCheck={false}
                value={source}
              />
            </label>
            <p className="mt-2 text-xs text-slate-400">
              Ctrl+Enter or Cmd+Enter starts a playground run. Format uses the shared{" "}
              <span className="font-mono">xo fmt</span> pretty-printer.
            </p>
          </section>

          <aside className="min-w-0">
            <div className="rounded-lg border border-slate-200 bg-slate-50 px-5 py-4">
              <p className="text-xs font-semibold uppercase tracking-wide text-slate-400">Check</p>
              <p className="mt-2 font-mono text-sm text-slate-800">
                {!api && !loadError
                  ? "loading checker…"
                  : result == null
                    ? "waiting"
                    : result.ok
                      ? "ok"
                      : `${errorCount} error${errorCount === 1 ? "" : "s"}`}
              </p>
              {api ? (
                <p className="mt-1 text-xs text-slate-500">Bundled std: {api.stdFileCount} files</p>
              ) : null}
            </div>

            <ul className="mt-4 space-y-3">
              {(result?.diagnostics ?? []).map((diag, index) => (
                <li key={`${diag.code ?? "diag"}-${diag.line}-${index}`}>
                  <button
                    className="w-full rounded-lg border border-slate-200 bg-white px-4 py-3 text-left transition hover:border-violet-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-300"
                    onClick={() => {
                      if (editorRef.current) {
                        selectAt(editorRef.current, diag.line, diag.column);
                      }
                    }}
                    type="button"
                  >
                    <p className="font-mono text-xs text-slate-500">{diagnosticLabel(diag)}</p>
                    <p className="mt-1 text-sm leading-6 text-slate-800">{diag.message}</p>
                  </button>
                </li>
              ))}
            </ul>

            <div className="mt-4 rounded-lg border border-slate-200 bg-slate-50 px-5 py-4">
              <p className="text-xs font-semibold uppercase tracking-wide text-slate-400">
                Playground run
              </p>
              {runResult?.ok && runResult.printed != null ? (
                <pre className="mt-2 overflow-x-auto font-mono text-sm leading-7 text-slate-900 whitespace-pre">
                  {runResult.printed}
                </pre>
              ) : runResult?.host_error ? (
                <p className="mt-2 text-sm leading-6 text-amber-900">{runResult.host_error}</p>
              ) : result?.ok ? (
                <p className="mt-2 text-sm leading-6 text-slate-500">
                  Check is clean. Press Playground run to capture io.print.
                </p>
              ) : (
                <p className="mt-2 text-sm leading-6 text-slate-500">
                  Playground run waits for a clean check.
                </p>
              )}
            </div>
          </aside>
        </div>
      </div>
    </main>
  );
}
