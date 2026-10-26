# Possession integration checks, 2026-10-26

Public implementation: `95cc0f3704f90669bc57e595d89397bcf4e8da4e`.
Run on r5900xt with Rust 1.99.1. Artifacts and SHA256SUMS:
`/mnt/v/output/evif/possession-20261026/`.

Passed:

- `just possession-check`: release clippy with both codec features and all targets.
- `just possession-test`: real AVIF/JXL encode/decode, byte and sample replay,
  odd 65×67 geometry, one pixel-changing control per family, invalid and unknown
  parameter rejection, codec-revision rejection, q5–q95 in steps of 10, and
  24 seeded draws per family.
- `cargo test --release --workspace --all-targets --all-features -- --test-threads=1`.
- `cargo test --doc --workspace --all-features`.
- CLI probes for both families: four requested samples completed, two findings
  retained per family, with baseline curves and scores from the builtin judges.

The artifact directory contains native baseline encodes, their PNG renditions,
recipe JSON, CLI output containers and ledgers, and full command logs. These are
integration checks, not evidence that either codec fooled a metric. No human
quality judgment or exhaustive parameter-space coverage is claimed.

Resource wrapper for public checks: `run-heavy --mem 12G --jobs 4`.
Initial codec test build log: `done rc=0 149s | peak-RSS 0.95GiB |
min-avail 32823MiB | peak-load 13.28`. This includes compilation and is not an
encoding performance measurement. Full commands are in the retained logs.
