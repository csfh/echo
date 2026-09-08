import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";

type Variant = "primary" | "secondary" | "ghost";

type Props = {
  to: string;
  children: ReactNode;
  variant?: Variant;
  className?: string;
};

const VARIANT: Record<Variant, string> = {
  primary:
    "inline-flex items-center justify-center rounded-lg bg-violet-600 px-4 py-2.5 text-sm font-semibold text-white shadow-sm shadow-violet-950/15 transition hover:bg-violet-700 hover:shadow-md hover:shadow-violet-950/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 focus-visible:ring-offset-2",
  secondary:
    "inline-flex items-center justify-center rounded-lg border border-slate-300 bg-white px-4 py-2.5 text-sm font-semibold text-slate-800 transition hover:border-violet-300 hover:text-violet-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-300 focus-visible:ring-offset-2",
  ghost:
    "inline-flex items-center justify-center rounded-lg px-3 py-2 text-sm font-semibold text-slate-600 transition hover:text-violet-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-300",
};

const COMPACT =
  "inline-flex items-center justify-center rounded-lg bg-violet-600 px-3 py-2 text-xs font-semibold text-white shadow-sm shadow-violet-950/15 transition hover:bg-violet-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 focus-visible:ring-offset-2 sm:text-sm";

/**
 * Primary navigation / marketing CTAs. Paths outside the typed route union
 * use the same cast pattern as the rest of the site.
 */
export function CtaLink({
  to,
  children,
  variant = "primary",
  className,
  compact = false,
}: Props & { compact?: boolean }) {
  const base = compact && variant === "primary" ? COMPACT : VARIANT[variant];
  const classes = className ? `${base} ${className}` : base;
  return (
    <Link className={classes} to={to as "/"}>
      {children}
    </Link>
  );
}
