# 0013. Tasks and event loop via `+` / `-` leaders (not `std/task`)

## Status

Accepted.

## Context

Real concurrent I/O (many connections, overlapping reads) needs a runtime
scheduler and a place to park work. Putting that surface in `std/task` would
make a package path the only way to express control flow—effectively a keyword
and a violation of “userland is ordinary Echo; language control is leaders.”

An older tree (`echo-php-old`) implemented an event loop **inside**
`echo_runtime` (`sched`, `poll` with **mio**, `task`, `task_group`)—not a
separate crate. That layout is the right ownership; the PHP-era `std/task` API
is not.

## Decision

1. **Concurrency is language + runtime**, not a standard-library control package.
   - **No** `std/task` (or similar) as the spawn/join API.
   - User programs use **statement leaders** only.

2. **Leaders**
   | Form | Meaning |
   |------|---------|
   | **`+ f(args)`** | Schedule free function with args |
   | `+ name = f(args)` | Same; bind **task handle** |
   | `+ () { … }` / `+ { … }` | Zero-arg body |
   | `+ () [a, b] { … }` | Capture list (optional `[]`); values at spawn → body params |
   | `- { … }` | **Immediate block**: schedule body, **join** before continuing |
   | `- name = { … }` | Immediate block; bind result |
   | `- handle` / `- name = handle` | Join handle |

   **Unjoined tasks are an error** at process end (every `+` needs a matching
   `-`). Prefer `+ accept(lis)` over inventing shared globals.

3. **Event loop** lives in **`echo_runtime`**, driven by **`mio`**
   (`Poll` + `Waker` + runnable queue). Not a separate crate; not Tokio/async.
   Process infrastructure: programs do not import or “start” it.
   `+` enqueues and **`Waker::wake`s** the loop **immediately**; `-` parks the
   **current** task until the target finishes.

4. **Task bodies** follow closed-function rules (no open closures). Prefer
   `+ f(args)` or blocks that only use allowed outer binds / args.

5. **Failure shapes** inside a task body use ordinary `^` / `!`; join exposes
   the same shape to the binder where applicable (v0 may start with plain
   returns and extend).

6. **I/O park:** `std/net` sockets are **nonblocking**. On `WouldBlock`, arm
   mio interest, **retry once** (close edge-trigger race), then park the
   worker (`SourceFd` + Condvar). Poller + other workers keep running.

7. **Ret shapes:** task entries carry shape 0/1/2 (plain/result/option); join
   returns packed **i128** for tagged shapes so `|` match works on `- name =`.

8. **Pass data into tasks** with **`+ f(args)`** or **`+ () [caps] { }`**.
   Capture names must be bound (`sem-task-capture` if not). Captures are
   **by reference** (shared heap handle identity; no deep clone).
   **Arity cap:** at most **8** args or captures (runtime `MAX_TASK_ARGS`).

9. **Diagnostics (v0)**
   | Code | When |
   |------|------|
   | `sem-task-capture` | Capture name unbound |
   | `sem-task-arity` | More than 8 captures |
   | `cg-task` | Codegen: unknown body, >8 args, bad join |

10. **No task cancel.** Tasks run to completion (or process exit). Long-running
    `http.serve` may leave connection tasks unjoined until the process ends;
    there is no cancel/nursery API.

## Consequences

- Lex/parse/check/HIR/MIR/codegen gain `+` / `-` as control leaders (dual-use
  with expr `+` / `-` only in leader position).
- Runtime exports task spawn/join (and loop internals); codegen lowers leaders
  to those symbols—**not** via userland `/ runtime` from apps.
- `std/net` stays free functions; concurrency does not require a task import.
- Reference material in `echo-php-old` may inform design; implementation is
  rewritten for this tree’s value model and pipeline.

## Related

- `docs/syntax.md`, `docs/lexer.md`, `docs/roadmap.md`
- ADR 0004 (Rust runtime owns executable semantics)
- ADR 0009 (full vertical slices)
