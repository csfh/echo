# Glossary

Shared vocabulary for Echo implementers. Expand as terms stabilize. Prefer one
definition here over redefining the same idea in every layer doc.

## Source and frontend

| Term | Meaning |
|------|---------|
| **Source** | Input text and identity (`SourceId`, path, bytes) owned by `echo_source`. |
| **Span** | Byte range in a source file. Prefer source-aware spans at API boundaries. |
| **Token** | Lexeme produced by `echo_lexer`. |
| **AST** | Source-shaped syntax tree (`echo_ast`). Records what was written, not runtime meaning. |
| **Diagnostic** | Structured error/warning/note with span, code, and severity (`echo_diagnostics`). |

## Analysis and IR

| Term | Meaning |
|------|---------|
| **Semantics** | Local meaning: bindings, scopes, inferred types, analysis diagnostics (`echo_semantics`). |
| **Kind inference** | Default: value kinds of binds/params/returns/fields are inferred. |
| **`unknown` (internal)** | Soft checker hole: not known yet; unifies by **adopting** `T` and freezing (e.g. empty list element). Not a keyword. |
| **`value` (internal)** | Universal ABI kind: intentionally dynamic; unifies with any concrete kind and **stays** open. Used for unconstrained eq/store/passthrough params (map keys, `std/reflect`). Not a keyword; not pass-by-value. |
| **Width tag** | Prefix on a numeric lit, e.g. `<i32>42` — storage width, not a type/generics system. |
| **Closed function** | Function body sees only params, its own locals, `#`, imports, and (if a method call) `.` — no outer `$`/`~`. |
| **Function value** | Nameless closed `(params) { … }` value; `$ name = …` names the **binding**. Pass/rebind/call-through supported for plain returns (code pointer). |
| **Range** | Inclusive integer interval `lo..hi` (empty if lo > hi). Value; for-in yields ints; match arm means membership. |
| **Anon struct** | `{ k: v }` structural product; **not** a map; no type tag. |
| **Named struct** | `% name { … }` shape + tagged lit `name { … }`; heap type tag for `%` match arms. |
| **Value vs reference** | Pass convention (not the internal kind `value`). Params always copy the binding. **Ref** = struct + list (copy ref, share). **Value** = everything else (copy bits). See `semantics.md`. |
| **`std/reflect`** | Userland runtime kind API (`kind` / `key_bytes` / …). Distinct from tools crate `echo_reflection`. |
| **RefValue** | IR class: `Struct` \| `List` — pass by reference. |
| **StaticValue** | IR class: int/float/bool/string/bytes/… — pass by value. |
| **Opaque handle** | Runtime-only resource id (e.g. `KIND_TCP_*`); lives in a struct **field**, not a userland type. |
| **Type arm** | Match arm `% TypeName { … }`; matches when scrutinee’s runtime type tag is `TypeName`; refines name scrutinee type in the arm. |
| **Struct return union** | Fn whose valued `^` paths are named struct lits of different types; monomorphic only after `%` match refine. |
| **Runtime free surface** | `/ runtime` exports are free functions only; never method receivers. |
| **Std wrapper type** | Named `%` in `std/` (e.g. `% conn`) that holds an opaque handle field; **passing a socket** = passing that **struct by ref**. |
| **Option** | Produce: bare `^` / `^ v`. Consume: `\|` match `$ name` some, `: { }` none. |
| **Result** | Produce: `^` / `!`. Consume: `\|` match `$ name` ok, `! name` err. No propagate. |
| **HIR** | Analyzed intermediate form still close to source structure (`echo_hir`). |
| **MIR** | Backend-neutral executable intermediate form (`echo_mir`). Not a VM bytecode. |
| **Codegen** | Lowering MIR to LLVM IR (`echo_codegen`). |
| **ABI** | Calling and symbol contracts between codegen and runtime (`echo_codegen_abi`). |

## Execution

| Term | Meaning |
|------|---------|
| **Runtime** | Executable behavior shared by AOT and JIT (`echo_runtime`, symbols `echo_runtime_*`). |
| **AOT** | Ahead-of-time native binary via LLVM IR + host link (`xo build`, default `xo run`). |
| **JIT** | In-process LLVM JIT (`xo run --jit`). |
| **std** | Privileged standard-library package (`/ std/…`); toolchain/install root. |
| **`/ runtime`** | Runtime-primitive package import; **legal only inside privileged std sources**. |
| **Runtime primitive** | Export of the `runtime` package (e.g. `runtime.print`); codegen → `echo_runtime_*`. |
| **Scope-owned memory** | Managed heap is owned by a lexical/dynamic scope; leave-scope edges **release** (ADR 0016). Not tracing GC. Map: [`memory.md`](memory.md); public `/docs/memory`. |
| **Graph promotion** | Region evacuation: escape of a root rehomes every reachable alloc still owned by the source frame (epoch-safe queue). |
| **Region evacuation** | Same as graph promotion — transfer ownership between scopes by walking managed references, not GC. |
| **Promotion / demotion / release** | Promote (graph) / demote / dispose at scope exit (ADR 0016). |
| **Garbage collection** | **Not** Echo’s product reclamation model. See scope-owned memory / [`memory.md`](memory.md). |

## Project model

| Term | Meaning |
|------|---------|
| **Module** | Path-addressable unit; **folder is a module** (no parent/child package tree). See ADR 0014. |
| **Package** | Optional `xo.toml` grouping modules + deps (not required for local work). |
| **Package cache** | User `$XO_HOME/packages/<id>/<version>/` only (always under `.xo`; `xo get` / install). |
| **Index** | Reusable project facts extracted from sources (`echo_index`). |
| **Resolver** | Project-wide name, module, and package resolution (`echo_resolver`). |
| **Compilation graph** | Closed set of sources admitted for one build (entry + imports + store). |
| **Fingerprint** | Identity of a compiler component or input used for cache invalidation. |
| **Cache** | Artifact store for incremental builds (`echo_cache` / `echo_build`). |

## Tooling and proof

| Term | Meaning |
|------|---------|
| **xo** | CLI entrypoint; orchestrates the pipeline and tools. |
| **Gate** | Focused verification dispatcher (`scripts/gate`). |
| **Vertical slice** | Feature landed through all relevant layers, not one layer in isolation. |
| **Echo 2026** | Current **language edition** and **canonical public Language Spec** (site section; ADR 0015). |
| **Fixture** | File-backed language case with Echo-owned expected outcomes under `echo26/` (executable contract of Echo 2026). |
| **echo26** | Fixture suite directory for Echo 2026 (`echo26/`). |
| **e26** | Black-box suite **runner** CLI (`e26 --binary <candidate>` over `echo26/`). Short tooling ID for Echo 2026 conformance. |
| **chumsky** | Combinator parser library used by `echo_parser` (ADR 0011). |
