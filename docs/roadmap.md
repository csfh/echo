# Language roadmap

Status of the **entire Echo language surface** and how it lands in the
toolchain. Spec source of truth: [`syntax.md`](syntax.md). Vertical checklist:
[`implementation.md`](implementation.md). Pipeline ownership:
[`architecture.md`](architecture.md).

| | |
|--|--|
| **Surface status** | **Core locked** — ready for frontend implementation |
| **Implementation** | Language: execute verticals ongoing · **Infra: fingerprint/cache/build v0** |
| **Exercise sources** | [`examples/`](../examples/), [`std/`](../std/), [`echo26/`](../echo26/) |
| **CLI** | `xo lex` · `ast` · `check` · `run` · `build` · `cache` · `e26 --binary …` |

**Legend**

| Tag | Meaning |
|-----|---------|
| **Locked** | Design frozen enough to implement; change needs an explicit decision |
| **Specified** | Written in `syntax.md` (or another domain doc) with workable rules |
| **Stub** | Sample / shape only (`std/`, layer docs empty or “not started”) |
| **Open** | Deferred or intentionally incomplete for v0 |
| **Impl** | Compiler / runtime work status for that area |

---

## 1. Design principles (locked)

1. **No English keywords** for control, binding, or definitions.
2. **Statement leaders** — single character at statement start; dual-use with
   expression operators by **position**.
3. **Whitespace after leader** required (except bare `<` / `>`).
4. **`{` on the same line** as its introducer; no `\` line continuation.
5. **Small core**; compose for depth (functions as values, members as values).
6. **Standalone language** — not a PHP superset ([ADR 0008](adr/0008-standalone-language-not-php-superset.md)).
7. **One shared pipeline** for CLI, LSP, fmt, and codegen ([ADR 0001](adr/0001-shared-compiler-pipeline.md)).
8. Naming: **snake_case**; **struct names lowercase**; `#` names **SCREAMING_SNAKE**.
9. **Kinds inferred by default** — no colon-ascription, no generics surface.
   Numeric **width tags** are prefix-only (`<i32>42`); defaults i64/f64
   (`docs/semantics.md`).

Compiler platform ADRs **0001–0010** still apply (LLVM-only backend, AST
source-shaped, Rust runtime owns executable semantics, closed compilation
graph, structured diagnostics, docs vs www, full verticals, platform baseline).

---

## Help wanted (broad scope)

Outside contributions are welcome under [`CONTRIBUTING.md`](../CONTRIBUTING.md)
and [`CLA.md`](../CLA.md). Useful work spans the whole tree — pick any area
below and open an issue or PR. Prefer **small vertical slices** with crate
tests + `echo26` + docs when behavior changes.

| Area | Where to look | Example contributions |
|------|---------------|------------------------|
| Language surface | `docs/syntax.md`, `docs/semantics.md`, `echo26/` | Spec gaps, fixtures, reject-path tests |
| Frontend | `echo_lexer`, `echo_parser`, `echo_semantics` | Parse/check bugs, clearer diagnostics |
| IR / backend | `echo_hir`, `echo_mir`, `echo_codegen` | Lowering, optimisations, ABI notes |
| Runtime | `echo_runtime`, ADR 0013/0016 | Tasks, memory, host edge cases |
| Standard library | `std/`, `docs/stdlib.md` | APIs, docs, examples; label `std` |
| Tooling | `xo`, `echo_lsp`, `e26`, `scripts/` | CLI UX, LSP features, gate/CI |
| Website | `www/` | Spec/reference accuracy, samples |
| Docs | `docs/`, `examples/` | Roadmap honesty, runnable demos |

Issue labels for triage: **`lexer`**, **`std`**. File bugs and proposals via
GitHub issues.

---

## 2. Full language surface map

Everything a programmer can write in Echo v0. Status is **design** status unless
noted under **Impl**.

### 2.1 Statement leaders

| Leader | Role | Design | Impl |
|--------|------|--------|------|
| `~` | Mutable bind / reassign / field & index assign | Locked | Lex+Parse ✓ |
| `$` | Immutable runtime bind (incl. free functions as values) | Locked | Lex+Parse ✓ |
| `#` | Compile-time constant (SCREAMING_SNAKE) | Locked | Lex+Parse ✓ |
| `%` | Struct **shape** (fields + optional members); **match type arm** | Locked | Lex+Parse+Run ✓ |
| `@` | **Extra members** for a struct (often other files) | Locked | Lex+Parse ✓ |
| `?` | If | Locked | Lex+Parse ✓ |
| `:` | Else-if / else / match default | Locked | Lex+Parse ✓ |
| `!` | Error return (Result err) | Locked (direction) | Lex+Parse ✓ |
| `^` | Return (`^ expr` or bare `^`) | Locked | Lex+Parse ✓ |
| `*` | Loop (infinite / while / for-each) | Locked | Lex+Parse ✓ |
| `<` | Break | Locked | Lex+Parse ✓ |
| `>` | Continue | Locked | Lex+Parse ✓ |
| `\|` | Match | Locked | Lex+Parse+Run ✓ (value + `% type` + result/option) |
| `+` | **Task spawn** — schedule body/call on **mio** event loop immediately | Locked (ADR 0013) | Lex+Parse+Run ✓ |
| `-` | **Task join / immediate block** — wait for task or block | Locked (ADR 0013) | Lex+Parse+Run ✓ |
| `&` | **Effect block** — auto-unwrap result/option; short-circuit on fail | Locked | Lex+Parse+Run ✓ (`echo26/run/effect`, `check/effect`, `leaders/effect`) |
| `/` | Import | Locked | Lex+Parse ✓ |
| `\` | Export | Locked | Lex+Parse ✓ |

**Task leaders (ADR 0013):** `+ { }` / `+ name = { }` / `+ call(…)` spawn;
`- { }` / `- name = { }` immediate block (schedule+join); `- handle` /
`- name = handle` join handle. **No `std/task`.** Event loop in `echo_runtime`
(not a separate crate; same ownership as `echo-php-old` `sched`/`poll`/`task`).

**Not leaders (by design):** bare call statements for side effects (`log(x)`,
`u.greet()`); free functions are **not** introduced with `@`.

### 2.2 Structs and members

| Topic | Design | Notes |
|-------|--------|-------|
| `% struct_name { … }` | Locked | One `%` per `struct_name` per package/program |
| `@ struct_name { … }` | Locked | Many `@`; merge with `%`; duplicate members = hard error |
| Multi-file split | Locked | `@` may live in other files; std keeps one file per shape for now |
| Members `$` / `~` / `#` | Locked | Data **or** function values |
| Tagged lit `user { k: v }` | Locked | Type name before `{` |
| Structural `{ k: v }` | Locked | Map/object, not a named struct |
| Method call `value.member()` | Locked | Binds implicit receiver |
| Receiver `.` | Locked | Only inside activation entered via method call |
| Free call `fn()` | Locked | No receiver; bare `.` illegal |
| Nested closures + `.` | Locked (v0) | Only **direct** method calls bind `.` |

### 2.3 Bindings, scoping, const

| Topic | Design | Notes |
|-------|--------|-------|
| No shadowing | Locked | Name introduced once per region |
| `~ name =` updates | Locked | Re-intro of same name is error if not mutable update |
| `$` init once | Locked | Immutable after bind |
| `#` const-eval | Locked | Literals + ops on other `#` only; **no calls** |
| Multi-bind | Locked | Same line `~ a = 1, b = 2` / `$ x = 1, y = 2` (expanded to sequential binds) |
| Params / `* item` | Locked | Introductions |
| Functions closed | Locked | Params + locals + `#` + imports; methods + `.`; **no** outer `$`/`~` capture |
| Nested fn values | Locked | Nameless closed values; bind names the binding; body symbol `__n_*` |
| First-class fn values | Locked | Pass/rebind/call-through incl. result/option; methods not values |
| Bind before use | Locked | Name in scope only after its bind (`$ a = b + 4` then `$ b = 5` → unbound) |
| Value vs reference | Locked | **Ref** = struct + list (copy ref, share). **Value** = everything else (copy value). Always copy the binding. **No** bare pointer types: sockets are `% conn` / `% listener` struct fields; pass the **struct by ref**. See [`semantics.md`](semantics.md) § Value vs reference. |

### 2.4 Control flow

| Topic | Design | Notes |
|-------|--------|-------|
| `?` / `:` chain | Locked | Else-if and else forms |
| Match `\| expr { arms }` | Locked | Value arms `e1, e2 { }`; type arms `% Name { }`; default `: { }` |
| Match patterns | Locked (v0) | Value `==` multi-expr + range membership + **`% Type`** + default; Option/Result |
| Range `lo..hi` | Locked | Inclusive int range value; for-in + match membership |
| Loops `*` | Locked | `* { }` · `* cond { }` · `* item : items { }` |
| Break / continue | Locked | Bare `<` / `>` |
| Return `^` | Locked | |
| Panic `!` | Locked | Other failures abort until richer errors |

### 2.5 Expressions and operators

| Topic | Design | Notes |
|-------|--------|-------|
| Arithmetic `+ - * / %`, unary `-` | Locked | |
| Bitwise `& \| ^ << >>`, unary `~` | Locked | ints only; `>>` arithmetic; count masked |
| Comparison `== != === !== < > <= >=` | Locked | |
| Boolean `&&` `\|\|`, prefix `!` | Locked | |
| Deep vs identity equality | Locked | `==` / `!=` deep; `===` / `!==` identity |
| Member / method | Locked | `value.field` · `value.method()` |
| Index | Locked | `xs[i]` · `~ xs[i] = expr` |
| Call | Locked | Free and method forms |
| Function expr | Locked | `(a, b) { … }` · `() { … }` |
| Grouping | Locked | `(…)` |
| Precedence | Locked | unary → `*/%` → `+-` → `<<>>` → `..` → cmp → `&` → `^` → `\|` → `&&` → `\|\|` |

### 2.6 Dual-use glyphs

| Glyph | Leader | Expression |
|-------|--------|------------|
| `*` | loop | multiply |
| `<` `>` | break / continue | comparisons / shifts (`<<` `>>`) |
| `!` | error return (result err) / match err arm | prefix not |
| `\|` | match | true literal **or** bitwise OR |
| `^` | return | bitwise XOR |
| `~` | mutable bind | bitwise NOT |
| `/` | import | divide |
| `%` | struct shape **or** match type arm | remainder |
| `.` | — | field/method or method-body receiver |

Lexer/parser **must** disambiguate by statement position vs expression context.

### 2.7 Literals and collections

| Kind | Form | Design |
|------|------|--------|
| Pure string | `'…'` | Locked — no escapes, no interp, no interior `'` |
| Rich string | `"…"` | Locked — escapes + `{name}` |
| Integers | decimal, `0x`, `0b`, `_` | Locked |
| Floats | `3.14`, `1e-3` | Locked |
| Bool | `\|` true · `_` false | Locked |
| List | `[a, b, c]` | Locked |
| Object/map | `{ k: v }` | Locked |
| Struct lit | `name { k: v }` | Locked |
| Null | — | **No null** (locked) |
| Trailing commas | — | **Forbidden** (locked) |

### 2.8 Layout, comments, program structure

| Topic | Design |
|-------|--------|
| One construct per line (except multi-bind) | Locked |
| Multi-line only via `{ }` structure | Locked |
| Comments `;` to EOL | Locked |
| Top-level runs in order | Locked |
| Import `/ path` · export `\ name` | Locked |
| Paths `./…` and bare `std/…` | Locked (shape) |
| Package / module model | **Locked** (ADR 0014) | Folder = module; paths + URL; optional `xo.toml`; user `$XO_HOME/packages/<id>/<ver>/` |
| Memory reclamation | **Law locked** (ADR 0016); **graph promote landed** | Scope-owned dispose — not tracing GC ([`memory.md`](memory.md), www `/docs/memory`). **Graph promotion** (region evacuation); immediate free; e26 `run/lifetime` 001–012 |

### 2.9 Failure model (v0)

| Topic | Design |
|-------|--------|
| Error return `! expr` (result shape) | Locked — not panic |
| result / option as syntax-driven shapes only | Locked (no keywords, no user type) |
| Catch / recover beyond `\|` match | Open (post-v0) |

### 2.10 Tasks and event loop (ADR 0013)

| Topic | Design |
|-------|--------|
| Spawn `+` / join `-` leaders | **Locked** — language surface, not `std/task` |
| Event loop in `echo_runtime` | **Locked** — mio poller + task queue; no user import |
| Immediate block `- name = { … }` | **Locked** — schedule body, join, bind result |
| `+ name = { … }` | **Locked** — `name` is **task handle** |
| Async/await colors | **Out** — cooperative tasks + park, ordinary `^`/`!` |
| I/O park on sockets | **Done** — nonblocking + mio park; worker pool |

### 2.11 Intentionally out of core (v0)

Not part of the locked core surface (do not invent keywords or leaders for these
without a roadmap update):

- Generics / parametric polymorphism as surface sugar  
- Traits / interfaces as separate declaration leaders  
- Async / await syntax (colored functions)  
- Macros / hygiene system  
- Line continuation, null, trailing commas  
- English keywords for any of the above  
- Package manager as language core  
- `std/task` (or any std package) as spawn/join keyword substitute

---

## 3. Standard library and samples

| Area | Design | Sources | Impl |
|------|--------|---------|------|
| Layout / style | Locked (stdlib.md) | [`std/`](../std/) | Stub Echo sources |
| Std root + `/ runtime` (std only) | **Locked** | `stdlib.md` / `modules.md` | implement resolver gate + codegen |
| `std/io` | Thin | `runtime.print` (strings) | e26 + examples |
| `std/str` | Thin | from_int/float/bytes/… + len/cat | e26 |
| `std/time` | **Done** | `now_ms` / `sleep_ms` | suite |
| `std/process` | **Done** | args / env / exit / `run` (spawn+wait) | e26 `run/process` + `xo test` + `examples/misc/process.echo` |
| `std/fs` | **Done** | paths, whole-file, copy/rename, `% meta`, streaming `% file` | e26 `run/fs` + `xo test` + `examples/misc/fs.echo` |
| `std/net` (tcp/, udp/ folders) | **TCP/UDP I/O done** | `std/net/tcp/{conn,listener,socket}`, `udp/socket` | e26 `run/net` |
| `std/net/http` | parse + format + **serve** + `handle_connection` | `std/net/http.echo` | e26 `run/http` + app |
| App HTTP demo | Finite dispatch + live TCP smoke | [`examples/app/main.echo`](../examples/app/main.echo) | run |
| App HTTP server | Long-running `http.serve` | [`examples/app/server.echo`](../examples/app/server.echo) | manual |
| App surface tour | Surface exercise | [`examples/app/surface.echo`](../examples/app/surface.echo) | — |
| Multi-file `%` / `@` | Locked pattern | `examples/app/user*.echo` (std: single-file shapes) | — |

Runtime bridges (`echo_runtime` / `echo_std`) implement behavior; Echo sources
are the public API shape.

---

## 4. Modules and packages

| Topic | Design | Spec |
|-------|--------|------|
| Import / export syntax | Locked | `syntax.md` |
| Closed compilation graph | Locked (ADR) | [ADR 0006](adr/0006-closed-compilation-graph.md) |
| `%` / `@` merge across files | Locked | `syntax.md` |
| Module identity / optional package | **Locked** (ADR 0014) | [`modules.md`](modules.md) · store + `xo get` to implement |
| Reserved `std` | Specified in samples | `stdlib.md` |

Multi-file **syntax** is locked; **resolver policy details** fill in with
`echo_index` / `echo_resolver` implementation.

---

## 5. Toolchain delivery (product phases)

Language features ship as **verticals** ([`implementation.md`](implementation.md)
§4–6). Product order:

| Phase | Goal | Stop after | Language coverage |
|-------|------|------------|-------------------|
| **0** | Design | Spec + samples | **Done** for core surface |
| **1** | Frontend | lex + parse + `xo lex` / `xo ast` + fixtures | **§2.1 done** (lex + chumsky parse + e26) |
| **2** | Check | semantics + `xo check` | Scopes, no shadow, types of lits, `.` rules |
| **3** | Multi-file | index + resolver | `/` `\` · `%`/`@` merge · `examples/app/` + `std/` |
| **4** | Execute | MIR + LLVM AOT/JIT (`xo run` / `ir` / `build`) | **Active:** i64, lists, strings, structs/methods, result/option, multi-file; expand surface. **Opt ownership:** MIR = form/escape; LLVM = `-O0`…`-O3`/`-Oz` mid-end |
| **5** | Fmt | `xo fmt` | **v0 done** — shared AST pretty-print; `-w` write |
| **6** | LSP | diagnostics, tokens, goto, complete, format | **v0:** docs + diags; more features later |
| **7** | Cache / build | fingerprint + cache + plan | **v4:** parse + check + IR + AOT binary; IR keys include opt; LSP uses same cache |
| **8** | AOT polish | `xo build`, link, opts | **Opt levels landed**; runtime `.a` finder takes newest in-profile; missing-symbol hint to rebuild `echo_runtime` |
| **9** | www | Public language + std docs | User-facing mirror of locked surface |

**Next concrete steps** (priority order as of 2026-08-13):

Locked-language honesty campaign is **done**: width/cast, nested match
payloads, `&` call-through unwrap, `#` function values, reject-path e26
belt, and dispose of task/TCP/UDP/FS-file handles. Name-keyed demote stays
**off**.

Follow-on (not locked-language rows): SOTA G9. Locator classification is locked
(path vs URI: `scheme://` / leading `/` / else relative; no normalize).

Core surface through run is largely green; prefer full verticals over new
shortcuts.

### 5.1 Cross-cutting hotspots (every phase)

From `syntax.md` / `implementation.md` §5 — features that touch many layers:

- Statement leaders + dual-use glyphs  
- Leader whitespace + same-line `{`  
- `%` / `@` multi-file merge  
- Members as `$`/`~`/`#` (data or fn)  
- Receiver `.` only on method-call activation  
- No shadowing; `~` updates  
- Tagged struct lit vs structural `{}`  
- Bare call statements vs `!` error return  

- Free functions as values  
- Import `/` vs divide; export `\`  
- Pure vs rich strings  
- Deep `==` vs identity `===`  

---

## 6. Layer specs vs language surface

| Domain doc | Owns | Status relative to language |
|------------|------|------------------------------|
| [`syntax.md`](syntax.md) | User-facing surface | **Core locked** |
| [`lexer.md`](lexer.md) | Tokens / scanning | Spec in progress; must match dual-use rules |
| [`parser.md`](parser.md) | Parse / AST build | Active (chumsky; keep in sync) |
| [`semantics.md`](semantics.md) | Scopes, types, `.`, match | Active (scopes + result/option + infer) |
| [`modules.md`](modules.md) | Modules / packages / paths | **Locked + landed** ADR 0014 |
| [`hir.md`](hir.md) / [`mir.md`](mir.md) | IRs | Active (see those docs) |
| [`incremental.md`](incremental.md) | Cache / fingerprint / plan | **v4** (see doc) |
| [`runtime-abi.md`](runtime-abi.md) | ABI / symbols | Active (structs, tasks, net, match tags) |
| [`stdlib.md`](stdlib.md) | Std layout | Design + thin/real nets |
| [`diagnostics.md`](diagnostics.md) | Codes / contract | ADR 0005; codes as features land |
| [`lsp.md`](lsp.md) | Editor boundary | After shared pipeline |
| [`llvm.md`](llvm.md) | Codegen / link | After MIR |

---

## 7. Worked coverage checklist

Use this as a “does the language doc set cover X?” matrix. Design column is
current; fill Impl as work lands.

### Leaders and statements

| Feature | Design | Lex | Parse | Sem | Run | e26 / notes |
|---------|--------|-----|-------|-----|-----|-------------|
| `~` `$` `#` binds | ✓ | ✓ | ✓ | ✓ | ✓ | `run/bind`, `run/hash` |
| Multi-bind same-line | ✓ | ✓ | ✓ | ✓ | ✓ | `run/bind/001_multi` |
| `%` / `@` structs | ✓ | ✓ | ✓ | ✓ | ✓ | methods + multi-file `@` |
| Struct field defaults | ✓ | ✓ | ✓ | ✓ | ✓ | `run/struct/007`, `check/struct/001` |
| `?` `:` chain | ✓ | ✓ | ✓ | ✓ | ✓ | if / else-if / else |
| `\|` match | ✓ | ✓ | ✓ | ✓ | ✓ | multi-value + **`% type`** + result/option |
| `*` loops / `<` `>` | ✓ | ✓ | ✓ | ✓ | ✓ | while / for / break / continue |
| `!` / `^` returns | ✓ | ✓ | ✓ | ✓ | ✓ | result/option shapes |
| `/` import / `\` export | ✓ | ✓ | ✓ | ✓ | ✓ | multi-file run |
| Bare call statement | ✓ | ✓ | ✓ | ✓ | ✓ | |

### Values and expressions

| Feature | Design | Lex | Parse | Sem | Run | e26 / notes |
|---------|--------|-----|-------|-----|-----|-------------|
| Function values | ✓ | ✓ | ✓ | ✓ | ✓ | first-class incl. result/option call-through; methods not values |
| Method call + `.` | ✓ | ✓ | ✓ | ✓ | ✓ | + `{.field}` interp; **`c.inc().value()`** and **`c.inc().n`** |
| Tagged / structural lits | ✓ | ✓ | ✓ | ✓ | ✓ | `{}` anon + named |
| Pure / rich strings | ✓ | ✓ | ✓ | ✓ | ✓ | **no** `+` concat |
| Numbers / bools / lists | ✓ | ✓ | ✓ | ✓ | ✓ | + index assign |
| Hex / bin ints | ✓ | ✓ | ✓ | ✓ | ✓ | `0x` / `0b` |
| Width tags i32/i64/f32/f64 | ✓ | ✓ | ✓ | ✓ | ✓ | native i32/f32 |
| Full `i*` / `ui*` + `byte`≡`ui8` | ✓ | ✓ | ✓ | ✓ | ✓ | grid + explicit `<width> expr`; unknown tag / non-numeric reject |
| Bytes | ✓ | ✓ | ✓ | ✓ | ✓ | `str.from_bytes` |
| Duration | ✓ | ✓ | ✓ | ✓ | ✓ | nanos + add |
| Locator `p` | ✓ | ✓ | ✓ | ✓ | ✓ | `str.from_locator` |
| Floats (default f64) | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Field / index assign | ✓ | ✓ | ✓ | ✓ | ✓ | chains + list set |
| `==` deep / `===` id | ✓ | ✓ | ✓ | ✓ | ✓ | `run/eq/001` |
| Dual-use operators | ✓ | ✓ | ✓ | ✓ | ✓ | |

### Program model

| Feature | Design | Index | Resolve | Sem | Run | Notes |
|---------|--------|-------|---------|-----|-----|-------|
| Top-level order | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Multi-file `%`/`@` | ✓ | ✓ | ✓ | ✓ | ✓ | graph method table |
| No shadowing | ✓ | | | ✓ | ✓ | check fixtures |
| Functions closed | ✓ | | | ✓ | ✓ | outer `$`/`~` → `sem-capture`; `#`/imports OK |
| Nested fn values | ✓ | ✓ | ✓ | ✓ | ✓ | closed body + `FnRef` bind; `__n_*` symbol |
| HIR bodies / FnRef | ✓ | — | — | — | ✓ | no language fn table; `bodies` + binds |
| `#` const-eval | ✓ | | | ✓ | ✓ | no calls in `#`; list/`..`/struct; field/index; omitted `%` defaults |
| Kitchen sink | — | | | | ✓ | `examples/app/surface.echo` |
| Std io/str | stub→thin | | | | ✓ | print strings-only |
| Std process | **Done** | | | | ✓ | e26 `run/process`; `xo test std/process.echo` |
| Std fs | **Done** | | | | ✓ | e26 `run/fs`; `xo test std/fs.echo`; streaming `% file` |
| Std net/http | TCP/UDP + parse/serve | | | | ✓ | e26 `run/net` / `run/http`; app server |
| Tasks `+` / `-` | ✓ | ✓ | ✓ | ✓ | ✓ | ADR 0013; e26 `run/task` |
| Match `% Type` arms | ✓ | ✓ | ✓ | ✓ | ✓ | type tag on named lits; `run/match/007` |
| Modules paths / package cache / `xo.toml` | ✓ | ✓ | ✓ host | ✓ | ✓ | ADR 0014; folder modules; `xo get` |
| `xo fmt` | ✓ | — | ✓ | — | ✓ | AST pretty-print; idempotent |
| Full LSP | ✓ | — | ✓ | — | ✓ | hover/def/refs/complete/sig/rename/symbols/fmt/tokens; **reliability:** session + versioned diags + incremental sync |
| REPL | ✓ | — | ✓ | — | ✓ | `xo repl` — rustyline + session + JIT |
| Task cancel | **out** (v0) | | | | — | no cancel API |

**Suite snapshot (2026-08-13):** `e26` **285** passed · **303** `.echo` · **172** `.run` · **63** `.check`.

---

## 8. Session log

| Date | Outcome |
|------|---------|
| 2026-07-16 | Clean-slate; statement-led keyword-free design |
| 2026-07-16 | Leaders, bindings, control, match, collections, strings |
| 2026-07-16 | Structs `%`/`@`, members `$`/`~`/`#`, receiver `.`, free fns |
| 2026-07-16 | Tagged lits, `!` err return, equality, no shadowing, captures |
| 2026-07-16 | `examples/app/` + `std/` HTTP multi-file samples |
| 2026-07-16 | `implementation.md` full-toolchain checklist |
| 2026-07-16 | Roadmap expanded to **entire language** coverage map |
| 2026-07-16 | §2.1 statement leaders: `echo_syntax` table + `echo_lexer` + `xo lex` |
| 2026-07-16 | Coarse leader modules: `echo_syntax/src/leaders/{bind,shape,control,module}.rs` |
| 2026-07-16 | `echo26/` suite + black-box runner binary `e26 --binary <candidate>` |
| 2026-07-16 | ADR 0011 chumsky; §2.1 parse + `xo ast` + `.ast` fixtures |
| 2026-07-16 | Policy: every language change updates echo26 + e26 (AGENTS / implementation) |
| 2026-07-16 | e26 parse catch-up: required `.ast`, `echo26/parse/**`, assign `~ .f` / `~ a[i]` |
| 2026-07-16 | Semantics v0 + `xo check` + e26 `.check` (`sem-*`) |
| 2026-07-16 | Multi-file: index, resolver, `%`/`@` merge, `echo26/multi/**` |
| 2026-07-16 | Locked: all types inferred (no annotation surface) |
| 2026-07-17 | Struct data + methods verticals on main |
| 2026-07-17 | Infra v0: `echo_fingerprint` / `echo_cache` / `echo_build` + `xo cache` + `docs/incremental.md` |
| 2026-07-17 | Infra v1: `xo check` semantic diagnostics cache (`--no-cache` / `--cache-status`) |
| 2026-07-17 | Infra v2: per-file parse AST cache (serde/bincode) in resolve |
| 2026-07-17 | Infra v3: LLVM IR artifact cache for xo run/ir/build |
| 2026-07-17 | Infra v4: AOT binary cache + echo_lsp document model / xo lsp |
| 2026-07-17 | LLVM opt pipeline ownership: drop generic MIR mid-end; keep escape/`NoEscape`; shared `OptLevel` O0–O3/Oz end-to-end + cache keys |
| 2026-07-17 | Multi-file `@` methods through run: graph-wide method table; call targets defining module |
| 2026-07-17 | Structural `{}` anon products through run (HIR empty-name StructLit → same runtime as named) |
| 2026-07-17 | Floats through run; `print` strings-only; `std/str` `from_int` / `from_float` |
| 2026-07-17 | Multi-level field assign `~ a.b.c =` / `~ .a.b =` through parse→run |
| 2026-07-17 | List index assign `~ xs[i] =` / `~ a.b[i] =` + IR/AOT cache key hardening |
| 2026-07-17 | `xo build` shares AOT binary cache with `xo run` |
| 2026-07-17 | Width tags: `<i32> -N` lit form; no tag after unary; native i32 ops through run |
| 2026-07-17 | Width tags: native `<f32>` ops through run (mirror i32; box via fpext→heap float) |
| 2026-07-17 | Bytes lits `b'…'` / `b"…"` through run; `str.from_bytes`; **no** string `+` |
| 2026-07-17 | Core surface closed through run (control/list/string/struct/method/result/option/width/float/bytes + multi `@`); `run/core/001` |
| 2026-07-17 | Duration lits through run (i64 nanos; +/−; `str.from_duration`) |
| 2026-07-17 | Hex/bin int lits `0x` / `0b` through HIR/const/`xo run` |
| 2026-07-17 | Locator `p'…'` / `p"…"` through run; `str.from_locator` |
| 2026-07-17 | `examples/app/surface.echo` through run; `{.field}` rich interp; partial `#` fold in strings |
| 2026-07-17 | Struct field defaults: omit optional fields; check required/unknown/method/dup |
| 2026-07-17 | Deep `==` (list/struct) vs identity `===` (`eq_id`) through run |
| 2026-07-17 | Multi-bind same-line `~ a = 1, b = 2` / `$ x = …` through parse→run |
| 2026-07-17 | Coverage audit: core language surface through run; checklist refreshed |
| 2026-07-17 | Thin/partials: else-if, # multi-bind, default+#const, `^ .` type, `sem-capture`, mix-eq check |
| 2026-07-17 | Method call chains `c.inc().value()` (expr receiver + returns_receiver type flow) |
| 2026-07-17 | Method plain fall-off returns `.` (implicit self); free fns unchanged |
| 2026-07-17 | **Functions closed** locked: no outer `$`/`~` capture; params/struct methods/`#`/imports only |
| 2026-07-17 | Nested closed fn values: nameless; bind-local; HIR hoist `__n_*` (no closures) |
| 2026-07-17 | Bind before use: `sem-unbound` for any name before its bind (values and calls) |
| 2026-07-17 | HIR IR cleanup: `bodies` + `FnRef` binds (no language-level function table) |
| 2026-07-17 | Match multi-value arms: `e1, e2, e3 { }` deep-`==` any; not only lits |
| 2026-07-17 | Honesty: function values partial (static call); AGENTS design-vs-impl rule |
| 2026-07-17 | First-class plain function values: FnValue + indirect call |
| 2026-07-19 | Function values complete for free fns; nested param call → sem-capture (no SEGV) |
| 2026-07-17 | Prove return-fn + field-stored fn (`b.f(args)` via field, not method) |
| 2026-07-17 | Lock: method bodies = free fn bodies for `^`/`!`; only entry + plain fall-off differ |
| 2026-07-17 | Inclusive range `lo..hi` as value: for-in + match membership |
| 2026-07-17 | First-class fn values carry ret shape; result/option call-through + `\|` |
| 2026-07-17 | `examples/app/main.echo` through run; rename `*_ops` → descriptive names |
| 2026-07-17 | `http.parse_request` backed by **httparse** (`runtime.http_parse_request`) |
| 2026-07-17 | HTTP parse fills `headers` product (lowercase, `-`→`_`) |
| 2026-07-17 | Real `std/net` TCP+UDP: runtime sockets + free-fn std wrappers + e26 `run/net` |
| 2026-07-17 | ADR 0013: task leaders `+`/`-` + event loop in runtime (no `std/task`) |
| 2026-07-17 | `+ () [caps]? { }` capture form; `+ f(args)`; unjoined tasks fail |
| 2026-07-17 | Free-fn `returns_struct` fixpoint (call chains); no task cancel |
| 2026-07-17 | Language: ordinary literal match (int/string/bool) run vertical |
| 2026-07-18 | Match **`% TypeName`** arms: dual-use `%`; runtime type tags on named struct lits |
| 2026-07-18 | Docs audit: roadmap dual-use/`|` match/layer status + suite snapshot refreshed |
| 2026-07-19 | **ADR 0014:** modules/paths; optional `xo.toml`; only `.xo/` trees; packages always → `$XO_HOME/packages/<id>/<ver>/` |
| 2026-07-19 | Implement package cache: `xo get` / `xo home`, host import parse (`github.com/…`), resolve from `$XO_HOME` |
| 2026-07-19 | Lock v0 package/module bundle + folder multi-file modules (export union, `res-import-cycle`) |
| 2026-07-19 | Package Q&A locks: no default alias; HEAD-hash pin; pin-prefer auto-get; cwd xo.toml; recursive --deps |
| 2026-07-19 | Named-struct **return unions**: HIR `returns_structs`; infer `Type::Union`; `%` match refines |
| 2026-07-19 | Serve polish: `http_headers_complete` + accumulate reads; empty peer; e26 `005_empty_peer` |
| 2026-07-19 | Lock: runtime free-only; std builds named net structs for methods (`stdlib.md`) |
| 2026-07-19 | Std net wrappers: `% conn`/`% listener` methods; factories; method return type flow |
| 2026-07-16 | Imports bind exports into importer scope (`res-export-*`, conflict, shadow) |
| 2026-07-16 | **Module-scoped imports only** (`module.export`; no symbol flood) |
| 2026-07-16 | Types/lits + Result: width tags prefix; `[]`/anon `{}`; bytes/p locators; `?expr`; `!` = err return; `|` Result arms `$`/`!` |
| 2026-07-16 | Option none = bare `^` in option-shaped fn; Result handle = `|` only (no propagate glyph) |
| 2026-07-16 | **No `?expr`** for option (never); **no** Result propagate glyph |
| 2026-07-16 | Option consume locked: `\|` match `$ name` some + `: { }` none |
| 2026-07-16 | Implemented Result/Option produce+consume in parser/semantics + e26/effect |
| 2026-07-16 | Kind inference v1 (unify + infer) + `echo26/infer/**` |
| 2026-07-16 | Docs: `pipeline.md` + expanded `implementation.md` (fmt/LSP/REPL/e26) |
| 2026-07-16 | Vertical: richer lits (width, duration, bytes, p) through lex→infer→e26 |
| 2026-07-20 | **ADR 0015:** Echo 2026 = edition + canonical public Language Spec (`www` `/e26`); `e26`/`echo26/` stay tooling IDs |
| 2026-07-20 | List push: `~ xs[] = e` / `~ a.b[] = e` → runtime `list_push`; e26 `run/list/003_push` |
| 2026-07-20 | `xo test` Model A: `std/test` + runtime registry + path/glob discovery |
| 2026-07-20 | Bitwise ops: `& \| ^ << >> ~` through run; dual-use `~`/`^`; e26 `run/bitwise` |
| 2026-07-20 | Integer widths locked: `i8`…`i64` + `ui8`…`ui64`, `byte`≡`ui8`; untagged int=`i64`; no `u8` spelling |
| 2026-07-20 | `std/bytes` + `std/crypto/hash` SipHash-2-4 (`hash.sip`); rich `\xHH`; paper vectors |
| 2026-07-21 | **`&` effect block**: auto-unwrap result/option; short-circuit on fail; `& name = { }` binds ok or err payload; dual-use bitwise `&`; e26 `run/effect`, `check/effect`, `leaders/effect` |
| 2026-07-23 | **`std/process`**: args, env, exit, spawn+wait; e26 `run/process`; `examples/misc/process.echo` |
| 2026-07-23 | **`std/fs`**: paths, whole-file, copy/rename, `% meta`, streaming `% file`; e26 `run/fs`; `examples/misc/fs.echo` |
| 2026-07-23 | Vertical closed: process+fs proofs (runtime unit + e26 + `xo test` + demos); next = memory slice 2 |
| 2026-07-23 | **Memory slice 2:** precise promote, demotion, owning_scope facts, immediate free; e26 lifetime 001–007 |
| 2026-07-23 | **Graph promote:** region evacuation + header epoch; crate nest/cycle/shared/deterministic; e26 010–012; www Memory |
| 2026-07-25 | **LSP reliability:** `LspSession` (testable protocol), incremental `didChange`, versioned multi-file `publishDiagnostics`, path-only diag attribution, percent-encoded URIs, UTF-16 position edges; docs `lsp.md` |
| 2026-07-25 | **`xo test --bench`:** `test.bench` + `runtime.test_bench_register`; harness auto-N / ns/op; co-located like `test.it` |
| 2026-07-25 | **Std benches:** co-located `test.bench` on math/str/bytes/list/json/hex/base64/path/sha256/sip/map; `xo test --bench std` discovery |
| 2026-07-25 | **Bench JSONL + compare:** `--bench-out` streams results; `--bench-baseline` / `--bench-threshold` report regressions |
| 2026-07-25 | **Sip lowerability:** pure Echo SipHash scalar ui64 + inlined rotl; ui64 `>>` → `lshr`/`fshl` (not ashr); proof-load IR |
| 2026-08-13 | Campaign: remaining language verticals = honesty (width/cast, PHI, `#`, reject belt, heap dispose). Width/cast slice: unknown tags error; real int↔float casts; `#` float bits; e26 `run/width/004`–`006` |
| 2026-08-13 | Honesty campaign closed: per-name match payloads; `&` unwrap of fn-value calls; `# F = () { }` run; reject-path e26; dispose task/tcp/udp/fs-file |
| 2026-08-13 | Field-after-call `c.inc().n`: plain method fall-off types as receiver; e26 `run/method/009` |
| 2026-08-13 | Imported + zero-arg function arity: `sem-arity` on `module.f` and `f()` extra args; import params stay `value` |
| 2026-08-13 | Rich escapes locked: unknown / bad `\x` is `lex-escape` (not rewritten); `\{` `\}` legal |
| 2026-08-14 | `xo cache gc`: drop stale compiler-stamp dirs, flat leftovers, and tmp; `clean` still wipes `.xo` |
| 2026-08-14 | LLVM line-table DI: compile unit + subprogram + loc (file, line 1); `-g` on AOT link |
| 2026-08-14 | Per-op DWARF lines + checker-kind locals; imported return kinds (`str.from_int` → string); import params stay `value`; AOT still one clang `-O0 -g` |
| 2026-08-14 | Locator class locked: URI `scheme://`, abs `/…`, else relative; `path.class` / `is_uri`; locators and strings share the rule |
| 2026-08-14 | AOT runtime `.a`: newest match in `xo`’s profile; clang missing `echo_runtime_*` → rebuild hint |
| 2026-08-14 | `!` only inside a function (`sem-error-return` at file scope); top-level `^` still process status |
| 2026-08-22 | `#` const-eval folds duration/bytes/locator lits (`# D = 5s`, `b'…'`, `p'…'`); duration `+`/`-`/`==` |
| 2026-08-22 | `#` const-eval folds list `[…]` and range `lo..hi` lits (`# XS = [1, 2]`, `# R = 1..3`) |
| 2026-08-22 | `#` const-eval folds named/anon struct lits (`# P = point { x: 1 }`, `# Q = { a: 1 }`) |
| 2026-08-22 | `#` const-eval folds field `P.x` and index `XS[i]` on other `#` values |
| 2026-08-22 | `#` const-eval fills omitted `%` defaults so `# N = I.n` folds |

---

## 9. How to change the language

1. Update **Echo 2026** public Spec / Reference (`www`) and implementer
   **`syntax.md`** (and `lexer.md` if tokens change).  
2. Update **this roadmap** status tables if scope or freeze changes.  
3. If hard to reverse → **ADR** under `docs/adr/`.  
4. Walk **`implementation.md`** checklist for every affected layer.  
5. Keep **`echo26/`**, **`examples/`**, **`std/`**, and **`www/`** in sync with
   the surface (green `e26`).

Do not implement behavior only in LSP, `xo`, or www.

Edition authority: [ADR 0015](adr/0015-echo-2026-canonical-edition.md).
