import { Link } from "@tanstack/react-router";
import { CtaLink } from "./components/cta-link";
import { InstallSnippet } from "./components/install-snippet";
import { installPage, isCodePart, isLinkPart, type InlinePart } from "./docs/install-content";
import { currentPrereleaseAssets, currentPrereleaseTag } from "./lib/current-release";

function renderInline(parts: readonly InlinePart[]) {
  return parts.map((part, index) => {
    if (typeof part === "string") {
      return <span key={index}>{part}</span>;
    }
    if (isLinkPart(part)) {
      return (
        <a
          className="font-mono font-semibold text-slate-800 underline-offset-4 hover:underline"
          href={part.href}
          key={index}
        >
          {part.label}
        </a>
      );
    }
    if (isCodePart(part)) {
      return (
        <span key={index} className="font-mono font-semibold text-slate-800">
          {part.code}
        </span>
      );
    }
    return null;
  });
}

/**
 * Product install page: current prerelease assets + source build fallback.
 */
export function InstallPage() {
  return (
    <main className="min-h-screen bg-white px-6 pb-24 pt-32 text-slate-950 sm:pt-36">
      <div className="mx-auto w-full max-w-3xl">
        <h1 className="text-3xl font-semibold tracking-normal text-slate-950 sm:text-4xl">
          {installPage.title}
        </h1>
        <p className="mt-4 text-pretty text-lg leading-8 text-slate-600">
          {renderInline(installPage.lead)}
        </p>

        {installPage.sections.map((section) => (
          <section className="mt-14" key={section.title}>
            <h2 className="text-xl font-semibold tracking-normal text-slate-950 sm:text-2xl">
              {section.title}
            </h2>
            {section.paragraphs.map((paragraph, index) => (
              <p
                className="mt-4 text-pretty text-base leading-7 text-slate-600 sm:text-lg sm:leading-8"
                key={index}
              >
                {renderInline(paragraph)}
              </p>
            ))}
            {section.assets ? (
              <ul className="mt-6 space-y-2 text-base leading-7 text-slate-600">
                {currentPrereleaseAssets.map((asset) => (
                  <li key={asset.artifact}>
                    <span className="font-mono font-semibold text-slate-800">{asset.archive}</span>
                    {" · "}
                    {asset.host}
                  </li>
                ))}
              </ul>
            ) : null}
            {section.code ? (
              <div className="mt-6">
                <InstallSnippet code={section.code} />
              </div>
            ) : null}
          </section>
        ))}

        <section className="mt-14">
          <h2 className="text-xl font-semibold tracking-normal text-slate-950 sm:text-2xl">
            {installPage.nextTitle}
          </h2>
          <ol className="mt-6 space-y-4 text-base leading-7 text-slate-600">
            {installPage.nextSteps.map((step, index) => (
              <li key={step.to}>
                <span className="font-semibold text-slate-950">
                  {index + 1}. {step.title}
                </span>
                : {step.text}
              </li>
            ))}
          </ol>
          <div className="mt-8 flex flex-wrap gap-3">
            {installPage.nextSteps.map((step) => (
              <CtaLink
                key={step.to}
                to={step.to as "/"}
                variant={step.variant === "secondary" ? "secondary" : undefined}
              >
                {step.label}
              </CtaLink>
            ))}
          </div>
          <p className="mt-8 text-sm leading-6 text-slate-500">
            {installPage.projectLead}
            <Link
              className="font-semibold text-slate-800 underline-offset-4 hover:underline"
              to={installPage.projectTo as "/"}
            >
              {installPage.projectLabel}
            </Link>
            .
          </p>
          <p className="sr-only">
            Current prerelease {currentPrereleaseTag}. Use from-release to install it.
          </p>
        </section>
      </div>
    </main>
  );
}

export default InstallPage;
