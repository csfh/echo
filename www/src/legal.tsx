import { useEffect } from "react";
import { Link } from "@tanstack/react-router";
import { type LegalPageContent } from "./docs/site";

const MAIL_RE = /([a-z.]+@christofferhallas\.com)/g;

function renderLegalText(text: string) {
  return text.split(MAIL_RE).map((part, index) => {
    if (part.endsWith("@christofferhallas.com")) {
      return (
        <a
          key={index}
          className="font-medium text-slate-800 underline-offset-4 hover:underline"
          href={`mailto:${part}`}
        >
          {part}
        </a>
      );
    }

    return <span key={index}>{part}</span>;
  });
}

export function LegalPage({ page }: { page: LegalPageContent }) {
  useEffect(() => {
    document.title = `${page.title} · Echo`;
    return () => {
      document.title = "Echo Programming Language";
    };
  }, [page.title]);

  return (
    <main className="min-h-screen bg-white px-6 pb-24 pt-32 text-slate-950 sm:pt-36">
      <div className="mx-auto w-full max-w-3xl">
        <h1 className="text-3xl font-semibold tracking-normal text-slate-950 sm:text-4xl">
          {page.title}
        </h1>
        <p className="mt-4 text-pretty text-lg leading-8 text-slate-600">{page.summary}</p>

        {page.sections.map((section) => (
          <section className="mt-14" key={section.title}>
            <h2 className="text-xl font-semibold tracking-normal text-slate-950 sm:text-2xl">
              {section.title}
            </h2>
            {section.paragraphs.map((paragraph) => (
              <p
                className="mt-4 text-pretty text-base leading-7 text-slate-600 sm:text-lg sm:leading-8"
                key={paragraph}
              >
                {renderLegalText(paragraph)}
              </p>
            ))}
          </section>
        ))}

        <nav aria-label="Legal pages" className="mt-16 flex flex-wrap gap-4 text-sm">
          <Link
            className="font-semibold text-slate-800 underline-offset-4 hover:underline"
            to="/privacy"
          >
            Privacy
          </Link>
          <Link
            className="font-semibold text-slate-800 underline-offset-4 hover:underline"
            to="/terms"
          >
            Terms
          </Link>
        </nav>
      </div>
    </main>
  );
}
