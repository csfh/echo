import { securityContact } from "./docs/site";

/**
 * Public vulnerability reporting. Same mailbox and policy file as SECURITY.md.
 */
export function SecurityPage() {
  return (
    <main className="min-h-screen bg-white px-6 pb-24 pt-32 text-slate-950 sm:pt-36">
      <div className="mx-auto w-full max-w-3xl">
        <h1 className="text-3xl font-semibold tracking-normal text-slate-950 sm:text-4xl">
          Security
        </h1>
        <p className="mt-4 text-pretty text-lg leading-8 text-slate-600">
          Report vulnerabilities by email. Do not open a public GitHub issue for a security report.
        </p>

        <section className="mt-14">
          <h2 className="text-xl font-semibold tracking-normal text-slate-950 sm:text-2xl">
            Mailbox
          </h2>
          <p className="mt-4 text-pretty text-base leading-7 text-slate-600 sm:text-lg sm:leading-8">
            Email{" "}
            <a
              className="font-semibold text-slate-800 underline-offset-4 hover:underline"
              href={securityContact.mailto}
            >
              {securityContact.email}
            </a>{" "}
            with a description of the issue and its impact, steps to reproduce or a proof of concept
            if available, and affected versions, commits, or platforms if known.
          </p>
        </section>

        <section className="mt-14">
          <h2 className="text-xl font-semibold tracking-normal text-slate-950 sm:text-2xl">
            Policy
          </h2>
          <p className="mt-4 text-pretty text-base leading-7 text-slate-600 sm:text-lg sm:leading-8">
            The repository policy is{" "}
            <a
              className="font-semibold text-slate-800 underline-offset-4 hover:underline"
              href={securityContact.policyUrl}
              rel="noreferrer"
              target="_blank"
            >
              SECURITY.md
            </a>
            . We acknowledge receipt when we can and work with you on coordinated disclosure. Give a
            reasonable window to investigate and ship a fix before public discussion.
          </p>
        </section>

        <section className="mt-14">
          <h2 className="text-xl font-semibold tracking-normal text-slate-950 sm:text-2xl">
            Other bugs
          </h2>
          <p className="mt-4 text-pretty text-base leading-7 text-slate-600 sm:text-lg sm:leading-8">
            Non-security bugs belong on{" "}
            <a
              className="font-semibold text-slate-800 underline-offset-4 hover:underline"
              href="https://github.com/csfh/echo/issues"
              rel="noreferrer"
              target="_blank"
            >
              GitHub issues
            </a>
            .
          </p>
        </section>
      </div>
    </main>
  );
}

export default SecurityPage;
