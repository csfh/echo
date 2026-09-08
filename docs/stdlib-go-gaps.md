# Standard library gaps: Echo vs Go

| | |
|--|--|
| **Status** | Living inventory (not a promise to clone Go) |
| **Date** | 2026-07-24 |
| **Authority** | Echo surface: [`stdlib.md`](stdlib.md); Go: [pkg.go.dev/std](https://pkg.go.dev/std) |
| **Related** | ADR 0013 (tasks), ADR 0017 (TLS), expansive roadmap in `stdlib.md` |

## Purpose

Compare **what Echo ships today** under `/ std/…` (and privileged runtime bridges)
with **what Go’s standard library covers** for day-to-day systems and scripting
work. Use this to prioritize std growth—not to treat Go’s package set as a
checklist to copy.

Echo is younger, scope-owned (ADR 0016), and deliberately thin: pure Echo when
enough; Rust **crates** for hard solved problems (JSON, HTTP parse, SHA-2, TLS).

## How to read the tables

| Echo column | Meaning |
|-------------|---------|
| **Strong** | Usable product surface for common cases (may still be “thin”) |
| **Thin** | Present but missing APIs Go users expect |
| **Partial** | Related surface exists; major Go capability missing |
| **Gap** | No meaningful Echo std equivalent |
| **N/A** | Language-owned or intentionally out of std (see notes) |

---

## Snapshot: what Echo already has

| Domain | Echo modules (today) |
|--------|----------------------|
| I/O print | `std/io` |
| Strings / bytes | `std/str` (incl. `parse_int`/`parse_float`), `std/bytes`, `std/bufio` |
| Lists / collections | `std/list`, `std/collections/{map,set,hash_table,queue}` |
| Math / random | `std/math`, `std/random`, `std/crypto/random` |
| Time | `std/time` (`now_ms`, `sleep_ms`, `mono_ms`, `format`, `parse`) |
| OS / process | `std/os`, `std/process` (`run`/`run_capture`/`run_cwd`/`spawn_pipes`) |
| Path / fs | `std/path` (clean/rel/walk), `std/fs` (chmod) |
| Encoding / JSON | `std/encoding/{hex,base64,csv}`, `std/json` |
| Compress | `std/compress/{gzip,zip}` |
| Crypto | hash sip/sha256/sha512, HMAC-SHA256, AES-GCM, CSPRNG |
| Net | TCP, UDP, Unix, DNS, HTTP serve + client (+TLS), URL, TLS |
| CLI / log / test | `std/cli`, `std/log` (kv), `std/test` |
| Reflect | `std/reflect` (runtime kinds; not tools reflection) |

**P0–P2 close (2026-07):** URL, richer HTTP(S) client, time format/parse, strconv,
bufio lines, process **pipes** (`spawn_pipes` + pipe read/write/wait) and cwd capture,
path clean/rel/shallow walk, chmod, HMAC/sha512/AES-GCM, platform TLS roots, CSV,
gzip/zip, structured log helpers, Unix sockets. Still **deferred:** regex (ADR),
recursive Walk, HTTP/2, full sync, templates, SQL.

Tasks/concurrency are **language leaders** `+` / `-` (ADR 0013), not `std/task`.

---

## Domain comparison

### Core text and data

| Go package | Role | Echo today | Gap severity |
|------------|------|------------|--------------|
| `strings` | rich string API | **Thin** `std/str` (+ `parse_int`/`parse_float`) | Medium |
| `bytes` | byte buffers, Reader/Writer | **Thin** `std/bytes` (contains/starts/ends; no buffer type, no scanners) | Medium |
| `strconv` | parse/format numbers | **Thin** `str.parse_int` / `parse_float` + `from_*` | Low–Medium |
| `unicode` / `utf8` / `utf16` | Unicode tables, strict UTF-8 | **Partial** `std/encoding/utf8` valid/decode; no unicode tables | High for i18n |
| `regexp` | RE2 regex | **Gap** (deferred; ADR if public) | High for text tools |
| `encoding/json` | marshal/unmarshal + streaming | **Thin** `std/json` (serde_json); no tags/stream | Medium |
| `encoding/xml`, `csv`, `gob` | structured formats | **Thin** `std/encoding/csv`; xml **Gap**; gob N/A | Medium (xml) |
| `encoding/hex`, `base64` | codecs | **Strong** thin | Low |
| `encoding/binary`, `pem`, `asn1` | binary/crypto formats | **Partial** PEM via `tls.load_pem`; no binary/asn1 | Medium–High |
| `fmt` | printf / scan | **Partial** `io.print` strings-only + `str.from_*` | Medium |
| `bufio` | buffered R/W, scanners | **Thin** `std/bufio.lines` / `read_lines` | Medium |
| `io` / `io/fs` | Reader/Writer, FS abstraction | **Partial** print + `io.read` (whole stdin bytes) + streaming `% file` | High (design) |
| `mime` / `mime/multipart` | content types, forms | **Gap** | Medium for HTTP forms |
| `archive/*`, `compress/*` | zip/tar/gzip | **Thin** `std/compress/{gzip,zip}` (no tar) | Low–Medium |
| `hash`, `crypto/*` | broad crypto suite | **Thin** sip/sha256/sha512, HMAC-SHA256, AES-GCM, CSPRNG; no ed25519/x509 | Medium |
| `math`, `math/big`, `math/rand` | numerics | **Thin** f64 libm + int min/max + PRNG | Medium (big int) |

### Files, path, OS, process

| Go package | Role | Echo today | Gap severity |
|------------|------|------------|--------------|
| `path`, `path/filepath` | path ops, Walk, Clean, Rel | **Thin** join/parent/name/ext + `clean`/`rel`/shallow `walk` | Low–Medium |
| `os` | files, env, process, signals | **Partial** `os`/`fs`/`process`; `chmod`; no signals/user | Medium–High |
| `os/exec` | Cmd, pipes, context cancel | **Thin** `run`/`run_capture`/`run_cwd` + **`spawn_pipes`** (stdin/out/err handles + wait) | Medium |
| `os/signal`, `os/user` | signals, passwd | **Gap** | Medium |
| `io/fs` | abstract FS | **Gap** | Low until multi-backend needed |
| `embed` | embed files in binary | **Gap** | Medium for single-binary apps |
| `flag` | CLI flags | **Thin** `std/cli` (not getopt/GNU) | Medium |
| `log`, `log/slog` | structured logging | **Thin** levels + `kv`/`info_kv` | Medium |
| `testing` | tests, benchmarks, fuzz | **Partial** `std/test` + `xo test` / `--bench`; no fuzz | Medium |

### Time

| Go package | Role | Echo today | Gap severity |
|------------|------|------------|--------------|
| `time` | Time, Duration, location, format/parse, Ticker/Timer | **Thin** ms wall/mono + sleep + `format`/`parse` (chrono) | Medium |
| `context` | cancellation / deadlines | **Gap** (tasks language-owned; no context type) | High for servers |

### Networking

| Go package | Role | Echo today | Gap severity |
|------------|------|------------|--------------|
| `net` | Dial/Listen, IP, Unix sockets | **Thin** TCP/UDP + **Unix domain** | Low–Medium |
| `net/http` | full HTTP client/server | **Partial** serve + `get`/`request` + TLS client helpers; no cookies/HTTP/2 | Medium |
| `net/http/httptest` | tests | **Gap** (e26/local only) | Low |
| `net/url` | URL parse/build | **Thin** `std/net/url` (http/https) | Low |
| `net/mail`, `net/smtp`, `net/textproto` | mail/SMTP | **Gap** | Low for core |
| `net/rpc` | RPC | **Gap** (intentionally out) | N/A product |
| `crypto/tls` | rich TLS config | **Thin** connect/accept + PEM + platform roots (empty ca_pem) | Medium |
| `database/sql` | DB drivers | **Gap** (keep out of core std) | Out |

### Sync, concurrency, reflection

| Go package | Role | Echo today | Gap severity |
|------------|------|------------|--------------|
| `sync`, `sync/atomic` | Mutex, WaitGroup, Map, atomics | **Gap** (no std mutex; memory model TBD) | High if multi-thread shared mut |
| `runtime` | GC, stacks, GOMAXPROCS | **N/A** — Echo scope ownership + runtime crate, not user `runtime` | N/A |
| `reflect` | full reflection | **Thin** kind/key_bytes for collections | High for generic libs |
| `unsafe` | raw memory | **Gap** / intentional | Out of v0 |
| `errors` | wrap/Is/As | **Partial** result `!` strings; no error type graph | Medium |
| `slices`, `maps`, `cmp` | generic helpers | **Partial** list `map`/`filter`/`fold` + `sum_ints`; no language generics | Medium |

### Go packages usually **not** targets for Echo std

These stay **out** unless an ADR says otherwise:

- `html/template`, `text/template` — app/framework space  
- `database/sql` + drivers — ecosystem packages  
- `go/*` toolchain packages — Echo has `xo` / crates  
- `plugin`, `debug/elf|pe|dwarf` — specialized  
- `expvar`, `runtime/pprof` — later observability  
- Full `image/*`, `cgi`, `fcgi` — niche  

---

## Capability gaps (ranked)

Priorities assume **systems + scripting** users (CLIs, small servers, file/JSON tools), Echo’s pure-Echo-vs-crate rule, and existing net/fs strength.

### P0 — Unblocks “looks like a real language” apps

| Gap | Go analog | Why it matters | Suggested approach |
|-----|-----------|----------------|--------------------|
| **URL parse/build** | `net/url` | Every HTTP client/server needs it | Pure Echo + small runtime if needed; crate only if complex |
| **Richer HTTP client** | `net/http.Client` | Methods, headers, body, status, redirects policy, HTTPS via `std/net/tls` | Std on TCP/TLS; httparse already for request side |
| **Time format/parse** | `time.Format` / `Parse` | Logs, APIs, files | Runtime (chrono-like crate) + thin std |
| **String/number parse** | `strconv` | `parse_int` / `parse_float` result-shaped | Runtime or pure; must not panic |
| **Buffered / line I/O** | `bufio` | CLI tools, protocols | Std buffer over bytes/file; or runtime reader |
| **Process pipes / richer exec** | `os/exec` | stdin/out/err streams, env, cwd | **Shipped** `spawn_pipes` + pipe r/w + wait; also `run_cwd` |

### P1 — Quality of life and depth

| Gap | Go analog | Notes |
|-----|-----------|--------|
| **filepath Walk / Clean / Rel** | `path/filepath` | Careful Walk (symlink loops); pure + `fs` |
| **HMAC + more hashes** | `crypto/hmac`, sha512, blake | Crates (`hmac`, `sha2`) |
| **AES-GCM / modern AEAD** | `crypto/aes` + `cipher` | Crate only; product API careful |
| **x509 / cert helpers** | `crypto/x509` | Optional; TLS already has PEM load |
| **Platform TLS roots** | system roots | rustls-native-certs / webpki-roots |
| **CSV** | `encoding/csv` | Pure Echo or crate |
| **gzip / zip** | `compress/gzip`, `archive/zip` | Crates |
| **Regex** | `regexp` | Defer or crate (heavy); ADR if public |
| **Structured log** | `log/slog` | Pure over io; JSON lines optional |
| **List/string HOFs** | `slices` | **List** `map`/`filter`/`fold` shipped (closed fn values); string HOFs still open |
| **Temp file API polish** | `os.CreateTemp` patterns | Mostly present; expand |
| **File permissions / chmod** | `os.Chmod` | Runtime |

### P2 — Nice later / ecosystem

| Gap | Go analog | Notes |
|-----|-----------|--------|
| Unix domain sockets | `net` | Runtime |
| HTTP/2, websockets | external in Go often | Out of core std |
| Templates | `html/template` | Apps, not std |
| SQL | `database/sql` | Package ecosystem |
| Embed | `embed` | Build/link story |
| Fuzz/bench | `testing` | Tooling |
| Full Unicode | `unicode` | ICU-sized; defer |

### Language / model dependencies (not “just std”)

Closing these **stdlib gaps** may require **language** work first:

| Need | Why std is blocked |
|------|---------------------|
| First-class function values (complete) | List `map`/`filter`/`fold` shipped; slog-style handlers still open |
| Error values richer than strings | `errors.Is`/`As` style without panic culture |
| Shared-memory concurrency model | `sync` only makes sense if threads share heap safely |
| Iterator / streaming I/O traits | Go interfaces; Echo may stay free-fn + `%` types |
| Generics | Go 1.18+ slices/maps helpers; Echo may stay monomorphic + pure helpers |

Do not invent language edges only to green std fixtures ([AGENTS.md](../AGENTS.md)).

---

## Side-by-side “minimum useful CLI / server”

| Need | Go | Echo today | Verdict |
|------|-----|------------|---------|
| Print + args | `fmt`, `os.Args`, `flag` | `io`, `process.args`, `cli` | **Usable** |
| Read/write files | `os`, `io` | `fs` whole-file + stream | **Usable** |
| JSON config | `encoding/json` | `json` | **Usable (thin)** |
| HTTP server | `net/http` | `net/http` serve | **Usable (thin)** |
| HTTPS client | `http.Get` | TLS sockets + DIY or cleartext only client | **Partial** — need HTTP-over-TLS helper |
| Timeouts / cancel | `context` | tasks only | **Gap** |
| Subprocess capture / pipes | `exec.Command` | `run_capture` + `spawn_pipes` | **Usable (thin)** |
| Path join | `filepath.Join` | `path` / `fs.join` | **Usable** |
| Regex | `regexp` | — | **Gap** |
| Logging levels | `log/slog` | `log` | **Usable (thin)** |

---

## Recommended near-term std work (practical)

Ordered for leverage under current policy:

1. **`std/net/url`** — parse/build (pure Echo if possible).  
2. **`std/net/http_client` depth** — POST/headers/status; optional HTTPS using `std/net/tls`.  
3. **`std/time` growth** — format/parse (crate-backed).  
4. **`std/strconv` or `str.parse_*`** — int/float parse with result.  
5. **`std/bufio` or line reader** — over `fs` / bytes.  
6. **`process` pipes** — spawn with stdin/stdout handles.  
7. **Crypto depth** — HMAC-SHA256 (crate), then stop before full x509 unless needed.  
8. **filepath Walk/Clean** — careful; e26 under temp roots.  
9. **Defer** regex/compress until an app demand + ADR.

Each item still needs the **three proofs** (crate if native · e26 · examples) and inventory rows in [`stdlib.md`](stdlib.md).

---

## What *not* to do

- Clone Go package-for-package into `std/`.  
- Hand-roll TLS/JSON/HTTP parse/crypto in Rust — **use crates** ([stdlib.md](stdlib.md) § Where logic lives).  
- Put spawn/join in `std/task` (ADR 0013).  
- Claim “HTTPS done” because TLS sockets exist — HTTP client framing is separate.  
- Mark modules Done without e26 when user-visible.

---

## Summary

Echo’s std already covers a **credible core**: files, process, path, JSON, math, collections, TCP/UDP/HTTP serve, cleartext HTTP get, TLS sockets, CLI flags, log, test.

Relative to Go, the largest **product** gaps for everyday use are:

1. **URL + richer HTTP(S) client**  
2. **Time format/parse and cancellation story**  
3. **Text tooling** (strconv, bufio, regex)  
4. **Crypto breadth** beyond hash/CSPRNG  
5. **Exec/OS depth** (pipes, signals, permissions)  
6. **Sync/reflection** (language-coupled)

Go’s advantage is decades of **breadth and API polish**. Echo’s advantage should stay **small composable modules, honest ownership, and crate-backed hard problems**—close P0 gaps with full verticals rather than a second Go stdlib.
