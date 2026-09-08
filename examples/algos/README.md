# `examples/algos/`

Famous, simple algorithms written in Echo. These programs exercise the locked
language surface (`docs/syntax.md`).

```bash
cargo build -p xo
./target/debug/xo run examples/algos/factorial.echo
./target/debug/xo run examples/algos/sort.echo

# Real algorithms + normal test.bench (release host recommended):
just bench-host
just algo-bench
```

Top-level statements run directly — no `$ demo` wrapper. Benches use the same
functions the demos call (`test.bench` via `/ std/test`).

| File | Algorithms |
|------|------------|
| [`factorial.echo`](factorial.echo) | Iterative and recursive factorial (result for negatives) |
| [`fibonacci.echo`](fibonacci.echo) | Iterative Fibonacci + nth term |
| [`gcd.echo`](gcd.echo) | Euclid GCD, LCM, Stein binary GCD |
| [`collatz.echo`](collatz.echo) | Collatz / 3n+1 step count |
| [`power.echo`](power.echo) | Exponentiation by squaring |
| [`primes.echo`](primes.echo) | Trial division + sieve of Eratosthenes |
| [`search.echo`](search.echo) | Linear search + binary search |
| [`sort.echo`](sort.echo) | Bubble sort + insertion sort (in place) |
| [`list.echo`](list.echo) | Sum, max/min, reverse, membership (`list.len` from std) |
| [`fizzbuzz.echo`](fizzbuzz.echo) | Classic FizzBuzz (kind codes + print) |
| [`hanoi.echo`](hanoi.echo) | Tower of Hanoi (recursive move count) |
| [`digits.echo`](digits.echo) | Digit sum, reverse digits, numeric palindrome |

## Notes

- Prefer **`list.len`** (`/ std/list`) over hand-rolled length loops.
- Lists are **literals** (and bound list values) with index / for-in; **append**
  with `~ xs[] = e` (and `~ a.b[] = e`).
