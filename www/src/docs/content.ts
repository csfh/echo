import { docsHubCatalog, languageFeatureEntries, type SiteLink } from "./site";
import {
  stdExportCount,
  stdExportKind,
  stdGroups,
  stdImportLine,
  stdMethodsFor,
  stdModules,
  type StdDocEntry,
  type StdExport,
  type StdModule,
} from "./std-reference";

export type DocsNavGroup = {
  title: string;
  links: DocsNavLink[];
};

export type DocsNavLink = {
  label: string;
  to: string;
  disabled?: boolean;
  children?: DocsNavLink[];
};

export type DocsTextPart = string | { code: string };

export type DocsBlock =
  | { kind: "paragraph"; text: DocsTextPart[] }
  | { kind: "code"; code: string; language?: "shellscript" | "echo" }
  | { kind: "catalog"; entries: SiteLink[] };

export type DocsSection = {
  title: string;
  blocks: DocsBlock[];
  tags?: string[];
  aliases?: string[];
};

export type DocsPage = {
  id: string;
  path: string;
  category: string;
  title: string;
  summary: string;
  tags: string[];
  aliases?: string[];
  sections: DocsSection[];
};

export function headingId(title: string) {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}

/** Left-nav trees for Docs, Book, and Echo 2026 as separate site sections. */
export const docsNavigation: DocsNavGroup[] = [
  {
    title: "Start",
    links: [
      { label: "Documents", to: "/docs" },
      { label: "First program", to: "/docs/first-program" },
      { label: "Project setup", to: "/docs/project" },
    ],
  },
  {
    title: "Language reference",
    links: [
      ...languageFeatureEntries.map((entry) => ({ label: entry.title, to: entry.to })),
      { label: "Memory", to: "/docs/memory" },
      { label: "Names and layout", to: "/docs/names" },
    ],
  },
  {
    title: "Standard library",
    links: [
      { label: "Overview", to: "/docs/std" },
      { label: "API reference", to: "/docs/std/reference" },
      ...stdGroups.map((group) => ({
        label: group.title,
        to: group.modules[0]?.docsPath ?? "/docs/std",
        children: group.modules.map((m) => ({
          label: m.title,
          to: m.docsPath,
        })),
      })),
    ],
  },
  {
    title: "Guides",
    links: [
      { label: "Packages", to: "/docs/guides/packages" },
      { label: "Diagnostics", to: "/docs/guides/diagnostics" },
      { label: "REPL", to: "/docs/guides/repl" },
      { label: "Cookbook", to: "/docs/guides/cookbook" },
    ],
  },
  {
    title: "Toolchain",
    links: [
      { label: "Toolchain", to: "/docs/toolchain" },
      {
        label: "Commands",
        to: "/docs/toolchain/commands",
      },
      { label: "Examples", to: "/docs/toolchain/examples" },
    ],
  },
];

export const bookNavigation: DocsNavGroup[] = [
  {
    title: "The Echo Book",
    links: [
      { label: "Introduction", to: "/book" },
      { label: "Leaders", to: "/book/leaders" },
      { label: "Binds and functions", to: "/book/binds" },
      { label: "Values and operators", to: "/book/values" },
      { label: "Collections and ranges", to: "/book/collections" },
      { label: "Control", to: "/book/control" },
      { label: "Result and option", to: "/book/result-option" },
      { label: "Strings", to: "/book/strings" },
      { label: "Modules and std", to: "/book/modules" },
      { label: "Structs", to: "/book/structs" },
      { label: "Tasks", to: "/book/tasks" },
      { label: "Names and layout", to: "/book/names" },
    ],
  },
];

export const e26Navigation: DocsNavGroup[] = [
  {
    title: "Edition",
    links: [
      { label: "Overview", to: "/e26" },
      { label: "Language Spec", to: "/e26/spec" },
    ],
  },
  {
    title: "Conformance",
    links: [
      { label: "Run", to: "/e26/run" },
      { label: "Layout", to: "/e26/layout" },
      { label: "Protocol", to: "/e26/protocol" },
    ],
  },
];

export function navigationForPath(pathname: string): DocsNavGroup[] {
  if (pathname === "/book" || pathname.startsWith("/book/")) {
    return bookNavigation;
  }
  if (pathname === "/e26" || pathname.startsWith("/e26/")) {
    return e26Navigation;
  }
  return docsNavigation;
}

/**
 * Laravel-style entry body: teach in prose, show the example next, then a
 * short parameters / return note. Matches the rate-limiting docs rhythm
 * (intro → code → follow-on detail) without marketing cadence.
 */
function stdEntryBlocks(entry: StdDocEntry): DocsBlock[] {
  return [
    {
      kind: "paragraph",
      text: [entry.description, " Call form: ", { code: entry.call }, "."],
    },
    {
      kind: "code",
      language: "echo",
      code: entry.example,
    },
    {
      kind: "paragraph",
      text: ["Parameters: ", entry.params, " Returns: ", entry.returns],
    },
  ];
}

function stdConstSection(m: StdModule, e: StdExport): DocsSection {
  return {
    title: e.name,
    tags: ["export", "const", "api", e.name, m.path, e.call],
    aliases: [e.name, e.call, `${m.name}.${e.name}`, `${m.path}.${e.name}`],
    blocks: stdEntryBlocks(e),
  };
}

function stdStructOverviewSection(m: StdModule, e: StdExport): DocsSection {
  const methods = stdMethodsFor(m.path, e.name);
  const methodNames = methods.map((x) => x.name);
  return {
    title: `Struct · ${e.name}`,
    tags: ["export", "struct", "api", e.name, m.path, e.call, ...methodNames],
    aliases: [e.name, e.call, `${m.name}.${e.name}`, `${m.path}.${e.name}`, `% ${e.name}`],
    blocks: [
      {
        kind: "paragraph",
        text: [
          e.description,
          " The shape is exported as ",
          { code: e.call },
          methods.length
            ? `. Methods on the receiver are listed next.`
            : `. Construct or obtain values through the free functions below when the package provides them.`,
        ],
      },
      {
        kind: "code",
        language: "echo",
        code: e.example,
      },
      {
        kind: "paragraph",
        text: ["Parameters: ", e.params, " Returns: ", e.returns],
      },
    ],
  };
}

function stdMethodSection(m: StdModule, struct: StdExport, method: StdDocEntry): DocsSection {
  const title = `${struct.name} · ${method.name}`;
  return {
    title,
    tags: ["export", "method", "api", method.name, struct.name, m.path, method.call],
    aliases: [
      method.name,
      method.call,
      `${struct.name}.${method.name}`,
      `${m.name}.${struct.name}.${method.name}`,
      title,
    ],
    blocks: stdEntryBlocks(method),
  };
}

function stdFuncSection(m: StdModule, e: StdExport): DocsSection {
  return {
    title: e.name,
    tags: ["export", "func", "api", e.name, m.path, e.call],
    aliases: [e.name, e.call, `${m.name}.${e.name}`, `${m.path}.${e.name}`],
    blocks: stdEntryBlocks(e),
  };
}

function stdModulePage(m: StdModule): DocsPage {
  const exportNames = m.exports.map((e) => e.name).join(", ");
  const exportCalls = m.exports.map((e) => e.call).join(", ");
  const consts = m.exports.filter((e) => stdExportKind(e) === "const");
  const structs = m.exports.filter((e) => stdExportKind(e) === "struct");
  const funcs = m.exports.filter((e) => stdExportKind(e) === "func");
  const methodNames = structs.flatMap((s) => stdMethodsFor(m.path, s.name).map((x) => x.name));

  const sections: DocsSection[] = [
    {
      title: "Introduction",
      tags: ["package", "import", "overview", m.path],
      blocks: [
        {
          kind: "paragraph",
          text: [
            m.summary,
            " Import ",
            { code: m.path },
            " to bind the module as ",
            { code: m.name },
            ". Call free functions as ",
            { code: `${m.name}.name(...)` },
            ". Struct methods use a receiver value.",
          ],
        },
        {
          kind: "code",
          language: "echo",
          code: stdImportLine(m),
        },
        {
          kind: "paragraph",
          text: [
            "This page is the package reference for ",
            { code: m.path },
            ". Private helpers used only by co-located tests are not listed.",
          ],
        },
      ],
    },
  ];

  if (consts.length > 0) {
    sections.push({
      title: "Constants",
      tags: ["constants", m.path],
      blocks: [
        {
          kind: "paragraph",
          text: [
            "Constants are module values you read without calling. Each constant below has its own heading.",
          ],
        },
      ],
    });
    for (const e of consts) {
      sections.push(stdConstSection(m, e));
    }
  }

  if (structs.length > 0) {
    for (const e of structs) {
      sections.push(stdStructOverviewSection(m, e));
      for (const method of stdMethodsFor(m.path, e.name)) {
        sections.push(stdMethodSection(m, e, method));
      }
    }
  }

  if (funcs.length > 0) {
    sections.push({
      title: "Functions",
      tags: ["functions", m.path],
      blocks: [
        {
          kind: "paragraph",
          text: [
            "Free functions on ",
            { code: m.name },
            ". Each function has a short description, an example, then parameters and return shape.",
          ],
        },
      ],
    });
    for (const e of funcs) {
      sections.push(stdFuncSection(m, e));
    }
  }

  return {
    id: `docs-std-${m.path.replace(/\//g, "-")}`,
    path: m.docsPath,
    category: "Standard library",
    title: m.title,
    summary: m.summary,
    tags: [
      "std",
      "api",
      "package",
      m.path,
      m.name,
      ...m.exports.map((e) => e.name),
      ...methodNames,
    ],
    aliases: [m.path, `std ${m.name}`, exportNames, exportCalls],
    sections,
  };
}

const docsPagesBase: DocsPage[] = [
  // ── Start ──────────────────────────────────────────────────────────

  // ── Documents hub ─────────────────────────────────────────────────
  {
    id: "docs",
    path: "/docs",
    category: "Documents",
    title: "Documents",
    summary:
      "Find the first program, language forms, standard-library packages, and the Echo 2026 Spec.",
    tags: ["docs", "overview", "documents", "reference", "echo 2026"],
    aliases: ["documentation", "home docs", "echo docs", "reference", "language spec"],
    sections: docsHubCatalog.map((group) => ({
      title: group.title,
      tags: [group.title.toLowerCase(), "catalog"],
      blocks: [
        {
          kind: "paragraph" as const,
          text: [group.description],
        },
        {
          kind: "catalog" as const,
          entries: group.entries,
        },
      ],
    })),
  },
  {
    id: "docs-first-program",
    path: "/docs/first-program",
    category: "Docs",
    title: "First program",
    summary: "Minimal runnable shape and xo commands. Build the toolchain first via /install.",
    tags: ["docs", "reference", "hello", "xo"],
    aliases: ["quickstart", "hello", "first program"],
    sections: [
      {
        title: "Program",
        tags: ["form", "runnable"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/io

$ xs = [1, 2, 3]
~ sum = 0
* x : xs {
    ~ sum = sum + x
}
io.print("sum={sum}")`,
          },
        ],
      },
      {
        title: "Read it",
        tags: ["rules", "walkthrough"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Top-level statements run in order. No reserved entry function is required. ",
              { code: "/ std/io" },
              " binds module ",
              { code: "io" },
              ". ",
              { code: "io.print" },
              " writes a string. The list is walked by ",
              { code: "* x : xs" },
              ", and the mutable accumulator is updated with ",
              { code: "~" },
              ". Userland printing goes through ",
              { code: "std" },
              "; there is no free global ",
              { code: "print" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Commands",
        tags: ["xo"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `cargo build -p xo
./target/debug/xo check examples/misc/sum_list.echo
./target/debug/xo run examples/misc/sum_list.echo
./target/debug/xo run --jit examples/misc/sum_list.echo
./target/debug/xo build examples/misc/sum_list.echo -o /tmp/sum-list
/tmp/sum-list`,
          },
        ],
      },
      {
        title: "Next steps",
        tags: ["guide", "navigation"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Open ",
              { code: "/try" },
              " to check this program and capture ",
              { code: "io.print" },
              ". Build ",
              { code: "xo" },
              " via ",
              { code: "/install" },
              " when you still need the toolchain. ",
              { code: "Project setup" },
              " starts a new directory. Learn the statement glyphs on ",
              { code: "Leaders" },
              ", then continue through binds, values, and collections. Spec TOC: ",
              { code: "/e26/spec" },
              ". Narrative chapters live under ",
              { code: "/book" },
              ".",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-project",
    path: "/docs/project",
    category: "Guide",
    title: "Project setup",
    summary: "Build xo, create an entry file, and establish a reliable local workflow.",
    tags: ["guide", "project", "setup", "install", "build", "getting started"],
    aliases: ["installation", "new project", "prerequisites", "linux", "quickstart"],
    sections: [
      {
        title: "Build the toolchain",
        tags: ["build", "source", "requirements"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Echo’s supported development baseline is Linux. Building this repository requires Rust, ",
              { code: "clang" },
              ", ",
              { code: "mold" },
              ", and ",
              { code: "sccache" },
              " because the workspace Cargo configuration selects them directly.",
            ],
          },
          {
            kind: "code",
            language: "shellscript",
            code: `git clone https://github.com/csfh/echo.git
cd echo
cargo build -p xo
./target/debug/xo --help`,
          },
          {
            kind: "paragraph",
            text: [
              "The remaining examples assume the built binary is available as ",
              { code: "xo" },
              ". If it is not on your PATH, replace that command with the absolute path to ",
              { code: "target/debug/xo" },
              " from the checkout.",
            ],
          },
        ],
      },
      {
        title: "Project shape",
        tags: ["files", "entry", "module"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "An Echo project needs only an entry ",
              { code: ".echo" },
              " file. Add sibling files or folder modules as the program grows. ",
              { code: "xo.toml" },
              " is optional and only needed for pinned external dependencies.",
            ],
          },
          {
            kind: "code",
            language: "shellscript",
            code: `my_app/
├── main.echo
├── config.echo
├── routes/
│   ├── health.echo
│   └── home.echo
└── xo.toml        # optional`,
          },
        ],
      },
      {
        title: "Entry file",
        tags: ["main", "program", "runnable"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/io

$ values = [10, 20, 12]
~ total = 0
* value : values {
    ~ total = total + value
}

io.print("total={total}")`,
          },
          {
            kind: "paragraph",
            text: [
              "The entry file’s top-level statements are the program body. Imports are resolved from that file into a closed module graph; no reserved main function is required.",
            ],
          },
        ],
      },
      {
        title: "Development loop",
        tags: ["check", "format", "run", "build", "jit"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `xo fmt --check main.echo
xo check main.echo
xo run main.echo
xo run --jit main.echo
xo build -O 2 main.echo -o my_app
./my_app`,
          },
          {
            kind: "paragraph",
            text: [
              { code: "check" },
              " is the fastest full-graph correctness pass. AOT ",
              { code: "run" },
              " and ",
              { code: "build" },
              " emit LLVM then link once with clang at ",
              { code: "-O0 -g" },
              "; ",
              { code: "run --jit" },
              " executes the same LLVM IR in-process.",
            ],
          },
        ],
      },
      {
        title: "Run from the project root",
        tags: ["cwd", "xo.toml", "cache"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Run ",
              { code: "xo" },
              " from the directory that owns ",
              { code: "xo.toml" },
              ". Project compiler artifacts live under ",
              { code: ".xo/cache" },
              "; downloaded packages live separately under the user root printed by ",
              { code: "xo home" },
              ".",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-leaders",
    path: "/docs/leaders",
    category: "Docs",
    title: "Leaders",
    summary: "Statement leader forms.",
    tags: ["docs", "reference", "leaders"],
    aliases: ["leader table", "glyphs", "keywords"],
    sections: [
      {
        title: "Rules",
        tags: ["rules"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Leader at statement start after indentation. Space after leader required (except bare ",
              { code: "<" },
              " / ",
              { code: ">" },
              "). ",
              { code: "{" },
              " on same line as introducer.",
            ],
          },
        ],
      },
      {
        title: "Forms",
        tags: ["forms", "table"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `$ name = expr     ; immutable bind
~ name = expr     ; mutable / reassign
# NAME = expr     ; compile-time const (SCREAMING_SNAKE)
% name { }        ; struct primary
@ name { }        ; struct extra members
? expr { }        ; if
: expr { }        ; else-if
: { }             ; else / match default
! expr            ; return err
^ expr            ; return
* { }             ; loop
* cond { }        ; while
* item : items { }; for-in
<                 ; break
>                 ; continue
| expr { arms }   ; match
+ call(args)      ; spawn task
+ job = () { }    ; spawn body, bind handle
- job             ; join task
- value = job     ; join and bind result
/ path            ; import
\\ name           ; export`,
          },
        ],
      },
      {
        title: "Dual-use",
        tags: ["operators"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Statement start decides whether a dual-use glyph is a leader. In expressions: ",
              { code: "+" },
              " / ",
              { code: "-" },
              " add or subtract, ",
              { code: "*" },
              " multiply, ",
              { code: "/" },
              " divide, ",
              { code: "!" },
              " not, ",
              { code: "<" },
              " / ",
              { code: ">" },
              " compare, ",
              { code: "%" },
              " is remainder, and ",
              { code: "|" },
              " is the true literal.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-binds",
    path: "/docs/binds",
    category: "Docs",
    title: "Binds and functions",
    summary: "Bind leaders and function values.",
    tags: ["docs", "reference", "bind", "functions"],
    aliases: ["dollar", "tilde", "hash"],
    sections: [
      {
        title: "Binds",
        tags: ["forms"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `$ x = 1, y = 2 ; sequential immutable binds
~ total = 0    ; mutable bind
~ total = x + y ; reassign
# A = 21       ; compile-time constant
# B = A + A    ; constants may use other constants
# D = 5s       ; duration / bytes / locator lits fold
# XS = [1, 2]  ; list lits fold
# R = 1..3     ; inclusive range lits fold
# P = point { x: 1, y: 2 } ; named / anon struct lits fold
# X = P.x      ; field / index on other constants fold
# FIRST = XS[0]`,
          },
        ],
      },
      {
        title: "Functions",
        tags: ["functions"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `$ add = (a, b) {
    ^ a + b
}

add(20, 22)`,
          },
          {
            kind: "paragraph",
            text: [
              "Free functions are ordinary values: pass them to another function, return them, or rebind a mutable function slot. They are introduced with ",
              { code: "$" },
              " / ",
              { code: "~" },
              " / ",
              { code: "#" },
              ". ",
              { code: "^ expr" },
              " returns a value. Bare ",
              { code: "^" },
              " is option none. ",
              { code: "! expr" },
              " is result err, only inside a function (see Result page).",
            ],
          },
        ],
      },
      {
        title: "Scope",
        tags: ["scope"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A name is available only after its bind. Echo does not shadow a name in the same region; update a mutable with ",
              { code: "~ name =" },
              ". Function parameters and local binds belong to the function body.",
            ],
          },
        ],
      },
      {
        title: "Const expressions",
        tags: ["const", "compile-time"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "#" },
              " accepts literals and operations over other constants, including duration, bytes, locator, list, range, and struct lits, plus field and index on other ",
              { code: "#" },
              " values. Omitted ",
              { code: "%" },
              " field defaults that are constant expressions are filled when folding a named struct. Runtime calls are not constant expressions. Constant names use ",
              { code: "SCREAMING_SNAKE" },
              ".",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-values",
    path: "/docs/values",
    category: "Docs",
    title: "Values and operators",
    summary: "Literal forms, operator precedence, equality, and copy behavior.",
    tags: ["docs", "reference", "values", "literals", "operators"],
    aliases: ["numbers", "booleans", "equality", "precedence", "value reference"],
    sections: [
      {
        title: "Literal forms",
        tags: ["literals", "numbers", "bool", "bytes", "duration", "locator"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `42  0xff  0b1010  1_000 ; integers
3.14  1e-3             ; floats
<i32> 42  <f32> 3.5    ; explicit width
|  _                    ; true, false
b'raw'  b"rich\\n{name}" ; bytes (rich interpolates like a string)
p'/tmp/file'            ; locator (absolute)
p'home/user'            ; locator (relative)
p"http://xo.run"        ; locator (URI)
250ms  5s  2m           ; duration`,
          },
          {
            kind: "paragraph",
            text: [
              "Integers default to ",
              { code: "i64" },
              " and floats to ",
              { code: "f64" },
              ". Width tags are prefix-only and support ",
              { code: "i32" },
              ", ",
              { code: "i64" },
              ", ",
              { code: "f32" },
              ", and ",
              { code: "f64" },
              ". Numeric kinds and explicit widths do not mix implicitly. A locator is one kind: ",
              { code: "path.class" },
              " reads the stored text and reports ",
              { code: "0" },
              " relative, ",
              { code: "1" },
              " absolute, or ",
              { code: "2" },
              " URI when a scheme is followed by ",
              { code: "://" },
              ". There is no path normalize on the stored payload. Rich ",
              { code: 'p"…"' },
              " interpolates a bound name the same way a rich string does: ",
              { code: "{name}" },
              " is a local, parameter, or ",
              { code: "#" },
              " const.",
            ],
          },
        ],
      },
      {
        title: "Operators and precedence",
        tags: ["operators", "precedence", "arithmetic", "boolean"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `-x  !ready
a * b  a / b  a % b
a + b  a - b
lo..hi
a == b  a != b  a === b  a !== b
a < b  a <= b  a > b  a >= b
left && right
left || right`,
          },
          {
            kind: "paragraph",
            text: [
              "Precedence runs from primary expressions to unary operators, multiplication/division/remainder, addition/subtraction, ranges, comparisons, ",
              { code: "&&" },
              ", then ",
              { code: "||" },
              ". Parentheses override that order. Integer division truncates toward zero; float division remains fractional.",
            ],
          },
        ],
      },
      {
        title: "Deep and identity equality",
        tags: ["equality", "identity", "deep"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "==" },
              " and ",
              { code: "!=" },
              " compare content recursively. For structs and lists, ",
              { code: "===" },
              " and ",
              { code: "!==" },
              " ask whether both names refer to the same object. Identity and deep equality are the same for value kinds such as numbers and strings.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
/ std/str

$ a = [1, 2]
$ b = [1, 2]
$ alias = a

io.print(str.from_int(a == b))
io.print(str.from_int(a === b))
io.print(str.from_int(a === alias))`,
          },
        ],
      },
      {
        title: "Copy behavior",
        tags: ["copy", "reference", "value"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Assignment, rebinding, and function calls always copy the binding. Named and anonymous structs and lists are references, so the copied binding shares one object. Numbers, booleans, strings, bytes, locators, durations, ranges, and function values copy their value. Reclamation of managed values is a separate topic; see ",
              { code: "/docs/memory" },
              ".",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-collections",
    path: "/docs/collections",
    category: "Docs",
    title: "Collections and ranges",
    summary: "Lists, anonymous products, indexing, assignment, and inclusive ranges.",
    tags: ["docs", "reference", "list", "collection", "range"],
    aliases: ["array", "index", "anonymous struct", "product", "range"],
    sections: [
      {
        title: "Lists",
        tags: ["list", "index", "assignment"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `$ xs = [1, 2, 3]
$ first = xs[0]
~ xs[0] = 9
~ xs[2] = xs[0] + xs[1]`,
          },
          {
            kind: "paragraph",
            text: [
              "Lists are ordered reference values. An alias or function parameter shares the same list, so an indexed write is visible through every alias. List literals and call arguments do not allow trailing commas.",
            ],
          },
        ],
      },
      {
        title: "Anonymous products",
        tags: ["anonymous", "struct", "product"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `$ point = { x: 3, y: 4 }
$ x = point.x`,
          },
          {
            kind: "paragraph",
            text: [
              { code: "{ name: value }" },
              " creates an anonymous struct: a structural product with named fields. It is not a general map and has no named type tag. Use a named struct literal when methods or type-match arms are needed.",
            ],
          },
        ],
      },
      {
        title: "Ranges",
        tags: ["range", "inclusive", "loop", "match"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "lo..hi" },
              " creates an inclusive integer range. Bind it, iterate over it, or use it as a match arm.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io

~ total = 0
* n : 1..4 {
    ~ total = total + n
}

| total {
    1..9 {
        io.print('small')
    }
    10..20 {
        io.print('medium')
    }
    : {
        io.print('large')
    }
}`,
          },
        ],
      },
      {
        title: "Unavailable literals",
        tags: ["null", "map", "set", "trailing comma"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Echo has no null literal, map literal, or set literal. Option values come from function return shapes; there is no standalone none literal. Trailing commas are rejected.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-control",
    path: "/docs/control",
    category: "Docs",
    title: "Control",
    summary: "If, loops, match, break, continue.",
    tags: ["docs", "reference", "control"],
    aliases: ["if", "loop", "match"],
    sections: [
      {
        title: "If chain",
        tags: ["if"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `? score >= 90 {
    io.print('excellent')
}
: score >= 70 {
    io.print('passing')
}
: {
    io.print('try again')
}`,
          },
        ],
      },
      {
        title: "Loops",
        tags: ["loop"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `* { }                  ; infinite
* ready == _ { }       ; while condition is true
* item : items { }     ; for-in list or range
<                      ; break nearest loop
>                      ; continue nearest loop`,
          },
        ],
      },
      {
        title: "Match",
        tags: ["match"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/io

$ number = 5
| number {
    1, 2, 3 {
        io.print('small')
    }
    4..9 {
        io.print('middle')
    }
    : {
        io.print('other')
    }
}

% user {
    $ name
}

$ person = user { name: 'Ada' }
| person {
    % user {
        io.print(person.name)
    }
    : {
        io.print('not a user')
    }
}`,
          },
          {
            kind: "paragraph",
            text: [
              "Value arms compare deeply and may list multiple expressions. Range arms test inclusive membership; ",
              { code: "% struct_name" },
              " arms test a named struct tag. The default arm is optional. Result and option use their own arm shapes; see ",
              { code: "Result and option" },
              ".",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-result-option",
    path: "/docs/result-option",
    category: "Docs",
    title: "Result and option",
    summary: "Produce and match result / option shapes.",
    tags: ["docs", "reference", "result", "option"],
    aliases: ["err", "ok", "some", "none"],
    sections: [
      {
        title: "Result",
        tags: ["result"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A function with an error-return path has a result shape. ",
              { code: "!" },
              " returns the error payload from the current function and leaves the process running. File-scope ",
              { code: "!" },
              " is ",
              { code: "sem-error-return" },
              ". ",
              { code: "^ v" },
              " is ok. ",
              { code: "! e" },
              " is err.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
/ std/str

$ checked = (x) {
    ? x < 0 {
        ! 'negative'
    }
    ^ x
}

| checked(7) {
    $ value {
        io.print(str.from_int(value))
    }
    ! error {
        io.print(error)
    }
}`,
          },
        ],
      },
      {
        title: "Option",
        tags: ["option"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "An option-shaped function returns ",
              { code: "^ v" },
              " for some and bare ",
              { code: "^" },
              " for none.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
/ std/str

$ maybe = (x) {
    ? x == 0 {
        ^
    }
    ^ x
}

| maybe(7) {
    $ value {
        io.print(str.from_int(value))
    }
    : {
        io.print('empty')
    }
}`,
          },
        ],
      },
      {
        title: "Rules",
        tags: ["rules"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A result match uses ",
              { code: "$ name" },
              " for ok and ",
              { code: "! name" },
              " for err. An option match uses ",
              { code: "$ name" },
              " for some and ",
              { code: ":" },
              " for none. Leaving either shape unhandled is a compile error; there is no silent discard or separate try form.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-strings",
    path: "/docs/strings",
    category: "Docs",
    title: "Strings",
    summary: "Pure and rich string forms.",
    tags: ["docs", "reference", "string"],
    aliases: ["quotes", "interp"],
    sections: [
      {
        title: "Forms",
        tags: ["forms"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `'pure'           ; no escapes, no interp, no interior '
"rich\\n{name}"   ; escapes + {name} interp`,
          },
        ],
      },
      {
        title: "Rules",
        tags: ["rules"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "String concatenation with ",
              { code: "+" },
              " is unavailable. Build text with rich strings. ",
              { code: "==" },
              " and ",
              { code: "!=" },
              " compare content.",
            ],
          },
        ],
      },
      {
        title: "Interpolation",
        tags: ["interpolation", "escape"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Rich strings interpolate a bound name with ",
              { code: "{name}" },
              ". Inside a method, ",
              { code: "{.field}" },
              " reads from the receiver. Pure strings preserve their contents literally and cannot contain an interior single quote.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `$ name = 'Ada'
$ visits = 3
$ message = "Hello, {name}; visits={visits}\\n"`,
          },
        ],
      },
    ],
  },
  {
    id: "docs-modules",
    path: "/docs/modules",
    category: "Docs",
    title: "Modules and std",
    summary: "Import, export, std, runtime.",
    tags: ["docs", "reference", "modules", "std"],
    aliases: ["import", "export", "packages"],
    sections: [
      {
        title: "Import / export",
        tags: ["import", "export"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/io          ; binds io
/ ./lib           ; binds lib
/ ../util         ; binds util
io.print(lib.message)
\\ name            ; export
\\ a, b            ; multi export`,
          },
        ],
      },
      {
        title: "Rules",
        tags: ["rules"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Each import binds the last path segment as one module name. Use ",
              { code: "module.export" },
              ". No star-import. ",
              { code: "io.print" },
              " accepts strings; convert other values explicitly through modules such as ",
              { code: "std/str" },
              ". There is no free ",
              { code: "print" },
              "; use ",
              { code: "std" },
              ". ",
              { code: "/ runtime" },
              " is allowed only in privileged std sources. An imported function whose defining module has a known return kind types that way at the importer (",
              { code: "str.from_int" },
              " is a string, so ",
              { code: "str.from_int(1) + 1" },
              " is a kind mismatch). Import parameters stay unspecialized: one call site does not freeze later argument kinds.",
            ],
          },
        ],
      },
      {
        title: "Files and folders",
        tags: ["file", "folder", "module", "export"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A relative path resolves to a matching ",
              { code: ".echo" },
              " file or a directory module. A directory module includes its sorted ",
              { code: "*.echo" },
              " files and exposes the union of their exports. Private names remain file-local; imports only see exported names.",
            ],
          },
        ],
      },
      {
        title: "External packages",
        tags: ["package", "xo get", "xo home"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Host-style imports resolve from the user package cache. ",
              { code: "xo get" },
              " installs a package, ",
              { code: "xo home" },
              " prints the cache locations, and an optional ",
              { code: "xo.toml" },
              " pins dependencies for check, run, and build.",
            ],
          },
          {
            kind: "code",
            language: "shellscript",
            code: `xo get github.com/owner/package@v1.2.3
xo get local-package --path ./local-package
xo home`,
          },
        ],
      },
    ],
  },
  {
    id: "docs-structs",
    path: "/docs/structs",
    category: "Docs",
    title: "Structs",
    summary: "% shape, @ members, method receiver.",
    tags: ["docs", "reference", "struct"],
    aliases: ["percent", "at", "methods"],
    sections: [
      {
        title: "Forms",
        tags: ["forms"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `% user {
    $ name
    ~ visits = 0
    $ greet = () {
        ^ "hi {.name}"
    }
}

@ user {
    $ label = () {
        ^ "{.name}#{.visits}"
    }
}

$ u = user { name: "Ada", visits: 0 }
u.greet()
u.label()`,
          },
        ],
      },
      {
        title: "Rules",
        tags: ["rules"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "One ",
              { code: "%" },
              " per ",
              { code: "struct_name" },
              ". ",
              { code: "@" },
              " adds members (other files ok). Members use ",
              { code: "$" },
              " / ",
              { code: "~" },
              " / ",
              { code: "#" },
              ". Method call ",
              { code: "v.m()" },
              " binds receiver ",
              { code: "." },
              ".",
            ],
          },
        ],
      },
      {
        title: "Fields and defaults",
        tags: ["field", "default", "assignment"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A member without an initializer must be supplied by the literal. A default initializer fills an omitted field. Use ",
              { code: "~ value.field = expr" },
              " or ",
              { code: "~ .field = expr" },
              " to update a mutable field; immutable fields reject writes.",
            ],
          },
        ],
      },
      {
        title: "Receiver returns",
        tags: ["method", "receiver", "chain"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A plain method that reaches the end without an explicit return yields its receiver, like ",
              { code: "^ ." },
              ". This enables method chains. Free functions and result- or option-shaped methods do not receive that fall-through behavior.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-tasks",
    path: "/docs/tasks",
    category: "Docs",
    title: "Tasks",
    summary: "Spawn work, capture values, and join task handles.",
    tags: ["docs", "reference", "task", "spawn", "join"],
    aliases: ["concurrency", "plus leader", "minus leader", "task handle", "capture"],
    sections: [
      {
        title: "Spawn and join",
        tags: ["spawn", "join", "handle"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `$ inc = (x) {
    ^ x + 1
}

+ job = inc(41)
- answer = job`,
          },
          {
            kind: "paragraph",
            text: [
              { code: "+" },
              " schedules a free-function call or task body. Binding the spawn result stores a task handle. ",
              { code: "- handle" },
              " waits and discards the result; ",
              { code: "- name = handle" },
              " waits and binds it.",
            ],
          },
        ],
      },
      {
        title: "Task bodies and captures",
        tags: ["body", "capture", "reference"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `$ base = 40
+ job = () [base] {
    ^ base + 2
}
- answer = job`,
          },
          {
            kind: "paragraph",
            text: [
              "A task body is closed to outer locals unless they are named in its optional capture list. Captures must already be bound and are passed by reference. Reference values share their object; value kinds carry the same value. A spawn call or capture list accepts up to eight values.",
            ],
          },
        ],
      },
      {
        title: "Immediate block",
        tags: ["immediate", "block", "result"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "- { body }" },
              " schedules and joins a body in one statement. Use ",
              { code: "- name = { body }" },
              " to keep its return value. Result- and option-shaped task returns are handled with the same match forms as ordinary function calls.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `- result = {
    ^ 42
}`,
          },
        ],
      },
      {
        title: "Lifecycle rule",
        tags: ["lifecycle", "unjoined"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Every spawned task must be joined before process exit. Leaving a task handle unjoined makes the program exit unsuccessfully. Task scheduling is a language feature; userland does not import a task module.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-memory",
    path: "/docs/memory",
    category: "Docs",
    title: "Memory",
    summary:
      "Managed values are owned by scopes and disposed when control leaves those scopes. Echo has no tracing garbage collector.",
    tags: ["docs", "reference", "memory", "garbage collection", "gc", "lifetime", "scope"],
    aliases: [
      "garbage collection",
      "gc",
      "reclamation",
      "scope-owned",
      "lifetime",
      "dispose",
      "free",
    ],
    sections: [
      {
        title: "Garbage collection",
        tags: ["gc", "tracing", "collector"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Echo has no tracing garbage collector. Mark-and-sweep pauses and concurrent heap walkers are outside the language model. Pure reference counting is outside the user-facing model as well.",
            ],
          },
          {
            kind: "paragraph",
            text: [
              "Reclamation is ",
              { code: "scope-owned" },
              ": every managed allocation has an owning lexical or dynamic scope. When control leaves that scope, values still owned by it are disposed on that edge, at a known program point.",
            ],
          },
        ],
      },
      {
        title: "Scope-owned dispose",
        tags: ["scope", "release", "promotion"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Reason about lifetimes through program structure. A block, function body, or other structured region owns the managed values created inside it. Leaving the scope (including ",
              { code: "^" },
              " return, ",
              { code: "<" },
              " break, and ",
              { code: ">" },
              " continue) releases what that scope still owns.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
/ std/str

$ show = () {
  $ xs = [1, 2, 3]
  io.print(str.from_int(xs[0]))
  ; when show returns, values still owned by the body are released
}

show()`,
          },
          {
            kind: "paragraph",
            text: [
              "When a value must outlive its creating scope (returned, stored into a longer-lived struct or list, and similar cases), ownership is ",
              { code: "promoted" },
              " outward first. Unpromoted owners are released when their scope ends.",
            ],
          },
        ],
      },
      {
        title: "Escape and graph promotion",
        tags: ["promotion", "escape", "graph", "nested"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Promotion is ",
              { code: "graph-based" },
              ": when a managed value escapes a scope, every reachable managed allocation ",
              { code: "still owned by that scope" },
              " is promoted with it (list elements, struct fields, nested products). Allocations already owned by a longer-lived scope stay where they are.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
/ std/str

$ make = () {
  $ xs = [7]
  $ holder = [xs]
  ^ holder
}

$ r = make()
io.print(str.from_int(r[0][0]))
; nested list survives make's frame via graph promotion`,
          },
          {
            kind: "paragraph",
            text: [
              "This is region ownership with graph evacuation. Shared longer-lived values that a nested structure only ",
              { code: "points at" },
              " stay put; only allocations owned by the escaping scope move, so they are neither stolen nor double-freed.",
            ],
          },
        ],
      },
      {
        title: "Values, references, and ownership",
        tags: ["value", "reference", "copy", "ownership"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Copy rules and dispose rules are different questions. Assignment always copies the binding: structs and lists share the object; numbers, strings, and other value kinds copy the value. See ",
              { code: "/docs/values" },
              " for copy behavior.",
            ],
          },
          {
            kind: "paragraph",
            text: [
              "Sharing storage does not invent a second free policy. ",
              { code: "Ownership for dispose" },
              " stays scope-based: one owning scope is responsible for release, and graph promotion moves that responsibility for the whole escaping subgraph when a value escapes.",
            ],
          },
        ],
      },
      {
        title: "Working model",
        tags: ["model", "predictable", "servers"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "You reason about lifetimes with ordinary program structure: nested blocks ending, functions returning, loop bodies finishing an iteration. Under the product model, managed heap is released on those edges at known program points, without waiting for process exit or a background collector.",
            ],
          },
          {
            kind: "paragraph",
            text: [
              "The language law is fixed: scope-owned dispose with graph promotion on escape. The toolchain implements registries, graph promote, and dispose on leave-scope edges.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-names",
    path: "/docs/names",
    category: "Docs",
    title: "Names and layout",
    summary: "Identifiers, comments, kinds.",
    tags: ["docs", "reference", "naming"],
    aliases: ["snake_case", "comments"],
    sections: [
      {
        title: "Identifiers",
        tags: ["rules", "identifiers"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "ASCII ",
              { code: "snake_case" },
              ". Struct names lowercase. ",
              { code: "#" },
              " → ",
              { code: "SCREAMING_SNAKE" },
              ". Names are case-sensitive. ",
            ],
          },
        ],
      },
      {
        title: "Layout",
        tags: ["layout", "comments"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: ";" },
              " starts a comment that runs to end of line. Write one construct per line except for multi-bind. Blocks put ",
              { code: "{" },
              " on the introducer line; Echo has no line-continuation marker and rejects trailing commas.",
            ],
          },
        ],
      },
      {
        title: "Kinds",
        tags: ["types", "inference", "width"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Kinds are inferred on bindings, parameters, returns, and fields. There is no colon ascription or generics surface. Numeric width tags appear only before literals: ",
              { code: "<i32>42" },
              " and ",
              { code: "<f64>3.14" },
              ". An imported function uses the defining module’s return kind when that kind is known; import parameters stay open.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std",
    path: "/docs/std",
    category: "Standard library",
    title: "Standard library",
    summary:
      "Userland modules for I/O, strings, files, process, JSON, encoding, crypto, collections, networking, and more.",
    tags: ["std", "standard library", "io", "network", "http", "json", "fs", "api"],
    aliases: ["stdlib", "library modules", "api overview", "standard library reference"],
    sections: [
      {
        title: "Import policy",
        tags: ["import", "userland"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "User programs import only ",
              { code: "std/…" },
              " modules. There are no free built-ins: output is ",
              { code: "io.print" },
              ", conversion is ",
              { code: "str.from_int" },
              ", and networking is accessed through its module.",
            ],
          },
        ],
      },
      {
        title: "Public surface",
        tags: ["api", "exports"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Each module page documents every public export from that module’s ",
              { code: "\\ " },
              " line in ",
              { code: "std/" },
              " as a package page: introduction, constants, structs with methods, then free functions. Each callable has prose, an example, and parameter/return notes. Helpers used only by co-located tests are not public. There are ",
              String(stdModules.length),
              " modules and ",
              String(stdExportCount),
              " public exports.",
            ],
          },
          {
            kind: "paragraph",
            text: [
              "Browse the full index under ",
              { code: "API reference" },
              ", or open a module from the Standard library nav.",
            ],
          },
        ],
      },
      {
        title: "Modules",
        tags: ["module", "exports", "surface"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: stdModules
              .map((m) => {
                const names = m.exports
                  .map((e) => e.name)
                  .slice(0, 6)
                  .join(", ");
                const more = m.exports.length > 6 ? ", …" : "";
                return `/ ${m.path.padEnd(22)} ; ${names}${more}`;
              })
              .join("\n"),
          },
          {
            kind: "paragraph",
            text: [
              "Every import binds one module name from the final path segment. Folder modules such as ",
              { code: "std/net/tcp" },
              " and ",
              { code: "std/crypto/hash" },
              " combine exports from the Echo files in that folder.",
            ],
          },
        ],
      },
      {
        title: "Values at the boundary",
        tags: ["string", "bytes", "struct", "reference"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Output accepts strings. Socket and file reads often return bytes. Convert explicitly with ",
              { code: "std/str" },
              ". Network listeners and connections are named structs passed by reference, so aliases share the same underlying resource.",
            ],
          },
        ],
      },
      {
        title: "Network failures",
        tags: ["failure", "handle", "open", "eof"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Many network constructors report failure with a resource whose ",
              { code: "handle" },
              " is ",
              { code: "0" },
              " and ",
              { code: "open" },
              " is false. Writes return ",
              { code: "-1" },
              " on failure. TCP reads return empty bytes on EOF or failure; UDP receive returns an empty data/from product on failure.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-io-strings",
    path: "/docs/std/io-strings",
    category: "Standard library",
    title: "I/O and strings",
    summary: "Print strings, read stdin, and explicitly convert, measure, or concatenate values.",
    tags: ["std", "io", "string", "print", "conversion", "stdin"],
    aliases: ["std io", "std str", "log", "eprint", "string length", "concat", "stdin"],
    sections: [
      {
        title: "Output",
        tags: ["io", "print", "log", "eprint"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/io

io.print('ordinary output')
io.log('log output')
io.eprint('error-oriented output')`,
          },
          {
            kind: "paragraph",
            text: [
              { code: "print(value)" },
              " and ",
              { code: "log(value)" },
              " write a string plus newline to standard output. ",
              { code: "eprint(value)" },
              " writes to standard error instead. Non-string values produce no output, so convert them explicitly.",
            ],
          },
        ],
      },
      {
        title: "Standard input",
        tags: ["io", "stdin", "read"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
/ std/str

| io.read() {
    $ b {
        io.print(str.from_bytes(b))
    }
    ! e {
        io.print(e)
    }
}`,
          },
          {
            kind: "paragraph",
            text: [
              { code: "io.read()" },
              " consumes process stdin until EOF and returns the payload as bytes. Empty stdin is empty bytes. Pipe a program: ",
              { code: "printf ping | xo run examples/misc/stdin.echo" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Conversions",
        tags: ["convert", "int", "float", "bytes", "duration", "locator"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `str.from_int(value)
str.from_float(value)
str.from_bytes(value)
str.from_duration(value)
str.from_locator(value)`,
          },
          {
            kind: "paragraph",
            text: [
              { code: "from_bytes" },
              " decodes with lossy UTF-8 replacement for invalid bytes. Duration formatting chooses the largest exact unit; locator conversion returns its path or URI text.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
/ std/str

$ count = str.from_int(42)
$ ratio = str.from_float(3.5)
$ timeout = str.from_duration(250ms)
$ location = str.from_locator(p'/tmp/echo')

io.print("count={count} ratio={ratio}")
io.print("timeout={timeout} location={location}")`,
          },
        ],
      },
      {
        title: "Length",
        tags: ["len", "length", "bytes", "utf8"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "str.len(value)" },
              " returns the byte length of a string or bytes value. It is not a Unicode character count. Invalid or unsupported values produce ",
              { code: "0" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Concatenation",
        tags: ["cat", "concat", "interpolation"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Echo does not overload ",
              { code: "+" },
              " for strings. Prefer rich-string interpolation when names fit the template; use ",
              { code: "str.cat(a, b)" },
              " when two values must be joined dynamically.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `$ greeting = "Hello, {name}"
$ path = str.cat(base, suffix)`,
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-tcp",
    path: "/docs/std/tcp",
    category: "Standard library",
    title: "TCP",
    summary: "Listen, connect, exchange bytes, and close shared TCP resources.",
    tags: ["std", "tcp", "network", "socket", "connection", "listener"],
    aliases: ["std net tcp", "listen", "connect", "accept", "read", "write"],
    sections: [
      {
        title: "Surface",
        tags: ["api", "listener", "conn", "helpers"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `tcp.listen(addr)              ; listener
tcp.connect(addr)             ; conn
listener.accept()             ; conn
conn.read(limit)              ; bytes
conn.write(string_or_bytes)   ; bytes written, -1 on failure
conn.close()                  ; closes and marks open false
listener.close()              ; closes and marks open false`,
          },
          {
            kind: "paragraph",
            text: [
              "The module also exports free ",
              { code: "accept" },
              ", ",
              { code: "read" },
              ", ",
              { code: "write" },
              ", and ",
              { code: "close" },
              " helpers. Prefer methods when the named struct type is known; method close also updates the resource’s ",
              { code: "open" },
              " field.",
            ],
          },
        ],
      },
      {
        title: "Resource fields",
        tags: ["fields", "handle", "open", "remote", "addr"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A listener exposes ",
              { code: "addr" },
              ", ",
              { code: "handle" },
              ", and mutable ",
              { code: "open" },
              ". A connection exposes ",
              { code: "remote" },
              ", ",
              { code: "handle" },
              ", and mutable ",
              { code: "open" },
              ". Treat the handle as an opaque failure check, not as a separate socket type.",
            ],
          },
        ],
      },
      {
        title: "Loopback exchange",
        tags: ["example", "loopback", "bytes", "runnable"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
/ std/str
/ std/net/tcp

$ lis = tcp.listen('127.0.0.1:39880')
$ client = tcp.connect('127.0.0.1:39880')
$ server = lis.accept()

client.write('ping')
$ received = server.read(64)
io.print(str.from_bytes(received))

client.close()
server.close()
lis.close()`,
          },
        ],
      },
      {
        title: "Blocking and tasks",
        tags: ["blocking", "event loop", "task", "server"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Accept, read, and write cooperate with Echo’s event loop when the operating-system socket would block. A server can spawn a task per accepted connection. Every task must still be joined before normal process exit. Long-running accept loops end when the process stops.",
            ],
          },
        ],
      },
      {
        title: "Failure and EOF",
        tags: ["failure", "eof", "empty", "limit"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Failed listen, connect, or accept returns a struct with ",
              { code: "handle == 0" },
              " and ",
              { code: "open == _" },
              ". Read limits at or below zero return empty bytes; other reads return empty bytes for EOF or failure. A single read is not a message boundary. Application protocols must frame their own data.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-udp",
    path: "/docs/std/udp",
    category: "Standard library",
    title: "UDP",
    summary: "Bind a datagram socket, send packets, receive sender metadata, and close it.",
    tags: ["std", "udp", "network", "socket", "datagram"],
    aliases: ["std net udp", "bind", "send_to", "recv_from", "packet"],
    sections: [
      {
        title: "Surface",
        tags: ["api", "socket", "helpers"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `udp.bind(addr)                    ; socket
socket.send_to(data, destination) ; bytes sent, -1 on failure
socket.recv_from(limit)            ; { data: bytes, from: string }
socket.close()                     ; closes and marks open false`,
          },
          {
            kind: "paragraph",
            text: [
              "The module also exports free ",
              { code: "send_to" },
              ", ",
              { code: "recv_from" },
              ", and ",
              { code: "close" },
              " helpers. A socket exposes ",
              { code: "addr" },
              ", ",
              { code: "handle" },
              ", and mutable ",
              { code: "open" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Loopback datagram",
        tags: ["example", "loopback", "packet", "runnable"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
/ std/str
/ std/net/udp

$ sock = udp.bind('127.0.0.1:39881')
sock.send_to('hello', '127.0.0.1:39881')

$ packet = sock.recv_from(64)
io.print(str.from_bytes(packet.data))
io.print(packet.from)

sock.close()`,
          },
        ],
      },
      {
        title: "Datagram behavior",
        tags: ["boundary", "limit", "failure", "from"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Each receive returns at most one datagram and preserves its sender in ",
              { code: "packet.from" },
              ". The byte limit caps the captured payload. Bind failure produces ",
              { code: "handle == 0" },
              "; receive failure produces empty ",
              { code: "data" },
              " and ",
              { code: "from" },
              " fields.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-http",
    path: "/docs/std/http",
    category: "Standard library",
    title: "HTTP",
    summary: "Parse requests, construct responses, dispatch exact routes, and serve over TCP.",
    tags: ["std", "http", "server", "request", "response", "route"],
    aliases: ["std net http", "parse_request", "dispatch", "serve", "routing"],
    sections: [
      {
        title: "Request and response shapes",
        tags: ["request", "response", "fields", "methods"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A request has ",
              { code: "method" },
              ", ",
              { code: "path" },
              ", ",
              { code: "headers" },
              ", and ",
              { code: "body" },
              " fields plus ",
              { code: "is_get()" },
              ", ",
              { code: "is_post()" },
              ", and ",
              { code: "has_body()" },
              ". A response carries mutable ",
              { code: "status" },
              ", ",
              { code: "headers" },
              ", and ",
              { code: "body" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Parse a request",
        tags: ["parse", "headers", "body"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
/ std/net/http

$ raw = "POST /items HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello"
$ req = http.parse_request(raw)

io.print(req.method)
io.print(req.path)
io.print(req.body)`,
          },
          {
            kind: "paragraph",
            text: [
              "Header names are normalized into fields on ",
              { code: "req.headers" },
              ". Parsing preserves a request body through the declared Content-Length when the full request is available.",
            ],
          },
        ],
      },
      {
        title: "Response helpers",
        tags: ["response", "text", "html", "json", "format"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `http.text_response(status, body)
http.html_response(status, body)
http.json_response(status, body)
http.format_response(response)`,
          },
          {
            kind: "paragraph",
            text: [
              "The three constructors set the corresponding Content-Type. ",
              { code: "format_response" },
              " emits an HTTP/1.1 response with status text, Content-Type, Content-Length, and Connection: close.",
            ],
          },
        ],
      },
      {
        title: "Dispatch routes",
        tags: ["route", "handler", "dispatch", "404"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Routes are a list of anonymous products with ",
              { code: "path" },
              " and callable ",
              { code: "handle" },
              " fields. Dispatch uses exact path equality and calls the first match. When no route matches, it returns a text 404 response.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
/ std/str
/ std/net/http

$ health = (req) {
    ^ http.json_response(200, "ok")
}

$ routes = [
    { path: "/health", handle: health }
]

$ req = http.parse_request("GET /health HTTP/1.1\r\nHost: echo\r\n\r\n")
$ res = http.dispatch(routes, req)
io.print(str.from_int(res.status))
io.print(res.body)`,
          },
        ],
      },
      {
        title: "Serve",
        tags: ["serve", "tcp", "connection", "long-running"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `$ routes = [
    { path: "/health", handle: health }
]

http.serve('127.0.0.1:8080', routes)`,
          },
          {
            kind: "paragraph",
            text: [
              { code: "serve" },
              " starts an infinite TCP accept loop and schedules each accepted connection as a task. Use ",
              { code: "handle_connection(conn, routes)" },
              " when an application owns the listener and needs a finite or custom accept loop. Listen failure is logged and returns a server value with ",
              { code: "running == _" },
              ".",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-math",
    path: "/docs/std/math",
    category: "Standard library",
    title: "Math",
    summary: "Integer min/max/abs and f64 libm helpers under std/math.",
    tags: ["std", "math", "float", "sqrt"],
    aliases: ["stdlib math", "trigonometry"],
    sections: [
      {
        title: "Surface",
        tags: ["import", "export"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/math

$ a = math.abs_i(-3)
$ b = math.min(1, 2)
$ r = math.sqrt(9.0)`,
          },
          {
            kind: "paragraph",
            text: [
              "Exports include ",
              { code: "abs_i" },
              ", ",
              { code: "min" },
              ", ",
              { code: "max" },
              ", ",
              { code: "floor" },
              ", ",
              { code: "ceil" },
              ", ",
              { code: "sqrt" },
              ", ",
              { code: "pow" },
              ", ",
              { code: "sin" },
              ", ",
              { code: "cos" },
              ", ",
              { code: "tan" },
              ", and ",
              { code: "abs_f" },
              ". Float ops use f64.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-fs",
    path: "/docs/std/fs",
    category: "Standard library",
    title: "Path and filesystem",
    summary: "Path helpers and filesystem I/O under std/path and std/fs.",
    tags: ["std", "fs", "path", "file"],
    aliases: ["stdlib fs", "files", "directories"],
    sections: [
      {
        title: "Path",
        tags: ["path", "join"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/path

$ p = path.join("/tmp", "out.txt")
$ name = path.file_name(p)`,
          },
          {
            kind: "paragraph",
            text: [
              { code: "std/path" },
              " exports ",
              { code: "join" },
              ", ",
              { code: "is_abs" },
              ", ",
              { code: "file_name" },
              ", ",
              { code: "parent" },
              ", and ",
              { code: "extension" },
              ", ",
              { code: "class" },
              ", and ",
              { code: "is_uri" },
              ". ",
              { code: "class" },
              " is ",
              { code: "0" },
              " relative, ",
              { code: "1" },
              " absolute (",
              { code: "/…" },
              "), or ",
              { code: "2" },
              " URI (",
              { code: "scheme://" },
              "). String and locator arguments share that rule. Path arguments elsewhere accept strings or locators.",
            ],
          },
        ],
      },
      {
        title: "Filesystem",
        tags: ["fs", "read", "write"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "std/fs" },
              " covers existence checks, whole-file read/write (bytes), directories, metadata, streaming ",
              { code: "% file" },
              ", plus ",
              { code: "temp_dir" },
              ", ",
              { code: "create_temp" },
              ", and ",
              { code: "symlink" },
              ". Failures use result/option shapes. The module does not panic on those paths.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-process",
    path: "/docs/std/process",
    category: "Standard library",
    title: "Process and OS",
    summary: "Process args/env/run and OS process/host info.",
    tags: ["std", "process", "os", "env"],
    aliases: ["stdlib process", "environment", "spawn"],
    sections: [
      {
        title: "Process",
        tags: ["args", "env", "run"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "std/process" },
              " exports ",
              { code: "args" },
              ", ",
              { code: "env" },
              ", ",
              { code: "env_set" },
              ", ",
              { code: "env_unset" },
              ", ",
              { code: "exit" },
              ", ",
              { code: "run" },
              ", and ",
              { code: "run_capture" },
              " (exit code plus stdout/stderr). Argv is shell-less.",
            ],
          },
        ],
      },
      {
        title: "OS",
        tags: ["pid", "cwd", "hostname"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/os

io.print(str.from_int(os.pid()))
io.print(os.platform())`,
          },
          {
            kind: "paragraph",
            text: [
              { code: "std/os" },
              " exports ",
              { code: "pid" },
              ", ",
              { code: "cwd" },
              ", ",
              { code: "chdir" },
              ", ",
              { code: "hostname" },
              ", and ",
              { code: "platform" },
              ".",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-json",
    path: "/docs/std/json",
    category: "Standard library",
    title: "JSON",
    summary: "Parse and stringify product values with std/json.",
    tags: ["std", "json", "parse"],
    aliases: ["stdlib json"],
    sections: [
      {
        title: "Surface",
        tags: ["parse", "stringify"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/json

$ v = json.parse('{"a":1}')
$ s = json.stringify(v)`,
          },
          {
            kind: "paragraph",
            text: [
              "v0 maps JSON to Echo product types (bools, numbers, strings, lists, structs). Errors are result-shaped. Streaming parse is out of scope.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-encoding",
    path: "/docs/std/encoding",
    category: "Standard library",
    title: "Encoding",
    summary: "Hex, Base64, and strict UTF-8 under std/encoding.",
    tags: ["std", "encoding", "hex", "base64", "utf8"],
    aliases: ["stdlib encoding", "base64", "hex", "utf8"],
    sections: [
      {
        title: "Hex, Base64, and UTF-8",
        tags: ["hex", "base64", "utf8"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/encoding/hex
/ std/encoding/base64
/ std/encoding/utf8

$ h = hex.encode(bytes_from_str)
$ b = base64.encode(bytes_from_str)
$ ok = utf8.valid(b"hi")`,
          },
          {
            kind: "paragraph",
            text: [
              "Both modules export ",
              { code: "encode" },
              " and ",
              { code: "decode" },
              ". Corrupt input fails with a result error.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-random-log",
    path: "/docs/std/random-log",
    category: "Standard library",
    title: "Random and log",
    summary: "Non-crypto PRNG and leveled logging.",
    tags: ["std", "random", "log"],
    aliases: ["stdlib random", "stdlib log", "prng"],
    sections: [
      {
        title: "Random (not crypto)",
        tags: ["random", "seed"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "std/random" },
              " exports ",
              { code: "seed" },
              ", ",
              { code: "u64" },
              ", and ",
              { code: "float" },
              ". It is not cryptographically secure. For CSPRNG use ",
              { code: "std/crypto/random" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Log levels",
        tags: ["log", "level"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "std/log" },
              " is pure over ",
              { code: "io" },
              ". Callers pass a minimum level into ",
              { code: "debug" },
              ", ",
              { code: "info" },
              ", ",
              { code: "warn" },
              ", and ",
              { code: "error" },
              " (or use ",
              { code: "emit" },
              " directly). Levels: 0 debug, 1 info, 2 warn, 3 error.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-collections",
    path: "/docs/std/collections",
    category: "Standard library",
    title: "Collections",
    summary: "Hash table, map, set, queue, and list helpers.",
    tags: ["std", "collections", "map", "set", "queue"],
    aliases: ["stdlib collections", "hash table"],
    sections: [
      {
        title: "Map, set, queue",
        tags: ["map", "set", "queue"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "std/collections/map" },
              " and ",
              { code: "set" },
              " sit on ",
              { code: "hash_table" },
              " with mixed keys via ",
              { code: "reflect.key_bytes" },
              ". ",
              { code: "std/collections/queue" },
              " exports ",
              { code: "make" },
              ", ",
              { code: "push" },
              ", and ",
              { code: "pop" },
              ". ",
              { code: "std/list" },
              " adds ",
              { code: "sum_ints" },
              ", ",
              { code: "sort_ints" },
              ", and closed-function ",
              { code: "map" },
              "/",
              { code: "filter" },
              "/",
              { code: "fold" },
              " beyond core length/get helpers.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-crypto",
    path: "/docs/std/crypto",
    category: "Standard library",
    title: "Crypto",
    summary: "SipHash, SHA-256, and CSPRNG under std/crypto.",
    tags: ["std", "crypto", "hash", "sha256"],
    aliases: ["stdlib crypto", "siphash", "sha256"],
    sections: [
      {
        title: "Hash and CSPRNG",
        tags: ["hash", "csprng"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "std/crypto/hash" },
              " provides SipHash-2-4 (",
              { code: "sip" },
              ") and ",
              { code: "sha256" },
              ". ",
              { code: "std/crypto/random" },
              " exports ",
              { code: "fill" },
              " and ",
              { code: "u64" },
              " as a CSPRNG. Do not confuse with non-crypto ",
              { code: "std/random" },
              ".",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-net-client",
    path: "/docs/std/net-client",
    category: "Standard library",
    title: "DNS and HTTP client",
    summary: "DNS lookup and cleartext HTTP GET client.",
    tags: ["std", "dns", "http", "client"],
    aliases: ["stdlib dns", "http client", "cleartext"],
    sections: [
      {
        title: "DNS",
        tags: ["dns", "lookup"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/net/dns

$ addrs = dns.lookup("localhost")`,
          },
        ],
      },
      {
        title: "Cleartext HTTP client",
        tags: ["http", "get", "tls"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/net/http_client
/ std/str

| http_client.get("127.0.0.1", 8080, "/health") {
    $ raw {
        io.print(str.from_bytes(raw))
    }
    ! e {
        io.print(e)
    }
}`,
          },
          {
            kind: "paragraph",
            text: [
              { code: "get(host, port, path)" },
              " opens a TCP connection, sends an HTTP/1.1 GET with ",
              { code: "Connection: close" },
              ", and returns the raw response as bytes. Use ",
              { code: "request" },
              " for other methods and bodies. ",
              { code: "get_tls" },
              " and ",
              { code: "request_tls" },
              " speak HTTPS via ",
              { code: "std/net/tls" },
              " (empty CA PEM uses platform trust roots).",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-tls",
    path: "/docs/std/tls",
    category: "Standard library",
    title: "TLS",
    summary: "TLS client and server sockets with PEM certificates or platform roots.",
    tags: ["std", "tls", "https"],
    aliases: ["stdlib tls", "ssl"],
    sections: [
      {
        title: "Client connect",
        tags: ["connect", "pem"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "std/net/tls.connect(host, port, server_name, ca_pem)" },
              " opens a TLS client connection. Pass a CA PEM string for a custom trust store, or an empty string to use platform roots. Read, write, and close live on connection methods. Pair with HTTP framing for HTTPS application traffic.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-std-cli",
    path: "/docs/std/cli",
    category: "Standard library",
    title: "CLI flags",
    summary: "Pure flag and option parsing over an argv list.",
    tags: ["std", "cli", "flags", "argv"],
    aliases: ["stdlib cli", "args parse"],
    sections: [
      {
        title: "Parse argv",
        tags: ["parse", "flags"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `/ std/cli

$ argv = ["tool", "--out", "a.txt", "in.echo"]
| cli.parse(argv) {
    $ t {
        | cli.get(t, "out") {
            $ v { io.print(v) }
            : { }
        }
    }
    ! e { io.print(e) }
}`,
          },
          {
            kind: "paragraph",
            text: [
              "Supports ",
              { code: "--name" },
              ", ",
              { code: "--name=value" },
              ", ",
              { code: "--name value" },
              ", short ",
              { code: "-x" },
              ", and ",
              { code: "--" },
              " end-of-options. Not a full getopt/GNU matrix.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-guide-packages",
    path: "/docs/guides/packages",
    category: "Guide",
    title: "Packages",
    summary: "Organize local modules, install pinned dependencies, and resolve host imports.",
    tags: ["guide", "package", "module", "xo get", "xo.toml", "dependency"],
    aliases: ["package manager", "dependencies", "package cache", "folder module"],
    sections: [
      {
        title: "Modules before packages",
        tags: ["module", "file", "folder", "local"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Local multi-file programs do not need package metadata. Import a sibling ",
              { code: "math.echo" },
              " file or a ",
              { code: "math/" },
              " folder with the same path. A folder module combines the exports of its sorted ",
              { code: "*.echo" },
              " files; private bindings remain file-local.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ ./math

$ total = math.add(20, 22)`,
          },
        ],
      },
      {
        title: "Install a dependency",
        tags: ["get", "git", "path", "version"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `xo get github.com/acme/lib@v1.2.3
xo get github.com/acme/lib@dev --path ../lib
xo get github.com/acme/lib@v1.2.3 --deps
xo home`,
          },
          {
            kind: "paragraph",
            text: [
              "Use an explicit tag, branch, or commit when reproducibility matters. ",
              { code: "--path" },
              " copies a local source tree into the same user package cache; ",
              { code: "--deps" },
              " recursively installs dependencies declared by that package. ",
              { code: "xo home" },
              " prints the active user root and package directory.",
            ],
          },
        ],
      },
      {
        title: "Pin project dependencies",
        tags: ["xo.toml", "pin", "auto-get", "cwd"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Place ",
              { code: "xo.toml" },
              " in the working directory used to run ",
              { code: "xo" },
              ". The tool reads its ",
              { code: "[dependencies]" },
              " table and automatically installs a missing pinned dependency during check, run, or build.",
            ],
          },
          {
            kind: "code",
            language: "shellscript",
            code: `[dependencies]
"github.com/acme/lib" = "v1.2.3"`,
          },
          {
            kind: "code",
            language: "echo",
            code: `/ github.com/acme/lib/math

$ answer = math.add(20, 22)`,
          },
        ],
      },
      {
        title: "Resolution rules",
        tags: ["resolve", "name", "cycle", "cache"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "An import binds its last path segment, so the example above binds ",
              { code: "math" },
              ". Echo has no import aliases or star imports. Two imports with the same final segment conflict, import cycles are errors, and external packages are never vendored into the project’s ",
              { code: ".xo/cache" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Troubleshoot resolution",
        tags: ["diagnostic", "graph", "res-import"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `xo check --graph main.echo
xo check --diag-codes main.echo
xo home`,
          },
          {
            kind: "paragraph",
            text: [
              { code: "res-import" },
              " means the target could not be resolved, ",
              { code: "res-import-name-conflict" },
              " means two imports want the same binding, and ",
              { code: "res-import-cycle" },
              " reports a cycle in the closed module graph.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-guide-diagnostics",
    path: "/docs/guides/diagnostics",
    category: "Guide",
    title: "Diagnostics",
    summary:
      "Read compiler locations and codes, isolate a failing stage, and inspect the module graph.",
    tags: ["guide", "diagnostics", "errors", "codes", "check", "debug"],
    aliases: ["compiler errors", "sem-unbound", "res-import", "parse-error", "diag codes"],
    sections: [
      {
        title: "Diagnostic shape",
        tags: ["severity", "code", "location", "message"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A diagnostic prints its severity, stable domain code, source location, and message. Codes identify the owning compiler stage: ",
              { code: "lex-*" },
              ", ",
              { code: "parse-*" },
              ", ",
              { code: "res-*" },
              ", ",
              { code: "sem-*" },
              ", or ",
              { code: "cg-*" },
              ".",
            ],
          },
          {
            kind: "code",
            language: "shellscript",
            code: "error[sem-unbound] src/main.echo:8:12: unbound name `total`",
          },
        ],
      },
      {
        title: "Normal workflow",
        tags: ["check", "format", "graph", "codes"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `xo fmt --check src/main.echo
xo check src/main.echo
xo check --graph src/main.echo
xo check --diag-codes src/main.echo
xo ir src/main.echo`,
          },
          {
            kind: "paragraph",
            text: [
              "Start with ",
              { code: "xo check" },
              " because it resolves the full import graph and runs semantics without linking a binary. ",
              { code: "--graph" },
              " prints resolved modules; ",
              { code: "--diag-codes" },
              " emits only codes for scripts and fixture-style assertions.",
            ],
          },
        ],
      },
      {
        title: "Isolate the stage",
        tags: ["lex", "ast", "ir", "pipeline"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `xo lex --kinds --diag-codes src/main.echo
xo ast --kinds --diag-codes src/main.echo
xo ir --diag-codes src/main.echo`,
          },
          {
            kind: "paragraph",
            text: [
              "If lexing fails, later stages have no trustworthy source structure. When AST output succeeds and check fails, focus on names, kinds, effects, and module rules. IR emission also exercises HIR, MIR, and LLVM code generation. The IR includes DWARF line locations from source spans and local-variable kinds from the checker.",
            ],
          },
        ],
      },
      {
        title: "Common semantic codes",
        tags: ["semantic", "unbound", "type", "effect", "task"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `sem-unbound           name is not available at this point
sem-shadow            name is introduced twice in one region
sem-immutable         write targets an immutable binding or field
sem-type-mismatch     incompatible kinds or numeric widths
sem-not-callable      call target is not a function value
sem-arity             argument count does not match
sem-unhandled-result  result must be matched with $ / ! arms
sem-unhandled-option  option must be matched with $ / : arms
sem-task-capture      capture does not name an existing binding
sem-task-arity        task call or capture list exceeds the ABI limit`,
          },
        ],
      },
      {
        title: "Common resolver codes",
        tags: ["resolver", "import", "export", "struct"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `res-entry                 entry file is missing or invalid
res-import                import cannot be resolved
res-import-name-conflict  two imports bind the same final segment
res-import-cycle          module graph contains a cycle
res-export-missing        exported name is not defined
res-runtime-forbidden     userland attempted / runtime
res-struct-dup-primary    multiple % declarations for one struct
res-struct-no-primary     @ members have no matching % declaration
res-struct-dup-member     merged struct members collide`,
          },
        ],
      },
      {
        title: "Cache checks",
        tags: ["cache", "no-cache", "doctor"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "When a local toolchain or std source has changed and output appears stale, bypass artifacts once with ",
              { code: "--no-cache" },
              " or inspect them with ",
              { code: "xo cache status" },
              " and ",
              { code: "xo cache doctor" },
              ". Cache cleaning should be a diagnosis step, not a substitute for fixing a source error.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-guide-repl",
    path: "/docs/guides/repl",
    category: "Guide",
    title: "REPL",
    summary: "Explore Echo through the shared compiler pipeline and in-process LLVM JIT.",
    tags: ["guide", "repl", "interactive", "jit", "session"],
    aliases: ["xo repl", "interactive shell", "eager eval", "meta commands"],
    sections: [
      {
        title: "Start a session",
        tags: ["start", "jit", "pipeline"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `xo repl`,
          },
          {
            kind: "paragraph",
            text: [
              "The REPL uses the same parser, semantic analysis, LLVM lowering, and runtime as ",
              { code: "xo run --jit" },
              ". Successful statements persist in the session and are compiled again with later input.",
            ],
          },
        ],
      },
      {
        title: "Evaluate and retain",
        tags: ["expression", "statement", "binding", "import"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `$ x = 40
x + 2

/ std/io
io.print('hello from the repl')`,
          },
          {
            kind: "paragraph",
            text: [
              "Bare expressions auto-display for ints, floats, bools, strings, and several heap shapes via ",
              { code: "str.from_debug" },
              ". Other kinds use ",
              { code: "io.print" },
              " or ",
              { code: "str.from_*" },
              ". Bindings, imports, structs, tasks, and other successful statements remain in the session until it is cleared. A ",
              { code: "+" },
              " spawn is kept even when it exits unjoined so a later ",
              { code: "-" },
              " can complete the pair.",
            ],
          },
        ],
      },
      {
        title: "Multi-line input",
        tags: ["block", "function", "brace"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Keep a block’s opening brace unmatched to continue on the next prompt. Evaluation begins when brace depth returns to zero.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `$ double = (value) {
    ^ value + value
}

double(21)`,
          },
        ],
      },
      {
        title: "Session commands",
        tags: ["help", "clear", "quit", "history"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `:help       # show help
:session    # print accumulated Echo source
:clear      # clear accumulated source
:quit       # leave (:exit and Ctrl-D also work)`,
          },
          {
            kind: "paragraph",
            text: [
              "Interactive history is stored under ",
              { code: "$XDG_STATE_HOME/xo/history" },
              " or ",
              { code: "~/.local/state/xo/history" },
              ". End-of-line hints preview integer expressions and complete meta commands or matching history entries.",
            ],
          },
        ],
      },
      {
        title: "Piped input",
        tags: ["stdin", "script", "non-interactive"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `printf '%s\n' '$ x = 1' 'x + 2' ':quit' | xo repl`,
          },
          {
            kind: "paragraph",
            text: [
              "When standard input is not a terminal, the REPL reads line-oriented input without the interactive editor. This is useful for reproducible smoke checks.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "docs-guide-cookbook",
    path: "/docs/guides/cookbook",
    category: "Guide",
    title: "Cookbook",
    summary:
      "Runnable starting points for language features, algorithms, networking, and applications.",
    tags: ["guide", "cookbook", "recipes", "examples", "run"],
    aliases: ["how to", "sample programs", "recipes", "demos"],
    sections: [
      {
        title: "Explore one feature",
        tags: ["misc", "language", "example"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "examples/misc" },
              " contains small programs with one primary idea. These are the quickest way to see a feature in a complete source file.",
            ],
          },
          {
            kind: "code",
            language: "shellscript",
            code: `xo run examples/misc/first_class_fn.echo
xo run examples/misc/eq_deep_id.echo
xo run examples/misc/match_type.echo
xo run examples/misc/method_chain.echo
xo run examples/misc/range.echo`,
          },
        ],
      },
      {
        title: "Run an algorithm",
        tags: ["algorithm", "algos", "data"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `xo run examples/algos/fibonacci.echo
xo run examples/algos/gcd.echo
xo run examples/algos/primes.echo
xo run examples/algos/sort.echo`,
          },
          {
            kind: "paragraph",
            text: [
              "Algorithm examples emphasize ordinary expressions, loops, lists, functions, and control flow without application infrastructure.",
            ],
          },
        ],
      },
      {
        title: "Inspect the compiler",
        tags: ["lex", "ast", "ir", "check"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `xo lex --kinds examples/misc/sum_list.echo
xo ast --kinds examples/misc/sum_list.echo
xo check --graph examples/misc/multi/main.echo
xo ir examples/misc/sum_list.echo`,
          },
        ],
      },
      {
        title: "Smoke TCP and UDP",
        tags: ["network", "tcp", "udp", "loopback"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `xo run echo26/run/net/001_tcp_loopback.echo
xo run echo26/run/net/002_udp_loopback.echo
xo run echo26/run/net/003_conn_methods.echo`,
          },
          {
            kind: "paragraph",
            text: [
              "These recipes bind fixed loopback ports. If a port is already occupied, choose a different high port in a copy of the source.",
            ],
          },
        ],
      },
      {
        title: "Run the HTTP application",
        tags: ["http", "app", "server", "curl"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "examples/app/main.echo" },
              " is a finite end-to-end smoke with modular routes and one live TCP exchange. ",
              { code: "server.echo" },
              " runs until interrupted.",
            ],
          },
          {
            kind: "code",
            language: "shellscript",
            code: `xo run --no-cache examples/app/main.echo

# terminal 1: long-running server
xo run --no-cache examples/app/server.echo

# terminal 2
curl -s http://127.0.0.1:8080/health`,
          },
        ],
      },
      {
        title: "Build a native binary",
        tags: ["build", "native", "optimization"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `xo check examples/misc/sum_list.echo
xo build -O 2 examples/misc/sum_list.echo -o /tmp/echo-sum
/tmp/echo-sum`,
          },
        ],
      },
    ],
  },

  // ── Book ───────────────────────────────────────────────────────────
  {
    id: "book",
    path: "/book",
    category: "Book",
    title: "Introduction",
    summary: "Why Echo looks this way, when to reach for each construct, and how to read programs.",
    tags: ["book", "language", "introduction"],
    aliases: ["language book", "echo book", "docs book", "the book"],
    sections: [
      {
        title: "Leaders and the file",
        tags: ["intent", "leaders"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Echo puts control and binding structure in a short set of leaders. The rest of each line stays an ordinary expression. After the glyphs become familiar, a scan starts at the leader. The denser line still reads like code you already know how to write.",
            ],
          },
          {
            kind: "paragraph",
            text: [
              "The file is the program: top-level runs in order. That choice keeps the language free of a reserved entry name.",
            ],
          },
        ],
      },
      {
        title: "A small program",
        tags: ["example", "runnable"],
        blocks: [
          {
            kind: "paragraph",
            text: ["A small program that uses leaders for bind, mutation, and iteration:"],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io

$ xs = [1, 2, 3]
~ sum = 0
* x : xs {
    ~ sum = sum + x
}
io.print("sum={sum}")`,
          },
        ],
      },
      {
        title: "How to read the Book",
        tags: ["guide"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Read left to right. Start with leaders and binds so the grammar settles. Then values and collections show how data is shared and compared. Control and result shapes govern flow. Later chapters cover modules, structs, and tasks when a program grows. Companion form sheets for each topic live under ",
              { code: "/docs" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Echo 2026",
        tags: ["echo 2026", "spec"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "These chapters are narrative for the ",
              { code: "Echo 2026" },
              " edition. Spec table of contents: ",
              { code: "/e26/spec" },
              ". Executable contract: ",
              { code: "echo26/" },
              " via ",
              { code: "e26" },
              ".",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "book-leaders",
    path: "/book/leaders",
    category: "Book",
    title: "Leaders",
    summary: "Why statement leaders exist, and how to read a line at a glance.",
    tags: ["book", "leaders", "syntax", "keywords"],
    aliases: ["no keywords", "statement leaders", "glyph"],
    sections: [
      {
        title: "Leaders at statement start",
        tags: ["why"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Statement leaders carry control and binding. The words on the rest of the line stay free for domain names: nothing is reserved for ",
              { code: "if" },
              ", ",
              { code: "for" },
              ", or ",
              { code: "return" },
              ". You learn a small glyph set once. Structure always begins at column zero of the statement.",
            ],
          },
        ],
      },
      {
        title: "How to read a line",
        tags: ["how"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "The first non-space character is the leader, or you are inside an expression. A space follows the leader (except bare ",
              { code: "<" },
              " / ",
              { code: ">" },
              "). ",
              { code: "{" },
              " opens on that same line. A glyph mid-expression is an operator; statement start decides leadership.",
            ],
          },
        ],
      },
      {
        title: "Table",
        tags: ["table", "reference"],
        blocks: [
          {
            kind: "code",
            language: "echo",
            code: `~ name = expr     ; mutable bind / reassign
$ name = expr     ; immutable bind
# NAME = expr     ; compile-time const (SCREAMING_SNAKE)
% struct_name { } ; struct shape
@ struct_name { } ; extra members (other files ok)
? expr { }        ; if
: expr { }        ; else-if
: { }             ; else / match default
! expr            ; return err (result)
^ expr            ; return
* { }             ; loop
* item : items { }; for-in
<                 ; break
>                 ; continue
| expr { arms }   ; match
+ call(args)      ; spawn task
- handle          ; join task
/ path            ; import
\\ name           ; export`,
          },
        ],
      },
      {
        title: "Dual-use glyphs",
        tags: ["operators", "dual"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Some characters are leaders only at statement start and operators in expressions. ",
              { code: "+" },
              " and ",
              { code: "-" },
              " schedule or join at statement start and do arithmetic in expressions. ",
              { code: "*" },
              " multiplies. ",
              { code: "/" },
              " divides. ",
              { code: "!" },
              " is prefix not. ",
              { code: "<" },
              " / ",
              { code: ">" },
              " compare. The same rule covers ",
              { code: "%" },
              " and ",
              { code: "|" },
              ". Position decides the role.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "book-binds",
    path: "/book/binds",
    category: "Book",
    title: "Binds and functions",
    summary: "When to pick $, ~, or #, and how free functions work as values.",
    tags: ["book", "bind", "functions", "const"],
    aliases: ["dollar", "tilde", "hash", "free functions"],
    sections: [
      {
        title: "Choosing a bind",
        tags: ["why"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Most names should stay fixed. ",
              { code: "$" },
              " makes that the default. ",
              { code: "~" },
              " is for loops, accumulators, and reassigned handlers. ",
              { code: "#" },
              " is for values that must exist before runtime: table sizes, version strings, pure chains of other constants. If a “const” needs a function call, bind it at runtime with ",
              { code: "$" },
              " or ",
              { code: "~" },
              ", not ",
              { code: "#" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Bind leaders",
        tags: ["immutable", "mutable", "const"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "$" },
              " is immutable at runtime. ",
              { code: "~" },
              " is mutable (",
              { code: "~ name =" },
              " updates). ",
              { code: "#" },
              " is compile-time only: literals and ops on other ",
              { code: "#" },
              " names (including list, range, and struct lits, plus field and index). Runtime calls are outside constant expressions.",
            ],
          },
        ],
      },
      {
        title: "Functions are values",
        tags: ["function", "return"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A free function is a bind whose value is a function expression. ",
              { code: "^" },
              " returns from the current function. Bare ",
              { code: "^" },
              " is none in option-shaped functions.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
/ std/str

# A = 21
# B = A + A

$ add = (a, b) {
    ^ a + b
}

io.print(str.from_int(B))
io.print(str.from_int(add(20, 22)))`,
          },
        ],
      },
      {
        title: "Compose with callables",
        tags: ["first-class", "higher-order"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Because a function is a value, higher-order code needs no separate declaration or interface. Pass the callable as an argument and call the parameter normally. Methods stay members invoked through a receiver.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `$ apply = (f, value) {
    ^ f(value)
}

$ increment = (value) {
    ^ value + 1
}

$ answer = apply(increment, 41)`,
          },
        ],
      },
      {
        title: "Names and shadowing",
        tags: ["scope", "program"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Names are introduced once per region. Shadowing is rejected. Rebind mutables with ",
              { code: "~" },
              ".",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "book-values",
    path: "/book/values",
    category: "Book",
    title: "Values and operators",
    summary: "How Echo separates value copying, shared objects, deep equality, and identity.",
    tags: ["book", "values", "operators", "equality", "reference"],
    aliases: ["data model", "copy", "identity", "numbers", "precedence"],
    sections: [
      {
        title: "A small value model",
        tags: ["value", "reference", "copy"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Every bind and parameter copies its binding. For numbers, booleans, strings, and other value kinds, that means an independent value. For a struct or list, it means another reference to the same object. There is no user-visible pointer type between the two classes.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `~ number = 3
~ number_copy = number
~ number_copy = 4       ; number is still 3

$ items = [10]
$ items_alias = items
~ items_alias[0] = 11   ; items[0] is now 11`,
          },
        ],
      },
      {
        title: "Content or identity",
        tags: ["deep", "identity", "equality"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Most comparisons ask about meaning, so ",
              { code: "==" },
              " is deep: two separately created lists can be equal. Use ",
              { code: "===" },
              " when object identity matters. For value kinds, identity adds nothing and agrees with deep equality.",
            ],
          },
        ],
      },
      {
        title: "Numbers stay explicit",
        tags: ["number", "width", "conversion"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Integers and floats do not silently mix, and explicit numeric widths must agree. That keeps arithmetic predictable at native-code boundaries. Start with the inferred ",
              { code: "i64" },
              " and ",
              { code: "f64" },
              " defaults; add a width tag only when the representation matters.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `$ count = 42
$ ratio = 3.5
$ packed_count = <i32> 42
$ packed_ratio = <f32> 3.5`,
          },
        ],
      },
    ],
  },
  {
    id: "book-collections",
    path: "/book/collections",
    category: "Book",
    title: "Collections and ranges",
    summary:
      "Use lists for sequences, products for fields, and ranges for inclusive integer spans.",
    tags: ["book", "list", "product", "range", "collection"],
    aliases: ["array", "anonymous struct", "index", "iteration"],
    sections: [
      {
        title: "Sequences are lists",
        tags: ["list", "sequence", "mutation"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A list is Echo’s core ordered collection. Index it for direct access or iterate when order matters. Lists are shared objects, so mutation through a parameter or alias is deliberate and visible to the caller.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `$ bump_first = (items) {
    ~ items[0] = items[0] + 1
    ^ items[0]
}

$ values = [10, 20, 30]
$ first = bump_first(values)`,
          },
        ],
      },
      {
        title: "Products group fields",
        tags: ["product", "anonymous", "named"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "An anonymous product is useful for a small bundle returned or passed as one value. Reach for a named struct when the shape is part of the domain, needs defaults or methods, or must participate in a type match. Neither form is a general dictionary.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `$ coordinate = { x: 4, y: 7 }
$ horizontal = coordinate.x`,
          },
        ],
      },
      {
        title: "Ranges compose",
        tags: ["range", "loop", "match"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A range is an ordinary value. The same inclusive ",
              { code: "1..10" },
              " span can be bound, iterated, passed, or used for membership in a match arm.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "book-control",
    path: "/book/control",
    category: "Book",
    title: "Control",
    summary: "If, loops, break, continue, and match.",
    tags: ["book", "if", "loop", "match", "break", "continue"],
    aliases: ["condition", "for-in", "while", "control flow"],
    sections: [
      {
        title: "Condition chains",
        tags: ["if", "else"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "?" },
              " starts a chain. ",
              { code: ": expr { }" },
              " is else-if. ",
              { code: ": { }" },
              " is else.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io

$ n = 3
? n == 0 {
    io.print('zero')
}
: n == 1 {
    io.print('one')
}
: {
    io.print('many')
}`,
          },
        ],
      },
      {
        title: "Loops",
        tags: ["loop", "for-in", "while"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "* { }" },
              " loops forever. ",
              { code: "* cond { }" },
              " is while. ",
              { code: "* item : items { }" },
              " walks a list or inclusive range. ",
              { code: "<" },
              " breaks; ",
              { code: ">" },
              " continues.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
/ std/str

~ i = 0
* i < 3 {
    io.print(str.from_int(i))
    ~ i = i + 1
}

$ xs = [10, 20, 12]
~ sum = 0
* x : xs {
    ~ sum = sum + x
    ? x == 20 {
        <
    }
}
io.print(str.from_int(sum))`,
          },
        ],
      },
      {
        title: "Match",
        tags: ["match", "pipe"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "| expr { arms }" },
              " matches. Value arms compare deeply and may group alternatives with commas. Inclusive ranges test membership, and ",
              { code: "% name" },
              " selects a named struct type. Default is ",
              { code: ": { body }" },
              ". Result and option arms are covered on the next page.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "book-result-option",
    path: "/book/result-option",
    category: "Book",
    title: "Result and option",
    summary: "Why errors are values, and when match is required.",
    tags: ["book", "result", "option", "error", "match"],
    aliases: ["err", "ok", "some", "none", "bang"],
    sections: [
      {
        title: "Errors as data",
        tags: ["why"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Abort is for the impossible. Recoverable failure is data the caller must see. ",
              { code: "!" },
              " packages that failure as a result err. Required match keeps the bad path visible at compile time, matching the discipline used for option some/none arms.",
            ],
          },
        ],
      },
      {
        title: "Result",
        tags: ["result", "bang"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "!" },
              " returns an error payload from the current function and leaves the process running. Any ",
              { code: "!" },
              " in a body makes that function a result: ",
              { code: "^ v" },
              " is ok, ",
              { code: "! e" },
              " is err.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io
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
        ^ 1
    }
}`,
          },
        ],
      },
      {
        title: "Option",
        tags: ["option", "some", "none"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Option-shaped functions use ",
              { code: "^ value" },
              " for some and bare ",
              { code: "^" },
              " for none. Match with ",
              { code: "$ name {…}" },
              " and ",
              { code: ": {…}" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Must handle",
        tags: ["compile", "unhandled"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Leaving a result or option unhandled is a compile error. There is no silent discard and no separate “try” keyword.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "book-strings",
    path: "/book/strings",
    category: "Book",
    title: "Strings",
    summary: "Pure quotes, rich quotes, and interpolation. String + is unavailable.",
    tags: ["book", "string", "interpolation", "pure", "rich"],
    aliases: ["quotes", "interp", "concat"],
    sections: [
      {
        title: "Two kinds",
        tags: ["pure", "rich"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "'…'" },
              " is pure: no escapes, no interpolation, no interior single quote. ",
              { code: '"…"' },
              " is rich: locked escapes ",
              { code: '\\n \\t \\r \\\\ \\" \\{ \\} \\xHH' },
              " plus ",
              { code: "{name}" },
              " interpolation. Anything else is ",
              { code: "lex-escape" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Building strings",
        tags: ["concat"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "String ",
              { code: "+" },
              " is unavailable. Build text with rich strings. ",
              { code: "==" },
              " compares content.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `/ std/io

$ pure = 'hello pure'
$ n = 7
$ rich = "n={n}!"
io.print(pure)
io.print(rich)
? pure == 'hello pure' {
    io.print('eq ok')
}`,
          },
        ],
      },
    ],
  },
  {
    id: "book-modules",
    path: "/book/modules",
    category: "Book",
    title: "Modules and std",
    summary: "Module-scoped imports. Userland printing goes through std.",
    tags: ["book", "modules", "import", "export", "std", "io"],
    aliases: ["packages", "runtime", "print"],
    sections: [
      {
        title: "Import one name",
        tags: ["import", "module-scoped"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "/ path" },
              " binds a single name: the last path segment. Use exports as ",
              { code: "module.name" },
              ". There is no star-import and no dumping symbols into the importer’s globals.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `; app.echo: multi-file sketch (see examples/misc/multi)
/ std/io
/ std/str
/ ./lib

io.print(str.from_int(lib.add(20, 22)))
io.print(str.from_int(lib.answer))

; lib.echo
$ add = (a, b) {
    ^ a + b
}
$ answer = 42
\\ add, answer`,
          },
        ],
      },
      {
        title: "std and runtime",
        tags: ["std", "runtime"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "There is no free ",
              { code: "print" },
              ". Go through ",
              { code: "std" },
              " (for example ",
              { code: "io.print" },
              "). ",
              { code: "/ runtime" },
              " is allowed only inside the privileged std package; userland cannot import it. Imported functions keep their defining-module return kind (",
              { code: "str.from_int" },
              " is a string). Import parameters stay unspecialized so one call site does not freeze later argument kinds.",
            ],
          },
        ],
      },
      {
        title: "Grow beyond one file",
        tags: ["folder", "package", "dependency"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A module can be one Echo file or a folder of Echo files. Folder modules combine their files’ exports while keeping private names file-local. External host paths use the same import syntax after ",
              { code: "xo get" },
              " installs the package. A project only needs ",
              { code: "xo.toml" },
              " when it wants dependency pins.",
            ],
          },
        ],
      },
      {
        title: "Export",
        tags: ["export"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "\\ name" },
              " (or a list) marks what the module exposes. Importers only see those names under the module object.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "book-structs",
    path: "/book/structs",
    category: "Book",
    title: "Structs",
    summary: "% declares a shape. @ adds members. Methods use . for the receiver.",
    tags: ["book", "struct", "percent", "at", "methods"],
    aliases: ["shape", "receiver", "members"],
    sections: [
      {
        title: "Primary and extra",
        tags: ["percent", "at"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "%" },
              " is the primary shape (one per name). ",
              { code: "@" },
              " adds members, often in another file. Members use the same ",
              { code: "$" },
              " / ",
              { code: "~" },
              " / ",
              { code: "#" },
              " leaders as top level.",
            ],
          },
        ],
      },
      {
        title: "Receiver",
        tags: ["method", "dot"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Call ",
              { code: "u.greet()" },
              " and inside the method bare ",
              { code: "." },
              " is the receiver. Free functions do not get a receiver.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `% user {
    $ name
    ~ visits = 0

    $ greet = () {
        ^ "Hello, {.name}"
    }

    $ visit = () {
        ~ .visits = .visits + 1
        ^ .
    }
}

$ u = user {
    name: "Ada",
    visits: 0
}
u.greet()
u.visit()`,
          },
        ],
      },
      {
        title: "Sharing and chaining",
        tags: ["reference", "chain", "falloff"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Struct values are references, so passing a struct or binding it to another name shares one domain object. A plain method that falls off its body returns that receiver. Mutation methods can therefore read naturally as chains without a special builder type.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "book-tasks",
    path: "/book/tasks",
    category: "Book",
    title: "Tasks",
    summary: "Schedule ordinary functions or closed bodies, then join every task explicitly.",
    tags: ["book", "task", "spawn", "join", "concurrency"],
    aliases: ["event loop", "handle", "capture", "plus minus"],
    sections: [
      {
        title: "Concurrency stays visible",
        tags: ["spawn", "join", "structured"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "+" },
              " says work may run as a task; ",
              { code: "-" },
              " says where the program waits for it. Scheduling and synchronization stay visible at statement boundaries. Functions keep ordinary call syntax.",
            ],
          },
          {
            kind: "code",
            language: "echo",
            code: `$ calculate = (input) {
    ^ input + 1
}

+ job = calculate(41)
; do independent work here
- answer = job`,
          },
        ],
      },
      {
        title: "Prefer ordinary functions",
        tags: ["function", "body", "capture"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Spawn an ordinary free-function call when inputs already describe the work. Use a task body for a small local operation, and list every outer value it needs in ",
              { code: "[captures]" },
              ". Explicit captures keep shared state reviewable.",
            ],
          },
        ],
      },
      {
        title: "Join what you spawn",
        tags: ["lifecycle", "error", "immediate"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "A task handle is an obligation: every spawn must be joined before process exit. When no overlap is needed, an immediate ",
              { code: "- { body }" },
              " block performs the schedule and join together. Task results use the ordinary plain, option, and result return shapes.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "book-names",
    path: "/book/names",
    category: "Book",
    title: "Names and layout",
    summary: "snake_case, lowercase structs, ; comments, one construct per line.",
    tags: ["book", "naming", "comments", "layout"],
    aliases: ["snake_case", "semicolon", "style"],
    sections: [
      {
        title: "Naming",
        tags: ["identifiers"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Identifiers are ASCII ",
              { code: "snake_case" },
              ". Struct names are lowercase (",
              { code: "user" },
              ", not ",
              { code: "User" },
              "). ",
              { code: "#" },
              " constants are ",
              { code: "SCREAMING_SNAKE" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Comments and layout",
        tags: ["comments", "blocks"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: ";" },
              " comments run to end of line. Prefer one construct per line. Multi-line structure uses ",
              { code: "{ }" },
              " blocks with ",
              { code: "{" },
              " on the introducer line.",
            ],
          },
        ],
      },
      {
        title: "Kinds",
        tags: ["types", "inference"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Kinds are inferred. There are no colon ascriptions and no generics surface. Numeric width tags on literals only: ",
              { code: "<i32>42" },
              ", ",
              { code: "<f64>3.14" },
              ". Imported functions use the defining module’s return kind when it is known; import parameters stay open.",
            ],
          },
        ],
      },
    ],
  },

  // ── Echo 2026 ──────────────────────────────────────────────────────
  {
    id: "e26",
    path: "/e26",
    category: "Echo 2026",
    title: "Echo 2026",
    summary:
      "Language edition and canonical public Language Spec. The suite under echo26/ is the executable contract.",
    tags: ["echo 2026", "e26", "echo26", "language spec", "edition", "fixtures", "suite"],
    aliases: ["e26", "echo26", "fixture suite", "language tests", "canonical spec", "2026"],
    sections: [
      {
        title: "What Echo 2026 is",
        tags: ["edition", "spec"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "Echo 2026" },
              " is this language edition: the name of the surface you implement and ship against today. The ",
              { code: "canonical public Language Spec" },
              " for the edition lives here on the site. Form-by-form rules are published under ",
              { code: "/docs" },
              ". Narrative chapters are at ",
              { code: "/book" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Executable contract",
        tags: ["policy", "proof", "echo26"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "echo26/" },
              " holds small programs and expected artifacts: the machine-checked contract of Echo 2026. The runner CLI is ",
              { code: "e26" },
              " (short tooling name); point it at ",
              { code: "xo" },
              " or any compatible candidate binary. Language work keeps the suite green, next to crate tests and ",
              { code: "examples/" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Pages",
        tags: ["nav"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "Language Spec" },
              ": table of contents for public rules (Reference pages and suite areas). ",
              { code: "Run" },
              ": invoke the suite. ",
              { code: "Layout" },
              ": fixture file naming. ",
              { code: "Protocol" },
              ": what a candidate binary must answer.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "e26-spec",
    path: "/e26/spec",
    category: "Echo 2026",
    title: "Language Spec",
    summary:
      "Table of contents for the Echo 2026 public Language Spec: form-by-form Reference pages and the executable suite map.",
    tags: ["echo 2026", "language spec", "toc", "reference", "canonical"],
    aliases: ["spec", "specification", "toc", "table of contents", "language law"],
    sections: [
      {
        title: "How to read the Spec",
        tags: ["authority", "reading"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "Echo 2026" },
              " is the language edition. Public law combines the form-by-form pages under ",
              { code: "/docs" },
              " with the machine-checked ",
              { code: "echo26/" },
              " suite. This page indexes both.",
            ],
          },
        ],
      },
      {
        title: "Language surface",
        tags: ["reference", "toc"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Form sheets for the Echo 2026 surface. Each path below states exact rules for that form.",
            ],
          },
          {
            kind: "paragraph",
            text: [
              { code: "/docs/leaders" },
              ": statement leaders. ",
              { code: "/docs/binds" },
              ": binds and functions. ",
              { code: "/docs/values" },
              ": values and operators. ",
              { code: "/docs/collections" },
              ": lists and ranges. ",
              { code: "/docs/control" },
              ": branches and loops. ",
              { code: "/docs/result-option" },
              ": Result and option. ",
              { code: "/docs/strings" },
              ": strings. ",
              { code: "/docs/modules" },
              ": modules and std. ",
              { code: "/docs/structs" },
              ": structs. ",
              { code: "/docs/tasks" },
              ": tasks. ",
              { code: "/docs/memory" },
              ": memory model. ",
              { code: "/docs/names" },
              ": names and layout.",
            ],
          },
        ],
      },
      {
        title: "Standard library",
        tags: ["std", "toc"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "/docs/std" },
              ": overview. ",
              { code: "/docs/std/reference" },
              ": full export index. Per-module pages such as ",
              { code: "/docs/std/io" },
              " and ",
              { code: "/docs/std/net-tcp" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Guides and toolchain",
        tags: ["guides", "toolchain"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "/install" },
              ": get ",
              { code: "xo" },
              ". ",
              { code: "/docs/first-program" },
              ": first runnable program. ",
              { code: "/docs/project" },
              ": project setup. ",
              { code: "/docs/guides/packages" },
              " · ",
              { code: "/docs/guides/diagnostics" },
              " · ",
              { code: "/docs/guides/repl" },
              " · ",
              { code: "/docs/guides/cookbook" },
              ". ",
              { code: "/docs/toolchain" },
              ": CLI surface.",
            ],
          },
        ],
      },
      {
        title: "Executable contract (suite areas)",
        tags: ["echo26", "suite", "conformance"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Under ",
              { code: "echo26/" },
              " in the repository. Drive with ",
              { code: "e26 --binary <candidate>" },
              ". See ",
              { code: "/e26/run" },
              ", ",
              { code: "/e26/layout" },
              ", and ",
              { code: "/e26/protocol" },
              ".",
            ],
          },
          {
            kind: "paragraph",
            text: [
              { code: "leaders/" },
              " · ",
              { code: "parse/" },
              " · ",
              { code: "check/" },
              " · ",
              { code: "infer/" },
              " · ",
              { code: "effect/" },
              " · ",
              { code: "lits/" },
              " · ",
              { code: "multi/" },
              " · ",
              { code: "run/" },
              " (including ",
              { code: "run/task" },
              ", ",
              { code: "run/net" },
              ", ",
              { code: "run/http" },
              ").",
            ],
          },
        ],
      },
      {
        title: "Book map",
        tags: ["book"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Narrative chapters mirror the Reference: ",
              { code: "/book" },
              " introduction, then leaders, binds, values, collections, control, result-option, strings, modules, structs, tasks, names.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "e26-run",
    path: "/e26/run",
    category: "Echo 2026",
    title: "Run",
    summary: "Build the e26 runner and point it at a candidate binary (Echo 2026 suite).",
    tags: ["echo 2026", "e26", "cli", "run"],
    aliases: ["how to run", "update fixtures", "conformance"],
    sections: [
      {
        title: "Commands",
        tags: ["commands"],
        blocks: [
          {
            kind: "code",
            language: "shellscript",
            code: `cargo build -p xo -p e26
cargo run -p e26 -- --binary target/debug/xo

e26 --binary /path/to/my-echo
e26 --binary target/debug/xo --filter leaders/bind
e26 --binary target/debug/xo --update`,
          },
        ],
      },
      {
        title: "Update",
        tags: ["update"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "--update" },
              " refreshes expectations from a known-good binary. Use it after intentional language changes. Leave unexpected failures red until the suite or the language is fixed.",
            ],
          },
        ],
      },
      {
        title: "Related",
        tags: ["nav"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Spec TOC: ",
              { code: "/e26/spec" },
              ". Layout: ",
              { code: "/e26/layout" },
              ". Protocol: ",
              { code: "/e26/protocol" },
              ". Form sheets: ",
              { code: "/docs" },
              ".",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "e26-layout",
    path: "/e26/layout",
    category: "Echo 2026",
    title: "Layout",
    summary: "Numbered roots and sidecar expectation files under echo26/.",
    tags: ["echo 2026", "e26", "layout", "echo26"],
    aliases: ["fixture layout", "directory structure"],
    sections: [
      {
        title: "Paths",
        tags: ["paths"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Under ",
              { code: "echo26/<area>/<feature>/" },
              ". Numbered ",
              { code: "NNN_*.echo" },
              " files are suite roots; other ",
              { code: ".echo" },
              " files are imports only.",
            ],
          },
          {
            kind: "code",
            language: "shellscript",
            code: `NNN_slug.echo     # source
NNN_slug.lex      # token kinds (required)
NNN_slug.ast      # AST kinds (required)
NNN_slug.diag     # optional lex diags
NNN_slug.check    # optional sem-* diags
NNN_slug.run      # optional stdout
NNN_slug.runexit  # optional exit code`,
          },
        ],
      },
      {
        title: "Areas",
        tags: ["areas"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "leaders" },
              ", ",
              { code: "parse" },
              ", ",
              { code: "check" },
              ", ",
              { code: "multi" },
              ", ",
              { code: "run" },
              ", ",
              { code: "lits" },
              ", ",
              { code: "infer" },
              ", ",
              { code: "effect" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Related",
        tags: ["nav"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Spec suite map: ",
              { code: "/e26/spec" },
              " (Executable contract). Protocol: ",
              { code: "/e26/protocol" },
              ". Run: ",
              { code: "/e26/run" },
              ".",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "e26-protocol",
    path: "/e26/protocol",
    category: "Echo 2026",
    title: "Protocol",
    summary: "Commands a candidate binary must answer for Echo 2026 conformance.",
    tags: ["echo 2026", "e26", "protocol", "binary"],
    aliases: ["candidate binary", "interface"],
    sections: [
      {
        title: "Stages",
        tags: ["lex", "ast", "check", "run"],
        blocks: [
          {
            kind: "paragraph",
            text: [{ code: "e26" }, " compares stdout and stderr to the fixture sidecars:"],
          },
          {
            kind: "code",
            language: "shellscript",
            code: `$binary lex --kinds --diag-codes path.echo
  stdout → .lex    stderr → .diag

$binary ast --kinds --diag-codes path.echo
  stdout → .ast

$binary check --diag-codes path.echo
  stderr → .check   # sem-* only; omit if none

$binary run path.echo
  stdout → .run     exit → .runexit`,
          },
        ],
      },
      {
        title: "Related",
        tags: ["nav"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "Edition Spec: ",
              { code: "/e26/spec" },
              ". Layout: ",
              { code: "/e26/layout" },
              ". Run: ",
              { code: "/e26/run" },
              ". Reference forms: ",
              { code: "/docs" },
              ".",
            ],
          },
        ],
      },
    ],
  },

  // ── Toolchain ──────────────────────────────────────────────────────
  {
    id: "toolchain",
    path: "/docs/toolchain",
    category: "Toolchain",
    title: "Toolchain",
    summary: "xo is the CLI: run, build, check, and stage dumps.",
    tags: ["xo", "cli", "toolchain"],
    aliases: ["xo commands", "compiler tools"],
    sections: [
      {
        title: "xo",
        tags: ["xo"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              "The ",
              { code: "xo" },
              " CLI drives the same compiler pipeline used by editor and analysis tooling. The everyday loop is ",
              { code: "xo check" },
              ", ",
              { code: "xo run" },
              ", and ",
              { code: "xo build" },
              ".",
            ],
          },
        ],
      },
      {
        title: "Pages",
        tags: ["nav"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "Commands" },
              ": the surface. ",
              { code: "Examples" },
              ": samples that run today.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "toolchain-commands",
    path: "/docs/toolchain/commands",
    category: "Toolchain",
    title: "Commands",
    summary: "Check, format, inspect, run, build, test, and manage an Echo project.",
    tags: ["xo", "cli", "commands"],
    aliases: ["command list", "help"],
    sections: [
      {
        title: "Surface",
        tags: ["list"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "run" },
              " compiles and executes through one clang ",
              { code: "-O0 -g" },
              " link and ",
              { code: "libecho_runtime" },
              ". ",
              { code: "--jit" },
              " executes the same LLVM IR in-process. Debug metadata carries per-statement source lines and checker kind labels for locals. Commands that analyze a program take its entry file and resolve the closed import graph from there.",
            ],
          },
          {
            kind: "code",
            language: "shellscript",
            code: `xo check <entry.echo>
xo fmt <file.echo>
xo lex <file.echo>
xo ast <file.echo>
xo ir <entry.echo>
xo run [--jit] [-O <level>] <entry.echo> [args...]
xo build [-O <level>] [--static] [--image oci:<dir>] <entry.echo> -o <out>
xo image push <layout> <host/name:tag>
xo test <path>
xo repl
xo lsp
xo get <package[@version]> [--deps]
xo home
xo cache status|clean|gc|doctor
xo index scan [roots...]
xo tools grammar tree-sitter --output <dir>`,
          },
        ],
      },
      {
        title: "Optimization",
        tags: ["optimization", "build", "run"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "-O" },
              " accepts ",
              { code: "0" },
              ", ",
              { code: "1" },
              ", ",
              { code: "2" },
              ", ",
              { code: "3" },
              ", or ",
              { code: "z" },
              ". Both AOT run and build default to level 0.",
            ],
          },
        ],
      },
      {
        title: "Images",
        tags: ["build", "oci", "static"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "xo build --static" },
              " links a fully static ELF (no dynamic loader). ",
              { code: "xo build --image oci:<dir>" },
              " does the same and writes a scratch OCI layout with ",
              { code: "/app" },
              " as the entrypoint. ",
              { code: "xo image push" },
              " uploads that layout. Auth is ",
              { code: "XO_REGISTRY_USER" },
              " and ",
              { code: "XO_REGISTRY_PASSWORD" },
              ".",
            ],
          },
          {
            kind: "code",
            language: "shellscript",
            code: `xo build --static main.echo -o app
xo build --image oci:./image main.echo -o app
xo image push oci:./image registry.example.com/acme/app:dev`,
          },
        ],
      },
      {
        title: "Gates",
        tags: ["gate", "testing"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "scripts/gate changed" },
              " for a tight loop. ",
              { code: "scripts/gate workspace" },
              " before broad commits. ",
              { code: "scripts/gate web" },
              " for this site. Language changes should keep the Echo 2026 suite (",
              { code: "e26" },
              ") green.",
            ],
          },
        ],
      },
    ],
  },
  {
    id: "toolchain-examples",
    path: "/docs/toolchain/examples",
    category: "Toolchain",
    title: "Examples",
    summary: "Small programs under examples/ that exercise the toolchain.",
    tags: ["examples", "misc", "run"],
    aliases: ["sample programs", "hello"],
    sections: [
      {
        title: "Trees",
        tags: ["paths"],
        blocks: [
          {
            kind: "paragraph",
            text: [
              { code: "examples/misc/" },
              " contains focused programs for core syntax, functions, equality, ranges, structs, and multi-file imports. ",
              { code: "examples/app/" },
              " demonstrates a larger modular shape. ",
              { code: "examples/algos/" },
              " collects familiar algorithms.",
            ],
          },
          {
            kind: "code",
            language: "shellscript",
            code: `./target/debug/xo run examples/misc/hello.echo
./target/debug/xo run --jit examples/misc/sum_list.echo
./target/debug/xo run examples/misc/eq_deep_id.echo
./target/debug/xo run examples/misc/range.echo
./target/debug/xo run examples/misc/multi/main.echo
./target/debug/xo run examples/algos/fibonacci.echo`,
          },
        ],
      },
    ],
  },
];

const stdOverviewPage = docsPagesBase.find((p) => p.path === "/docs/std");
const stdReferenceIndexPage: DocsPage = {
  id: "docs-std-reference",
  path: "/docs/std/reference",
  category: "Standard library",
  title: "API reference",
  summary: `Complete index of ${stdExportCount} public exports across ${stdModules.length} standard-library modules.`,
  tags: [
    "std",
    "api",
    "reference",
    "exports",
    "index",
    ...stdModules.flatMap((m) => m.exports.map((e) => e.name)),
  ],
  aliases: [
    "stdlib api",
    "full standard library",
    "export index",
    "every function",
    ...stdModules.flatMap((m) => m.exports.map((e) => e.call)),
  ],
  sections: [
    {
      title: "How to read this index",
      tags: ["exports"],
      blocks: [
        {
          kind: "paragraph",
          text: [
            "Each row is a public export from a ",
            { code: "std/" },
            " module. Import the module, then call ",
            { code: "module.export(...)" },
            ". Open the module page for the package outline (constants, structs, methods, functions). Private test helpers are not included.",
          ],
        },
      ],
    },
    ...stdGroups.map((group) => ({
      title: group.title,
      tags: [
        "std",
        group.title.toLowerCase(),
        ...group.modules.flatMap((m) => m.exports.map((e) => e.name)),
      ],
      aliases: group.modules.flatMap((m) => [
        m.path,
        ...m.exports.map((e) => e.call),
        ...m.exports.map((e) => e.name),
      ]),
      blocks: [
        {
          kind: "paragraph" as const,
          text: [
            "Modules: ",
            group.modules.map((m) => `${m.title} (${m.docsPath})`).join("; "),
            ".",
          ],
        },
        {
          kind: "code" as const,
          language: "echo" as const,
          code: group.modules
            .map((m) => {
              const listing = m.exports.map((e) => `  ${e.call}: ${e.role}`).join("\n");
              return `; ${m.path} → ${m.docsPath}\n${stdImportLine(m)}\n${listing}`;
            })
            .join("\n\n"),
        },
      ],
    })),
  ],
};

const stdApiPages: DocsPage[] = stdModules.map(stdModulePage);

/** Prefer generated complete std API pages over older partial std articles. */
export const docsPages: DocsPage[] = [
  ...docsPagesBase.filter((p) => !p.path.startsWith("/docs/std")),
  ...(stdOverviewPage ? [stdOverviewPage] : []),
  stdReferenceIndexPage,
  ...stdApiPages,
];

export const docsPageByPath = new Map(docsPages.map((page) => [page.path, page]));

/** All navigable docs paths in left-nav order (for prev/next and route generation). */
export function flattenNavPaths(navigation: DocsNavGroup[] = docsNavigation): string[] {
  const paths: string[] = [];

  function walk(links: DocsNavLink[]) {
    for (const link of links) {
      if (!link.disabled) {
        paths.push(link.to);
      }
      if (link.children) {
        walk(link.children);
      }
    }
  }

  for (const group of navigation) {
    walk(group.links);
  }

  return paths;
}

export function textPartText(part: DocsTextPart) {
  return typeof part === "string" ? part : part.code;
}
