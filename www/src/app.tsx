import { Link } from "@tanstack/react-router";
import { CtaLink } from "./components/cta-link";
import { EchoCode } from "./components/echo-code";
import { homePage } from "./docs/site";

/** Keep the hyphenated word on one line so the heading never breaks mid-word. */
function splitHyphenated(text: string, word: string): [string, string, string] {
  const index = text.indexOf(word);
  if (index < 0) {
    return [text, "", ""];
  }
  return [text.slice(0, index), word, text.slice(index + word.length)];
}

export function HomePage() {
  const definitionParts = splitHyphenated(homePage.definition, "glyph-led");
  return (
    <main className="bg-white px-6 pb-12 pt-28 text-slate-950 sm:pt-32">
      <section className="mx-auto grid w-full max-w-7xl items-start gap-12 lg:grid-cols-[minmax(0,0.92fr)_minmax(28rem,1.08fr)] lg:gap-16">
        <div className="max-w-2xl">
          <h1 className="text-balance font-display text-[clamp(2.25rem,7vw,3.75rem)] font-bold leading-[1.02] tracking-[-0.045em] text-slate-950">
            {definitionParts[0]}
            <span className="whitespace-nowrap">{definitionParts[1]}</span>
            {definitionParts[2]}
          </h1>
          <p className="mt-7 max-w-xl text-pretty text-lg leading-8 text-slate-600 sm:text-xl sm:leading-8">
            {homePage.lead}
          </p>
          <p className="mt-5 max-w-xl text-pretty text-base leading-7 text-slate-500 sm:text-lg sm:leading-8">
            {homePage.status}
          </p>
          <div className="mt-9 flex flex-wrap items-center gap-3">
            <CtaLink to="/install">Install xo</CtaLink>
            <CtaLink to="/try" variant="secondary">
              Try Echo
            </CtaLink>
            <CtaLink to="/docs" variant="ghost">
              Documents
            </CtaLink>
          </div>
        </div>

        <figure className="min-w-0">
          <div className="overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-sm">
            <div className="flex items-center justify-between border-b border-slate-200 bg-slate-50 px-4 py-3">
              <figcaption className="font-mono text-xs font-semibold text-slate-500">
                {homePage.sampleCaption}
              </figcaption>
              <span className="rounded-full bg-violet-100 px-2 py-1 font-mono text-[0.65rem] font-semibold text-violet-700">
                source
              </span>
            </div>
            <EchoCode
              aria-label="Echo source example"
              className="overflow-x-auto bg-white"
              code={homePage.sample}
              language="echo"
              variant="inline-block"
            />
            <div className="border-t border-slate-200 bg-slate-50 px-4 py-3">
              <p className="font-mono text-[0.65rem] font-semibold uppercase tracking-wide text-slate-500">
                xo run output
              </p>
              <pre
                aria-label="sum.echo output"
                className="mt-2 overflow-x-auto font-mono text-sm leading-6 text-slate-800"
              >
                {homePage.sampleOutput}
              </pre>
            </div>
          </div>
        </figure>
      </section>

      <section
        aria-label="What Echo is"
        className="mx-auto mt-20 grid w-full max-w-7xl gap-x-12 gap-y-10 border-t border-slate-200 pt-12 sm:grid-cols-2 lg:grid-cols-4 lg:gap-x-10"
      >
        {homePage.highlights.map((item) => (
          <div key={item.title}>
            <h2 className="font-display text-xl font-bold tracking-tight text-slate-950">
              <Link className="transition hover:text-violet-700" to={item.to as "/"}>
                {item.title}
              </Link>
            </h2>
            <p className="mt-3 text-pretty text-sm leading-6 text-slate-600">{item.text}</p>
          </div>
        ))}
      </section>

      <section
        aria-labelledby="home-examples"
        className="mx-auto mt-20 w-full max-w-7xl border-t border-slate-200 pt-12"
      >
        <h2
          className="font-display text-3xl font-bold tracking-tight text-slate-950"
          id="home-examples"
        >
          {homePage.examplesTitle}
        </h2>
        <p className="mt-3 max-w-2xl text-pretty text-base leading-7 text-slate-600">
          {homePage.examplesLead}
        </p>
        <div className="mt-10 grid gap-8 lg:grid-cols-3">
          {homePage.examples.map((example) => (
            <figure key={example.source} className="flex min-w-0 flex-col">
              <p className="mb-4 text-pretty text-sm leading-6 text-slate-600">{example.note}</p>
              <div className="flex flex-1 flex-col overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-sm">
                <div className="flex items-center justify-between border-b border-slate-200 bg-slate-50 px-4 py-3">
                  <figcaption className="font-mono text-xs font-semibold text-slate-500">
                    {example.caption}
                  </figcaption>
                  <span className="rounded-full bg-violet-100 px-2 py-1 font-mono text-[0.65rem] font-semibold text-violet-700">
                    source
                  </span>
                </div>
                <EchoCode
                  aria-label={`${example.caption} source`}
                  className="flex-1 overflow-x-auto bg-white"
                  code={example.code}
                  language="echo"
                  variant="inline-block"
                />
                <div className="border-t border-slate-200 bg-slate-50 px-4 py-3">
                  <p className="font-mono text-[0.65rem] font-semibold uppercase tracking-wide text-slate-500">
                    xo run output
                  </p>
                  <pre
                    aria-label={`${example.caption} output`}
                    className="mt-2 overflow-x-auto font-mono text-sm leading-6 text-slate-800"
                  >
                    {example.output}
                  </pre>
                </div>
              </div>
            </figure>
          ))}
        </div>
      </section>

      <nav
        aria-label="Language documentation"
        className="mx-auto mt-20 grid w-full max-w-7xl gap-0 border-t border-slate-200 sm:grid-cols-3"
      >
        {homePage.links.map((link) => (
          <Link
            key={link.to}
            className="group border-b border-slate-200 py-8 sm:border-b-0 sm:px-6 sm:py-10 sm:first:pl-0 sm:last:pr-0 sm:[&:not(:first-child)]:border-l sm:[&:not(:first-child)]:border-slate-200"
            to={link.to as "/"}
          >
            <h2 className="font-display text-2xl font-bold tracking-tight text-slate-950 transition group-hover:text-violet-700">
              {link.title}
            </h2>
            <p className="mt-3 max-w-sm text-sm leading-6 text-slate-500">{link.description}</p>
          </Link>
        ))}
      </nav>
    </main>
  );
}

export default HomePage;
