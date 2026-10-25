# Algorithm-tagged hash verification

Source: `6fb85af4140eab6803ed9d4af698e0f7238c6d3c`. The major-1 draft uses
`{algorithm, digest}` records and media domain `EVIF/media/4`.
XXH3-128 remains the writer's default canonical sample algorithm.

Artifacts on r5900xt: `/mnt/v/output/evif/hash-records-20261025/`.
`provenance.json` identifies the source and fixture purpose; `sha256.json`
covers the fixtures, extracted payloads, plans and validation logs. All 27
backup files were reread and hash-verified.

| Fixture | SHA-256 of file bytes |
|---|---|
| apng/transfer-16.evif | 7abde91ee2fc210d66d407b4031b7482f7db30868722aaea2ec326fc3686483e |
| apng/transfer-18.evif | 48c6638d16df5477b7d3705d94e8154b60235b71224ad09160f39621fc618890 |
| future/digest-4096.evif | 2585c5f4c0c6f9e1e91d98d9ba0d2d6595c96e2e4146f29da598f5b5b4ec25ab |

The PQ/HLG APNG fixtures were decoded with FFmpeg. The independent Python
oracle checked frame XXH3-128 with the system C xxHash library and SHA-256
with hashlib, and independently reproduced the tagged media roots.

The four future-algorithm fixtures carry 256-, 512-, 1024- and 4096-bit
sample digests. Their declared samples and external payloads are not verified:
these fixtures test transport and identity construction. The Python oracle
reproduced every media root from algorithm IDs, explicit lengths and digest
bytes, without implementing the unknown sample algorithm.

Rust tests cover streaming/slice agreement, preservation of longer unknown
hashes and external references, algorithm-sensitive identity, changes in the
last digest byte, optional-check independence, malformed spellings and known
algorithm widths, duplicate checks, resource limits, and explicit refusal of
unsupported embedded-payload/media-root verification. Score matching includes
the algorithm; matching digest bytes under another algorithm are refused.

Validation passed:

```text
cargo test --workspace --all-targets --release
cargo test -p evif --test wire --release
cargo test --workspace --doc --release
cargo clippy --workspace --all-targets --release -- -D warnings
cargo fmt -p evif -p evif-probe --check
cargo build -p evif --no-default-features
RUSTFLAGS='-C target-feature=+simd128' cargo build -p evif --no-default-features --target wasm32-wasip1
just wire-oracle /mnt/v/output/evif/hash-records-20261025/apng
python3 tools/wire_oracle.py /mnt/v/output/evif/hash-records-20261025/future/*.evif
```

Heavy commands ran through the resource wrapper with 12 GiB / four-job limits.
These checks establish hash-record behavior, not computation of new algorithms
or HDR display rendering. The format has no fixed digest-width ceiling; caller
JSON limits remain enforced.
