import {
  createRootRoute,
  createRoute,
  createRouter,
  Link,
  Outlet,
  useLocation,
  useNavigate,
} from "@tanstack/react-router";
import { RiCloseLine, RiMenuLine } from "@remixicon/react";
import { AnimatePresence, motion } from "motion/react";
import {
  createContext,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { HomePage } from "./app";
import { CtaLink } from "./components/cta-link";
import { DocsSearch } from "./components/docs-search";
import { EchoCode } from "./components/echo-code";
import { Logo } from "./components/logo";
import { InstallPage } from "./install";
import { LegalPage } from "./legal";
import { SecurityPage } from "./security";
import { TryPage } from "./try";
import {
  docsPageByPath,
  docsPages,
  flattenNavPaths,
  headingId,
  navigationForPath,
  type DocsBlock,
  type DocsNavGroup,
  type DocsNavLink,
  type DocsPage,
  type DocsTextPart,
} from "./docs/content";
import {
  footerBlurb,
  footerLinkGroups,
  installCta,
  primaryNav,
  primaryNavItemIsActive,
  privacyPage,
  termsPage,
  type SiteLink,
} from "./docs/site";

type DocsShellProps = {
  category: string;
  title: string;
  headings: string[];
  children: ReactNode;
};

type DocsPageMeta = Omit<DocsShellProps, "children">;

type DocsLayoutContextValue = {
  setMeta: (meta: DocsPageMeta) => void;
};

const defaultDocsPageMeta: DocsPageMeta = {
  category: docsPages[0].category,
  headings: docsPages[0].sections.map((section) => section.title),
  title: docsPages[0].title,
};

const DocsLayoutContext = createContext<DocsLayoutContextValue | null>(null);

function docsPage(path: string) {
  const page = docsPageByPath.get(path);
  if (!page) {
    throw new Error(`Missing docs page: ${path}`);
  }
  return page;
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

function isDocsShellPath(pathname: string) {
  return (
    pathname === "/docs" ||
    pathname.startsWith("/docs/") ||
    pathname === "/book" ||
    pathname.startsWith("/book/") ||
    pathname === "/e26" ||
    pathname.startsWith("/e26/")
  );
}

function Topbar() {
  const location = useLocation();
  const [isMenuOpen, setIsMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const onDocsShell = isDocsShellPath(location.pathname);
  const docsNavigation = useMemo(
    () => (onDocsShell ? navigationForPath(location.pathname) : null),
    [location.pathname, onDocsShell],
  );

  useEffect(() => {
    setIsMenuOpen(false);
  }, [location.pathname]);

  useEffect(() => {
    if (!isMenuOpen) {
      return;
    }

    const previousOverflow = document.body.style.overflow;
    const previousFocusedElement =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    document.body.style.overflow = "hidden";

    function handleMenuKeyboard(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setIsMenuOpen(false);
        return;
      }

      if (event.key !== "Tab") {
        return;
      }

      const focusableElements = menuRef.current?.querySelectorAll<HTMLElement>(
        'a[href], button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex="-1"])',
      );

      if (!focusableElements || focusableElements.length === 0) {
        return;
      }

      const firstElement = focusableElements[0];
      const lastElement = focusableElements[focusableElements.length - 1];

      if (event.shiftKey && document.activeElement === firstElement) {
        event.preventDefault();
        lastElement.focus();
      } else if (!event.shiftKey && document.activeElement === lastElement) {
        event.preventDefault();
        firstElement.focus();
      }
    }

    window.addEventListener("keydown", handleMenuKeyboard);

    return () => {
      document.body.style.overflow = previousOverflow;
      window.removeEventListener("keydown", handleMenuKeyboard);
      previousFocusedElement?.focus();
    };
  }, [isMenuOpen]);

  return (
    <>
      <header className="fixed inset-x-0 top-0 z-50 border-b border-slate-200/70 bg-white/88 px-5 shadow-2xs backdrop-blur-xl sm:px-6">
        <div className="mx-auto flex h-20 w-full max-w-7xl items-center justify-between gap-4 md:grid md:grid-cols-[auto_1fr_auto] md:gap-6 lg:grid-cols-[220px_minmax(0,720px)_minmax(220px,auto)] lg:gap-12">
          <Link
            aria-label="Echo home"
            className="block w-16 shrink-0 opacity-90 transition hover:opacity-100 lg:w-20"
            to="/"
          >
            <img alt="Echo" className="h-8 w-full" src="/logo.svg" />
          </Link>
          <nav
            aria-label="Primary navigation"
            className="hidden items-center justify-start gap-7 text-sm font-semibold text-slate-500 md:flex lg:gap-8"
          >
            {primaryNav.map((item) => (
              <Link
                key={item.to}
                className={
                  primaryNavItemIsActive(item.to, location.pathname)
                    ? "text-slate-950"
                    : "transition hover:text-violet-700"
                }
                to={item.to as "/"}
              >
                {item.label}
              </Link>
            ))}
          </nav>
          <div className="flex items-center justify-end gap-2 sm:gap-3">
            <span className="hidden xl:inline-flex">
              <DocsSearch />
            </span>
            <span className="hidden md:inline-flex xl:hidden">
              <DocsSearch iconOnly />
            </span>
            {/* Mobile: search in the bar so the single menu is only for nav + docs TOC. */}
            <span className="inline-flex md:hidden">
              <DocsSearch iconOnly />
            </span>
            <CtaLink compact to="/install">
              Install
            </CtaLink>
            <button
              aria-controls="primary-mobile-menu"
              aria-expanded={isMenuOpen}
              aria-label={isMenuOpen ? "Close menu" : "Open menu"}
              className={
                onDocsShell
                  ? "inline-flex size-10 items-center justify-center rounded-lg border border-slate-200 bg-white text-slate-600 transition hover:border-violet-300 hover:text-violet-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-300 lg:hidden"
                  : "inline-flex size-10 items-center justify-center rounded-lg border border-slate-200 bg-white text-slate-600 transition hover:border-violet-300 hover:text-violet-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-300 md:hidden"
              }
              onClick={() => setIsMenuOpen((open) => !open)}
              type="button"
            >
              {isMenuOpen ? <RiCloseLine size={20} /> : <RiMenuLine size={20} />}
            </button>
          </div>
        </div>
      </header>

      <AnimatePresence>
        {isMenuOpen ? (
          <motion.div
            animate={{ opacity: 1, y: 0 }}
            aria-label="Site menu"
            aria-modal="true"
            className={
              onDocsShell
                ? "fixed inset-0 z-[60] overflow-y-auto bg-white lg:hidden"
                : "fixed inset-0 z-[60] overflow-y-auto bg-white md:hidden"
            }
            exit={{ opacity: 0, y: -8 }}
            id="primary-mobile-menu"
            initial={{ opacity: 0, y: -8 }}
            ref={menuRef}
            role="dialog"
            transition={{ duration: 0.16, ease: "easeOut" }}
          >
            <div className="flex h-20 items-center justify-between border-b border-slate-200 px-5">
              <Link
                aria-label="Echo home"
                className="block w-16 opacity-90 transition hover:opacity-100"
                onClick={() => setIsMenuOpen(false)}
                to="/"
              >
                <img alt="Echo" className="h-8 w-full" src="/logo.svg" />
              </Link>
              <button
                aria-label="Close menu"
                autoFocus
                className="inline-flex size-10 items-center justify-center rounded-lg border border-slate-200 text-slate-600 transition hover:border-violet-300 hover:text-violet-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-violet-300"
                onClick={() => setIsMenuOpen(false)}
                type="button"
              >
                <RiCloseLine size={20} />
              </button>
            </div>
            <div className="mx-auto flex w-full max-w-lg flex-col px-5 pb-10 pt-6">
              <DocsSearch fullWidth onNavigate={() => setIsMenuOpen(false)} />
              <nav
                aria-label="Mobile primary navigation"
                className="mt-6 border-t border-slate-200"
              >
                {[...primaryNav, installCta].map(({ label, to }) => (
                  <Link
                    key={to}
                    className="flex items-center justify-between border-b border-slate-200 py-5 font-display text-2xl font-bold tracking-tight text-slate-950 transition hover:text-violet-700"
                    onClick={() => setIsMenuOpen(false)}
                    to={to as "/"}
                  >
                    {label}
                    <span className="text-lg font-normal text-slate-400" aria-hidden="true">
                      →
                    </span>
                  </Link>
                ))}
              </nav>
              {docsNavigation ? (
                <nav
                  aria-label="Documentation sections"
                  className="mt-10 border-t border-slate-200 pt-8"
                >
                  <p className="text-xs font-semibold uppercase tracking-wide text-slate-400">
                    On this section
                  </p>
                  <div className="mt-5">
                    <DocsNavigationList
                      navigation={docsNavigation}
                      onNavigate={() => setIsMenuOpen(false)}
                      pathname={location.pathname}
                    />
                  </div>
                </nav>
              ) : null}
              <p className="mt-8 text-sm leading-6 text-slate-500">
                Echo is early, open source, and actively implemented in Rust and LLVM.
              </p>
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </>
  );
}

function SiteFooter() {
  return (
    <footer className="overflow-hidden border-t border-slate-200 bg-white px-6 pt-24 text-slate-600">
      <div className="mx-auto grid w-full max-w-7xl gap-14 lg:grid-cols-[minmax(0,360px)_1fr]">
        <section>
          <p className="max-w-sm text-xl font-semibold leading-8 text-slate-950">Echo</p>
          <p className="mt-5 max-w-sm text-sm leading-6 text-slate-500">{footerBlurb}</p>
          <p className="mt-10 text-sm text-slate-400">© 2026 Christoffer Hallas</p>
        </section>

        <nav aria-label="Footer navigation" className="grid gap-10 sm:grid-cols-2 lg:grid-cols-4">
          {footerLinkGroups.map((group) => (
            <section key={group.title}>
              <h2 className="text-sm font-semibold text-slate-950">{group.title}</h2>
              <ul className="mt-6 flex flex-col gap-4">
                {group.links.map((link) => (
                  <li key={link.label}>
                    {link.href.startsWith("http") ? (
                      <a
                        className="text-sm text-slate-500 transition hover:text-slate-950"
                        href={link.href}
                        rel={link.href.startsWith("mailto:") ? undefined : "noreferrer"}
                        target={link.href.startsWith("mailto:") ? undefined : "_blank"}
                      >
                        {link.label}
                      </a>
                    ) : (
                      <Link
                        className="text-sm text-slate-500 transition hover:text-slate-950"
                        to={link.href}
                      >
                        {link.label}
                      </Link>
                    )}
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </nav>
      </div>

      <div className="mx-auto mt-40 flex w-full max-w-7xl justify-end pb-10">
        <div className="w-full max-w-md opacity-90">
          <Logo />
        </div>
      </div>
    </footer>
  );
}

function RootLayout() {
  return (
    <>
      <Topbar />
      <Outlet />
      <SiteFooter />
    </>
  );
}

function NotFoundPage() {
  return (
    <main className="bg-white px-6 py-28 text-slate-950">
      <section className="mx-auto max-w-3xl">
        <p className="text-sm font-semibold uppercase tracking-[0.18em] text-slate-500">404</p>
        <h1 className="mt-5 text-4xl font-semibold leading-tight sm:text-5xl">Page not found</h1>
        <p className="mt-5 max-w-2xl text-lg leading-8 text-slate-600">
          The requested page does not exist or has moved.
        </p>
        <div className="mt-9 flex flex-wrap gap-3">
          <Link
            className="inline-flex items-center justify-center rounded-md bg-slate-950 px-4 py-2 text-sm font-semibold text-white transition hover:bg-slate-800"
            to={"/book" as "/"}
          >
            Open the Book
          </Link>
          <Link
            className="inline-flex items-center justify-center rounded-md border border-slate-300 px-4 py-2 text-sm font-semibold text-slate-700 transition hover:border-slate-400 hover:text-slate-950"
            to="/"
          >
            Back home
          </Link>
        </div>
      </section>
    </main>
  );
}

function navLinkIsActive(link: DocsNavLink, pathname: string): boolean {
  if (pathname === link.to) {
    return true;
  }
  return Boolean(link.children?.some((child) => navLinkIsActive(child, pathname)));
}

function activeNavPath(link: DocsNavLink, pathname: string): string | null {
  if (pathname === link.to) {
    return link.to;
  }
  for (const child of link.children ?? []) {
    const hit = activeNavPath(child, pathname);
    if (hit) {
      return hit;
    }
  }
  return null;
}

function DocsNavLinkItem({
  link,
  onNavigate,
  pathname,
  registerItem,
}: {
  link: DocsNavLink;
  onNavigate?: () => void;
  pathname: string;
  registerItem?: (path: string, element: HTMLLIElement | null) => void;
}) {
  const isActive = pathname === link.to;
  const hasActiveChild = link.children?.some((child) => navLinkIsActive(child, pathname));
  const shouldShowChildren = Boolean(link.children && (isActive || hasActiveChild));
  const textClass = link.disabled
    ? "text-sm leading-6 text-slate-300"
    : isActive
      ? "text-sm font-semibold leading-6 text-slate-950"
      : "text-sm leading-6 text-slate-500 transition hover:text-slate-950";

  return (
    <li
      ref={(element) => {
        registerItem?.(link.to, element);
      }}
    >
      {link.disabled ? (
        <span className={textClass}>{link.label}</span>
      ) : (
        <Link className={textClass} onClick={onNavigate} to={link.to}>
          {link.label}
        </Link>
      )}
      <AnimatePresence initial={false}>
        {shouldShowChildren ? (
          <motion.div
            animate={{ height: "auto", opacity: 1 }}
            className="overflow-hidden"
            exit={{ height: 0, opacity: 0 }}
            initial={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.2, ease: "easeOut" }}
          >
            {/* Nested links only: single primary TOC rail (no nested rail/train). */}
            <ul className="mt-3 space-y-3 pl-3">
              {link.children?.map((child) => (
                <DocsNavLinkItem
                  key={child.label}
                  link={child}
                  onNavigate={onNavigate}
                  pathname={pathname}
                  registerItem={registerItem}
                />
              ))}
            </ul>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </li>
  );
}

/**
 * Left docs TOC: one continuous rail (same as “On this page”) with section
 * titles and page links on the track. Gradient train marks the active page.
 * Nested std group children indent under that rail (never a second rail).
 */
function DocsNavigationList({
  navigation,
  onNavigate,
  pathname,
}: {
  navigation: DocsNavGroup[];
  onNavigate?: () => void;
  pathname: string;
}) {
  const railRef = useRef<HTMLDivElement | null>(null);
  const itemRefs = useRef<Record<string, HTMLLIElement | null>>({});
  const [trainY, setTrainY] = useState(0);

  const activeKey = useMemo(() => {
    for (const group of navigation) {
      for (const link of group.links) {
        const hit = activeNavPath(link, pathname);
        if (hit) {
          return hit;
        }
      }
    }
    return null;
  }, [navigation, pathname]);

  useLayoutEffect(() => {
    if (!activeKey) {
      setTrainY(0);
      return;
    }

    let animationFrame = 0;

    function updateTrainPosition() {
      const rail = railRef.current;
      const item = activeKey ? itemRefs.current[activeKey] : null;

      if (!rail || !item) {
        setTrainY(0);
        return;
      }

      // Prefer the link row, not the whole expanded <li> when children are open.
      const label = item.querySelector(":scope > a, :scope > span");
      const target = label ?? item;
      const railRect = rail.getBoundingClientRect();
      const itemRect = target.getBoundingClientRect();
      setTrainY(itemRect.top - railRect.top + itemRect.height / 2 - 9);
    }

    function scheduleUpdate() {
      window.cancelAnimationFrame(animationFrame);
      animationFrame = window.requestAnimationFrame(updateTrainPosition);
    }

    scheduleUpdate();
    window.addEventListener("resize", scheduleUpdate);

    return () => {
      window.cancelAnimationFrame(animationFrame);
      window.removeEventListener("resize", scheduleUpdate);
    };
  }, [activeKey, navigation, pathname]);

  return (
    <div className="relative pl-6" ref={railRef}>
      <span aria-hidden="true" className="absolute bottom-0 left-0 top-0 w-px bg-slate-200" />
      {activeKey ? (
        <motion.span
          aria-hidden="true"
          animate={{ y: trainY }}
          className="docs-primary-nav-train docs-logo-gradient-rail absolute left-[-1px] top-0 h-[18px] w-[3px] rounded-full"
          initial={false}
          transition={{ duration: 0.16, ease: "easeOut" }}
        />
      ) : null}

      <div className="space-y-8">
        {navigation.map((group) => (
          <section key={group.title}>
            <h2 className="text-sm font-semibold leading-6 text-slate-950">{group.title}</h2>
            <ul className="mt-3 space-y-3">
              {group.links.map((link) => (
                <DocsNavLinkItem
                  key={link.label}
                  link={link}
                  onNavigate={onNavigate}
                  pathname={pathname}
                  registerItem={(path, element) => {
                    itemRefs.current[path] = element;
                  }}
                />
              ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}

function DocsShell({ category, title, headings, children }: DocsShellProps) {
  const docsLayout = useContext(DocsLayoutContext);

  useLayoutEffect(() => {
    docsLayout?.setMeta({ category, headings, title });
  }, [category, docsLayout, headings, title]);

  return <>{children}</>;
}

function DocsLayout() {
  const location = useLocation();
  const navigation = useMemo(() => navigationForPath(location.pathname), [location.pathname]);
  const [meta, setMeta] = useState<DocsPageMeta>(defaultDocsPageMeta);
  const docsLayoutContext = useMemo(() => ({ setMeta }), []);
  const { category, headings, title } = meta;
  const [activeHeading, setActiveHeading] = useState(headings[0] ?? "");
  const onThisPageViewportRef = useRef<HTMLDivElement | null>(null);
  const onThisPageRailRef = useRef<HTMLDivElement | null>(null);
  const onThisPageItemRefs = useRef<Record<string, HTMLLIElement | null>>({});
  const [onThisPageTrainY, setOnThisPageTrainY] = useState(0);

  useEffect(() => {
    let animationFrame = 0;

    function updateActiveHeading() {
      const nextActiveHeading =
        headings.findLast((heading) => {
          const element = document.getElementById(headingId(heading));
          return element ? element.getBoundingClientRect().top <= 160 : false;
        }) ??
        headings.find((heading) => document.getElementById(headingId(heading))) ??
        headings[0] ??
        "";

      setActiveHeading(nextActiveHeading);
    }

    function scheduleUpdate() {
      window.cancelAnimationFrame(animationFrame);
      animationFrame = window.requestAnimationFrame(updateActiveHeading);
    }

    setActiveHeading(headings[0] ?? "");
    scheduleUpdate();
    window.addEventListener("scroll", scheduleUpdate, { passive: true });
    window.addEventListener("resize", scheduleUpdate);

    return () => {
      window.cancelAnimationFrame(animationFrame);
      window.removeEventListener("scroll", scheduleUpdate);
      window.removeEventListener("resize", scheduleUpdate);
    };
  }, [headings]);

  useLayoutEffect(() => {
    let animationFrame = 0;

    function updateTrainPosition() {
      const rail = onThisPageRailRef.current;
      const item = onThisPageItemRefs.current[activeHeading];

      if (!rail || !item) {
        setOnThisPageTrainY(0);
        return;
      }

      const railRect = rail.getBoundingClientRect();
      const itemRect = item.getBoundingClientRect();
      setOnThisPageTrainY(itemRect.top - railRect.top + itemRect.height / 2 - 9);
    }

    function scheduleUpdate() {
      window.cancelAnimationFrame(animationFrame);
      animationFrame = window.requestAnimationFrame(updateTrainPosition);
    }

    scheduleUpdate();
    window.addEventListener("resize", scheduleUpdate);

    return () => {
      window.cancelAnimationFrame(animationFrame);
      window.removeEventListener("resize", scheduleUpdate);
    };
  }, [activeHeading, headings]);

  useLayoutEffect(() => {
    const container = onThisPageViewportRef.current;
    const item = onThisPageItemRefs.current[activeHeading];

    if (!container || !item) {
      return;
    }

    scrollElementIntoContainerView(container, item, "smooth");
  }, [activeHeading]);

  function scrollToHeading(heading: string) {
    const id = headingId(heading);
    const element = document.getElementById(id);

    if (!element) {
      return;
    }

    window.history.pushState(null, "", `#${id}`);
    window.scrollTo({
      behavior: "smooth",
      top: element.getBoundingClientRect().top + window.scrollY - 112,
    });
  }

  return (
    <main className="min-h-screen bg-white px-6 pb-24 pt-28 text-slate-950 lg:pt-32">
      {/* Mobile: site hamburger (topbar) includes this TOC. Desktop: left rail. */}
      <div className="mx-auto grid w-full max-w-7xl grid-cols-1 gap-12 lg:grid-cols-[220px_minmax(0,720px)] xl:grid-cols-[220px_minmax(0,720px)_220px]">
        <aside className="hidden lg:block">
          <nav
            aria-label="Documentation sections"
            className="scrollbar-nice sticky top-32 max-h-[calc(100vh-12rem)] overflow-y-auto pr-2"
          >
            <DocsNavigationList navigation={navigation} pathname={location.pathname} />
          </nav>
        </aside>

        <DocsLayoutContext.Provider value={docsLayoutContext}>
          <article className="max-w-none">
            <p className="text-sm font-semibold text-slate-500">{category}</p>
            <h1 className="mt-6 text-5xl font-semibold tracking-normal text-slate-950">{title}</h1>
            <Outlet />
          </article>
        </DocsLayoutContext.Provider>

        <aside className="hidden xl:block">
          <nav aria-label="On this page" className="sticky top-32">
            <h2 className="text-xs font-semibold uppercase tracking-wide text-slate-400">
              On this page
            </h2>
            <div
              className="scrollbar-nice mt-5 max-h-[calc(100vh-12rem)] overflow-y-auto pr-2"
              ref={onThisPageViewportRef}
            >
              <div className="relative pl-6" ref={onThisPageRailRef}>
                <span
                  aria-hidden="true"
                  className="absolute bottom-0 left-0 top-0 w-px bg-slate-200"
                />
                <motion.span
                  aria-hidden="true"
                  animate={{ y: onThisPageTrainY }}
                  className="docs-on-this-page-train docs-logo-gradient-rail absolute left-[-1px] top-0 h-[18px] w-[3px] rounded-full"
                  transition={{ duration: 0.16, ease: "easeOut" }}
                />
                <ul className="docs-on-this-page-links space-y-3">
                  {headings.map((heading) => (
                    <li
                      key={heading}
                      ref={(element) => {
                        onThisPageItemRefs.current[heading] = element;
                      }}
                    >
                      <a
                        className={
                          activeHeading === heading
                            ? "text-sm font-semibold leading-6 text-slate-950 transition"
                            : "text-sm leading-6 text-slate-500 transition hover:text-slate-950"
                        }
                        href={`#${headingId(heading)}`}
                        onClick={(event) => {
                          if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
                            return;
                          }

                          event.preventDefault();
                          scrollToHeading(heading);
                        }}
                      >
                        {heading}
                      </a>
                    </li>
                  ))}
                </ul>
              </div>
            </div>
          </nav>
        </aside>
      </div>
    </main>
  );
}

function renderInlineText(parts: DocsTextPart[]) {
  return parts.map((part, index) =>
    typeof part === "string" ? (
      <span key={index}>{part}</span>
    ) : (
      <code
        key={index}
        className="rounded bg-slate-100 px-1.5 py-0.5 font-mono text-[0.9em] text-slate-800"
      >
        {part.code}
      </code>
    ),
  );
}

function renderCatalog(entries: SiteLink[], key: number) {
  return (
    <ul key={key} className="mt-8 divide-y divide-slate-200 border-y border-slate-200">
      {entries.map((entry) => (
        <li key={entry.to}>
          <Link className="group block py-5 transition hover:bg-slate-50" to={entry.to as "/"}>
            <span className="font-display text-xl font-bold tracking-tight text-slate-950 group-hover:text-violet-700">
              {entry.title}
            </span>
            <span className="mt-2 block text-base leading-7 text-slate-600">
              {entry.description}
            </span>
          </Link>
        </li>
      ))}
    </ul>
  );
}

function renderBlock(block: DocsBlock, key: number) {
  if (block.kind === "paragraph") {
    return (
      <p key={key} className="mt-5 text-base leading-7 text-slate-600 sm:text-lg sm:leading-8">
        {renderInlineText(block.text)}
      </p>
    );
  }

  if (block.kind === "catalog") {
    return renderCatalog(block.entries, key);
  }

  return <EchoCode key={key} code={block.code} language={block.language ?? "echo"} />;
}

function DocsContentPage({ page }: { page: DocsPage }) {
  const location = useLocation();
  const navigate = useNavigate();
  const sectionPaths = useMemo(
    () => flattenNavPaths(navigationForPath(location.pathname)),
    [location.pathname],
  );
  const currentIndex = sectionPaths.indexOf(location.pathname);
  const previousPath = currentIndex > 0 ? sectionPaths[currentIndex - 1] : null;
  const nextPath =
    currentIndex >= 0 && currentIndex + 1 < sectionPaths.length
      ? sectionPaths[currentIndex + 1]
      : null;

  useEffect(() => {
    if (!previousPath && !nextPath) {
      return;
    }

    function isTypingTarget(target: EventTarget | null) {
      if (!(target instanceof HTMLElement)) {
        return false;
      }

      const tagName = target.tagName;
      return (
        target.isContentEditable ||
        tagName === "INPUT" ||
        tagName === "TEXTAREA" ||
        tagName === "SELECT"
      );
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
        return;
      }

      if (isTypingTarget(event.target)) {
        return;
      }

      if (event.key === "j" && previousPath) {
        event.preventDefault();
        navigate({ to: previousPath });
      }

      if (event.key === "k" && nextPath) {
        event.preventDefault();
        navigate({ to: nextPath });
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [navigate, nextPath, previousPath]);

  return (
    <DocsShell
      category={page.category}
      headings={page.sections.map((section) => section.title)}
      title={page.title}
    >
      <p className="mt-6 text-lg leading-8 text-slate-600">{page.summary}</p>
      {page.sections.map((section) => (
        <section className="mt-16 scroll-mt-28" id={headingId(section.title)} key={section.title}>
          <h2 className="text-3xl font-semibold tracking-normal text-slate-950">{section.title}</h2>
          {section.blocks.map((block, index) => renderBlock(block, index))}
        </section>
      ))}
      {previousPath || nextPath ? (
        <div className="mt-16 flex w-full items-center justify-between">
          {previousPath ? (
            <Link
              className="inline-flex items-center justify-center rounded-md border border-slate-200 bg-slate-50 px-3 py-2 font-mono text-sm font-semibold tracking-wide text-slate-700 transition hover:border-slate-300 hover:bg-white hover:text-slate-950 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-300"
              to={previousPath}
            >
              Previous
            </Link>
          ) : (
            <span />
          )}
          {nextPath ? (
            <Link
              className="inline-flex items-center justify-center rounded-md border border-slate-200 bg-slate-50 px-3 py-2 font-mono text-sm font-semibold tracking-wide text-slate-700 transition hover:border-slate-300 hover:bg-white hover:text-slate-950 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-300"
              to={nextPath}
            >
              Next
            </Link>
          ) : null}
        </div>
      ) : null}
    </DocsShell>
  );
}

const rootRoute = createRootRoute({
  component: RootLayout,
  notFoundComponent: NotFoundPage,
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: HomePage,
});

const installRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/install",
  component: InstallPage,
});

const securityRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/security",
  component: SecurityPage,
});

const tryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/try",
  component: TryPage,
});

const privacyRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/privacy",
  component: () => <LegalPage page={privacyPage} />,
});

const termsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/terms",
  component: () => <LegalPage page={termsPage} />,
});

// ── /docs (all paths from docsPages under /docs) ─────────────────────
const docsLayoutRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/docs",
  component: DocsLayout,
});
function docsChild(path: string) {
  const full = path === "/" ? "/docs" : `/docs/${path}`;
  return createRoute({
    getParentRoute: () => docsLayoutRoute,
    path,
    component: () => <DocsContentPage page={docsPage(full)} />,
  });
}
/** Every docs page under /docs, derived from docsPages so std API routes stay in sync. */
const docsChildren = docsPages
  .filter((p) => p.path === "/docs" || p.path.startsWith("/docs/"))
  .map((p) => {
    if (p.path === "/docs") {
      return docsChild("/");
    }
    return docsChild(p.path.slice("/docs/".length));
  });

// ── /book ────────────────────────────────────────────────────────────
const bookLayoutRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/book",
  component: DocsLayout,
});
function bookChild(path: string) {
  const full = path === "/" ? "/book" : `/book/${path}`;
  return createRoute({
    getParentRoute: () => bookLayoutRoute,
    path,
    component: () => <DocsContentPage page={docsPage(full)} />,
  });
}
const bookChildren = [
  bookChild("/"),
  bookChild("leaders"),
  bookChild("binds"),
  bookChild("values"),
  bookChild("collections"),
  bookChild("control"),
  bookChild("result-option"),
  bookChild("strings"),
  bookChild("modules"),
  bookChild("structs"),
  bookChild("tasks"),
  bookChild("names"),
];

// ── /e26 ─────────────────────────────────────────────────────────────
const e26LayoutRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/e26",
  component: DocsLayout,
});
function e26Child(path: string) {
  const full = path === "/" ? "/e26" : `/e26/${path}`;
  return createRoute({
    getParentRoute: () => e26LayoutRoute,
    path,
    component: () => <DocsContentPage page={docsPage(full)} />,
  });
}
const e26Children = [
  e26Child("/"),
  e26Child("spec"),
  e26Child("run"),
  e26Child("layout"),
  e26Child("protocol"),
];

const routeTree = rootRoute.addChildren([
  indexRoute,
  installRoute,
  securityRoute,
  tryRoute,
  privacyRoute,
  termsRoute,
  docsLayoutRoute.addChildren(docsChildren),
  bookLayoutRoute.addChildren(bookChildren),
  e26LayoutRoute.addChildren(e26Children),
]);

export const router = createRouter({
  routeTree,
  defaultPreload: "intent",
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
