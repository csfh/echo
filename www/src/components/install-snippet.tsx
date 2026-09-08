import { useState } from "react";

type Props = {
  code: string;
  label?: string;
};

/**
 * Copyable shell block for install / first-run commands.
 */
export function InstallSnippet({ code, label = "Shell" }: Props) {
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  }

  return (
    <div className="overflow-hidden rounded-lg border border-slate-200 bg-slate-50">
      <div className="flex items-center justify-between border-b border-slate-200 px-4 py-2">
        <span className="font-mono text-xs font-semibold tracking-wide text-slate-500">
          {label}
        </span>
        <button
          type="button"
          onClick={() => void copy()}
          className="rounded-md border border-slate-200 bg-white px-2.5 py-1 text-xs font-semibold text-slate-600 transition hover:border-slate-300 hover:text-slate-950 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-300"
        >
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      {/*
        Force mono + disable ligatures/calt so sequences like `xo --help` and
        `bash -s --` never collapse to `xo--help` / `bash -s--` in the UI.
      */}
      <pre
        className="install-snippet-pre overflow-x-auto px-5 py-4 font-mono text-sm leading-7 text-slate-800 whitespace-pre"
        tabIndex={0}
      >
        {code}
      </pre>
      <span className="sr-only" aria-live="polite">
        {copied ? "Copied to clipboard" : ""}
      </span>
    </div>
  );
}
