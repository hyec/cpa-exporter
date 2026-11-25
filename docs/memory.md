# Memory comparison

Measured on Linux x86_64 using the final Rust release binary and the previous
Go implementation with the same CPA v8/name-label functionality. Values below
are medians of three fresh processes. The full results and binary hashes are
in [memory-comparison.json](memory-comparison.json).

| Process RSS | Go | Rust |
| --- | ---: | ---: |
| Idle, after 10 scrapes | 17.2 MiB | 3.1 MiB |
| After workload and 50 scrapes | 21.9 MiB | 6.9 MiB |

Loaded RSS was 68.4% lower in this test.
The workload is 10,000 HTTP usage records with 100 distinct label combinations,
100 records per batch, canonical token accounting, provider names, masked source,
auth index, and a six-character client API-key prefix. Loaded RSS is the median
of ten samples after scrapes. This measures process RSS from `/proc`, rather
than container working set. It uses local HTTP without TLS and does not model
production traffic or unbounded cardinality. Metrics remain in memory for the
process lifetime, so both implementations grow with additional label values.

The Go binary was built using `go build` (Go 1.27.1); Rust used
`cargo build --locked --release` (Rust 1.94.0). This is a comparison of the
implementations as shipped, without tuning Go's GC or either allocator.

To reproduce on Linux, retain a Go binary before replacing the Go source:

```sh
cargo build --locked --release
python3 scripts/benchmark-memory.py \
  --go-binary /path/to/previous-go-cpa-exporter \
  --rust-binary target/release/cpa-exporter
```
