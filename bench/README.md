# Echo benchmarks

Normal `test.bench(...)` next to **real** Echo functions — same Model A as
`test.it`. See [`docs/testing.md`](../docs/testing.md).

## Two optimization layers

```text
1. HOST (Rust)     just bench-host   → release xo + libecho_runtime.a
2. ECHO (LLVM)     xo test --bench -O2 …  → optimizes .echo, links runtime
3. MEASURE         auto-N loop in the AOT child only (ns/op)
```

```bash
just bench-host
just std-bench          # all std co-located benches
just algo-bench         # algorithms + selected std hot paths
just std-bench-save     # promote .xo/bench/last.jsonl → baseline
just std-bench-compare
```

## Where benches live

| Location | What |
|----------|------|
| `std/**/*.echo` | Real std APIs (`list.sum_ints`, `hash.sip`, `map.put`, …) |
| `examples/algos/*.echo` | Real algorithms (`fib`, `gcd`, `sum_to`, sorts, collatz, primes) + demos |

No separate “canary wrapper” module — call the real function with real args
built in the bench body (Echo free functions are closed; no outer captures).

## Useful canaries (what to watch)

| Bench | File | Signal |
|-------|------|--------|
| `sum_to_1e6` / `1e7` | `examples/algos/list.echo` | tight integer loop |
| `fib_40` | `examples/algos/fibonacci.echo` | iterative arith |
| `gcd_euclid` | `examples/algos/gcd.echo` | rem/branch |
| `sip_*` size series | `std/crypto/hash/sip.echo` | pure Echo → LLVM (`fshl`) |
| `sum_ints_1k` / `10k` | `std/list.echo` | ~Θ(n) scale |
| `sort_ints_1k` | `std/list.echo` | heavier than sum |
| `seed_1k_get` | `std/collections/map.echo` | hash table + sip |
| `checksum_*` | `std/bytes.echo` | `bytes.get` scan (setup uses `str.repeat` O(n)) |
| `sha256_*` | `std/crypto/hash/sha256.echo` | native floor |
| `empty_call` | `examples/algos/call.echo` | call floor |

**Ratios (same run):** e.g. `list_sum_10k / list_sum_1k ≈ 10`, `sort_1k ≫ sum_1k`,
`sip_1k ≫ sip_empty`, `sip_empty` near thin floor after lowerability.

**Runtime hot-path notes (keep green):** `bytes_get`/`str_get`/`bytes_len` must
**borrow** heap payloads (no full-buffer clone per index). `str_slice` /
`str_contains` / `starts_with` / `ends_with` borrow then copy only the result
range. HTTP `request_complete` / `parse_request` borrow the accumulating buffer.
`str.repeat` is O(n) via `runtime.str_repeat`. Map/set bucket arrays use
`runtime.list_new_empty_lists`.

## Notes

- Default host: `XO=target/release/xo` (override as needed).
- JSONL keys: `file::name@opt` (e.g. `…/sip.echo::sip_empty@O2`).
- Absolute ns/op is machine-local; use baselines + thresholds for regressions.
