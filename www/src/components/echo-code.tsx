import { useEffect, useState } from "react";
import { highlightEchoHtml } from "../lib/echo-highlight";

type Props = {
  code: string;
  /** When omitted or `echo`, run tree-sitter highlight. Other languages stay plain. */
  language?: "echo" | "shellscript" | string;
  /**
   * `block` — docs-style bordered panel (default).
   * `hero` — homepage example (hover glass, larger mono).
   * `inline-block` — bordered but tighter (get-started shell).
   */
  variant?: "block" | "hero" | "inline-block";
  className?: string;
  "aria-label"?: string;
};

const VARIANT_WRAP: Record<NonNullable<Props["variant"]>, string> = {
  block: "mt-8 overflow-hidden rounded-lg border border-slate-200 bg-slate-50",
  hero: "mt-12 max-w-full overflow-hidden rounded-lg border border-transparent bg-white/0 px-0 py-0 text-left shadow-none backdrop-blur-none transition-all duration-300 ease-out hover:border-slate-200/70 hover:bg-white/60 hover:shadow-sm hover:backdrop-blur-sm focus-within:border-slate-200/70 focus-within:bg-white/60 focus-within:shadow-sm focus-within:backdrop-blur-sm",
  "inline-block": "mt-6 overflow-x-auto rounded-lg border border-slate-200 bg-slate-50",
};

const VARIANT_PRE: Record<NonNullable<Props["variant"]>, string> = {
  block: "echo-code overflow-x-auto px-5 py-5 font-mono text-sm leading-7 text-slate-800",
  hero: "echo-code echo-code-hero overflow-x-auto px-5 py-5 font-mono text-[clamp(0.8rem,1.8vw,1.125rem)] font-semibold leading-relaxed text-slate-950 sm:px-7 sm:py-6",
  "inline-block": "echo-code overflow-x-auto px-5 py-4 font-mono text-sm leading-7 text-slate-800",
};

/**
 * Syntax-highlighted code. Echo uses web-tree-sitter + generated highlights.scm.
 * Falls back to plain text while loading or on error.
 */
export function EchoCode({
  code,
  language = "echo",
  variant = "block",
  className,
  "aria-label": ariaLabel,
}: Props) {
  const isEcho = !language || language === "echo";
  const [html, setHtml] = useState<string | null>(null);

  useEffect(() => {
    if (!isEcho) {
      setHtml(null);
      return;
    }
    let cancelled = false;
    setHtml(null);
    highlightEchoHtml(code)
      .then((next) => {
        if (!cancelled) {
          setHtml(next);
        }
      })
      .catch((err) => {
        console.error("echo highlight failed", err);
        if (!cancelled) {
          setHtml(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [code, isEcho]);

  const body =
    isEcho && html != null ? (
      <code
        className="echo-code-body block whitespace-pre"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    ) : (
      <code className="echo-code-body block whitespace-pre">{code}</code>
    );

  return (
    <div className={className ?? VARIANT_WRAP[variant]} aria-label={ariaLabel}>
      <pre className={`${VARIANT_PRE[variant]} whitespace-pre`}>{body}</pre>
    </div>
  );
}
