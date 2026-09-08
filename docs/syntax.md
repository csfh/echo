# Syntax

Echo language surface — **statement-led, keyword-free core**.

**Edition:** implementer surface for **Echo 2026** (canonical public Language
Spec: site `/e26` + Reference `/docs`; executable contract: `echo26/` — ADR 0015).

| | |
|--|--|
| **Status** | Core locked for implementation |
| **Related** | `docs/lexer.md`, `docs/roadmap.md`, `docs/adr/0015-echo-2026-canonical-edition.md`, `examples/app/main.echo` |

## Intent

- **No English keywords** for control, binding, or definitions.
- **Single-character statement leaders** in leader position only.
- Ordinary **expressions**: bare names + operators + literals.
- **Naming:** `snake_case`; **struct names lowercase** (`user`, `http_request`).
  `#` constants **SCREAMING_SNAKE** only.
- **`%` = struct shape** (may include function members). **`@` = extra
  behavior** for a `struct_name` (more members), often in other files.

## Statement leaders

A **leader** is one character at the **start of a statement** (after indentation).
**Whitespace is required after the leader** (except bare `<` and `>`).

Opening **`{` for a block must be on the same line** as its introducer.

| Leader | Role | Form |
|--------|------|------|
| `~` | Mutable bind / reassign | `~ name = expr` · multi `~ a = 1, b = 2` · `~ a.b.c =` · `~ xs[i] =` · `~ xs[] =` (list push) |
| `$` | Immutable runtime bind | `$ name = expr` · multi `$ x = 1, y = 2` · init once |
| `#` | Compile-time constant | `# NAME = expr` · multi ok · SCREAMING_SNAKE |
| `%` | Struct declaration **or** match type arm | `% struct_name { members }` · inside `\|` → `% Type { body }` |
| `@` | Additional members for a struct (often other file) | `@ struct_name { members }` |
| `?` | If | `? expr { ... }` |
| `:` | Else-if / else / match default | `: expr { ... }` · `: { ... }` |
| `!` | Return **error** (Result err) | `! expr` |
| `^` | Return | `^ expr` · bare `^` |
| `*` | Loop | `* { }` · `* expr { }` · `* item : items { }` |
| `<` | Break | `<` |
| `>` | Continue | `>` |
| `\|` | Match | `\| expr { arms }` |
| `+` | Task spawn | see below |
| `-` | Task join / immediate block | `- { … }` · `- name = { … }` (result) · `- handle` · `- name = handle` |
| `&` | **Effect block** — auto-unwrap result/option | `& { … }` · `& name = { … }` |
| `/` | Import | `/ path` |
| `\\` | Export | `\\ name` · `\\ a, b` |

**Effect block (`&`):**

| Form | Meaning |
|------|---------|
| `& { … }` | Run body; on result err / option none, **short-circuit** the rest of the body and continue after the block |
| `& name = { … }` | Same; on **success** (`^ value` in the block), bind the payload to `name`; on **fail**, bind the **err payload** (result) or none payload (option) to `name` |

Inside the block:

- Free-function (and module) calls that return **result** or **option** are
  **automatically unwrapped** into the ok/some payload. No `|` match required.
- Plain values are unchanged.
- Prefer `^ expr` for the success value of an assigned block.
- Dual-use: bitwise `&` remains an expression operator outside leader position.

**Tasks (ADR 0013):**

| Form | Meaning |
|------|---------|
| **`+ f(args)`** | Schedule free function `f` with args |
| `+ name = f(args)` | Same; bind **task handle** |
| `+ () { … }` / `+ name = () { … }` | Zero-arg body (empty param list) |
| `+ () [a, b] { … }` | Captures `a`,`b` **by reference** (see below) |
| `+ { … }` / `+ name = { … }` | Zero-arg body (same as `+ () { … }`) |
| `- …` | Join / immediate block |

`[captures]` is **optional**.

**Spawn forms (detail)**

| Form | Args / captures | Notes |
|------|-----------------|--------|
| `+ f(a, b)` | Up to **8** args | `f` must be a free function (call); not a bare name |
| `+ name = f(a, b)` | Same | `name` is the **task handle** |
| `+ () [a, b] { … }` | Up to **8** captures | Each name must be bound (`sem-task-capture`) |
| `+ () { … }` / `+ { … }` | None | No outer free locals in the body |

**Capture rules**

- Each name in `[…]` must **already be bound** in the enclosing scope; unbound →
  `sem-task-capture` (hard error).
- Captures are **by reference**: the task receives the same runtime value
  (handle identity for heap objects — sockets, structs, lists, strings). There is
  **no deep copy**. Field / socket mutations through that handle are shared with
  the outer binding.
- Names become parameters of the task body (closed to other outer names).
- **Max 8** captures or call args (v0 ABI); more → `sem-task-arity` / `cg-task`.

**Join rules**

- Every `+` must be matched by a `-` before process end → else exit status ≠ 0
  (`echo_runtime: N task(s) left unjoined`).
- Immediate block `- name = { … }` / `- { … }` schedules and joins in one step.

No `std/task`. Dual-use: expr `+`/`-` only outside leader position.

```echo
$ lis = tcp.listen("127.0.0.1:8080")
+ job = () [lis] {
    $ c = tcp.accept(lis)
    ^
}
- job

; preferred: free factory + task on a free function
$ handle = (c) { … }
+ job = handle(c)
```


**Free functions** are values: `$ f = (a, b) { ... }` — not `@`.

**Side-effect calls** are bare call statements: `log(x)` · `u.greet()`.

### Structs: members are all `$` / `~` / `#`

Inside `% struct_name { }` and `@ struct_name { }`, members use the **same
bind leaders** as top level—data or functions:

```echo
% user {
    $ name
    ~ visits = 0
    # KIND = 'user'

    $ greet = () {
        ^ "Hello, {.name}"
    }

    $ visit = () {
        ~ .visits = .visits + 1
        ; fall-off returns `.` in plain methods (same as ^ .)
    }
}

; more behavior in another file (or later in this file)
@ user {
    $ label = () {
        ^ "{.name}#{.visits}"
    }

    ~ handler = () {
        log(.name)
    }

    # DEFAULT_LABEL = () {
        ^ 'user'
    }
}

$ u = user {
    name: "Ada",
    visits: 0
}
u.greet()
u.visit()
u.label()
```

| Member leader | Meaning on a struct |
|---------------|---------------------|
| `$ name` / `$ name = expr` | Immutable field (or immutable method slot if value is a function) |
| `~ name` / `~ name = expr` | Mutable field or **reassignable** method slot |
| `# NAME = expr` | Compile-time constant field or constant function |

**`%` vs `@`:**

- **`% struct_name`** — primary declaration (fields required here; methods optional).
- **`@ struct_name`** — **additional** members only (typically methods); may live in
  **other files**. Merged with `%`; **duplicate member names** are a hard error.
- Exactly **one** `% struct_name` per program/package.
- `@` requires `%` for that `struct_name` to exist.

**Receiver and `.` resolution (important):**

- Bare **`.`** is only meaningful while executing a function that was entered as
  a **method call** `value.member(...)`.
- On entry, the callee is invoked with an implicit **receiver** = `value`.
- Inside that activation:
  - `.field` / `.method` / `^ .` / `~ .field = …` / `~ .a.b = …` use that receiver
  - Nested free-style function values defined on the struct and called as
    methods get the same rule
- If a function expression is called **without** method call syntax
  (`fn()` not `value.fn()`), **`.` is illegal** in that activation
  (no receiver).
- Free top-level `$ f = () { .x }` is illegal (`.` with no method receiver).
- Inside a method, a nested `$ g = () { .name }` captures nothing special until
  `g` is invoked; when `g` is called as `.g()` (method style on receiver) or
  only if you define call rules—**v0: only direct method calls bind `.`**;
  nested closures do not see `.` unless invoked as methods on a value.

**Call site:**

- `u.greet()` — looks up member `greet` on `u`'s struct, calls it with receiver `u`.
- `greet(u)` — only if `greet` is a **free** function taking `u` explicitly.

### Condition chain

```echo
? cond1 { ... }
: cond2 { ... }
: { ... }
```

### Match

`|` is the match leader (locked).

**Ordinary arms** (value match): one or more **value expressions**, comma-separated,
then a block. Arm runs if scrutinee deep-equals **any** of them, **or** lies in a
syntactic **`lo..hi` range** (inclusive). Default: `: { body }`. No trailing comma.

**Type arms** (named structs): `% TypeName { body }` — dual-use `%` (shape decl at
statement level; type arm inside match). Arm runs if the scrutinee is a heap
struct whose type tag is `TypeName` (from a tagged lit `TypeName { … }`).
Anonymous `{ … }` and runtime-built products have no tag and never match. Type
arms may mix with value arms and `:`; they must **not** mix with `$` / `!`
Option/Result arms.

```echo
| x {
    1, 2, 3 {
        io.print("small")
    }
    4..6 {
        io.print("mid")
    }
    n + 1 {
        io.print("next")
    }
    : {
        io.print("other")
    }
}

% circle {
    ~ r
}
% rect {
    ~ w
    ~ h
}
$ shape = circle { r: 3 }
| shape {
    % circle {
        io.print(str.from_int(shape.r))
    }
    % rect {
        io.print(str.from_int(shape.w))
    }
    : {
        io.print("other")
    }
}
```

**Option arms** (scrutinee is Option) — locked:

```echo
| find_user(id) {
    $ user {
        render(user)
    }
    : {
        render_missing()
    }
}
```

- `$ name { … }` — **some**; payload bound as `name`
- `: { … }` — **none**; no payload

**Result arms** (scrutinee is Result) — locked:

```echo
| load_user(id) {
    $ user {
        render(user)
    }
    ! error {
        log(error)
    }
}
```

- `$ name { … }` — **ok**; payload bound as `name`
- `! name { … }` — **err**; payload bound as `name`

Arm dialect is fixed by scrutinee kind. Unhandled Option/Result → compile error.
No Result/Option propagate glyph.

### Error return (`!`)

Not a process abort. **`! expr` returns from the current function as Result err**
with payload `expr` ( recoverable error path ).

```echo
! "unreachable"
```

Ok path still uses normal `^ value`. Together they form a **Result**.

### Free functions (anonymous values)

```echo
$ add = (a, b) {
    ^ a + b
}

~ handler = (r) {
    ^ text_response(200, "ok")
}
```

- **Functions are nameless values.** `$ name = (params) { … }` binds a value;
  the function itself has no name. Nested binds are allowed and are the same
  kind of value as top-level free-fn binds.
- **Functions are closed** (see `docs/semantics.md`): params, inner locals, `#`,
  imports; methods also get `.`. No outer `$`/`~` capture (no closures).
- No shadowing inside bodies.
- No implicit receiver — use explicit params (`greet(u)` if free).

## Import and export

**Module-scoped imports only** (no dumping exports into the importer’s scope).

```echo
/ ./math
/ std/net/http

$ x = math.add(1, 2)
$ s = http.serve(addr, routes)
$ r = http.response { status: 200, body: "" }

\ add, user
```

- Paths: `/ ./relative/...`, `/ ../sibling/...`, or `/ bare/segments`.
- `/ path` binds **one** name = **last path segment** (`http`, `math`, …).
- Use exports as **`module.name`** (and `module.Type { … }` for tagged lits).
- `\` lists what that module exports (binds, `%` types, or re-exports).
- `\` is export only — **not** line continuation.

Details: [`modules.md`](modules.md).

## Loops

```echo
* { ... }
* n < 10 { ... }
* item : items { ... }
* x : 4..6 { ... }          ; inclusive range value as iterator
```

## Dual-use glyphs

| Glyph | Leader | Expression |
|-------|--------|------------|
| `*` | loop | multiply |
| `<` `>` | break / continue | comparisons |
| `!` | error return / match err arm | prefix not |
| `\|` | match | true |
| `/` | import | divide |
| `%` | struct shape **or** match type arm | remainder (expr) |
| `.` | — | field/method on a value **or** receiver in `@ struct_name` method bodies |

## Names

- Identifiers: `[A-Za-z_][A-Za-z0-9_]*`, ASCII, case-sensitive.
- **snake_case**; **struct names** lowercase (`user`, not `User`).
- `#`: SCREAMING_SNAKE only.

## Expressions

| Kind | Forms |
|------|--------|
| Arithmetic | `+ - * / %` · unary `-` |
| Bitwise | `& \| ^ << >>` · unary `~` (integers only) |
| Comparison | `== != === !== < > <= >=` |
| Boolean | `&& \|\|` · prefix `!` |
| Member | `value.field` · `value.method()` |
| Receiver (method body only) | `.field` · `.method()` · bare `.` as value |
| Index | `xs[i]` · `~ xs[i] = expr` · `~ xs[] = expr` (append) · `~ a.b[] = expr` |
| Call | `name(args)` · `value.method(args)` |
| Range | `lo..hi` — inclusive integer range value |
| Function expr | `(a, b) { ... }` · `() { ... }` |
| Struct lit | `struct_name { k: v, ... }` |
| Grouping | `(...)` |

**Bitwise rules (locked):**

| Op | Meaning |
|----|---------|
| `&` `\|` `^` | Bit and / or / xor; same integer width both sides (`i*` / `ui*` / `byte`) |
| `<<` | Shift left; count masked to width (`& 63` / `& 31` / `& 15` / `& 7`) |
| `>>` | **Arithmetic** on signed `i*`; **logical** on unsigned `ui*` / `byte`; count masked |
| `~` | Bitwise complement |

Dual-use: expr `~` / `^` vs leaders bind / return; binary `\|` vs true atom / match
leader (position decides).

**Precedence:** primary → unary (`-`, `!` not, `~` bit-not) → `* / %` → `+ -` →
`<< >>` → `..` (range) → comparisons → `&` → `^` → `\|` → `&&` → `||`.

## Literals and collections

| Kind | Form |
|------|------|
| String pure | `'...'` — no escapes, no interp, no interior `'` |
| String rich | `"..."` — escapes (`\n` `\t` `\r` `\\` `\"` `\{` `\}` `\xHH`) + `{name}` interp (no `+` concat) |
| Bytes pure | `b'...'` — like pure string, byte payload |
| Bytes rich | `b"..."` — like rich string: same escapes (`\n` `\t` `\r` `\\` `\"` `\{` `\}` `\xHH`) + `{name}` interp; payload is bytes |
| Locator (`p`) | `p'...'` / `p"..."` — one kind; class is URI (`scheme://`), abs (`/…`), or relative; rich `p"..."` has the same escapes + `{name}` interp as rich strings (see semantics) |
| Integers | decimal, `0x`, `0b`, `_` separators |
| Floats | `3.14`, `1e-3` |
| Duration | number + suffix: `us` `ms` `s` `m` `h` (e.g. `100us`, `5s`) |
| Bool | `\|` true · `_` false |
| List | `[a, b, c]` — **only** list literal |
| Anon struct | `{ k: v, ... }` — structural / anonymous product, **not** a map |
| Named struct | `user { name: "Ada" }` · `mod.user { ... }` tagged |
| Numeric width tag | `<i32> 123` / `<ui8> 255` / `<byte> 1` — **prefix only** (space preferred); sign after tag on signed/float lits |
| Width cast | `<ui64> expr` — explicit convert (no silent mix); `byte` ≡ `ui8` |

**Integer width names (locked):** `i8` `i16` `i32` `i64` · `ui8` `ui16` `ui32` `ui64` ·
alias **`byte` = `ui8`**. Floats: `f32` `f64`. Untagged int default **`i64`**.

**Option** is **not** a literal form. It appears only as a **function result
shape** (see `docs/semantics.md`): bare `^` / `^ value` returns in option-shaped
fns. **`?expr` is never used** for option. Statement `?` is **if** only.

**Maps / sets / other structures:** not language literals; **stdlib later**.
Core collections: **list** `[]`, **anon struct** `{}`, **named struct** lits.

**No null literal.** No trailing commas. No `\` line continuation.

### Result / option handling

| Kind | Produce | Consume (`\|` match) | Auto-unwrap |
|------|---------|----------------------|-------------|
| Option | bare `^` / `^ v` in option-shaped fn | `$ name {…}` some · `: {…}` none | inside `& { … }` |
| Result | `^ v` / `! e` when any `!` in fn | `$ name {…}` ok · `! name {…}` err | inside `& { … }` |

No `?expr`. No none assignment lit. No separate propagate glyph — use **`&`**
effect blocks for short-circuit unwrap chains.

## Layout

- One construct per line (except multi-bind).
- **Multi-bind (locked):** one leader, then `name [= expr]` pairs separated by
  `,` (no trailing comma): `~ a = 1, b = 2` · `$ x = 10, y = 20`. Same semantics
  as sequential single binds. Leader is **not** repeated before each name.
- `{` on same line as introducer.
- Multi-line via `{ }` structure only.

## Program structure

- Top-level runs in order; a name is usable only after its bind (same for
  `$ a = b + 4` before `$ b = 5` and for function values).
- `/` import · `\` export.

## Comments

- `;` to EOL.

## Scoping

- **No shadowing** — name introduced once per region; `~ name =` updates mutables.
- Params and `* item` are introductions.

## Types

- **Default: kinds are inferred** on binds, params, returns, and fields.
- **No** colon-style ascriptions; **no** generics surface.
- **Width tags** on numeric lits only: `<i32>42`, `<f64>3.14` (prefix only).
- Nominal domain values: **named struct lits** + `%` shapes.
- See `docs/semantics.md`.

## Const `#`

Literals + ops on other `#` only — no calls. Includes number, bool, string,
bytes, locator, duration, list `[…]`, range `lo..hi`, named struct
`name { … }`, and anon `{ k: v }` lits; duration `+` / `-` / `==` on other `#`;
list `==` / `!=` (deep); range `==` / `!=` / `===`; struct `==` / `!=` (deep,
by field name). Field `name.f` and index `xs[i]` on other `#` values fold
(in-range list elements; struct fields including foldable omitted `%`
defaults). Calls are not `#`-folded.

## Structs, params, mutation (value vs reference)

**Locked** — full table and IR taxonomy: [`semantics.md`](semantics.md) § Value vs reference.

| If it is… | You copy… |
|-----------|-----------|
| **struct** (named or anon) or **list** | the **reference** (share the object) |
| **anything else** (int, float, bool, string, bytes, …) | the **value** |

- Params always **copy the binding**; class decides ref vs value.
- **Sockets are structs.** Userland passes `% conn` / `% listener` **by
  reference**. The runtime TCP id lives only in a **`handle` field** — not a
  separate language type. Std reifies `runtime.tcp_*` products into named structs
  at the boundary ([`stdlib.md`](stdlib.md)).
- In methods, `.` is the receiver; `~ .field` / `~ .a.b` only for `~` fields.

## Equality

| Op | Meaning |
|----|---------|
| `==` / `!=` | Deep / structural |
| `===` / `!==` | Identity |

## Failure (v0 / direction)

- `! expr` — **error return** (Result err), not hard process abort.
- Hard abort / process panic — not designed yet (if ever separate from Result).

## Worked programs

| Path | Role |
|------|------|
| [`examples/app/main.echo`](../examples/app/main.echo) | HTTP demo |
| [`examples/app/surface.echo`](../examples/app/surface.echo) | Surface exercise |
| [`examples/`](../examples/) | Classic algorithms |
| [`std/`](../std/) | Std stubs |
